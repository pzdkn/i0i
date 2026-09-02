//! Project-scoped Research State commands (RFC 0112).

use crate::domain::research_state::{
    EntryLifecycle, ResearchEntryDetail, ResearchEntryDraft, ResearchEntryUpdate,
    ResearchEvidenceCandidate, ResearchStateMutation, ResearchStateSnapshot,
};
use crate::storage::library_store::LibraryStore;

#[tauri::command]
pub fn get_research_state(
    store: tauri::State<'_, LibraryStore>,
    project_id: String,
    revision: Option<i64>,
) -> Result<ResearchStateSnapshot, String> {
    store.get_research_state(&project_id, revision)
}

#[tauri::command]
pub fn get_research_entry(
    store: tauri::State<'_, LibraryStore>,
    entry_id: String,
    revision: Option<i64>,
) -> Result<ResearchEntryDetail, String> {
    store.get_research_entry(&entry_id, revision)
}

#[tauri::command]
pub fn list_research_evidence_candidates(
    store: tauri::State<'_, LibraryStore>,
    project_id: String,
    query: Option<String>,
) -> Result<Vec<ResearchEvidenceCandidate>, String> {
    store.list_research_evidence_candidates(&project_id, query.as_deref())
}

#[tauri::command]
pub fn create_research_entry(
    store: tauri::State<'_, LibraryStore>,
    project_id: String,
    expected_revision: i64,
    draft: ResearchEntryDraft,
) -> Result<ResearchStateMutation, String> {
    store.create_research_entry(&project_id, expected_revision, &draft)
}

#[tauri::command]
pub fn revise_research_entry(
    store: tauri::State<'_, LibraryStore>,
    expected_revision: i64,
    update: ResearchEntryUpdate,
) -> Result<ResearchStateMutation, String> {
    store.revise_research_entry(expected_revision, &update)
}

#[tauri::command]
pub fn set_research_entry_lifecycle(
    store: tauri::State<'_, LibraryStore>,
    entry_id: String,
    expected_revision: i64,
    lifecycle: EntryLifecycle,
    reason: String,
) -> Result<ResearchStateMutation, String> {
    store.set_research_entry_lifecycle(&entry_id, expected_revision, lifecycle, &reason)
}
