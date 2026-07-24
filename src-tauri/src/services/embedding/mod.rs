//! Local embedding reranker (RFC 0054).
//!
//! Turns a fresh candidate set into semantic-similarity scores against the
//! query, so ranking can sort by meaning rather than substring overlap. This is
//! *reranking*, not vector search: we embed the ~tens of candidates one query
//! returned and score them in-process — no index, no network, nothing stored.
//!
//! The real embedding model lives behind the optional `embeddings` Cargo
//! feature (see `fastembed_backend`). Everything here — the trait, the cosine
//! math, the score assembly, and the "not ready" contract — compiles and is
//! tested regardless of that feature, so the relevance logic ships whether or
//! not the ONNX backend builds in a given environment.

use std::sync::Arc;

use crate::domain::discovery::PaperCandidate;

#[cfg(feature = "embeddings")]
pub mod fastembed_backend;

/// How much of a candidate's abstract feeds the embedding. Titles carry most of
/// the topical signal; a bounded abstract slice adds context without blowing up
/// tokenization cost.
const ABSTRACT_EMBED_CHARS: usize = 400;

/// Build the reranker at app startup. With the `embeddings` feature enabled it
/// loads the local model (caching under `cache_dir`); without the feature, or on
/// any load failure, it returns a disabled reranker and ranking transparently
/// uses legacy weights.
pub fn build(cache_dir: std::path::PathBuf) -> EmbeddingReranker {
    // Borrowed so the parameter is "used" even when the feature is off.
    let _ = &cache_dir;
    #[cfg(feature = "embeddings")]
    match fastembed_backend::FastEmbedder::load(cache_dir) {
        Ok(embedder) => {
            eprintln!("[embedding] local model ready");
            return EmbeddingReranker::with_embedder(Arc::new(embedder));
        }
        Err(error) => {
            eprintln!("[embedding] model load failed; ranking uses legacy weights: {error}");
        }
    }
    EmbeddingReranker::disabled()
}

/// A synchronous text embedder. Implementations are CPU-bound and pure, so the
/// reranker calls them inside `spawn_blocking`.
pub trait TextEmbedder: Send + Sync {
    /// Embed each input into a fixed-width vector. All outputs share one
    /// dimensionality. Returns `Err` on model failure (callers degrade to
    /// no semantic scores).
    fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, String>;
}

/// Reranker handle held on app state. When no embedder is attached (feature off,
/// model missing, or load failed) it reports `is_ready() == false` and produces
/// no scores, which the ranker reads as "use legacy weights".
#[derive(Clone)]
pub struct EmbeddingReranker {
    embedder: Option<Arc<dyn TextEmbedder>>,
}

impl EmbeddingReranker {
    /// A reranker with no model. `semantic_scores` always returns empty.
    pub fn disabled() -> Self {
        Self { embedder: None }
    }

    // Used by the feature-gated fastembed backend and by tests; unused in a
    // feature-off non-test build.
    #[cfg_attr(not(feature = "embeddings"), allow(dead_code))]
    pub fn with_embedder(embedder: Arc<dyn TextEmbedder>) -> Self {
        Self {
            embedder: Some(embedder),
        }
    }

    pub fn is_ready(&self) -> bool {
        self.embedder.is_some()
    }

    /// Score each candidate's semantic similarity to `query`, index-aligned with
    /// `candidates`. Returns an **empty vec** when not ready or on any embedding
    /// failure — the ranker treats an empty slice as "no semantic signal" and
    /// falls back to legacy weights, so ranking never regresses.
    pub async fn semantic_scores(&self, query: &str, candidates: &[PaperCandidate]) -> Vec<f64> {
        // Runtime kill switch (RFC 0055): the compile-time feature governs
        // whether the model is built in; this lets the user turn semantic
        // ranking off without a rebuild. Default on.
        if !crate::services::settings::preference_bool("search.reranker_enabled", true) {
            return Vec::new();
        }
        let Some(embedder) = self.embedder.clone() else {
            return Vec::new();
        };
        if candidates.is_empty() {
            return Vec::new();
        }

        // Index 0 is the query; 1..=N are the candidates, in order.
        let mut texts = Vec::with_capacity(candidates.len() + 1);
        texts.push(query.trim().to_string());
        for candidate in candidates {
            texts.push(candidate_embed_text(candidate));
        }

        // Embedding is CPU-bound; keep it off the async runtime.
        let embeddings = match tokio::task::spawn_blocking(move || embedder.embed(&texts)).await {
            Ok(Ok(embeddings)) => embeddings,
            Ok(Err(error)) => {
                eprintln!("[embedding] embed failed: {error}");
                return Vec::new();
            }
            Err(error) => {
                eprintln!("[embedding] embed task panicked: {error}");
                return Vec::new();
            }
        };

        // A shape mismatch means we can't trust the alignment; drop the signal.
        if embeddings.len() != candidates.len() + 1 {
            return Vec::new();
        }
        let (query_embedding, candidate_embeddings) = embeddings.split_first().unwrap();
        candidate_embeddings
            .iter()
            .map(|embedding| cosine_similarity(query_embedding, embedding).clamp(0.0, 1.0))
            .collect()
    }
}

/// The text we embed for a candidate: title plus a bounded abstract slice.
fn candidate_embed_text(candidate: &PaperCandidate) -> String {
    match candidate.abstract_text.as_deref() {
        Some(abstract_text) if !abstract_text.trim().is_empty() => {
            let mut slice: String = abstract_text
                .trim()
                .chars()
                .take(ABSTRACT_EMBED_CHARS)
                .collect();
            slice.insert(0, ' ');
            slice.insert_str(0, candidate.title.trim());
            slice
        }
        _ => candidate.title.trim().to_string(),
    }
}

/// Cosine similarity of two equal-length vectors, in `[-1, 1]`. Returns 0 for a
/// length mismatch or a zero-magnitude vector (both are "no usable signal").
fn cosine_similarity(a: &[f32], b: &[f32]) -> f64 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let mut dot = 0.0f64;
    let mut norm_a = 0.0f64;
    let mut norm_b = 0.0f64;
    for (x, y) in a.iter().zip(b.iter()) {
        let x = *x as f64;
        let y = *y as f64;
        dot += x * y;
        norm_a += x * x;
        norm_b += y * y;
    }
    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }
    dot / (norm_a.sqrt() * norm_b.sqrt())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::discovery::{CandidateMatch, PaperCandidate};

    /// Deterministic fake: a tiny bag-of-words embedding over a fixed vocab, so
    /// tests exercise the cosine + assembly path without a real model.
    struct BagOfWordsEmbedder;

    const VOCAB: [&str; 6] = ["llm", "language", "model", "protein", "diffusion", "search"];

    impl TextEmbedder for BagOfWordsEmbedder {
        fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, String> {
            Ok(texts
                .iter()
                .map(|text| {
                    let lower = text.to_lowercase();
                    VOCAB
                        .iter()
                        .map(|word| lower.matches(word).count() as f32)
                        .collect()
                })
                .collect())
        }
    }

    fn candidate(title: &str, abstract_text: Option<&str>) -> PaperCandidate {
        PaperCandidate {
            id: title.to_string(),
            source_provider: "test".to_string(),
            source_id: title.to_string(),
            title: title.to_string(),
            authors: Vec::new(),
            abstract_text: abstract_text.map(ToString::to_string),
            year: None,
            publication_date: None,
            venue: None,
            citation_count: None,
            doi: None,
            openalex_id: None,
            arxiv_id: None,
            external_url: None,
            pdf_url: None,
            open_access: None,
            match_summary: CandidateMatch {
                score: None,
                reasons: Vec::new(),
                matched_keywords: Vec::new(),
                from_seed_paper_ids: Vec::new(),
            },
            already_in_library: false,
        }
    }

    #[test]
    fn cosine_of_identical_vectors_is_one() {
        let v = vec![0.3f32, 0.1, 0.9];
        assert!((cosine_similarity(&v, &v) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn cosine_of_orthogonal_vectors_is_zero() {
        assert_eq!(cosine_similarity(&[1.0, 0.0], &[0.0, 1.0]), 0.0);
    }

    #[test]
    fn cosine_handles_length_mismatch_and_zero_vectors() {
        assert_eq!(cosine_similarity(&[1.0, 2.0], &[1.0]), 0.0);
        assert_eq!(cosine_similarity(&[0.0, 0.0], &[1.0, 1.0]), 0.0);
    }

    #[test]
    fn disabled_reranker_is_not_ready_and_scores_nothing() {
        let reranker = EmbeddingReranker::disabled();
        assert!(!reranker.is_ready());
        let scores = tokio_block(reranker.semantic_scores("llm", &[candidate("x", None)]));
        assert!(scores.is_empty());
    }

    #[test]
    fn scores_are_index_aligned_and_rank_topical_match_highest() {
        let reranker = EmbeddingReranker::with_embedder(Arc::new(BagOfWordsEmbedder));
        assert!(reranker.is_ready());
        let candidates = vec![
            candidate(
                "language model search",
                Some("large language model retrieval"),
            ),
            candidate(
                "protein diffusion",
                Some("diffusion models for protein design"),
            ),
        ];
        let scores = tokio_block(reranker.semantic_scores("llm language model", &candidates));
        assert_eq!(scores.len(), 2);
        assert!(
            scores[0] > scores[1],
            "topical match should score higher: {scores:?}"
        );
    }

    #[test]
    fn empty_candidates_produce_empty_scores() {
        let reranker = EmbeddingReranker::with_embedder(Arc::new(BagOfWordsEmbedder));
        let scores = tokio_block(reranker.semantic_scores("llm", &[]));
        assert!(scores.is_empty());
    }

    fn tokio_block<F: std::future::Future>(future: F) -> F::Output {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(future)
    }

    /// Live check of the real ONNX backend: downloads/loads the model and
    /// confirms it produces sane embeddings and ranks a topical match highest.
    /// Ignored (network + model download).
    #[cfg(feature = "embeddings")]
    #[test]
    #[ignore = "downloads the embedding model"]
    fn live_model_ranks_topical_match_highest() {
        let cache_dir = std::env::temp_dir().join("i0i-embed-test");
        let embedder = Arc::new(super::fastembed_backend::FastEmbedder::load(cache_dir).unwrap());

        let vectors = embedder
            .embed(&["dense retrieval with language models".to_string()])
            .unwrap();
        assert!((cosine_similarity(&vectors[0], &vectors[0]) - 1.0).abs() < 1e-5);

        let reranker = EmbeddingReranker::with_embedder(embedder);
        let candidates = vec![
            candidate(
                "Dense passage retrieval for open-domain question answering",
                Some("retrieval with dense vector representations and language models"),
            ),
            candidate(
                "Crystal structure of a bacterial ribosome",
                Some("x-ray crystallography of ribosomal subunits"),
            ),
        ];
        let scores = tokio_block(reranker.semantic_scores("LLM document search", &candidates));
        eprintln!("live semantic scores: {scores:?}");
        assert_eq!(scores.len(), 2);
        assert!(
            scores[0] > scores[1],
            "topical match should win: {scores:?}"
        );
    }
}
