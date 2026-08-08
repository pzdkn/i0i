//! Search within papers we already hold (RFC 0076).
//!
//! **Not** deep-research discovery — that is `services::research::SearchManager`,
//! which finds papers the library does *not* have. This service finds passages
//! inside papers it does.
//!
//! Deliberately agnostic about its caller. The reader searches one PDF, the
//! vault view searches one vault, global search searches everything, and the
//! agent searches to enrich its own context — all the same call with a
//! different scope. Three things follow:
//!
//! - **No "current" anything.** Current paper and current vault are UI state.
//!   The command layer resolves them; a service that knew about the open PDF
//!   could not serve global search or the agent.
//! - **Scope is a filter, not a mode.** No `searchDocument` versus
//!   `searchLibrary`; one call, narrowed.
//! - **No caller-specific ranking.** Everyone gets the same ordering.

pub mod fusion;

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::domain::library::{DocumentChunk, EmbeddingCoverage};
use crate::services::embedding::{TextEmbedder, MODEL_NAME, MODEL_VERSION};
use crate::storage::library_store::LibraryStore;

use fusion::{reciprocal_rank_fusion, single_signal, Fused, Signal};

/// Candidates each signal fetches before fusion, as a multiple of `limit`.
///
/// Over-fetching is what lets a chunk ranked 15th lexically and 3rd
/// semantically reach a top-12 response; without it, fusion could only reorder
/// what both signals already agreed to show.
const CANDIDATE_MULTIPLIER: usize = 4;

const DEFAULT_LIMIT: usize = 12;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum SearchMode {
    Lexical,
    Semantic,
    #[default]
    Hybrid,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchRequest {
    pub query: String,
    /// Empty means unconstrained on this dimension — see `resolve_search_scope`.
    #[serde(default)]
    pub paper_ids: Vec<String>,
    #[serde(default)]
    pub vault_ids: Vec<String>,
    #[serde(default)]
    pub mode: SearchMode,
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChunkHit {
    pub chunk: DocumentChunk,
    /// The number this response was sorted by — **comparable only within this
    /// response**. RRF in `Hybrid`, negated BM25 in `Lexical`, negated distance
    /// in `Semantic`. Deliberately not normalized to 0–100: that would invite
    /// comparison across queries and modes, and every such comparison is
    /// meaningless. The per-signal fields below are the honest version.
    pub score: f64,
    pub lexical: Option<Signal>,
    pub semantic: Option<Signal>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScopeSummary {
    pub paper_ids: Vec<String>,
    pub empty_reason: Option<EmptyScope>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum EmptyScope {
    /// The requested papers and vaults do not overlap — usually a paper open
    /// outside the active vault. A routine UI state, not an error.
    NoIntersection,
    /// Nothing to search: an empty library, or ids that do not exist.
    NoPapers,
}

/// Whether semantic retrieval actually ran, and how completely.
///
/// Continues RFC 0075's split: the reranker degrades silently, retrieval counts
/// its holes. A hybrid request against a paper whose embeddings are still being
/// written returns lexical hits and says so, rather than claiming to have run a
/// hybrid search — a case that *will* happen, since the startup sweep takes
/// minutes on an existing library.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum SemanticStatus {
    Ran { coverage: EmbeddingCoverage },
    /// sqlite-vec or the embedding model is unavailable.
    Unavailable,
    NotRequested,
    /// The scope has chunks, none of them embedded yet.
    NoEmbeddings,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResponse {
    pub hits: Vec<ChunkHit>,
    pub scope: ScopeSummary,
    pub semantic: SemanticStatus,
}

#[derive(Clone)]
pub struct SearchService {
    store: LibraryStore,
    embedder: Option<Arc<dyn TextEmbedder>>,
}

impl SearchService {
    pub fn new(store: LibraryStore, embedder: Option<Arc<dyn TextEmbedder>>) -> Self {
        Self { store, embedder }
    }

    pub async fn search(&self, request: SearchRequest) -> Result<SearchResponse, String> {
        let limit = request.limit.unwrap_or(DEFAULT_LIMIT).max(1);
        let candidates = (limit * CANDIDATE_MULTIPLIER) as i64;

        let paper_ids = self
            .store
            .resolve_search_scope(&request.paper_ids, &request.vault_ids)?;
        let scope = summarize_scope(&request, paper_ids.clone());

        if paper_ids.is_empty() {
            // A legitimate answer, not an error: asking for a paper that is not
            // in the named vault has the honest answer "nothing". Throwing would
            // turn a routine UI state into an error dialog; returning zero hits
            // silently would leave the caller unable to tell "nothing matched"
            // from "you asked for an impossible scope".
            if let Some(reason) = scope.empty_reason {
                eprintln!(
                    "[search] empty scope ({reason:?}): papers {:?} ∩ vaults {:?}",
                    request.paper_ids, request.vault_ids
                );
            }
            return Ok(SearchResponse {
                hits: Vec::new(),
                scope,
                semantic: SemanticStatus::NotRequested,
            });
        }

        let query = request.query.trim();
        if query.is_empty() {
            return Ok(SearchResponse {
                hits: Vec::new(),
                scope,
                semantic: SemanticStatus::NotRequested,
            });
        }

        let lexical = match request.mode {
            SearchMode::Semantic => Vec::new(),
            _ => self
                .store
                .lexical_chunk_ranking(&paper_ids, query, candidates)?,
        };

        let (semantic, status) = match request.mode {
            SearchMode::Lexical => (Vec::new(), SemanticStatus::NotRequested),
            _ => self.semantic_ranking(&paper_ids, query, candidates).await?,
        };

        let fused = match request.mode {
            SearchMode::Hybrid => reciprocal_rank_fusion(&lexical, &semantic),
            SearchMode::Lexical => single_signal(&lexical, true),
            SearchMode::Semantic => single_signal(&semantic, false),
        };

        Ok(SearchResponse {
            hits: self.hydrate(fused, limit)?,
            scope,
            semantic: status,
        })
    }

    /// Embed the query and run KNN, reporting why if it could not.
    ///
    /// Distance is negated so that, like BM25, higher means better everywhere
    /// downstream — one rule for both signals rather than a per-signal
    /// direction that a future caller would have to remember.
    async fn semantic_ranking(
        &self,
        paper_ids: &[String],
        query: &str,
        candidates: i64,
    ) -> Result<(Vec<(String, f64)>, SemanticStatus), String> {
        let Some(embedder) = self.embedder.clone() else {
            return Ok((Vec::new(), SemanticStatus::Unavailable));
        };

        let coverage = self.scope_coverage(paper_ids)?;
        if coverage.embedded == 0 {
            return Ok((Vec::new(), SemanticStatus::NoEmbeddings));
        }

        let owned = vec![query.to_string()];
        let embedded = tokio::task::spawn_blocking(move || embedder.embed(&owned))
            .await
            .map_err(|error| format!("query embed task panicked: {error}"))?;

        let vector = match embedded {
            Ok(mut vectors) if !vectors.is_empty() => vectors.remove(0),
            Ok(_) => return Ok((Vec::new(), SemanticStatus::Unavailable)),
            Err(error) => {
                eprintln!("[search] query embedding failed: {error}");
                return Ok((Vec::new(), SemanticStatus::Unavailable));
            }
        };

        let ranking = self
            .store
            .semantic_chunk_ranking(paper_ids, &vector, candidates)?
            .into_iter()
            .map(|(chunk_id, distance)| (chunk_id, -distance))
            .collect();

        Ok((ranking, SemanticStatus::Ran { coverage }))
    }

    fn scope_coverage(&self, paper_ids: &[String]) -> Result<EmbeddingCoverage, String> {
        let mut total = EmbeddingCoverage {
            chunks: 0,
            embedded: 0,
        };
        for paper_id in paper_ids {
            let coverage = self
                .store
                .embedding_coverage(paper_id, MODEL_NAME, MODEL_VERSION)?;
            total.chunks += coverage.chunks;
            total.embedded += coverage.embedded;
        }
        Ok(total)
    }

    /// Load text for the survivors only.
    ///
    /// Truncating before hydration is the point of ranking on ids: the two
    /// signals produced up to `limit * 8` candidates between them, and only
    /// `limit` of them need their text.
    fn hydrate(&self, fused: Vec<Fused>, limit: usize) -> Result<Vec<ChunkHit>, String> {
        let winners: Vec<Fused> = fused.into_iter().take(limit).collect();
        let ids: Vec<String> = winners.iter().map(|f| f.chunk_id.clone()).collect();
        let chunks = self.store.chunks_by_ids(&ids)?;

        // `chunks_by_ids` returns table order; rebuild the fused order. A chunk
        // that vanished between ranking and hydration is dropped rather than
        // erroring — deletion mid-search is rare but not impossible.
        Ok(winners
            .into_iter()
            .filter_map(|fused| {
                chunks
                    .iter()
                    .find(|chunk| chunk.id == fused.chunk_id)
                    .map(|chunk| ChunkHit {
                        chunk: chunk.clone(),
                        score: fused.score,
                        lexical: fused.lexical,
                        semantic: fused.semantic,
                    })
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::chunking::CHUNK_VERSION;
    use crate::domain::library::{DocumentBlock, PaperDraft, PaperSourceDraft};
    use crate::services::embedding::MODEL_DIMENSIONS;

    /// Embeds a query onto a fixed axis, so "nearest" is arranged by the test
    /// rather than by a model. Index 0 is the attention axis.
    struct AxisEmbedder;

    impl TextEmbedder for AxisEmbedder {
        fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, String> {
            Ok(texts
                .iter()
                .map(|text| {
                    let mut vector = vec![0.0f32; MODEL_DIMENSIONS];
                    if text.contains("attention") {
                        vector[0] = 1.0;
                    } else {
                        vector[1] = 1.0;
                    }
                    vector
                })
                .collect())
        }
    }

    fn axis_vector(index: usize) -> Vec<f32> {
        let mut vector = vec![0.0f32; MODEL_DIMENSIONS];
        vector[index] = 1.0;
        vector
    }

    struct Fixture {
        store: LibraryStore,
        _dir: std::path::PathBuf,
    }

    /// Two papers with one chunk each: one about attention, one not.
    fn fixture() -> Fixture {
        // A sequence counter alongside the timestamp: parallel tests can start
        // within the same nanosecond and would otherwise share a database file,
        // which surfaces as an unrelated "database is locked".
        static SEQUENCE: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "i0i-search-test-{}-{}-{}",
            std::process::id(),
            SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let store = LibraryStore::for_test(dir.join("library.sqlite"));
        store.init().unwrap();

        add_paper(&store, "attention-paper", "The attention mechanism scales well.");
        add_paper(&store, "cooking-paper", "Braising requires low sustained heat.");

        Fixture { store, _dir: dir }
    }

    fn add_paper(store: &LibraryStore, paper_id: &str, body: &str) {
        let draft = PaperDraft {
            id: paper_id.to_string(),
            title: paper_id.to_string(),
            authors: vec!["Author".to_string()],
            venue: "Venue".to_string(),
            year: 2024,
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
            .set_document_source_cached(&source.id, "/tmp/search.pdf")
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
        let block = DocumentBlock {
            id: format!("{}:block:0:0", extraction.id),
            paper_id: paper_id.to_string(),
            source_id: source.id.clone(),
            extraction_id: extraction.id.clone(),
            page_index: 0,
            block_index: 0,
            reading_order: 0,
            kind: "paragraph".to_string(),
            text: Some(body.to_string()),
            asset_id: None,
            source_start: Some(0),
            source_end: Some(body.chars().count() as i64),
            bbox_json: None,
        };
        store
            .finish_document_extraction(&extraction.id, &[], &[block], &[])
            .unwrap();
    }

    /// Embed every chunk onto the axis its text implies.
    fn embed_all(store: &LibraryStore) {
        for paper_id in ["attention-paper", "cooking-paper"] {
            let extraction = store
                .get_document_sources(paper_id)
                .unwrap()
                .into_iter()
                .find_map(|source| {
                    store
                        .ready_document_extraction_for_source(&source.id, "pdfium_basic")
                        .ok()
                        .flatten()
                })
                .expect("ready extraction");
            for chunk in store.chunks_for_extraction(&extraction.id).unwrap() {
                let axis = if chunk.text.contains("attention") { 0 } else { 1 };
                store
                    .save_chunk_embedding(
                        &chunk.id,
                        MODEL_NAME,
                        MODEL_VERSION,
                        CHUNK_VERSION,
                        &axis_vector(axis),
                    )
                    .unwrap();
            }
        }
    }

    fn request(query: &str) -> SearchRequest {
        SearchRequest {
            query: query.to_string(),
            paper_ids: Vec::new(),
            vault_ids: Vec::new(),
            mode: SearchMode::Hybrid,
            limit: None,
        }
    }

    #[tokio::test]
    async fn hybrid_search_finds_the_relevant_paper() {
        let fx = fixture();
        embed_all(&fx.store);
        let service = SearchService::new(fx.store.clone(), Some(Arc::new(AxisEmbedder)));

        let response = service.search(request("attention")).await.expect("search");

        assert!(!response.hits.is_empty());
        assert_eq!(response.hits[0].chunk.paper_id, "attention-paper");
        assert!(matches!(response.semantic, SemanticStatus::Ran { .. }));
    }

    #[tokio::test]
    async fn hybrid_hits_carry_both_signals_when_both_matched() {
        let fx = fixture();
        embed_all(&fx.store);
        let service = SearchService::new(fx.store.clone(), Some(Arc::new(AxisEmbedder)));

        let response = service.search(request("attention")).await.expect("search");
        let top = &response.hits[0];

        // The UI explains a hit from these, which is why they are not collapsed
        // into the single fused score.
        assert!(top.lexical.is_some(), "lexical matched 'attention'");
        assert!(top.semantic.is_some(), "semantic matched the attention axis");
    }

    #[tokio::test]
    async fn scope_narrows_to_one_paper() {
        let fx = fixture();
        embed_all(&fx.store);
        let service = SearchService::new(fx.store.clone(), Some(Arc::new(AxisEmbedder)));

        let mut req = request("attention");
        req.paper_ids = vec!["cooking-paper".to_string()];
        let response = service.search(req).await.expect("search");

        assert_eq!(response.scope.paper_ids, vec!["cooking-paper".to_string()]);
        assert!(
            response
                .hits
                .iter()
                .all(|hit| hit.chunk.paper_id == "cooking-paper"),
            "scope must not leak other papers"
        );
    }

    /// A paper open outside the active vault. Routine UI state, so it reports
    /// rather than throws — and reports *why*, so the caller can tell this from
    /// "nothing matched".
    #[tokio::test]
    async fn a_disjoint_scope_reports_rather_than_failing() {
        let fx = fixture();
        let service = SearchService::new(fx.store.clone(), Some(Arc::new(AxisEmbedder)));

        let mut req = request("attention");
        req.paper_ids = vec!["attention-paper".to_string()];
        req.vault_ids = vec!["nonexistent-vault".to_string()];

        let response = service.search(req).await.expect("must not error");
        assert!(response.hits.is_empty());
        assert_eq!(response.scope.empty_reason, Some(EmptyScope::NoIntersection));
    }

    #[tokio::test]
    async fn lexical_mode_reports_semantic_as_not_requested() {
        let fx = fixture();
        embed_all(&fx.store);
        let service = SearchService::new(fx.store.clone(), Some(Arc::new(AxisEmbedder)));

        let mut req = request("attention");
        req.mode = SearchMode::Lexical;
        let response = service.search(req).await.expect("search");

        assert!(!response.hits.is_empty());
        assert!(matches!(response.semantic, SemanticStatus::NotRequested));
        assert!(response.hits.iter().all(|hit| hit.semantic.is_none()));
    }

    #[tokio::test]
    async fn semantic_mode_finds_what_lexical_cannot() {
        let fx = fixture();
        embed_all(&fx.store);
        let service = SearchService::new(fx.store.clone(), Some(Arc::new(AxisEmbedder)));

        // "attention" appears nowhere in the cooking paper, and this query
        // shares no term with either document — lexical would return nothing.
        let mut req = request("attention");
        req.mode = SearchMode::Semantic;
        let response = service.search(req).await.expect("search");

        assert!(!response.hits.is_empty());
        assert_eq!(response.hits[0].chunk.paper_id, "attention-paper");
        assert!(response.hits.iter().all(|hit| hit.lexical.is_none()));
    }

    /// The case that will actually happen: RFC 0075's startup sweep takes
    /// minutes, and a user opening a PDF in that window must not get quietly
    /// worse results with nothing to explain them.
    #[tokio::test]
    async fn unembedded_chunks_report_no_embeddings_and_still_return_lexical() {
        let fx = fixture(); // deliberately not embedded
        let service = SearchService::new(fx.store.clone(), Some(Arc::new(AxisEmbedder)));

        let response = service.search(request("attention")).await.expect("search");

        assert!(!response.hits.is_empty(), "lexical still answers");
        assert!(matches!(response.semantic, SemanticStatus::NoEmbeddings));
    }

    #[tokio::test]
    async fn no_model_reports_unavailable_and_still_returns_lexical() {
        let fx = fixture();
        let service = SearchService::new(fx.store.clone(), None);

        let response = service.search(request("attention")).await.expect("search");

        assert!(!response.hits.is_empty());
        assert!(matches!(response.semantic, SemanticStatus::Unavailable));
    }

    /// FTS5 MATCH is a query language: `(`, `*`, and bare `AND` are syntax
    /// errors, not searches. Users type these.
    #[tokio::test]
    async fn punctuation_in_a_query_never_errors() {
        let fx = fixture();
        embed_all(&fx.store);
        let service = SearchService::new(fx.store.clone(), Some(Arc::new(AxisEmbedder)));

        for query in ["attention (revised)", "AND", "*", "\"unbalanced", "NEAR x"] {
            service
                .search(request(query))
                .await
                .unwrap_or_else(|error| panic!("query {query:?} errored: {error}"));
        }
    }

    #[tokio::test]
    async fn an_empty_query_returns_nothing_without_searching() {
        let fx = fixture();
        let service = SearchService::new(fx.store.clone(), Some(Arc::new(AxisEmbedder)));

        let response = service.search(request("   ")).await.expect("search");
        assert!(response.hits.is_empty());
    }

    #[tokio::test]
    async fn hits_are_ordered_by_score_after_hydration() {
        let fx = fixture();
        embed_all(&fx.store);
        let service = SearchService::new(fx.store.clone(), Some(Arc::new(AxisEmbedder)));

        let response = service.search(request("attention")).await.expect("search");

        // Hydration loads in table order; the fused ranking must survive it.
        for pair in response.hits.windows(2) {
            assert!(
                pair[0].score >= pair[1].score,
                "hydration reordered the ranking"
            );
        }
    }

    #[tokio::test]
    async fn limit_is_respected() {
        let fx = fixture();
        embed_all(&fx.store);
        let service = SearchService::new(fx.store.clone(), Some(Arc::new(AxisEmbedder)));

        let mut req = request("attention");
        req.limit = Some(1);
        let response = service.search(req).await.expect("search");

        assert_eq!(response.hits.len(), 1);
    }
}

fn summarize_scope(request: &SearchRequest, paper_ids: Vec<String>) -> ScopeSummary {
    let empty_reason = if !paper_ids.is_empty() {
        None
    } else if !request.paper_ids.is_empty() && !request.vault_ids.is_empty() {
        Some(EmptyScope::NoIntersection)
    } else {
        Some(EmptyScope::NoPapers)
    };

    ScopeSummary {
        paper_ids,
        empty_reason,
    }
}
