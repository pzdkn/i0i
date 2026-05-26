use crate::domain::library::{
    LibrarySnapshot, PaperDraft, PaperNote, PaperNoteDraft, VaultDraft, VaultRenameDraft,
};
use crate::storage::library_store::LibraryStore;

#[tauri::command]
pub fn get_library(store: tauri::State<'_, LibraryStore>) -> Result<LibrarySnapshot, String> {
    store.get_library()
}

#[tauri::command]
pub fn add_paper_to_vaults(
    store: tauri::State<'_, LibraryStore>,
    paper: PaperDraft,
    vault_ids: Vec<String>,
) -> Result<LibrarySnapshot, String> {
    store.add_paper_to_vaults(&paper, &vault_ids)
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
