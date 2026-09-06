import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  HarnessConfiguration,
  HarnessConfigurationVersion,
  HarnessChangeSet,
  EffectiveInstructionStack,
  HarnessRun,
  HarnessSnapshot,
  Search,
  SearchCandidate,
  SearchCandidatesPreview,
  SearchDraft,
  SearchUpdated,
  RunReconciliationPlan,
  ResearchCheckpoint,
} from "$lib/domain/research";
import type { ResearchStateSnapshot } from "$lib/domain/research-state";

export async function createSearch(draft: SearchDraft): Promise<Search> {
  return invoke<Search>("create_search", { draft });
}

export async function listSearches(): Promise<Search[]> {
  return invoke<Search[]>("list_searches");
}

export async function getSearch(searchId: string): Promise<Search> {
  return invoke<Search>("get_search", { searchId });
}

export async function listSearchCandidates(searchId: string): Promise<SearchCandidate[]> {
  return invoke<SearchCandidate[]>("list_search_candidates", { searchId });
}

/** Enqueue a run; resolves with the run id. Progress arrives via events. */
export async function runSearch(searchId: string, mode = "deep"): Promise<string> {
  return invoke<string>("run_search", { searchId, mode });
}

export async function cancelSearchRun(runId: string): Promise<void> {
  return invoke<void>("cancel_search_run", { runId });
}

export async function getResearchHarness(projectId: string): Promise<HarnessSnapshot> {
  return invoke<HarnessSnapshot>("get_research_harness", { projectId });
}

export async function saveResearchHarness(
  projectId: string,
  configuration: HarnessConfiguration,
): Promise<HarnessSnapshot> {
  return invoke<HarnessSnapshot>("save_research_harness", { projectId, configuration });
}

export async function listHarnessConfigurationVersions(
  projectId: string,
): Promise<HarnessConfigurationVersion[]> {
  return invoke<HarnessConfigurationVersion[]>("list_harness_configuration_versions", {
    projectId,
  });
}

export async function clearHarnessConfigurationHistory(projectId: string): Promise<number> {
  return invoke<number>("clear_harness_configuration_history", { projectId });
}

export async function getHarnessRunInstructions(
  runId: string,
): Promise<EffectiveInstructionStack> {
  return invoke<EffectiveInstructionStack>("get_harness_run_instructions", { runId });
}

export async function getResearchCheckpoint(runId: string): Promise<ResearchCheckpoint> {
  return invoke<ResearchCheckpoint>("get_research_checkpoint", { runId });
}

export async function listResearchCheckpoints(projectId: string): Promise<ResearchCheckpoint[]> {
  return invoke<ResearchCheckpoint[]>("list_research_checkpoints", { projectId });
}

export async function restoreResearchCheckpoint(
  projectId: string,
  runId: string,
  expectedCurrentRevision: number,
): Promise<ResearchStateSnapshot> {
  return invoke<ResearchStateSnapshot>("restore_research_checkpoint", {
    projectId,
    runId,
    expectedCurrentRevision,
  });
}

export async function getHarnessChangeSet(runId: string): Promise<HarnessChangeSet> {
  return invoke<HarnessChangeSet>("get_harness_change_set", { runId });
}

export async function applyHarnessChangeSet(id: string): Promise<HarnessChangeSet> {
  return invoke<HarnessChangeSet>("apply_harness_change_set", { id });
}

export async function rejectHarnessChangeSet(
  id: string,
  reason: string,
): Promise<HarnessChangeSet> {
  return invoke<HarnessChangeSet>("reject_harness_change_set", { id, reason });
}

export async function editHarnessChangeSet(
  id: string,
  plan: RunReconciliationPlan,
): Promise<HarnessChangeSet> {
  return invoke<HarnessChangeSet>("edit_harness_change_set", { id, patch: { plan } });
}

export async function runProjectResearch(projectId: string): Promise<HarnessRun> {
  return invoke<HarnessRun>("run_project_research", { projectId });
}

export async function cancelProjectResearch(projectId: string): Promise<void> {
  return invoke<void>("cancel_project_research", { projectId });
}

export async function pauseResearchHarness(projectId: string): Promise<HarnessSnapshot> {
  return invoke<HarnessSnapshot>("pause_research_harness", { projectId });
}

export async function resumeResearchHarness(projectId: string): Promise<HarnessSnapshot> {
  return invoke<HarnessSnapshot>("resume_research_harness", { projectId });
}

export async function stopResearchHarness(projectId: string): Promise<HarnessSnapshot> {
  return invoke<HarnessSnapshot>("stop_research_harness", { projectId });
}

export async function markSearchCandidatesSeen(searchId: string): Promise<void> {
  return invoke<void>("mark_search_candidates_seen", { searchId });
}

export async function markSearchCandidateSaved(
  candidateId: string,
  saved: boolean,
): Promise<void> {
  return invoke<void>("mark_search_candidate_saved", { candidateId, saved });
}

/** Subscribe to background run progress. Returns an unlisten function. */
export async function listenSearchUpdated(
  onEvent: (payload: SearchUpdated) => void,
): Promise<UnlistenFn> {
  return listen<SearchUpdated>("search_updated", (event) => onEvent(event.payload));
}

/** Subscribe to transient deep-search candidate previews. */
export async function listenSearchCandidatesPreview(
  onEvent: (payload: SearchCandidatesPreview) => void,
): Promise<UnlistenFn> {
  return listen<SearchCandidatesPreview>("search_candidates_preview", (event) =>
    onEvent(event.payload),
  );
}
