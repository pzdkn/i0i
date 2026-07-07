import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  Search,
  SearchCandidate,
  SearchCandidatesPreview,
  SearchDraft,
  SearchUpdated,
} from "$lib/domain/research";

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
export async function runSearch(searchId: string): Promise<string> {
  return invoke<string>("run_search", { searchId });
}

export async function cancelSearchRun(runId: string): Promise<void> {
  return invoke<void>("cancel_search_run", { runId });
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
