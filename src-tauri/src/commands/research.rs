//! Tauri commands for deep-research agentic search (RFC 0037).

use crate::domain::harness::{
    EffectiveInstructionStack, HarnessConfiguration, HarnessConfigurationVersion, HarnessRun,
    HarnessRunTrigger, HarnessSnapshot, ResearchCheckpoint,
};
use crate::domain::reconciliation::{HarnessChangeSet, HarnessChangeSetPatch};
use crate::domain::research::{Search, SearchCandidate, SearchDraft};
use crate::domain::research_state::ResearchStateSnapshot;
use crate::services::research::controller::ProjectResearchController;
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

#[tauri::command]
pub fn get_research_harness(
    store: tauri::State<'_, LibraryStore>,
    project_id: String,
) -> Result<HarnessSnapshot, String> {
    store.get_harness_snapshot(&project_id)
}

#[tauri::command]
pub fn save_research_harness(
    store: tauri::State<'_, LibraryStore>,
    project_id: String,
    configuration: HarnessConfiguration,
) -> Result<HarnessSnapshot, String> {
    store.save_harness_configuration(&project_id, &configuration)
}

#[tauri::command]
pub fn list_harness_configuration_versions(
    store: tauri::State<'_, LibraryStore>,
    project_id: String,
) -> Result<Vec<HarnessConfigurationVersion>, String> {
    store.list_harness_configuration_versions(&project_id)
}

#[tauri::command]
pub fn clear_harness_configuration_history(
    store: tauri::State<'_, LibraryStore>,
    project_id: String,
) -> Result<usize, String> {
    store.clear_harness_configuration_history(&project_id)
}

#[tauri::command]
pub fn get_harness_run_instructions(
    store: tauri::State<'_, LibraryStore>,
    run_id: String,
) -> Result<EffectiveInstructionStack, String> {
    store.get_harness_run_instructions(&run_id)
}

#[tauri::command]
/// Loads one persisted Run checkpoint.
pub fn get_research_checkpoint(
    store: tauri::State<'_, LibraryStore>,
    run_id: String,
) -> Result<ResearchCheckpoint, String> {
    store.get_research_checkpoint(&run_id)
}

#[tauri::command]
/// Lists persisted Run checkpoints for a Project.
pub fn list_research_checkpoints(
    store: tauri::State<'_, LibraryStore>,
    project_id: String,
) -> Result<Vec<ResearchCheckpoint>, String> {
    store.list_research_checkpoints(&project_id)
}

#[tauri::command]
/// Appends a new State revision matching the selected same-Project checkpoint.
pub fn restore_research_checkpoint(
    store: tauri::State<'_, LibraryStore>,
    project_id: String,
    run_id: String,
    expected_current_revision: i64,
) -> Result<ResearchStateSnapshot, String> {
    store.restore_research_checkpoint(&project_id, &run_id, expected_current_revision)
}

#[tauri::command]
/// Loads the immutable Project-change record associated with a completed Run.
pub fn get_harness_change_set(
    store: tauri::State<'_, LibraryStore>,
    run_id: String,
) -> Result<HarnessChangeSet, String> {
    store.get_harness_change_set(&run_id)
}

#[tauri::command]
/// Applies a proposed Change Set atomically within the Run's authority snapshot.
pub fn apply_harness_change_set(
    store: tauri::State<'_, LibraryStore>,
    id: String,
) -> Result<HarnessChangeSet, String> {
    store.apply_harness_change_set(&id)
}

#[tauri::command]
/// Retains but rejects a proposed Change Set with a researcher-supplied reason.
pub fn reject_harness_change_set(
    store: tauri::State<'_, LibraryStore>,
    id: String,
    reason: String,
) -> Result<HarnessChangeSet, String> {
    store.reject_harness_change_set(&id, &reason)
}

#[tauri::command]
/// Replaces bounded editable plan fields after full server-side revalidation.
pub fn edit_harness_change_set(
    store: tauri::State<'_, LibraryStore>,
    id: String,
    patch: HarnessChangeSetPatch,
) -> Result<HarnessChangeSet, String> {
    store.edit_harness_change_set(&id, &patch.plan)
}

#[tauri::command]
pub fn run_project_research(
    controller: tauri::State<'_, ProjectResearchController>,
    project_id: String,
) -> Result<HarnessRun, String> {
    controller.start(&project_id, HarnessRunTrigger::Manual, None)
}

#[tauri::command]
pub fn pause_research_harness(
    store: tauri::State<'_, LibraryStore>,
    project_id: String,
) -> Result<HarnessSnapshot, String> {
    store.pause_research_harness(&project_id)
}

#[tauri::command]
pub fn resume_research_harness(
    store: tauri::State<'_, LibraryStore>,
    project_id: String,
) -> Result<HarnessSnapshot, String> {
    store.resume_research_harness(&project_id)
}

#[tauri::command]
pub async fn stop_research_harness(
    store: tauri::State<'_, LibraryStore>,
    controller: tauri::State<'_, ProjectResearchController>,
    project_id: String,
) -> Result<HarnessSnapshot, String> {
    let snapshot = store.stop_research_harness(&project_id)?;
    if snapshot.runs.iter().any(|run| {
        matches!(
            run.status.as_str(),
            "queued" | "planning" | "searching" | "assessing" | "ranking" | "reconciling"
        )
    }) {
        controller.cancel(&project_id).await?;
    }
    Ok(snapshot)
}

#[tauri::command]
pub async fn cancel_project_research(
    controller: tauri::State<'_, ProjectResearchController>,
    project_id: String,
) -> Result<(), String> {
    controller.cancel(&project_id).await
}
