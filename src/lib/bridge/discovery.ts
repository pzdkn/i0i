import { invoke } from "@tauri-apps/api/core";
import type { DiscoverySearchRequest, DiscoverySearchResponse } from "$lib/domain/discover";

export async function searchPapers(request: DiscoverySearchRequest): Promise<DiscoverySearchResponse> {
  return invoke<DiscoverySearchResponse>("search_papers", { request });
}
