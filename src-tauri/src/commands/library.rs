use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};

use serde::Serialize;
use sha2::{Digest, Sha256};
use tauri::{Emitter, Manager};

use crate::domain::library::{
    DocumentSource, LibrarySnapshot, LocalPdfImport, LocalPdfImportResult, MetadataCandidate,
    PaperDraft, PaperMetadataUpdate, VaultDraft, VaultRenameDraft,
};
use crate::pdf_ingestion::PdfDownloadManager;
use crate::services::metadata_enrichment::MetadataEnrichmentService;
use crate::services::reader_service::ReaderService;
use crate::storage::library_store::LibraryStore;

#[tauri::command]
pub fn get_library(store: tauri::State<'_, LibraryStore>) -> Result<LibrarySnapshot, String> {
    store.get_library()
}

#[tauri::command]
pub fn add_paper_to_vaults(
    store: tauri::State<'_, LibraryStore>,
    pdf_downloads: tauri::State<'_, PdfDownloadManager>,
    reader_service: tauri::State<'_, ReaderService>,
    paper: PaperDraft,
    vault_ids: Vec<String>,
) -> Result<LibrarySnapshot, String> {
    let mut queue_sources = Vec::new();
    store.add_paper_to_vaults(&paper, &vault_ids)?;
    for source in store
        .get_document_sources(&paper.id)?
        .into_iter()
        .filter(|source| source.status == "remote_available")
    {
        // If the user already opened this paper from Discover, prefer promoting
        // the temporary cached PDF over starting a second network download.
        if reader_service.promote_discovery_cached_pdf(&source)? {
            pdf_downloads.queue_source(source.id);
        } else {
            queue_sources.push(source);
        }
    }

    pdf_downloads.queue_sources(queue_sources);
    store.get_library()
}

#[tauri::command]
pub fn import_local_pdfs(
    app: tauri::AppHandle,
    store: tauri::State<'_, LibraryStore>,
    vault_id: String,
    files: Vec<LocalPdfImport>,
) -> Result<LocalPdfImportResult, String> {
    if files.is_empty() {
        return Err("Choose at least one PDF to import.".to_string());
    }

    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?;
    let mut imported_paper_ids = Vec::new();

    for file in files {
        let source_path = PathBuf::from(file.path);
        validate_pdf_path(&source_path)?;

        let hash = sha256_file(&source_path)?;
        let short_hash = &hash[..12];
        let paper_id = format!("local:{short_hash}");
        let source_id = format!("pdf:{paper_id}:{short_hash}");
        let source_url = format!("local://sha256/{hash}");
        let paper = local_pdf_paper_draft(&source_path, &paper_id);
        let pdf_path = cached_local_pdf_path(&app_data_dir, &paper_id, &source_id);

        if !same_file(&source_path, &pdf_path) {
            let parent = pdf_path.parent().ok_or_else(|| {
                format!(
                    "Could not resolve cache directory for {}",
                    source_path.display()
                )
            })?;
            fs::create_dir_all(parent).map_err(|error| error.to_string())?;
            fs::copy(&source_path, &pdf_path).map_err(|error| {
                format!(
                    "Could not copy {} into i0i document storage: {error}",
                    source_path.display()
                )
            })?;
        }

        store.add_local_pdf_to_vault(
            &paper,
            &vault_id,
            &source_id,
            &source_url,
            &pdf_path.to_string_lossy(),
        )?;
        imported_paper_ids.push(paper_id);
    }

    Ok(LocalPdfImportResult {
        snapshot: store.get_library()?,
        imported_paper_ids,
    })
}

/// Add a web page to a vault by URL (RFC 0065): fetch + sanitize the page, save
/// it as a durable snapshot, and register it as a permanent `html:` paper the
/// reader can annotate. Cache-aware — re-adding the same URL reuses the frozen
/// snapshot rather than re-fetching.
#[tauri::command]
pub async fn import_html_url(
    store: tauri::State<'_, LibraryStore>,
    reader_service: tauri::State<'_, ReaderService>,
    vault_id: String,
    url: String,
) -> Result<LocalPdfImportResult, String> {
    let normalized = normalize_html_url(&url)?;
    let short_hash = sha256_str(&normalized)[..12].to_string();
    // `web:` keeps saved pages in their own id namespace, distinct from the
    // `local:` PDF-import namespace (RFC 0065).
    let paper_id = format!("web:{short_hash}");
    let source_id = format!("html:{short_hash}");

    let stored = reader_service
        .acquire_and_store_html(&paper_id, &source_id, &normalized)
        .await?;
    let paper = html_paper_draft(stored.acquired.title.as_deref(), &normalized, &paper_id);
    store.add_local_html_to_vault(&paper, &vault_id, &source_id, &normalized, &stored.local_path)?;

    Ok(LocalPdfImportResult {
        snapshot: store.get_library()?,
        imported_paper_ids: vec![paper_id],
    })
}

#[tauri::command]
pub fn autofill_paper_metadata(
    metadata_enrichment: tauri::State<'_, MetadataEnrichmentService>,
    paper_id: String,
) -> Result<(), String> {
    metadata_enrichment.queue_paper(paper_id);
    Ok(())
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct PaperMetadataUpdatedEvent {
    paper_id: String,
    status: String,
    error: Option<String>,
}

fn emit_paper_metadata_updated(app: &tauri::AppHandle, paper_id: &str) {
    let _ = app.emit(
        "paper_metadata_updated",
        PaperMetadataUpdatedEvent {
            paper_id: paper_id.to_string(),
            status: "ready".to_string(),
            error: None,
        },
    );
}

#[tauri::command]
pub fn apply_paper_metadata_candidate(
    app: tauri::AppHandle,
    store: tauri::State<'_, LibraryStore>,
    paper_id: String,
    candidate: MetadataCandidate,
) -> Result<LibrarySnapshot, String> {
    // User-approved apply: bypass the needs-review gate that protects
    // worker auto-applies, then notify every open surface (RFC 0049).
    store.apply_paper_metadata_enrichment_with_policy(&paper_id, &candidate.into(), false)?;
    emit_paper_metadata_updated(&app, &paper_id);
    store.get_library()
}

#[tauri::command]
pub fn update_paper_metadata(
    app: tauri::AppHandle,
    store: tauri::State<'_, LibraryStore>,
    paper_id: String,
    update: PaperMetadataUpdate,
) -> Result<LibrarySnapshot, String> {
    store.update_paper_metadata(&paper_id, &update)?;
    emit_paper_metadata_updated(&app, &paper_id);
    store.get_library()
}

#[tauri::command]
pub fn get_document_sources(
    store: tauri::State<'_, LibraryStore>,
    paper_id: String,
) -> Result<Vec<DocumentSource>, String> {
    store.get_document_sources(&paper_id)
}

#[tauri::command]
pub fn download_paper_pdf(
    store: tauri::State<'_, LibraryStore>,
    pdf_downloads: tauri::State<'_, PdfDownloadManager>,
    paper_id: String,
    source_id: Option<String>,
) -> Result<DocumentSource, String> {
    let source = if let Some(source_id) = source_id {
        store.get_document_source(&source_id)?
    } else {
        store
            .get_document_sources(&paper_id)?
            .into_iter()
            .find(|source| {
                source.source_kind == "pdf"
                    && (source.status == "remote_available" || source.status == "failed")
            })
            .ok_or_else(|| format!("No downloadable PDF source found for paper {paper_id}"))?
    };

    if source.status == "failed" {
        let source = store.reset_document_source_to_remote_available(&source.id)?;
        pdf_downloads.queue_source(source.id.clone());
        return Ok(source);
    }

    pdf_downloads.queue_source(source.id.clone());
    Ok(source)
}

#[tauri::command]
pub fn create_vault(
    store: tauri::State<'_, LibraryStore>,
    draft: VaultDraft,
) -> Result<LibrarySnapshot, String> {
    store.create_vault(&draft)
}

#[tauri::command]
pub fn rename_vault(
    store: tauri::State<'_, LibraryStore>,
    draft: VaultRenameDraft,
) -> Result<LibrarySnapshot, String> {
    store.rename_vault(&draft)
}

#[tauri::command]
pub fn delete_vault(
    store: tauri::State<'_, LibraryStore>,
    vault_id: String,
) -> Result<LibrarySnapshot, String> {
    store.delete_vault(&vault_id)
}

#[tauri::command]
pub fn remove_paper_from_vault(
    store: tauri::State<'_, LibraryStore>,
    vault_id: String,
    paper_id: String,
) -> Result<LibrarySnapshot, String> {
    store.remove_paper_from_vault(&vault_id, &paper_id)
}

#[tauri::command]
pub fn delete_paper_globally(
    store: tauri::State<'_, LibraryStore>,
    paper_id: String,
) -> Result<LibrarySnapshot, String> {
    store.delete_paper_globally(&paper_id)
}

fn validate_pdf_path(path: &Path) -> Result<(), String> {
    if !path.is_file() {
        return Err(format!("Not a file: {}", path.display()));
    }

    let has_pdf_extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("pdf"));

    let mut file =
        File::open(path).map_err(|error| format!("Could not open {}: {error}", path.display()))?;
    let mut header = [0_u8; 5];
    file.read_exact(&mut header)
        .map_err(|error| format!("Could not read PDF header from {}: {error}", path.display()))?;

    if has_pdf_extension && &header == b"%PDF-" {
        return Ok(());
    }

    Err(format!(
        "Only PDF files can be imported: {}",
        path.display()
    ))
}

fn sha256_file(path: &Path) -> Result<String, String> {
    let mut file =
        File::open(path).map_err(|error| format!("Could not open {}: {error}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];

    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|error| format!("Could not read {}: {error}", path.display()))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }

    Ok(hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

fn local_pdf_paper_draft(path: &Path, paper_id: &str) -> PaperDraft {
    PaperDraft {
        id: paper_id.to_string(),
        title: title_from_pdf_path(path),
        authors: vec![],
        venue: "Local PDF".to_string(),
        year: 0,
        citations: 0,
        tags: vec!["local".to_string(), "needs-review".to_string()],
        status: "UNREAD".to_string(),
        abstract_text: None,
        sources: vec![],
    }
}

fn title_from_pdf_path(path: &Path) -> String {
    let raw_title = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("Imported PDF");
    let title = raw_title
        .replace(['_', '-'], " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");

    if title.is_empty() {
        "Imported PDF".to_string()
    } else {
        title
    }
}

/// Validate + normalize a page URL for import (RFC 0065). Trims whitespace and
/// strips only the `#fragment` — deliberately no query/trailing-slash
/// canonicalization, so `a` and `a/` remain distinct pages.
fn normalize_html_url(url: &str) -> Result<String, String> {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return Err("Enter a URL to add.".to_string());
    }
    if !(trimmed.starts_with("http://") || trimmed.starts_with("https://")) {
        return Err("Enter a full http(s):// URL.".to_string());
    }
    Ok(trimmed.split('#').next().unwrap_or(trimmed).to_string())
}

fn sha256_str(input: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Draft paper for a saved web page. Title comes from the page (`<title>`),
/// falling back to the host and then the raw URL.
fn html_paper_draft(title: Option<&str>, url: &str, paper_id: &str) -> PaperDraft {
    let title = title
        .map(str::trim)
        .filter(|title| !title.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| host_from_url(url).unwrap_or_else(|| url.to_string()));
    PaperDraft {
        id: paper_id.to_string(),
        title,
        authors: vec![],
        venue: "Web page".to_string(),
        year: 0,
        citations: 0,
        tags: vec!["web".to_string(), "needs-review".to_string()],
        status: "UNREAD".to_string(),
        abstract_text: None,
        sources: vec![],
    }
}

fn host_from_url(url: &str) -> Option<String> {
    let after_scheme = url.split("://").nth(1)?;
    let host = after_scheme.split('/').next()?;
    if host.is_empty() {
        None
    } else {
        Some(host.to_string())
    }
}

fn cached_local_pdf_path(app_data_dir: &Path, paper_id: &str, source_id: &str) -> PathBuf {
    app_data_dir
        .join("documents")
        .join(paper_id)
        .join("sources")
        .join(sanitize_path_component(source_id))
        .join("source.pdf")
}

fn sanitize_path_component(input: &str) -> String {
    input
        .chars()
        .map(|character| match character {
            'A'..='Z' | 'a'..='z' | '0'..='9' | '.' | '-' | '_' => character,
            _ => '_',
        })
        .collect()
}

fn same_file(left: &Path, right: &Path) -> bool {
    match (left.canonicalize(), right.canonicalize()) {
        (Ok(left), Ok(right)) => left == right,
        _ => false,
    }
}
