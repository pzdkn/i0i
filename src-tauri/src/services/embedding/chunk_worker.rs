//! Embedding stored chunks in the background (RFC 0075 R5).
//!
//! Deliberately not a queue. The work list is a *query* — chunks with no
//! embedding row at the current model and chunk version — so there is no status
//! column, no state machine, and nothing to recover after a crash. Running the
//! sweep twice is harmless; missing a run costs only latency.
//!
//! ## Failure policy
//!
//! This module and `EmbeddingReranker` sit next to each other with deliberately
//! opposite contracts, which is worth stating rather than leaving to be
//! rediscovered.
//!
//! The reranker degrades silently: a failed rerank costs relevance on one query
//! and falling back to legacy weights is correct.
//!
//! Chunk embedding *reports coverage*. It is a persistence job with a definite
//! completion state, so it can be counted — and a missing embedding is a hole in
//! retrieval that a user could never see on their own. Failures log and retry on
//! the next sweep. A model that will not load reports zero coverage rather than
//! crashing: a paper with no embeddings is still readable and still fully
//! searchable through FTS5.

use std::sync::Arc;

use crate::domain::chunking::CHUNK_VERSION;
use crate::storage::library_store::LibraryStore;

use super::{TextEmbedder, MODEL_NAME, MODEL_VERSION};

/// Chunks per embedding call. Large enough to amortize the model call, small
/// enough that a failure re-does little work and the sweep stays interruptible.
const BATCH_SIZE: i64 = 32;

/// Ceiling on one sweep, so a first run over a large library cannot occupy the
/// blocking pool indefinitely. Whatever is left is picked up by the next sweep.
const MAX_BATCHES_PER_SWEEP: usize = 200;

#[derive(Clone)]
pub struct ChunkEmbeddingWorker {
    store: LibraryStore,
    embedder: Option<Arc<dyn TextEmbedder>>,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct SweepOutcome {
    pub embedded: usize,
    pub failed: usize,
}

impl ChunkEmbeddingWorker {
    pub fn new(store: LibraryStore, embedder: Option<Arc<dyn TextEmbedder>>) -> Self {
        Self { store, embedder }
    }

    pub fn is_ready(&self) -> bool {
        self.embedder.is_some()
    }

    /// Embed everything currently missing an embedding.
    ///
    /// Returns what happened rather than logging and swallowing it, so the
    /// caller can report coverage. Never returns `Err`: a sweep that fails
    /// partway has still done real work, and the remainder is picked up next
    /// time.
    pub async fn sweep(&self) -> SweepOutcome {
        let mut outcome = SweepOutcome::default();

        let Some(embedder) = self.embedder.clone() else {
            // Not an error to report upward — coverage already tells the story,
            // and every paper remains searchable through FTS5.
            return outcome;
        };

        for _ in 0..MAX_BATCHES_PER_SWEEP {
            let pending = match self
                .store
                .chunks_missing_embedding(MODEL_NAME, MODEL_VERSION, BATCH_SIZE)
            {
                Ok(pending) => pending,
                Err(error) => {
                    eprintln!("[embedding] could not read pending chunks: {error}");
                    return outcome;
                }
            };
            if pending.is_empty() {
                break;
            }

            let texts: Vec<String> = pending.iter().map(|chunk| chunk.text.clone()).collect();
            let embedder = embedder.clone();
            let embeddings =
                match tokio::task::spawn_blocking(move || embedder.embed(&texts)).await {
                    Ok(Ok(embeddings)) => embeddings,
                    Ok(Err(error)) => {
                        eprintln!("[embedding] batch failed, retrying next sweep: {error}");
                        outcome.failed += pending.len();
                        return outcome;
                    }
                    Err(error) => {
                        eprintln!("[embedding] embed task panicked: {error}");
                        outcome.failed += pending.len();
                        return outcome;
                    }
                };

            // A shape mismatch means we cannot trust which vector belongs to
            // which chunk. Writing them anyway would corrupt retrieval in a way
            // nothing downstream could detect.
            if embeddings.len() != pending.len() {
                eprintln!(
                    "[embedding] expected {} vectors, got {} — skipping batch",
                    pending.len(),
                    embeddings.len()
                );
                outcome.failed += pending.len();
                return outcome;
            }

            for (chunk, embedding) in pending.iter().zip(embeddings.iter()) {
                match self.store.save_chunk_embedding(
                    &chunk.id,
                    MODEL_NAME,
                    MODEL_VERSION,
                    CHUNK_VERSION,
                    embedding,
                ) {
                    Ok(()) => outcome.embedded += 1,
                    Err(error) => {
                        eprintln!("[embedding] could not save {}: {error}", chunk.id);
                        outcome.failed += 1;
                    }
                }
            }
        }

        outcome
    }

    /// Re-chunk anything below the current `CHUNK_VERSION`, then embed.
    ///
    /// The startup entry point (RFC 0075 R6). Re-chunking first matters: chunks
    /// from an older chunker would otherwise be embedded and then immediately
    /// replaced.
    pub async fn recover_and_sweep(&self) -> SweepOutcome {
        match self.store.extractions_needing_rechunk() {
            Ok(stale) => {
                for extraction_id in stale {
                    match self.store.rechunk_extraction(&extraction_id) {
                        Ok(count) => eprintln!("[embedding] re-chunked {extraction_id}: {count}"),
                        Err(error) => {
                            eprintln!("[embedding] re-chunk failed for {extraction_id}: {error}")
                        }
                    }
                }
            }
            Err(error) => eprintln!("[embedding] could not list stale extractions: {error}"),
        }

        let outcome = self.sweep().await;

        // Index anything embedded before the vector index existed, or left
        // behind if it was ever dropped (RFC 0076). Costs no re-embedding — the
        // vectors already exist, this only inserts them into vec0. After the
        // sweep, so newly written embeddings are included in one pass.
        match self.store.index_missing_chunk_vectors() {
            Ok(0) => {}
            Ok(indexed) => eprintln!("[search] indexed {indexed} chunk vectors"),
            Err(error) => eprintln!("[search] vector backfill failed: {error}"),
        }

        outcome
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Deterministic stand-in for the real model: a 4-dimensional vector keyed
    /// off the text, so tests exercise batching and persistence without ONNX.
    struct StubEmbedder {
        calls: AtomicUsize,
        fail: bool,
        /// Return the wrong number of vectors, to exercise the alignment guard.
        truncate: bool,
    }

    impl StubEmbedder {
        fn working() -> Self {
            Self {
                calls: AtomicUsize::new(0),
                fail: false,
                truncate: false,
            }
        }
    }

    impl TextEmbedder for StubEmbedder {
        fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, String> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            if self.fail {
                return Err("stub failure".to_string());
            }
            let mut out: Vec<Vec<f32>> = texts
                .iter()
                .map(|text| {
                    let length = text.chars().count() as f32;
                    vec![length, length / 2.0, 1.0, 0.0]
                })
                .collect();
            if self.truncate {
                out.pop();
            }
            Ok(out)
        }
    }

    use crate::domain::library::{DocumentBlock, PaperDraft, PaperSourceDraft};

    /// A store holding one paper whose extraction is ready and chunked, which
    /// is the only state this worker ever operates on.
    fn store_with_chunks(paper_id: &str) -> (LibraryStore, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "i0i-chunk-worker-test-{}-{}-{paper_id}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let store = LibraryStore::for_test(dir.join("library.sqlite"));
        store.init().unwrap();

        let draft = PaperDraft {
            id: paper_id.to_string(),
            title: "Attention Is All You Need".to_string(),
            authors: vec!["Vaswani".to_string()],
            venue: "NeurIPS".to_string(),
            year: 2017,
            citations: 0,
            tags: Vec::new(),
            status: "unread".to_string(),
            abstract_text: None,
            sources: vec![PaperSourceDraft {
                source_kind: "pdf".to_string(),
                source_url: format!("https://example.test/{paper_id}.pdf"),
                landing_url: None,
            }],
        };
        store
            .add_paper_to_vaults(&draft, &["attention".to_string()])
            .unwrap();

        let source = store.get_document_sources(paper_id).unwrap().remove(0);
        store
            .set_document_source_cached(&source.id, "/tmp/worker.pdf")
            .unwrap();
        let extraction = store
            .start_document_extraction(
                &source.id,
                "pdfium_basic",
                "0.2.0",
                &format!("pdfium_basic:{}", source.id),
                false,
            )
            .unwrap();

        // Long enough to produce several chunks, so batching is exercised.
        let body = "attention mechanisms relate positions within a sequence ".repeat(400);
        let block = DocumentBlock {
            id: format!("{}:block:0:0", extraction.id),
            paper_id: extraction.paper_id.clone(),
            source_id: extraction.source_id.clone(),
            extraction_id: extraction.id.clone(),
            page_index: 0,
            block_index: 0,
            reading_order: 0,
            kind: "paragraph".to_string(),
            text: Some(body.clone()),
            asset_id: None,
            source_start: Some(0),
            source_end: Some(body.chars().count() as i64),
            bbox_json: None,
        };
        store
            .finish_document_extraction(&extraction.id, &[], &[block], &[])
            .unwrap();

        (store, dir)
    }

    #[tokio::test]
    async fn a_sweep_embeds_every_pending_chunk() {
        let (store, _dir) = store_with_chunks("sweep-paper");
        let worker = ChunkEmbeddingWorker::new(store.clone(), Some(Arc::new(StubEmbedder::working())));

        let total = store
            .embedding_coverage("sweep-paper", MODEL_NAME, MODEL_VERSION)
            .expect("coverage");
        assert!(total.chunks > 0);
        assert_eq!(total.embedded, 0);

        let outcome = worker.sweep().await;
        assert_eq!(outcome.embedded, total.chunks as usize);
        assert_eq!(outcome.failed, 0);

        let after = store
            .embedding_coverage("sweep-paper", MODEL_NAME, MODEL_VERSION)
            .expect("coverage");
        assert_eq!(after.embedded, after.chunks);
    }

    #[tokio::test]
    async fn a_second_sweep_does_nothing() {
        let (store, _dir) = store_with_chunks("idempotent-paper");
        let embedder = Arc::new(StubEmbedder::working());
        let worker = ChunkEmbeddingWorker::new(store.clone(), Some(embedder.clone()));

        worker.sweep().await;
        let calls_after_first = embedder.calls.load(Ordering::Relaxed);

        let second = worker.sweep().await;
        assert_eq!(second.embedded, 0);
        // One extra call: the query that finds nothing pending.
        assert!(embedder.calls.load(Ordering::Relaxed) <= calls_after_first + 1);
    }

    #[tokio::test]
    async fn a_failing_model_leaves_chunks_retryable() {
        let (store, _dir) = store_with_chunks("failing-paper");
        let failing = Arc::new(StubEmbedder {
            calls: AtomicUsize::new(0),
            fail: true,
            truncate: false,
        });
        let worker = ChunkEmbeddingWorker::new(store.clone(), Some(failing));

        let outcome = worker.sweep().await;
        assert_eq!(outcome.embedded, 0);
        assert!(outcome.failed > 0);

        // The whole point of the query-as-queue: a failure needs no cleanup.
        let coverage = store
            .embedding_coverage("failing-paper", MODEL_NAME, MODEL_VERSION)
            .expect("coverage");
        assert_eq!(coverage.embedded, 0);

        let recovered = ChunkEmbeddingWorker::new(store.clone(), Some(Arc::new(StubEmbedder::working())))
            .sweep()
            .await;
        assert_eq!(recovered.embedded, coverage.chunks as usize);
    }

    #[tokio::test]
    async fn a_misaligned_batch_is_skipped_rather_than_written() {
        let (store, _dir) = store_with_chunks("misaligned-paper");
        let worker = ChunkEmbeddingWorker::new(
            store.clone(),
            Some(Arc::new(StubEmbedder {
                calls: AtomicUsize::new(0),
                fail: false,
                truncate: true,
            })),
        );

        let outcome = worker.sweep().await;
        assert_eq!(outcome.embedded, 0, "a misaligned batch must write nothing");

        let coverage = store
            .embedding_coverage("misaligned-paper", MODEL_NAME, MODEL_VERSION)
            .expect("coverage");
        assert_eq!(coverage.embedded, 0);
    }

    #[tokio::test]
    async fn no_model_reports_zero_coverage_without_failing() {
        let (store, _dir) = store_with_chunks("modelless-paper");
        let worker = ChunkEmbeddingWorker::new(store.clone(), None);

        assert!(!worker.is_ready());
        let outcome = worker.sweep().await;
        assert_eq!(outcome, SweepOutcome::default());

        // Still fully searchable lexically — that is why this is not fatal.
        let hits = store
            .search_chunks_lexical("modelless-paper", "attention", 10)
            .expect("lexical search");
        assert!(!hits.is_empty());
    }
}
