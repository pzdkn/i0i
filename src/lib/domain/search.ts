import type { DocumentChunk } from "$lib/domain/library";

/**
 * Search within papers we already hold (RFC 0076).
 *
 * Not deep-research discovery — that finds papers the library does *not* have.
 */
export type SearchMode = "lexical" | "semantic" | "hybrid";

/**
 * `paperIds` and `vaultIds` are AND-ed, each empty meaning unconstrained:
 *
 * | paperIds | vaultIds | scope                                |
 * |----------|----------|--------------------------------------|
 * | `[]`     | `[]`     | the whole library — global search     |
 * | `[p]`    | `[]`     | paper `p`                             |
 * | `[]`     | `[v]`    | every paper in vault `v`              |
 * | `[p]`    | `[v]`    | `p` if it is in `v`, otherwise nothing|
 *
 * "Current paper" and "current vault" are resolved here, by the caller — the
 * backend has no notion of what is open, which is what lets the same service
 * serve the reader, the vault view, global search, and the agent.
 */
export type SearchRequest = {
  query: string;
  paperIds?: string[];
  vaultIds?: string[];
  mode?: SearchMode;
  limit?: number;
};

/** One signal's opinion of one chunk. */
export type SearchSignal = {
  /** 1-based position in that signal's own ranking. */
  rank: number;
  /** Native score: BM25 for lexical, negated distance for semantic. */
  score: number;
};

export type ChunkHit = {
  chunk: DocumentChunk;
  /**
   * The number this response was sorted by — comparable only *within* this
   * response, never across queries or modes. Show `lexical`/`semantic` if you
   * need to explain a hit; do not render this as a percentage.
   */
  score: number;
  lexical?: SearchSignal;
  semantic?: SearchSignal;
};

/** Why a scope resolved to nothing. Not an error — see `SearchResponse`. */
export type EmptyScope = "noIntersection" | "noPapers";

export type ScopeSummary = {
  paperIds: string[];
  emptyReason?: EmptyScope;
};

export type EmbeddingCoverage = {
  chunks: number;
  embedded: number;
};

/**
 * Whether semantic retrieval ran, and how completely.
 *
 * `noEmbeddings` is the one to surface: the startup embedding sweep takes
 * minutes on an existing library, and during that window a hybrid search is
 * really a lexical one.
 */
export type SemanticStatus =
  | { status: "ran"; coverage: EmbeddingCoverage }
  | { status: "unavailable" }
  | { status: "notRequested" }
  | { status: "noEmbeddings" };

export type SearchResponse = {
  hits: ChunkHit[];
  scope: ScopeSummary;
  semantic: SemanticStatus;
};
