use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde::Serialize;
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter, Manager};

use crate::domain::library::{DocumentSource, Paper};
use crate::domain::reader::{
    DiscoveryReaderCandidate, ReaderAsset, ReaderBlock, ReaderDocument, ReaderPage,
    ReaderParagraph, ReaderSpan, ReaderTextBlock,
};
use crate::services::source_acquisition::{
    AcquiredHtml, PdfLocationHints, SourceAcquisitionService,
};
use crate::storage::library_store::LibraryStore;

const DISCOVERY_PDF_SOURCE_PREFIX: &str = "temp-pdf";
const DISCOVERY_HTML_SOURCE_PREFIX: &str = "temp-html";
const DISCOVERY_NO_SOURCE_PREFIX: &str = "temp-meta";
const READER_MAX_PDF_BYTES: u64 = 104_857_600;

/// Reader-side status values for RFC 0051 background PDF acquisition.
const PDF_STATUS_ACQUIRING: &str = "acquiring";

/// Sidecar metadata persisted next to a cached HTML `source.html` so a re-open
/// can rebuild the reader document without re-fetching (RFC 0056).
#[derive(Debug, Clone, Serialize, serde::Deserialize)]
struct HtmlSourceMeta {
    title: Option<String>,
    source_text: String,
    final_url: String,
}

/// A web page persisted as a durable vault snapshot (RFC 0065): the on-disk
/// `source.html` path plus the ingested title/text the paper draft needs.
pub struct StoredHtml {
    pub local_path: String,
    pub acquired: AcquiredHtml,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct PdfAcquisitionProgress {
    source_id: String,
    paper_id: String,
    status: String,
    message: String,
    error: Option<String>,
}

/// Builds Reader documents from either saved papers or transient discovery candidates.
///
/// The Reader UI stays unified by asking this service for the same
/// `ReaderDocument` shape regardless of whether the user opened a library paper
/// or an unsaved discovery result.
#[derive(Clone)]
pub struct ReaderService {
    app: AppHandle,
    store: LibraryStore,
    source_acquisition: SourceAcquisitionService,
    /// In-flight background discovery acquisitions, keyed by source id, so
    /// re-opening a tab never starts a duplicate download and Cancel can
    /// abort the task (RFC 0051).
    acquisitions: Arc<Mutex<HashMap<String, tauri::async_runtime::JoinHandle<()>>>>,
}

enum ReaderTarget<'a> {
    SavedPaper {
        paper_id: &'a str,
        extraction_id: Option<&'a str>,
    },
    DiscoveryCandidate(&'a DiscoveryReaderCandidate),
}

impl ReaderService {
    /// Create a Reader service with access to durable storage and app-local cache paths.
    pub fn new(
        app: AppHandle,
        store: LibraryStore,
        source_acquisition: SourceAcquisitionService,
    ) -> Self {
        Self {
            app,
            store,
            source_acquisition,
            acquisitions: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Load a Reader document for a saved library paper.
    ///
    /// Args:
    ///     paper_id: Durable library paper id.
    ///     extraction_id: Optional extraction override for structured Reader data.
    ///
    /// Returns:
    ///     A `ReaderDocument` backed by durable library state.
    pub async fn get_reader_document(
        &self,
        paper_id: &str,
        extraction_id: Option<&str>,
    ) -> Result<ReaderDocument, String> {
        self.get_reader_document_for_target(ReaderTarget::SavedPaper {
            paper_id,
            extraction_id,
        })
        .await
    }

    /// Load a Reader document directly from a discovery candidate.
    ///
    /// Args:
    ///     candidate: Transient candidate metadata from the Discover UI.
    ///
    /// Returns:
    ///     A `ReaderDocument` that may use temporary app-cache-backed assets.
    pub async fn get_discovery_reader_document(
        &self,
        candidate: &DiscoveryReaderCandidate,
        force: bool,
    ) -> Result<ReaderDocument, String> {
        match self
            .get_reader_document_for_target(ReaderTarget::DiscoveryCandidate(candidate))
            .await
        {
            Ok(mut document) => {
                if force && document.pdf_local_path.is_none() {
                    // Retry bypasses the negative cache in the spawned task.
                    let hints = discovery_location_hints(candidate);
                    if hints.has_any_location() {
                        self.spawn_discovery_pdf_acquisition(
                            candidate,
                            &document.source_id,
                            hints,
                            true,
                        );
                        document.pdf_status = Some(PDF_STATUS_ACQUIRING.to_string());
                    }
                }
                Ok(document)
            }
            Err(error) => Err(error),
        }
    }

    /// Cancel an in-flight background discovery PDF acquisition.
    pub fn cancel_discovery_pdf_acquisition(
        &self,
        source_id: &str,
        paper_id: &str,
    ) -> Result<(), String> {
        let handle = self
            .acquisitions
            .lock()
            .expect("acquisitions lock")
            .remove(source_id);
        if let Some(handle) = handle {
            handle.abort();
            self.emit_acquisition_progress(
                source_id,
                paper_id,
                "failed",
                "PDF download cancelled.",
                Some("Cancelled by user".to_string()),
            );
        }
        Ok(())
    }

    /// Classify PDF availability for a discovery candidate without downloading
    /// it (RFC 0051 discover-time verification).
    pub async fn probe_discovery_candidate_pdf(
        &self,
        candidate: &DiscoveryReaderCandidate,
    ) -> &'static str {
        let hints = discovery_location_hints(candidate);
        if !hints.has_any_location() {
            return "unavailable";
        }
        self.source_acquisition.probe_pdf_availability(&hints).await
    }

    /// Read PDF bytes for a Reader source id.
    ///
    /// Durable source ids resolve through `LibraryStore`. Temporary discovery
    /// source ids resolve through the app cache path derived from the source id.
    pub fn get_reader_pdf_bytes(&self, source_id: &str) -> Result<Vec<u8>, String> {
        // One Reader UI means one byte-loading entrypoint, but the backing file
        // lives either in durable library storage or in temporary discovery cache.
        let local_path = if is_discovery_source_id(source_id) {
            let path = self.discovery_pdf_path(source_id)?;
            if !validate_cached_pdf(&path) {
                return Err(format!(
                    "Cached discovery PDF is missing or invalid: {source_id}"
                ));
            }
            path
        } else {
            let source = self.store.get_document_source(source_id)?;
            if source.source_kind != "pdf" {
                return Err(format!("Document source is not a PDF: {source_id}"));
            }
            if source.status != "cached" {
                return Err(format!(
                    "PDF source is not cached yet: {source_id} ({})",
                    source.status
                ));
            }

            PathBuf::from(
                source
                    .local_path
                    .ok_or_else(|| format!("Cached PDF source has no local path: {source_id}"))?,
            )
        };

        fs::read(&local_path).map_err(|error| {
            format!(
                "Failed to read cached PDF {}: {error}",
                local_path.display()
            )
        })
    }

    /// Open an arbitrary URL as an HTML reader document (RFC 0056): fetch,
    /// extract to clean article HTML, cache it, and return a document the reader
    /// renders in its reading column. Annotation/chat reuse the flow-text
    /// (`TextOffset`) anchor path.
    ///
    /// RFC 0083 R4.1: `force` re-fetches and re-ingests a page already in the
    /// cache. Reference resolution happens at ingest, so a page cached before
    /// that landed keeps its `??` placeholders until it is re-extracted.
    pub async fn open_html_document(
        &self,
        url: &str,
        force: bool,
    ) -> Result<ReaderDocument, String> {
        let source_id = discovery_html_source_id(url);
        let acquired = self.cache_discovery_html(&source_id, url, force).await?;
        Ok(html_reader_document(source_id, url, acquired))
    }

    /// Serve the sanitized HTML for an `html`-kind `ReaderDocument`. Transient
    /// discovery pages (`temp-html:`) come from the app cache (RFC 0056); pages
    /// saved into a vault (`html:`) resolve to their durable snapshot via the
    /// stored `local_path` (RFC 0065).
    pub fn get_reader_html(&self, source_id: &str) -> Result<String, String> {
        if is_discovery_html_source(source_id) {
            let path = self.discovery_html_path(source_id)?;
            return fs::read_to_string(&path)
                .map_err(|error| format!("Cached HTML source is missing: {source_id} ({error})"));
        }
        self.store.read_html_snapshot(source_id)
    }

    /// Fetch + ingest a page into clean article HTML and cache it for serving.
    /// On a cache hit (both `source.html` and its `meta.json` present) the page
    /// is served from disk without re-fetching — parity with the discovery PDF
    /// cache (RFC 0056). Returns the ingested result (title / text) for the doc.
    async fn cache_discovery_html(
        &self,
        source_id: &str,
        url: &str,
        force: bool,
    ) -> Result<AcquiredHtml, String> {
        let local_path = self.discovery_html_path(source_id)?;
        let meta_path = html_meta_path(&local_path);

        // Cache hit: reuse the cached article + its metadata.
        if local_path.is_file() && !force {
            if let Ok(meta_json) = fs::read_to_string(&meta_path) {
                if let Ok(meta) = serde_json::from_str::<HtmlSourceMeta>(&meta_json) {
                    let clean_html = fs::read_to_string(&local_path).unwrap_or_default();
                    return Ok(AcquiredHtml {
                        final_url: meta.final_url,
                        title: meta.title,
                        clean_html,
                        source_text: meta.source_text,
                    });
                }
            }
        }

        let acquired = self
            .source_acquisition
            .acquire_html_page(url)
            .await
            .map_err(|error| error.to_string())?;

        if let Some(parent) = local_path.parent() {
            fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        let partial_path = PathBuf::from(format!("{}.part", local_path.display()));
        fs::write(&partial_path, acquired.clean_html.as_bytes())
            .map_err(|error| error.to_string())?;
        fs::rename(&partial_path, &local_path).map_err(|error| error.to_string())?;
        // Persist metadata so a re-open serves from disk without re-fetching.
        let meta = HtmlSourceMeta {
            title: acquired.title.clone(),
            source_text: acquired.source_text.clone(),
            final_url: acquired.final_url.clone(),
        };
        if let Ok(meta_json) = serde_json::to_string(&meta) {
            let _ = fs::write(&meta_path, meta_json);
        }
        Ok(acquired)
    }

    /// App-cache path for a cached HTML source (mirrors the discovery-PDF path).
    fn discovery_html_path(&self, source_id: &str) -> Result<PathBuf, String> {
        let app_cache_dir = self
            .app
            .path()
            .app_cache_dir()
            .map_err(|error| error.to_string())?;
        Ok(app_cache_dir
            .join("reader")
            .join("discovery")
            .join(sanitize_path_component(source_id))
            .join("source.html"))
    }

    /// Durable (non-cache) snapshot path for a web page saved into a vault
    /// (RFC 0065), mirroring the durable local-PDF layout under app-data.
    fn durable_html_path(&self, paper_id: &str, source_id: &str) -> Result<PathBuf, String> {
        let app_data_dir = self
            .app
            .path()
            .app_data_dir()
            .map_err(|error| error.to_string())?;
        Ok(app_data_dir
            .join("documents")
            .join(paper_id)
            .join("sources")
            .join(sanitize_path_component(source_id))
            .join("source.html"))
    }

    /// Acquire + sanitize a URL and persist it as a **durable** vault snapshot
    /// (RFC 0065). Cache-aware: if a snapshot already exists on disk it is reused
    /// verbatim rather than re-fetched, so re-adding the same page never re-hits
    /// the network and never shifts the text offsets existing annotations anchor
    /// to. Returns the snapshot path plus the ingested title/text for the paper.
    pub async fn acquire_and_store_html(
        &self,
        paper_id: &str,
        source_id: &str,
        url: &str,
    ) -> Result<StoredHtml, String> {
        let local_path = self.durable_html_path(paper_id, source_id)?;
        let meta_path = html_meta_path(&local_path);

        // Reuse an existing snapshot (frozen offsets, no re-fetch).
        if local_path.is_file() {
            if let Ok(meta_json) = fs::read_to_string(&meta_path) {
                if let Ok(meta) = serde_json::from_str::<HtmlSourceMeta>(&meta_json) {
                    return Ok(StoredHtml {
                        local_path: local_path.to_string_lossy().to_string(),
                        acquired: AcquiredHtml {
                            final_url: meta.final_url,
                            title: meta.title,
                            clean_html: fs::read_to_string(&local_path).unwrap_or_default(),
                            source_text: meta.source_text,
                        },
                    });
                }
            }
        }

        let acquired = self
            .source_acquisition
            .acquire_html_page(url)
            .await
            .map_err(|error| error.to_string())?;

        if let Some(parent) = local_path.parent() {
            fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        let partial_path = PathBuf::from(format!("{}.part", local_path.display()));
        fs::write(&partial_path, acquired.clean_html.as_bytes())
            .map_err(|error| error.to_string())?;
        fs::rename(&partial_path, &local_path).map_err(|error| error.to_string())?;
        let meta = HtmlSourceMeta {
            title: acquired.title.clone(),
            source_text: acquired.source_text.clone(),
            final_url: acquired.final_url.clone(),
        };
        if let Ok(meta_json) = serde_json::to_string(&meta) {
            let _ = fs::write(&meta_path, meta_json);
        }
        Ok(StoredHtml {
            local_path: local_path.to_string_lossy().to_string(),
            acquired,
        })
    }

    /// Promote a discovery-cached PDF into the durable document-source layout.
    ///
    /// Returns `Ok(true)` when a matching cached discovery PDF existed and was
    /// attached to the durable source, or `Ok(false)` when there was nothing to
    /// promote and the normal download path should continue.
    pub fn promote_discovery_cached_pdf(&self, source: &DocumentSource) -> Result<bool, String> {
        let Some(source_url) = source.source_url.as_deref() else {
            return Ok(false);
        };

        // The temporary cache key is derived from the paper id plus PDF URL, so
        // "open in Discover" and "save to Vault" can converge on the same file.
        let discovery_source_id = discovery_pdf_source_id(&source.paper_id, source_url);
        let discovery_pdf_path = self.discovery_pdf_path(&discovery_source_id)?;
        if !validate_cached_pdf(&discovery_pdf_path) {
            return Ok(false);
        }

        let durable_pdf_path = self.durable_source_pdf_path(source)?;
        if let Some(parent) = durable_pdf_path.parent() {
            fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }

        fs::copy(&discovery_pdf_path, &durable_pdf_path).map_err(|error| error.to_string())?;
        self.store
            .set_document_source_cached(&source.id, &durable_pdf_path.to_string_lossy())?;
        Ok(true)
    }

    /// Resolve a Reader target into the shared `ReaderDocument` output model.
    async fn get_reader_document_for_target(
        &self,
        target: ReaderTarget<'_>,
    ) -> Result<ReaderDocument, String> {
        match target {
            ReaderTarget::SavedPaper {
                paper_id,
                extraction_id,
            } => self.get_saved_reader_document(paper_id, extraction_id),
            ReaderTarget::DiscoveryCandidate(candidate) => {
                self.get_discovery_candidate_reader_document(candidate)
                    .await
            }
        }
    }

    /// Build a Reader document from durable library state.
    fn get_saved_reader_document(
        &self,
        paper_id: &str,
        extraction_id: Option<&str>,
    ) -> Result<ReaderDocument, String> {
        let snapshot = self.store.get_library()?;

        let paper = snapshot
            .papers
            .into_iter()
            .find(|p| p.id == paper_id)
            .ok_or_else(|| format!("Paper not found: {paper_id}"))?;

        let source = snapshot
            .document_sources
            .into_iter()
            .find(|s| {
                paper.active_source_id.as_deref() == Some(&s.id)
                    || (paper.active_source_id.is_none() && s.paper_id == paper_id)
            })
            .filter(|s| s.status == "cached" || s.status == "remote_available");

        // A web page saved into the vault (RFC 0065) is served as an HTML reader
        // document from its durable snapshot — not through the PDF/extraction
        // path below.
        if let Some(ref s) = source {
            if s.source_kind == "html" {
                return self.saved_html_reader_document(&paper, s);
            }
        }

        let active_extraction = if let Some(id) = extraction_id {
            snapshot
                .document_extractions
                .into_iter()
                .find(|e| e.id == id && e.status == "ready")
        } else {
            // If the caller does not pin an extraction, follow the paper's active
            // extraction first and otherwise fall back to the extraction for the
            // chosen source so older records still remain readable.
            snapshot.document_extractions.into_iter().find(|e| {
                e.status == "ready"
                    && (paper.active_extraction_id.as_deref() == Some(&e.id)
                        || (paper.active_extraction_id.is_none()
                            && source
                                .as_ref()
                                .map(|s| s.id == e.source_id)
                                .unwrap_or(false)))
            })
        };

        let (pages, blocks, spans, assets, source_text) =
            if let Some(ref extraction) = &active_extraction {
                let pages: Vec<ReaderPage> = snapshot
                    .document_pages
                    .iter()
                    .filter(|p| p.extraction_id == extraction.id)
                    .map(|p| ReaderPage {
                        page_index: p.page_index,
                        width: p.width,
                        height: p.height,
                    })
                    .collect();

                // Loaded per-extraction rather than filtered out of the library
                // snapshot (RFC 0075 R2) — this is the one extraction we need,
                // and the snapshot used to carry every paper's.
                let structure = self.store.extraction_structure(&extraction.id)?;

                let blocks: Vec<ReaderBlock> = structure
                    .blocks
                    .into_iter()
                    .map(|b| ReaderBlock {
                        id: b.id,
                        page_index: b.page_index,
                        block_index: b.block_index,
                        reading_order: b.reading_order,
                        kind: b.kind,
                        text: b.text,
                        asset_id: b.asset_id,
                        source_start: b.source_start,
                        source_end: b.source_end,
                        bbox_json: b.bbox_json,
                    })
                    .collect();

                let spans: Vec<ReaderSpan> = structure
                    .spans
                    .into_iter()
                    .map(|s| ReaderSpan {
                        id: s.id,
                        block_id: s.block_id,
                        page_index: s.page_index,
                        text: s.text,
                        source_start: s.source_start,
                        source_end: s.source_end,
                        bbox_json: s.bbox_json,
                    })
                    .collect();

                let assets: Vec<ReaderAsset> = snapshot
                    .document_assets
                    .iter()
                    .filter(|a| a.extraction_id == extraction.id)
                    .cloned()
                    .map(|a| ReaderAsset {
                        id: a.id,
                        paper_id: a.paper_id,
                        source_id: a.source_id,
                        extraction_id: a.extraction_id,
                        asset_kind: a.asset_kind,
                        page_index: a.page_index,
                        bbox_json: a.bbox_json,
                        local_path: a.local_path,
                        caption: a.caption,
                        created_at: a.created_at,
                        updated_at: a.updated_at,
                    })
                    .collect();

                // Structured extraction rows still drive the durable Reader path.
                let source_text: String = blocks
                    .iter()
                    .filter_map(|b| b.text.as_deref())
                    .collect::<Vec<_>>()
                    .join("\n\n");

                (pages, blocks, spans, assets, source_text)
            } else {
                (
                    Vec::new(),
                    Vec::new(),
                    Vec::new(),
                    Vec::new(),
                    String::new(),
                )
            };

        let (source_id, pdf_local_path, pdf_source_url, pdf_error) = if let Some(s) = source {
            (s.id, s.local_path, s.source_url, s.error)
        } else {
            (format!("no-source:{paper_id}"), None, None, None)
        };

        let identifier = format!("{}:{}", paper.id, paper.venue.to_lowercase());
        let citation_key = paper.id.clone();
        let tags = paper.tags.clone();
        let text_blocks = blocks
            .iter()
            .filter_map(reader_text_block_from_block)
            .collect::<Vec<_>>();

        Ok(ReaderDocument {
            paper_id: paper.id,
            source_id,
            extraction_id: active_extraction.as_ref().map(|e| e.id.clone()),
            annotation_source_id: active_extraction
                .as_ref()
                .map(|e| e.annotation_source_id.clone()),
            title: paper.title,
            authors: paper.authors,
            venue: paper.venue,
            year: paper.year,
            identifier,
            citation_key,
            tags,
            pdf_local_path,
            pdf_source_url,
            pdf_error,
            pdf_status: None,
            content_kind: "pdf".to_string(),
            source_url: None,
            source_text,
            pages,
            blocks,
            spans,
            assets,
            text_blocks,
            paragraphs: Vec::new(),
            marks: Vec::new(),
        })
    }

    /// Build an HTML reader document for a web page saved into a vault (RFC
    /// 0065). The sanitized markup is served separately by `get_reader_html`;
    /// here we carry the passage-anchor `source_text` (from the snapshot's
    /// `meta.json`) and the durable source id highlights anchor on. Crucially
    /// `pdf_local_path` stays `None` so the reader picks the HTML surface.
    fn saved_html_reader_document(
        &self,
        paper: &Paper,
        source: &DocumentSource,
    ) -> Result<ReaderDocument, String> {
        let source_text = source
            .local_path
            .as_deref()
            .map(|path| html_meta_path(Path::new(path)))
            .and_then(|meta_path| fs::read_to_string(meta_path).ok())
            .and_then(|json| serde_json::from_str::<HtmlSourceMeta>(&json).ok())
            .map(|meta| meta.source_text)
            .unwrap_or_default();

        let identifier = format!("{}:{}", paper.id, paper.venue.to_lowercase());
        Ok(ReaderDocument {
            paper_id: paper.id.clone(),
            source_id: source.id.clone(),
            extraction_id: None,
            annotation_source_id: None,
            title: paper.title.clone(),
            authors: paper.authors.clone(),
            venue: paper.venue.clone(),
            year: paper.year,
            identifier,
            citation_key: paper.id.clone(),
            tags: paper.tags.clone(),
            pdf_local_path: None,
            pdf_source_url: None,
            pdf_error: None,
            pdf_status: None,
            content_kind: "html".to_string(),
            source_url: source.source_url.clone(),
            source_text,
            pages: Vec::new(),
            blocks: Vec::new(),
            spans: Vec::new(),
            assets: Vec::new(),
            text_blocks: Vec::new(),
            paragraphs: Vec::new(),
            marks: Vec::new(),
        })
    }

    /// Build a Reader document from a transient discovery candidate.
    ///
    /// If the candidate exposes a PDF URL, this path tries to cache a temporary
    /// copy in the app cache so the existing PDF Reader UI can render it.
    async fn get_discovery_candidate_reader_document(
        &self,
        candidate: &DiscoveryReaderCandidate,
    ) -> Result<ReaderDocument, String> {
        let hints = discovery_location_hints(candidate);
        if !hints.has_any_location() {
            let source_id = format!("{DISCOVERY_NO_SOURCE_PREFIX}:{}", candidate.id);
            return Ok(self.discovery_reader_document(candidate, source_id, None, None, None));
        }

        let source_id = discovery_pdf_source_id(&candidate.id, &acquisition_key(&hints));
        let local_path = self.discovery_pdf_path(&source_id)?;
        if validate_cached_pdf(&local_path) {
            return Ok(self.discovery_reader_document(
                candidate,
                source_id,
                Some(local_path.to_string_lossy().to_string()),
                None,
                None,
            ));
        }

        // RFC 0051: return the metadata document immediately and fetch the
        // PDF in the background, so the Reader never blocks on a slow source.
        self.spawn_discovery_pdf_acquisition(candidate, &source_id, hints, false);
        Ok(self.discovery_reader_document(
            candidate,
            source_id,
            None,
            None,
            Some(PDF_STATUS_ACQUIRING.to_string()),
        ))
    }

    /// Start (or join) the background acquisition task for a discovery PDF.
    fn spawn_discovery_pdf_acquisition(
        &self,
        candidate: &DiscoveryReaderCandidate,
        source_id: &str,
        hints: PdfLocationHints,
        force: bool,
    ) {
        let mut tasks = self.acquisitions.lock().expect("acquisitions lock");
        if tasks.contains_key(source_id) {
            return;
        }

        let service = self.clone();
        let task_source_id = source_id.to_string();
        let paper_id = candidate.id.clone();
        let handle = tauri::async_runtime::spawn(async move {
            service.emit_acquisition_progress(
                &task_source_id,
                &paper_id,
                "running",
                "Fetching PDF…",
                None,
            );
            let result = service
                .cache_discovery_pdf(&task_source_id, &hints, force)
                .await;
            service
                .acquisitions
                .lock()
                .expect("acquisitions lock")
                .remove(&task_source_id);
            match result {
                Ok(_) => service.emit_acquisition_progress(
                    &task_source_id,
                    &paper_id,
                    "ready",
                    "PDF ready.",
                    None,
                ),
                Err(error) => service.emit_acquisition_progress(
                    &task_source_id,
                    &paper_id,
                    "failed",
                    "PDF could not be fetched automatically.",
                    Some(error),
                ),
            }
        });
        tasks.insert(source_id.to_string(), handle);
    }

    fn emit_acquisition_progress(
        &self,
        source_id: &str,
        paper_id: &str,
        status: &str,
        message: &str,
        error: Option<String>,
    ) {
        let _ = self.app.emit(
            "reader_pdf_acquisition_progress",
            PdfAcquisitionProgress {
                source_id: source_id.to_string(),
                paper_id: paper_id.to_string(),
                status: status.to_string(),
                message: message.to_string(),
                error,
            },
        );
    }

    /// Adapt a discovery candidate into the standard `ReaderDocument` shape.
    fn discovery_reader_document(
        &self,
        candidate: &DiscoveryReaderCandidate,
        source_id: String,
        pdf_local_path: Option<String>,
        pdf_error: Option<String>,
        pdf_status: Option<String>,
    ) -> ReaderDocument {
        let source_text = candidate.abstract_text.clone().unwrap_or_default();
        let (text_blocks, paragraphs) = text_fallback(&source_text);

        ReaderDocument {
            paper_id: candidate.id.clone(),
            source_id,
            extraction_id: None,
            annotation_source_id: None,
            title: candidate.title.clone(),
            authors: candidate.authors.clone(),
            venue: candidate.venue.clone(),
            year: candidate.year,
            identifier: format!("{}:{}", candidate.id, candidate.venue.to_lowercase()),
            citation_key: candidate.id.clone(),
            tags: candidate.tags.clone(),
            pdf_local_path,
            pdf_source_url: candidate.pdf_url.clone(),
            pdf_error,
            pdf_status,
            content_kind: "pdf".to_string(),
            source_url: candidate.external_url.clone(),
            source_text,
            pages: Vec::new(),
            blocks: Vec::new(),
            spans: Vec::new(),
            assets: Vec::new(),
            text_blocks,
            paragraphs,
            marks: Vec::new(),
        }
    }

    /// Cache a discovery PDF into the app cache area using the same basic
    /// validation rules as the durable PDF ingestion flow.
    async fn cache_discovery_pdf(
        &self,
        source_id: &str,
        hints: &PdfLocationHints,
        force: bool,
    ) -> Result<PathBuf, String> {
        let local_path = self.discovery_pdf_path(source_id)?;
        if validate_cached_pdf(&local_path) {
            return Ok(local_path);
        }

        if let Some(parent) = local_path.parent() {
            fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }

        let partial_path = PathBuf::from(format!("{}.part", local_path.display()));
        let acquired = self
            .source_acquisition
            .acquire_pdf_with_hints(hints, READER_MAX_PDF_BYTES, force)
            .await
            .map_err(|error| error.to_string())?;

        // Write through a temporary file so partial downloads do not look like a
        // valid cached PDF to later Reader opens.
        fs::write(&partial_path, &acquired.bytes).map_err(|error| error.to_string())?;
        fs::rename(&partial_path, &local_path).map_err(|error| error.to_string())?;
        Ok(local_path)
    }

    /// Resolve the app-cache path for a temporary discovery PDF source id.
    fn discovery_pdf_path(&self, source_id: &str) -> Result<PathBuf, String> {
        let app_cache_dir = self
            .app
            .path()
            .app_cache_dir()
            .map_err(|error| error.to_string())?;
        Ok(app_cache_dir
            .join("reader")
            .join("discovery")
            .join(sanitize_path_component(source_id))
            .join("source.pdf"))
    }

    /// Resolve the durable on-disk PDF path for a saved document source.
    fn durable_source_pdf_path(&self, source: &DocumentSource) -> Result<PathBuf, String> {
        let app_data_dir = self
            .app
            .path()
            .app_data_dir()
            .map_err(|error| error.to_string())?;
        Ok(app_data_dir
            .join("documents")
            .join(&source.paper_id)
            .join("sources")
            .join(sanitize_path_component(&source.id))
            .join("source.pdf"))
    }
}

/// Build acquisition hints from everything a discovery candidate knows.
fn discovery_location_hints(candidate: &DiscoveryReaderCandidate) -> PdfLocationHints {
    PdfLocationHints {
        pdf_url: candidate.pdf_url.clone(),
        landing_url: candidate.external_url.clone(),
        doi: candidate.doi.clone(),
        arxiv_id: candidate.arxiv_id.clone(),
    }
}

/// Stable cache key for a hint set: the strongest known location wins, so a
/// candidate keeps the same temporary source id across opens.
fn acquisition_key(hints: &PdfLocationHints) -> String {
    hints
        .pdf_url
        .clone()
        .or_else(|| {
            hints
                .arxiv_id
                .as_ref()
                .map(|id| format!("https://arxiv.org/pdf/{id}"))
        })
        .or_else(|| hints.doi.clone())
        .unwrap_or_default()
}

/// Returns whether a source id belongs to the temporary discovery-reader path.
fn is_discovery_source_id(source_id: &str) -> bool {
    source_id.starts_with(&format!("{DISCOVERY_PDF_SOURCE_PREFIX}:"))
}

/// Derive a stable temporary PDF source id from paper id and source URL.
fn discovery_pdf_source_id(paper_id: &str, pdf_url: &str) -> String {
    let digest = Sha256::digest(pdf_url.as_bytes());
    let suffix = format!("{:x}", digest);
    format!("{DISCOVERY_PDF_SOURCE_PREFIX}:{paper_id}:{}", &suffix[..12])
}

/// The metadata sidecar path next to a cached `source.html` (RFC 0056).
fn html_meta_path(html_path: &Path) -> PathBuf {
    html_path.with_file_name("meta.json")
}

/// Whether a source id belongs to the transient discovery HTML cache (as opposed
/// to a durable vault `html:` source, RFC 0065). Exact prefix — must not match
/// `html:` ids.
fn is_discovery_html_source(source_id: &str) -> bool {
    source_id.starts_with(&format!("{DISCOVERY_HTML_SOURCE_PREFIX}:"))
}

/// Derive a stable temporary HTML source id from a page URL (RFC 0056).
fn discovery_html_source_id(url: &str) -> String {
    let digest = Sha256::digest(url.as_bytes());
    let suffix = format!("{:x}", digest);
    format!("{DISCOVERY_HTML_SOURCE_PREFIX}:{}", &suffix[..12])
}

/// Build an HTML reader document from an ingested page (RFC 0056). It carries no
/// PDF; the reader renders `content_kind = "html"` via `get_reader_html`, and
/// selections anchor as flow-text `TextOffset`s over `source_text`.
fn html_reader_document(source_id: String, url: &str, acquired: AcquiredHtml) -> ReaderDocument {
    let title = acquired
        .title
        .filter(|title| !title.trim().is_empty())
        .unwrap_or_else(|| url.to_string());
    ReaderDocument {
        paper_id: source_id.clone(),
        source_id,
        extraction_id: None,
        annotation_source_id: None,
        title,
        authors: Vec::new(),
        venue: String::new(),
        year: 0,
        identifier: url.to_string(),
        citation_key: String::new(),
        tags: Vec::new(),
        pdf_local_path: None,
        pdf_source_url: None,
        pdf_error: None,
        pdf_status: None,
        content_kind: "html".to_string(),
        source_url: Some(acquired.final_url),
        source_text: acquired.source_text,
        pages: Vec::new(),
        blocks: Vec::new(),
        spans: Vec::new(),
        assets: Vec::new(),
        text_blocks: Vec::new(),
        paragraphs: Vec::new(),
        marks: Vec::new(),
    }
}

/// Convert a structured Reader block into the lighter text-block view model.
fn reader_text_block_from_block(block: &ReaderBlock) -> Option<ReaderTextBlock> {
    let kind = match block.kind.as_str() {
        "title" | "authors" | "heading" | "paragraph" => block.kind.clone(),
        _ => return None,
    };

    Some(ReaderTextBlock {
        id: block.id.clone(),
        kind,
        text: block.text.clone()?,
        source_start: block.source_start?,
        highlight: None,
    })
}

/// Build a minimal text fallback for unsaved discovery candidates.
///
/// The first increment uses abstract text as a lightweight reading surface when
/// no structured extraction exists yet.
fn text_fallback(source_text: &str) -> (Vec<ReaderTextBlock>, Vec<ReaderParagraph>) {
    let trimmed = source_text.trim();
    if trimmed.is_empty() {
        return (Vec::new(), Vec::new());
    }

    (
        vec![ReaderTextBlock {
            id: "p1".to_string(),
            kind: "paragraph".to_string(),
            text: trimmed.to_string(),
            source_start: 0,
            highlight: None,
        }],
        vec![ReaderParagraph {
            id: "p1".to_string(),
            kind: "paragraph".to_string(),
            text: trimmed.to_string(),
            highlight: None,
        }],
    )
}

/// Check whether a cached file looks like a PDF by validating the magic header.
fn validate_cached_pdf(path: &Path) -> bool {
    let Ok(bytes) = fs::read(path) else {
        return false;
    };
    bytes.starts_with(b"%PDF-")
}

/// Replace filesystem-hostile characters in generated cache path components.
fn sanitize_path_component(input: &str) -> String {
    input
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.') {
                character
            } else {
                '_'
            }
        })
        .collect()
}
