//! Local ONNX embedding backend (RFC 0054), behind the `embeddings` feature.
//!
//! Wraps `fastembed` (ONNX Runtime + tokenizers) to run a small bi-encoder
//! on-device. The model downloads once into the app cache dir and loads from
//! disk thereafter. This module only compiles when the `embeddings` feature is
//! on, so the rest of the reranker builds without the native ONNX dependency.

use std::path::PathBuf;

use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};

use super::TextEmbedder;

/// BGE-small-en-v1.5: 384-dim, strong on short title/abstract text, CPU-friendly.
const MODEL: EmbeddingModel = EmbeddingModel::BGESmallENV15;

pub struct FastEmbedder {
    model: TextEmbedding,
}

impl FastEmbedder {
    /// Load (downloading on first use) the model, caching under `cache_dir`.
    pub fn load(cache_dir: PathBuf) -> Result<Self, String> {
        let model = TextEmbedding::try_new(
            InitOptions::new(MODEL)
                .with_cache_dir(cache_dir)
                .with_show_download_progress(true),
        )
        .map_err(|error| format!("fastembed init failed: {error}"))?;
        Ok(Self { model })
    }
}

impl TextEmbedder for FastEmbedder {
    fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, String> {
        self.model
            .embed(texts.to_vec(), None)
            .map_err(|error| format!("fastembed embed failed: {error}"))
    }
}
