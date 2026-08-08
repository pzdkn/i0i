import { invoke } from "@tauri-apps/api/core";
import type { EmbeddingCoverage, SearchRequest, SearchResponse } from "$lib/domain/search";

/**
 * Search chunks across an explicit scope (RFC 0076).
 *
 * A scope that resolves to nothing returns no hits with `scope.emptyReason`
 * set — it does not throw. That case is routine: a paper open outside the
 * active vault.
 */
export async function searchChunks(request: SearchRequest): Promise<SearchResponse> {
  return invoke<SearchResponse>("search_chunks", { request });
}

/** Search one paper — what the reader wants. */
export async function searchPaper(
  query: string,
  paperId: string,
  options: Omit<SearchRequest, "query" | "paperIds"> = {},
): Promise<SearchResponse> {
  return searchChunks({ ...options, query, paperIds: [paperId] });
}

/** Search one vault — what the vault view wants. */
export async function searchVault(
  query: string,
  vaultId: string,
  options: Omit<SearchRequest, "query" | "vaultIds"> = {},
): Promise<SearchResponse> {
  return searchChunks({ ...options, query, vaultIds: [vaultId] });
}

/**
 * How much of a paper is embedded, so a caller can show that semantic search
 * is still catching up rather than silently returning weaker results.
 */
export async function chunkEmbeddingCoverage(paperId: string): Promise<EmbeddingCoverage> {
  return invoke<EmbeddingCoverage>("chunk_embedding_coverage", { paperId });
}
