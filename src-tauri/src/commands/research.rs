//! Tauri commands for deep-research agentic search (RFC 0037).

use crate::domain::research::{Search, SearchCandidate, SearchDraft};
use crate::services::research::manager::SearchManager;
use crate::storage::library_store::LibraryStore;

#[tauri::command]
pub fn create_search(
    store: tauri::State<'_, LibraryStore>,
    draft: SearchDraft,
) -> Result<Search, String> {
    store.create_search(&draft)
}

#[tauri::command]
pub fn list_searches(store: tauri::State<'_, LibraryStore>) -> Result<Vec<Search>, String> {
    store.list_searches()
}

#[tauri::command]
pub fn get_search(
    store: tauri::State<'_, LibraryStore>,
    search_id: String,
) -> Result<Search, String> {
    store.get_search(&search_id)
}

#[tauri::command]
pub fn list_search_candidates(
    store: tauri::State<'_, LibraryStore>,
    search_id: String,
) -> Result<Vec<SearchCandidate>, String> {
    store.list_search_candidates(&search_id)
}

/// Enqueue a run (manual or re-run) and return its run id immediately. The UI
/// then follows progress via `search_updated` events.
#[tauri::command]
pub fn run_search(
    manager: tauri::State<'_, SearchManager>,
    search_id: String,
    mode: Option<String>,
) -> Result<String, String> {
    manager.run_search(search_id, mode)
}

#[tauri::command]
pub fn cancel_search_run(manager: tauri::State<'_, SearchManager>, run_id: String) {
    manager.cancel_run(&run_id);
}

#[tauri::command]
pub fn mark_search_candidate_saved(
    store: tauri::State<'_, LibraryStore>,
    candidate_id: String,
    saved: bool,
) -> Result<(), String> {
    store.mark_search_candidate_saved(&candidate_id, saved)
}

#[tauri::command]
pub fn mark_search_candidates_seen(
    store: tauri::State<'_, LibraryStore>,
    search_id: String,
) -> Result<(), String> {
    store.mark_search_candidates_seen(&search_id)
}
