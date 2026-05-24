use crate::domain::library::{LibrarySnapshot, PaperDraft};
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
