use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};
use tauri::Manager;

use crate::domain::library::{
    DocumentSource, LibrarySnapshot, LocalPdfImport, LocalPdfImportResult, PaperDraft, VaultDraft,
    VaultRenameDraft,
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

#[tauri::command]
pub fn autofill_paper_metadata(
    metadata_enrichment: tauri::State<'_, MetadataEnrichmentService>,
    paper_id: String,
) -> Result<(), String> {
    metadata_enrichment.queue_paper(paper_id);
    Ok(())
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
