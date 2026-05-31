use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use reqwest::StatusCode;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::Semaphore;

use crate::domain::library::DocumentSource;
use crate::storage::library_store::LibraryStore;

const DEFAULT_MAX_CONCURRENT_DOWNLOADS: usize = 2;
const DEFAULT_MAX_PDF_BYTES: u64 = 104_857_600;
const DEFAULT_RETRY_INITIAL_BACKOFF_MS: u64 = 5_000;
const DEFAULT_RETRY_MAX_ATTEMPTS: u32 = 3;

type PdfResult<T> = Result<T, String>;

#[derive(Debug, Clone)]
pub struct PdfIngestionConfig {
    pub max_concurrent_downloads: usize,
    pub max_pdf_bytes: u64,
    pub retry_initial_backoff_ms: u64,
    pub retry_max_attempts: u32,
}

#[derive(Clone)]
pub struct PdfDownloadManager {
    app: AppHandle,
    store: LibraryStore,
    config: PdfIngestionConfig,
    client: reqwest::Client,
    semaphore: Arc<Semaphore>,
    queued_or_active: Arc<Mutex<HashSet<String>>>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentSourceUpdated {
    pub paper_id: String,
    pub source_id: String,
    pub status: String,
    pub bytes_downloaded: Option<u64>,
    pub content_length: Option<u64>,
    pub local_path: Option<String>,
    pub error: Option<String>,
}

impl PdfIngestionConfig {
    pub fn load(app: &AppHandle) -> Self {
        let mut config = Self::default();
        let config_paths = candidate_config_paths(app);

        for path in config_paths {
            if let Ok(contents) = fs::read_to_string(path) {
                config.apply_toml_like_overrides(&contents);
                break;
            }
        }

        config
    }

    fn apply_toml_like_overrides(&mut self, contents: &str) {
        let mut in_pdf_ingestion = false;

        for line in contents.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            if line.starts_with('[') && line.ends_with(']') {
                in_pdf_ingestion = line == "[pdf_ingestion]";
                continue;
            }

            if !in_pdf_ingestion {
                continue;
            }

            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let key = key.trim();
            let value = value.trim().trim_matches('"');

            match key {
                "max_concurrent_downloads" => {
                    if let Ok(value) = value.parse::<usize>() {
                        self.max_concurrent_downloads = value.max(1);
                    }
                }
                "max_pdf_bytes" => {
                    if let Ok(value) = value.parse::<u64>() {
                        self.max_pdf_bytes = value;
                    }
                }
                "retry_initial_backoff_ms" => {
                    if let Ok(value) = value.parse::<u64>() {
                        self.retry_initial_backoff_ms = value;
                    }
                }
                "retry_max_attempts" => {
                    if let Ok(value) = value.parse::<u32>() {
                        self.retry_max_attempts = value.max(1);
                    }
                }
                _ => {}
            }
        }
    }
}

impl Default for PdfIngestionConfig {
    fn default() -> Self {
        Self {
            max_concurrent_downloads: DEFAULT_MAX_CONCURRENT_DOWNLOADS,
            max_pdf_bytes: DEFAULT_MAX_PDF_BYTES,
            retry_initial_backoff_ms: DEFAULT_RETRY_INITIAL_BACKOFF_MS,
            retry_max_attempts: DEFAULT_RETRY_MAX_ATTEMPTS,
        }
    }
}

impl PdfDownloadManager {
    pub fn new(app: AppHandle, store: LibraryStore, config: PdfIngestionConfig) -> Self {
        let client = reqwest::Client::builder()
            .user_agent(concat!(
                env!("CARGO_PKG_NAME"),
                "/",
                env!("CARGO_PKG_VERSION")
            ))
            .build()
            .expect("reqwest client should build");
        Self {
            app,
            store,
            semaphore: Arc::new(Semaphore::new(config.max_concurrent_downloads)),
            config,
            client,
            queued_or_active: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    pub fn queue_source(&self, source_id: String) {
        if !self.mark_queued(&source_id) {
            return;
        }

        let manager = self.clone();
        tauri::async_runtime::spawn(async move {
            let source_id_for_cleanup = source_id.clone();
            let result = manager.download_source(source_id).await;
            manager.mark_finished(&source_id_for_cleanup);

            if let Err(error) = result {
                eprintln!("PDF download failed: {error}");
            }
        });
    }

    pub fn queue_sources(&self, sources: Vec<DocumentSource>) {
        for source in sources {
            self.queue_source(source.id);
        }
    }

    pub fn recover_and_queue_startup_downloads(&self) -> PdfResult<()> {
        for source in self.store.stale_downloading_pdf_sources()? {
            self.recover_stale_download(source)?;
        }

        self.queue_sources(self.store.remote_available_pdf_sources()?);

        Ok(())
    }

    fn recover_stale_download(&self, source: DocumentSource) -> PdfResult<()> {
        let Some(local_path) = source.local_path.as_deref() else {
            let source = self
                .store
                .reset_document_source_to_remote_available(&source.id)?;
            self.emit_update(&source, None, None);
            self.queue_source(source.id);
            return Ok(());
        };

        if validate_cached_pdf(Path::new(local_path)) {
            let source = self
                .store
                .set_document_source_cached(&source.id, local_path)?;
            self.emit_update(&source, None, None);
            return Ok(());
        }

        let partial_path = PathBuf::from(format!("{local_path}.part"));
        let _ = fs::remove_file(partial_path);
        let source = self
            .store
            .reset_document_source_to_remote_available(&source.id)?;
        self.emit_update(&source, None, None);
        self.queue_source(source.id);
        Ok(())
    }

    async fn download_source(&self, source_id: String) -> PdfResult<()> {
        let _permit = self
            .semaphore
            .clone()
            .acquire_owned()
            .await
            .map_err(|error| error.to_string())?;

        let source = self.store.get_document_source(&source_id)?;
        if source.status == "cached" {
            return Ok(());
        }
        if source.source_kind != "pdf" {
            return Err(format!("Document source is not a PDF: {source_id}"));
        }

        let source_url = source
            .source_url
            .clone()
            .ok_or_else(|| format!("Document source has no URL: {source_id}"))?;
        let local_path = self.source_pdf_path(&source)?;
        let partial_path = PathBuf::from(format!("{}.part", local_path.display()));

        if let Some(parent) = local_path.parent() {
            fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }

        let source = self.store.set_document_source_downloading(&source_id)?;
        self.emit_update(&source, None, None);

        let mut last_error = None;
        for attempt in 1..=self.config.retry_max_attempts {
            match self
                .download_once(&source_url, &partial_path, &local_path, &source)
                .await
            {
                Ok(source) => {
                    self.emit_update(&source, None, None);
                    return Ok(());
                }
                Err(error) => {
                    last_error = Some(error);
                    let _ = fs::remove_file(&partial_path);

                    if attempt < self.config.retry_max_attempts {
                        let backoff = self.config.retry_initial_backoff_ms * u64::from(attempt);
                        tokio::time::sleep(Duration::from_millis(backoff)).await;
                    }
                }
            }
        }

        let error = last_error.unwrap_or_else(|| "PDF download failed".to_string());
        let source = self.store.set_document_source_failed(&source_id, &error)?;
        self.emit_update(&source, None, None);
        Err(error)
    }

    async fn download_once(
        &self,
        source_url: &str,
        partial_path: &Path,
        local_path: &Path,
        source: &DocumentSource,
    ) -> PdfResult<DocumentSource> {
        let response = self
            .client
            .get(source_url)
            .send()
            .await
            .map_err(|error| error.to_string())?;
        let status = response.status();
        if !status.is_success() {
            return Err(status_error(status));
        }

        let content_length = response.content_length();
        if let Some(content_length) = content_length {
            if content_length > self.config.max_pdf_bytes {
                return Err(format!(
                    "PDF is too large: {content_length} bytes exceeds {} bytes",
                    self.config.max_pdf_bytes
                ));
            }
        }

        self.emit_update(source, Some(0), content_length);

        let bytes = response.bytes().await.map_err(|error| error.to_string())?;
        let bytes_downloaded = bytes.len() as u64;
        if bytes_downloaded > self.config.max_pdf_bytes {
            return Err(format!(
                "PDF is too large: {bytes_downloaded} bytes exceeds {} bytes",
                self.config.max_pdf_bytes
            ));
        }
        if !bytes.starts_with(b"%PDF-") {
            return Err("Downloaded file was not a PDF".to_string());
        }

        fs::write(partial_path, &bytes).map_err(|error| error.to_string())?;
        fs::rename(partial_path, local_path).map_err(|error| error.to_string())?;

        let source = self
            .store
            .set_document_source_cached(&source.id, &local_path.to_string_lossy())?;
        self.emit_update(&source, Some(bytes_downloaded), content_length);

        Ok(source)
    }

    fn source_pdf_path(&self, source: &DocumentSource) -> PdfResult<PathBuf> {
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

    fn emit_update(
        &self,
        source: &DocumentSource,
        bytes_downloaded: Option<u64>,
        content_length: Option<u64>,
    ) {
        let payload = DocumentSourceUpdated {
            paper_id: source.paper_id.clone(),
            source_id: source.id.clone(),
            status: source.status.clone(),
            bytes_downloaded,
            content_length,
            local_path: source.local_path.clone(),
            error: source.error.clone(),
        };
        let _ = self.app.emit("document_source_updated", payload);
    }

    fn mark_queued(&self, source_id: &str) -> bool {
        let mut queued_or_active = self
            .queued_or_active
            .lock()
            .expect("PDF queue lock should not be poisoned");
        queued_or_active.insert(source_id.to_string())
    }

    fn mark_finished(&self, source_id: &str) {
        let mut queued_or_active = self
            .queued_or_active
            .lock()
            .expect("PDF queue lock should not be poisoned");
        queued_or_active.remove(source_id);
    }
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

fn status_error(status: StatusCode) -> String {
    format!("PDF download returned HTTP status {status}")
}

fn validate_cached_pdf(path: &Path) -> bool {
    let Ok(bytes) = fs::read(path) else {
        return false;
    };
    bytes.starts_with(b"%PDF-")
}

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
