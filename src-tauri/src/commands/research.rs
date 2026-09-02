//! Tauri commands for deep-research agentic search (RFC 0037).

use crate::domain::discovery::DiscoveryProviderChoice;
use crate::domain::harness::{
    EffectiveInstructionStack, HarnessConfiguration, HarnessConfigurationVersion, HarnessRun,
    HarnessRunTrigger, HarnessSnapshot, ResearchCheckpoint,
};
use crate::domain::reconciliation::{HarnessChangeSet, HarnessChangeSetPatch};
use crate::domain::research::{Search, SearchCandidate, SearchConstraints, SearchDraft};
use crate::domain::research_state::ResearchStateSnapshot;
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
    store: tauri::State<'_, LibraryStore>,
    manager: tauri::State<'_, SearchManager>,
    project_id: String,
) -> Result<HarnessRun, String> {
    start_project_research(
        &store,
        &manager,
        &project_id,
        HarnessRunTrigger::Manual,
        None,
    )
}

pub(crate) fn start_project_research(
    store: &LibraryStore,
    manager: &SearchManager,
    project_id: &str,
    trigger: HarnessRunTrigger,
    scheduled_for: Option<&str>,
) -> Result<HarnessRun, String> {
    store.ensure_harness_can_start(project_id)?;
    let snapshot = store.get_harness_snapshot(&project_id)?;
    if snapshot.runs.iter().any(|run| {
        matches!(
            run.status.as_str(),
            "queued" | "planning" | "searching" | "assessing" | "ranking" | "reconciling"
        )
    }) {
        return Err("A Research Run is already active for this Project".to_string());
    }
    let configuration = snapshot.harness.configuration;
    if configuration.goal.trim().is_empty() {
        return Err("Research goal cannot be empty".to_string());
    }
    let mut strategy = configuration.depth.budget();
    if let Some(limit) = configuration.stop_conditions.maximum_provider_queries {
        strategy.max_provider_queries = strategy.max_provider_queries.min(limit);
    }
    if let Some(limit) = configuration.stop_conditions.maximum_llm_calls {
        strategy.max_llm_calls = strategy.max_llm_calls.min(limit);
    }
    // Reconciliation may make one plan call and one correction call. Keep
    // those inside the Run ceiling rather than silently spending beyond it.
    strategy.max_llm_calls = strategy.max_llm_calls.saturating_sub(2);
    let search = store.create_search(&SearchDraft {
        title: configuration.goal.trim().to_string(),
        goal: effective_research_goal(&configuration),
        constraints: SearchConstraints {
            year_from: None,
            year_to: None,
            providers: configured_providers(&configuration.sources),
            open_access: false,
            target_count: configuration.paper_budget,
            venues: Vec::new(),
            authors: Vec::new(),
            fields_of_study: Vec::new(),
            seed_paper_ids: Vec::new(),
        },
        strategy,
        schedule: None,
    })?;
    let harness_run =
        store.create_harness_run_with_trigger(project_id, &search.id, trigger, scheduled_for)?;
    match manager.run_harness_search_with_timeout(
        search.id,
        Some("project_harness".to_string()),
        configuration.stop_conditions.maximum_run_seconds,
        &harness_run.id,
    ) {
        Ok(run) => Ok(run),
        Err(error) => {
            store.fail_harness_run(&harness_run.id, &error)?;
            Err(error)
        }
    }
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
pub fn stop_research_harness(
    store: tauri::State<'_, LibraryStore>,
    manager: tauri::State<'_, SearchManager>,
    project_id: String,
) -> Result<HarnessSnapshot, String> {
    let snapshot = store.stop_research_harness(&project_id)?;
    if let Some(run) = snapshot.runs.iter().find(|run| {
        matches!(
            run.status.as_str(),
            "queued" | "planning" | "searching" | "assessing" | "ranking" | "reconciling"
        )
    }) {
        if let Some(search_run_id) = run.search_run_id.as_deref() {
            manager.cancel_run(search_run_id);
        }
    }
    Ok(snapshot)
}

#[tauri::command]
pub fn cancel_project_research(
    store: tauri::State<'_, LibraryStore>,
    manager: tauri::State<'_, SearchManager>,
    project_id: String,
) -> Result<(), String> {
    let snapshot = store.get_harness_snapshot(&project_id)?;
    let active = snapshot
        .runs
        .iter()
        .find(|run| {
            matches!(
                run.status.as_str(),
                "queued" | "planning" | "searching" | "assessing" | "ranking" | "reconciling"
            )
        })
        .ok_or_else(|| "No Research Run is active for this Project".to_string())?;
    let search_run_id = active
        .search_run_id
        .as_deref()
        .ok_or_else(|| "Research Run has not started its search yet".to_string())?;
    manager.cancel_run(search_run_id);
    Ok(())
}

fn configured_providers(sources: &[String]) -> Vec<DiscoveryProviderChoice> {
    let mut providers = Vec::new();
    if sources.iter().any(|source| source == "open_alex") {
        providers.push(DiscoveryProviderChoice::OpenAlex);
    }
    if sources.iter().any(|source| source == "arxiv") {
        providers.push(DiscoveryProviderChoice::Arxiv);
    }
    providers
}

fn effective_research_goal(configuration: &HarnessConfiguration) -> String {
    format!(
        "Goal: {}\nScope: {}\nResearch exclusions: {}\nResearcher instructions: {}\nPreferred concepts: {}\nExcluded query concepts: {}",
        configuration.goal.trim(),
        configuration.scope.trim(),
        configuration.exclusions.trim(),
        configuration.research_instructions.trim(),
        configuration.preferred_concepts.join(", "),
        configuration.excluded_concepts.join(", ")
    )
}
