use crate::domain::library::{
    DocumentSource, LibrarySnapshot, PaperDraft, PaperNote, PaperNoteDraft, VaultDraft,
    VaultRenameDraft,
};
use crate::pdf_ingestion::PdfDownloadManager;
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
        if !reader_service.promote_discovery_cached_pdf(&source)? {
            queue_sources.push(source);
        }
    }

    pdf_downloads.queue_sources(queue_sources);
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

#[tauri::command]
pub fn get_paper_notes(
    store: tauri::State<'_, LibraryStore>,
    paper_id: String,
) -> Result<Vec<PaperNote>, String> {
    store.get_paper_notes(&paper_id)
}

#[tauri::command]
pub fn create_paper_note(
    store: tauri::State<'_, LibraryStore>,
    draft: PaperNoteDraft,
) -> Result<Vec<PaperNote>, String> {
    store.create_paper_note(&draft)
}

#[tauri::command]
pub fn delete_paper_note(
    store: tauri::State<'_, LibraryStore>,
    paper_id: String,
    note_id: String,
) -> Result<Vec<PaperNote>, String> {
    store.delete_paper_note(&note_id)?;
    store.get_paper_notes(&paper_id)
}

#[tauri::command]
pub fn update_paper_note(
    store: tauri::State<'_, LibraryStore>,
    paper_id: String,
    note_id: String,
    body: String,
) -> Result<Vec<PaperNote>, String> {
    store.update_paper_note(&note_id, &body)?;
    store.get_paper_notes(&paper_id)
}
