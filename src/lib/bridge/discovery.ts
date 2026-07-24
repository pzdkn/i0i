import { invoke } from "@tauri-apps/api/core";
import type { DiscoverySearchRequest, DiscoverySearchResponse } from "$lib/domain/discover";

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
