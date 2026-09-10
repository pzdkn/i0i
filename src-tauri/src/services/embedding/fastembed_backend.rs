//! Local ONNX embedding backend (RFC 0054), behind the `embeddings` feature.
//!
//! Wraps `fastembed` (ONNX Runtime + tokenizers) to run a small bi-encoder
//! on-device. The model downloads once into the app cache dir and loads from
//! disk thereafter. This module only compiles when the `embeddings` feature is
//! on, so the rest of the reranker builds without the native ONNX dependency.

use std::path::PathBuf;
use std::sync::OnceLock;

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

/// Defers model download and initialization until semantic ranking first uses it.
///
/// Tauri constructs this lightweight handle during startup. The first call to
/// `embed()` initializes the model on the caller's blocking worker thread, so a
/// missing model never delays creation of the main application window.
pub struct LazyFastEmbedder {
    cache_dir: PathBuf,
    model: OnceLock<Result<FastEmbedder, String>>,
}

impl LazyFastEmbedder {
    /// Create an uninitialized model handle for the application cache.
    pub fn new(cache_dir: PathBuf) -> Self {
        Self {
            cache_dir,
            model: OnceLock::new(),
        }
    }
}

impl TextEmbedder for LazyFastEmbedder {
    fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, String> {
        let model = self
            .model
            .get_or_init(|| {
                eprintln!("[embedding] loading local model on first use");
                FastEmbedder::load(self.cache_dir.clone())
            })
            .as_ref()
            .map_err(Clone::clone)?;
        model.embed(texts)
    }
}

impl TextEmbedder for FastEmbedder {
    fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, String> {
        self.model
            .embed(texts.to_vec(), None)
            .map_err(|error| format!("fastembed embed failed: {error}"))
    }
}
