//! Background text extraction for cached PDFs.
//!
//! RFC 0035 keeps this deliberately small: once a PDF is cached, extract plain
//! page text with Pdfium and persist one reader block per page. The Reader and
//! Chat services already know how to consume those rows through `source_text`.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use pdfium_render::prelude::*;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::Semaphore;

use crate::domain::library::{
    DocumentBlock, DocumentExtraction, DocumentPage, DocumentSource, DocumentSpan,
};
use crate::pdf_layout::{group_page, TextFragment};
use crate::storage::library_store::LibraryStore;

const DEFAULT_MAX_CONCURRENT_EXTRACTIONS: usize = 1;
const DEFAULT_PAGE_CAP: usize = 200;
const DEFAULT_TIMEOUT_MS: u64 = 120_000;

/// The extractor's *name* is a component of `document_extraction_id` and
/// `annotation_source_id`, so renaming it does not re-key existing extractions —
/// it mints new ones beside them and orphans the originals, which
/// `stale_document_extractions` then cannot see to clean up. The name stays;
/// only the version moves (RFC 0075 R1).
const EXTRACTOR: &str = "pdfium_basic";

/// 0.2.0 = structural extraction: sub-page blocks, spans, and geometry.
/// Bumping this is the entire migration — `stale_document_extractions` re-queues
/// every extraction below it and `clear_extraction_children` tears down the old
/// rows.
const EXTRACTOR_VERSION: &str = "0.2.0";

/// Characters `"\n\n"` contributes between blocks in the canonical
/// `source_text`. Named because the offset arithmetic below is meaningless
/// without it.
const BLOCK_JOIN_CHARS: i64 = 2;

type ExtractionResult<T> = Result<T, String>;

/// Runtime configuration for the lightweight Pdfium extractor.
///
/// This mirrors `PdfIngestionConfig`: defaults work out of the box, and local
/// overrides can live in `i0i.config.toml` under `[pdf_extraction]`.
#[derive(Debug, Clone)]
pub struct PdfExtractionConfig {
    pub max_concurrent_extractions: usize,
    pub page_cap: usize,
    pub timeout_ms: u64,
    pub pdfium_library_path: Option<PathBuf>,
}

/// Owns the extraction queue and emits lifecycle events.
///
/// The manager is intentionally separate from the Reader: extraction is a
/// background write path, while the Reader is a read path over persisted rows.
#[derive(Clone)]
pub struct PdfExtractionManager {
    app: Option<AppHandle>,
    store: LibraryStore,
    config: PdfExtractionConfig,
    semaphore: Arc<Semaphore>,
    queued_or_active: Arc<Mutex<HashSet<String>>>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentExtractionUpdated {
    pub paper_id: String,
    pub source_id: String,
    pub extraction_id: String,
    pub status: String,
    pub error: Option<String>,
}

impl PdfExtractionConfig {
    pub fn load(app: &AppHandle) -> Self {
        let mut config = Self::default();

        for path in candidate_config_paths(app) {
            if let Ok(contents) = fs::read_to_string(path) {
                config.apply_toml_like_overrides(&contents);
                break;
            }
        }

        if let Ok(path) = std::env::var("I0I_PDFIUM_LIBRARY_PATH") {
            let trimmed = path.trim();
            if !trimmed.is_empty() {
                config.pdfium_library_path = Some(PathBuf::from(trimmed));
            }
        }

        config
    }

    fn apply_toml_like_overrides(&mut self, contents: &str) {
        let mut in_pdf_extraction = false;

        for line in contents.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            if line.starts_with('[') && line.ends_with(']') {
                in_pdf_extraction = line == "[pdf_extraction]";
                continue;
            }

            if !in_pdf_extraction {
                continue;
            }

            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let key = key.trim();
            let value = value.trim().trim_matches('"');

            match key {
                "max_concurrent_extractions" => {
                    if let Ok(value) = value.parse::<usize>() {
                        self.max_concurrent_extractions = value.max(1);
                    }
                }
                "page_cap" => {
                    if let Ok(value) = value.parse::<usize>() {
                        self.page_cap = value.max(1);
                    }
                }
                "timeout_ms" => {
                    if let Ok(value) = value.parse::<u64>() {
                        self.timeout_ms = value.max(1);
                    }
                }
                "pdfium_library_path" => {
                    if !value.is_empty() {
                        self.pdfium_library_path = Some(PathBuf::from(value));
                    }
                }
                _ => {}
            }
        }
    }
}

impl Default for PdfExtractionConfig {
    fn default() -> Self {
        Self {
            max_concurrent_extractions: DEFAULT_MAX_CONCURRENT_EXTRACTIONS,
            page_cap: DEFAULT_PAGE_CAP,
            timeout_ms: DEFAULT_TIMEOUT_MS,
            pdfium_library_path: None,
        }
    }
}

impl PdfExtractionManager {
    pub fn new(app: AppHandle, store: LibraryStore, config: PdfExtractionConfig) -> Self {
        Self {
            app: Some(app),
            store,
            semaphore: Arc::new(Semaphore::new(config.max_concurrent_extractions)),
            config,
            queued_or_active: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    /// Verify that the configured Pdfium runtime can be loaded.
    ///
    /// Pdfium bindings are process-global, so a successful readiness check is
    /// reused by later extraction jobs rather than loading a second copy.
    pub fn verify_runtime(&self) -> Result<(), String> {
        PdfiumBasicAdapter::new(self.app.clone(), self.config.clone())
            .bind_pdfium()
            .map(drop)
    }

    /// Construct an extractor without native-window events for explicit backend evaluation.
    #[cfg(test)]
    pub(crate) fn for_evaluation(store: LibraryStore, config: PdfExtractionConfig) -> Self {
        Self {
            app: None,
            store,
            semaphore: Arc::new(Semaphore::new(config.max_concurrent_extractions)),
            config,
            queued_or_active: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    pub fn queue_source(&self, source_id: String, force: bool) {
        if !self.mark_queued(&source_id) {
            return;
        }

        extraction_log(format!("queued source_id={source_id} force={force}"));
        let manager = self.clone();
        tauri::async_runtime::spawn(async move {
            let source_id_for_cleanup = source_id.clone();
            let result = manager.extract_source(source_id, force).await;
            manager.mark_finished(&source_id_for_cleanup);

            if let Err(error) = result {
                extraction_log(format!(
                    "failed source_id={source_id_for_cleanup} error={error}"
                ));
            }
        });
    }

    pub fn queue_sources(&self, sources: Vec<DocumentSource>, force: bool) {
        for source in sources {
            self.queue_source(source.id, force);
        }
    }

    pub fn queue_paper(
        &self,
        paper_id: String,
        source_id: Option<String>,
        force: bool,
    ) -> ExtractionResult<()> {
        let source = self
            .store
            .resolve_cached_pdf_source(&paper_id, source_id.as_deref())?;
        self.queue_source(source.id, force);
        Ok(())
    }

    pub fn recover_and_queue_startup_extractions(&self) -> ExtractionResult<()> {
        for extraction in self.store.stale_document_extractions(EXTRACTOR)? {
            let extraction = self
                .store
                .reset_document_extraction_to_queued(&extraction.id)?;
            self.emit_update(&extraction);
            self.queue_source(extraction.source_id, false);
        }

        let sources = self
            .store
            .cached_pdf_sources_without_ready_extraction(EXTRACTOR)?;
        self.queue_sources(sources, false);

        // Re-extract anything produced by an older extractor (RFC 0075 R1).
        //
        // This is what makes an `EXTRACTOR_VERSION` bump actually migrate an
        // existing library. Neither sweep above notices a version change: the
        // first looks only at status, and the second skips any source that has
        // *a* ready extraction whatever version made it. Without this, only new
        // papers would get the new extractor.
        //
        // Forced, because the extraction is `ready` — the non-forced path would
        // see that and skip. Forcing deletes the old extraction, and its pages,
        // blocks, spans, chunks, and embeddings cascade away with it. That is
        // correct: all of it is derived from the PDF.
        let outdated = self
            .store
            .outdated_document_extractions(EXTRACTOR, EXTRACTOR_VERSION)?;
        if !outdated.is_empty() {
            extraction_log(format!(
                "re-extracting {} document(s) below version {EXTRACTOR_VERSION}",
                outdated.len()
            ));
        }
        for extraction in outdated {
            self.queue_source(extraction.source_id, true);
        }

        Ok(())
    }

    async fn extract_source(&self, source_id: String, force: bool) -> ExtractionResult<()> {
        let _permit = self
            .semaphore
            .clone()
            .acquire_owned()
            .await
            .map_err(|error| error.to_string())?;

        if !force
            && self
                .store
                .ready_document_extraction_for_source(&source_id, EXTRACTOR)?
                .is_some()
        {
            extraction_log(format!("skip ready source_id={source_id}"));
            return Ok(());
        }

        let extraction = self.store.start_document_extraction(
            &source_id,
            EXTRACTOR,
            EXTRACTOR_VERSION,
            &annotation_source_id(&source_id),
            force,
        )?;
        self.emit_update(&extraction);

        let source = self.store.get_document_source(&source_id)?;
        extraction_log(format!(
            "start source_id={} paper_id={} extraction_id={}",
            source.id, source.paper_id, extraction.id
        ));

        let config = self.config.clone();
        let app = self.app.clone();
        let source_for_job = source.clone();
        let extraction_for_job = extraction.clone();
        let timeout = Duration::from_millis(config.timeout_ms);

        let extracted = match tokio::time::timeout(
            timeout,
            tokio::task::spawn_blocking(move || {
                PdfiumBasicAdapter::new(app, config).extract(&source_for_job, &extraction_for_job)
            }),
        )
        .await
        {
            Ok(joined) => joined.map_err(|error| error.to_string())?,
            Err(_) => Err(format!(
                "PDF extraction timed out after {} ms",
                self.config.timeout_ms
            )),
        };

        match extracted {
            Ok(document) => {
                let extraction = self.store.finish_document_extraction(
                    &extraction.id,
                    &document.pages,
                    &document.blocks,
                    &document.spans,
                )?;
                self.emit_update(&extraction);
                extraction_log(format!(
                    "ready source_id={} extraction_id={} pages={} blocks={}",
                    source.id,
                    extraction.id,
                    document.pages.len(),
                    document.blocks.len()
                ));
                Ok(())
            }
            Err(error) => {
                let extraction = self
                    .store
                    .set_document_extraction_failed(&extraction.id, &error)?;
                self.emit_update(&extraction);
                Err(error)
            }
        }
    }

    fn emit_update(&self, extraction: &DocumentExtraction) {
        let payload = DocumentExtractionUpdated {
            paper_id: extraction.paper_id.clone(),
            source_id: extraction.source_id.clone(),
            extraction_id: extraction.id.clone(),
            status: extraction.status.clone(),
            error: extraction.error.clone(),
        };
        if let Some(app) = &self.app {
            let _ = app.emit("document_extraction_updated", payload);
        }
    }

    fn mark_queued(&self, source_id: &str) -> bool {
        let mut queued_or_active = self
            .queued_or_active
            .lock()
            .expect("extraction queue lock should not be poisoned");
        queued_or_active.insert(source_id.to_string())
    }

    fn mark_finished(&self, source_id: &str) {
        let mut queued_or_active = self
            .queued_or_active
            .lock()
            .expect("extraction queue lock should not be poisoned");
        queued_or_active.remove(source_id);
    }
}

struct ExtractedDocumentRows {
    pages: Vec<DocumentPage>,
    blocks: Vec<DocumentBlock>,
    spans: Vec<DocumentSpan>,
}

struct PdfiumBasicAdapter {
    app: Option<AppHandle>,
    config: PdfExtractionConfig,
}

impl PdfiumBasicAdapter {
    fn new(app: Option<AppHandle>, config: PdfExtractionConfig) -> Self {
        Self { app, config }
    }

    fn extract(
        &self,
        source: &DocumentSource,
        extraction: &DocumentExtraction,
    ) -> ExtractionResult<ExtractedDocumentRows> {
        let local_path = source
            .local_path
            .as_deref()
            .ok_or_else(|| format!("Cached PDF source has no local path: {}", source.id))?;
        let local_path = PathBuf::from(local_path);
        if !local_path.exists() {
            return Err(format!(
                "Cached PDF does not exist: {}",
                local_path.display()
            ));
        }

        let pdfium = self.bind_pdfium()?;
        let document = pdfium
            .load_pdf_from_file(&local_path, None)
            .map_err(|error| format!("Pdfium could not open {}: {error}", local_path.display()))?;

        let mut pages = Vec::new();
        let mut blocks = Vec::new();
        let mut spans = Vec::new();
        let mut source_offset = 0_i64;
        let mut reading_order = 0_i32;

        for (index, page) in document
            .pages()
            .iter()
            .enumerate()
            .take(self.config.page_cap)
        {
            let page_index = i32::try_from(index)
                .map_err(|_| format!("PDF page index is too large: {index}"))?;
            let page_width = page.width().value;
            let page_height = page.height().value;

            pages.push(DocumentPage {
                id: format!("{}:page:{page_index}", extraction.id),
                paper_id: extraction.paper_id.clone(),
                source_id: extraction.source_id.clone(),
                extraction_id: extraction.id.clone(),
                page_index,
                width: f64::from(page_width),
                height: f64::from(page_height),
            });

            let text = page.text().map_err(|error| {
                format!("Pdfium could not read page {page_index} text: {error}")
            })?;
            let fragments = page_fragments(&text);
            let laid_out = group_page(&fragments, page_width, page_height);

            for (block_index, layout) in laid_out.into_iter().enumerate() {
                // The canonical `source_text` is block text joined by "\n\n"
                // (RFC 0075). Every offset below is measured against that
                // string, and in *characters* — the reader's offsets are
                // browser-space, not bytes.
                if !blocks.is_empty() {
                    source_offset += BLOCK_JOIN_CHARS;
                }

                let block_id = format!("{}:block:{page_index}:{block_index}", extraction.id);
                let block_start = source_offset;
                source_offset += layout.text.chars().count() as i64;

                for (span_index, span) in layout.spans.iter().enumerate() {
                    spans.push(DocumentSpan {
                        id: format!("{block_id}:span:{span_index}"),
                        paper_id: extraction.paper_id.clone(),
                        source_id: extraction.source_id.clone(),
                        extraction_id: extraction.id.clone(),
                        block_id: block_id.clone(),
                        page_index,
                        text: span.text.clone(),
                        // Span offsets nest inside their block's range, which
                        // is what lets a retrieved chunk resolve to rectangles.
                        source_start: block_start + span.start as i64,
                        source_end: block_start + span.end as i64,
                        bbox_json: serde_json::to_string(&span.rect)
                            .map_err(|error| format!("span bbox serialize failed: {error}"))?,
                    });
                }

                blocks.push(DocumentBlock {
                    id: block_id,
                    paper_id: extraction.paper_id.clone(),
                    source_id: extraction.source_id.clone(),
                    extraction_id: extraction.id.clone(),
                    page_index,
                    block_index: block_index as i32,
                    reading_order,
                    kind: layout.kind.to_string(),
                    text: Some(layout.text),
                    asset_id: None,
                    source_start: Some(block_start),
                    source_end: Some(source_offset),
                    bbox_json: Some(
                        serde_json::to_string(&layout.rect)
                            .map_err(|error| format!("block bbox serialize failed: {error}"))?,
                    ),
                });
                reading_order += 1;
            }
        }

        Ok(ExtractedDocumentRows {
            pages,
            blocks,
            spans,
        })
    }

    fn bind_pdfium(&self) -> ExtractionResult<Pdfium> {
        let mut errors = Vec::new();
        for candidate in self.pdfium_library_candidates() {
            match bind_pdfium_library(&candidate) {
                Ok(pdfium) => {
                    extraction_log(format!("bound pdfium path={}", candidate.display()));
                    return Ok(pdfium);
                }
                Err(error) => errors.push(format!("{} ({error})", candidate.display())),
            }
        }

        match Pdfium::bind_to_system_library() {
            Ok(bindings) => {
                extraction_log("bound pdfium from system library");
                Ok(Pdfium::new(bindings))
            }
            Err(error) => {
                errors.push(format!("system library ({error})"));
                Err(format!(
                    "Pdfium library not found. Set I0I_PDFIUM_LIBRARY_PATH or [pdf_extraction].pdfium_library_path. Tried: {}",
                    errors.join("; ")
                ))
            }
        }
    }

    fn pdfium_library_candidates(&self) -> Vec<PathBuf> {
        let mut candidates = Vec::new();
        if let Some(path) = &self.config.pdfium_library_path {
            candidates.push(path.clone());
        }

        if let Some(app) = &self.app {
            if let Ok(resource_dir) = app.path().resource_dir() {
                candidates.push(resource_dir.join("libpdfium.dylib"));
                candidates.push(resource_dir.join("pdfium").join("libpdfium.dylib"));
                candidates.push(
                    resource_dir
                        .join("resources/pdfium")
                        .join("libpdfium.dylib"),
                );
            }
        }

        if let Ok(cwd) = std::env::current_dir() {
            candidates.push(cwd.join("resources/pdfium/libpdfium.dylib"));
            if let Some(repo_root) = cwd.parent() {
                candidates.push(repo_root.join("src-tauri/resources/pdfium/libpdfium.dylib"));
            }

            // Developer convenience for the extractor spike environments.
            candidates.push(
                cwd.join("scripts/extractor_spike/.venvs/mineru/lib/python3.12/site-packages/pypdfium2_raw/libpdfium.dylib"),
            );
            candidates.push(
                cwd.join("scripts/extractor_spike/.venvs/docling/lib/python3.12/site-packages/pypdfium2_raw/libpdfium.dylib"),
            );
            candidates.push(
                cwd.join("scripts/extractor_spike/.venvs/marker/lib/python3.12/site-packages/pypdfium2_raw/libpdfium.dylib"),
            );
        }

        candidates
    }
}

/// Pdfium's text segments as layout fragments.
///
/// A segment is a run of text sharing a style and a line, which is exactly the
/// granularity `document_spans` wants. Font size comes from the segment's first
/// character — a segment is a single style run, so one sample is the run.
fn page_fragments(text: &PdfPageText<'_>) -> Vec<TextFragment> {
    text.segments()
        .iter()
        .filter_map(|segment| {
            let content = segment.text();
            if content.trim().is_empty() {
                return None;
            }
            let bounds = segment.bounds();
            Some(TextFragment {
                text: content,
                left: bounds.left().value,
                right: bounds.right().value,
                bottom: bounds.bottom().value,
                top: bounds.top().value,
                font_size: segment_font_size(&segment),
            })
        })
        .collect()
}

/// Falls back to the segment's own height when the character API is
/// unavailable. Height overstates font size slightly (it spans the full glyph
/// box) but every threshold in `pdf_layout` is a *ratio* between sizes on the
/// same page, so a consistent overstatement cancels out.
fn segment_font_size(segment: &PdfPageTextSegment<'_>) -> f32 {
    segment
        .chars()
        .ok()
        .and_then(|chars| chars.iter().next().map(|c| c.scaled_font_size().value))
        .unwrap_or_else(|| {
            let bounds = segment.bounds();
            bounds.top().value - bounds.bottom().value
        })
}

fn bind_pdfium_library(path: &Path) -> ExtractionResult<Pdfium> {
    let library_path = if path.is_dir() {
        Pdfium::pdfium_platform_library_name_at_path(path)
    } else {
        path.to_path_buf()
    };

    if !library_path.exists() {
        return Err("library path does not exist".to_string());
    }

    match Pdfium::bind_to_library(library_path) {
        Ok(bindings) => Ok(Pdfium::new(bindings)),
        Err(PdfiumError::PdfiumLibraryBindingsAlreadyInitialized) => Ok(Pdfium::default()),
        Err(error) => Err(error.to_string()),
    }
}

fn annotation_source_id(source_id: &str) -> String {
    format!("{EXTRACTOR}:{source_id}")
}

fn candidate_config_paths(app: &AppHandle) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Ok(cwd) = std::env::current_dir() {
        paths.push(cwd.join("i0i.config.toml"));
    }
    if let Ok(config_dir) = app.path().app_config_dir() {
        paths.push(config_dir.join("i0i.config.toml"));
    }
    paths
}

fn extraction_log(message: impl AsRef<str>) {
    let timestamp_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0);
    eprintln!("[pdf-extraction {timestamp_ms}] {}", message.as_ref());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_pdfium_can_be_reused_after_first_bind() {
        let path =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources/pdfium/libpdfium.dylib");
        if !path.exists() {
            return;
        }

        bind_pdfium_library(&path)
            .unwrap_or_else(|error| panic!("failed to bind {}: {error}", path.display()));
        bind_pdfium_library(&path)
            .unwrap_or_else(|error| panic!("failed to reuse {}: {error}", path.display()));
    }
}
