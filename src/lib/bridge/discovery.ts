import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  BrowserRuntimeStatus,
  DiscoveryProgress,
  DiscoverySearchRequest,
  DiscoverySearchResponse,
} from "$lib/domain/discover";

export async function getBrowserRuntimeStatus(): Promise<BrowserRuntimeStatus> {
  return invoke<BrowserRuntimeStatus>("get_browser_runtime_status");
}

export async function retryBrowserRuntime(): Promise<BrowserRuntimeStatus> {
  return invoke<BrowserRuntimeStatus>("retry_browser_runtime");
}

export async function listenBrowserRuntimeStatus(
  onEvent: (payload: BrowserRuntimeStatus) => void,
): Promise<UnlistenFn> {
  return listen<BrowserRuntimeStatus>("browser_runtime_status", (event) => onEvent(event.payload));
}

export async function searchPapers(request: DiscoverySearchRequest): Promise<DiscoverySearchResponse> {
  return invoke<DiscoverySearchResponse>("search_papers", { request });
}

/// Progressive query expansion (RFC 0054): re-runs the query plus cheap-LLM
/// variants and returns the merged, reranked superset. Called after the literal
/// results are already shown.
export async function expandSearch(
  request: DiscoverySearchRequest,
): Promise<DiscoverySearchResponse> {
  return invoke<DiscoverySearchResponse>("expand_search", { request });
}

/** Subscribe to browser, resolver, and ranking progress for Quick Search. */
export async function listenDiscoveryProgress(
  onEvent: (payload: DiscoveryProgress) => void,
): Promise<UnlistenFn> {
  return listen<DiscoveryProgress>("discovery_progress", (event) => onEvent(event.payload));
}
