// Mirrors the Rust context domain types (src-tauri/src/domain/context.rs).
// RFC 0077.
//
// Only *persistent* context is addressable from here. The current selection and
// page are ephemeral — assembled at ask time and never stored — so there is
// nothing to add or remove for them.

/** A rectangle in reader space: normalized 0..1, origin top-left. */
export type NormRect = { x: number; y: number; width: number; height: number };

export type PageRects = { pageIndex: number; rects: NormRect[] };

/**
 * What a `[C1]` marker in an answer resolves to.
 *
 * Stored with the answer rather than recomputed: handles are assigned per
 * assembly, so the same chunk is `[C3]` in one turn and `[C1]` in the next.
 */
export type ContextCitation = {
  /** `"C1"`, without the brackets. */
  handle: string;
  itemId: string;
  paperId: string;
  pageStart: number;
  headingPath: string | null;
  chunkId: string | null;
  /** JSON-encoded `PageRects[]`. `"[]"` when the blocks carry no geometry. */
  rectsJson: string;
};

/**
 * A passage the model was shown this turn, cited or not (RFC 0078).
 *
 * Distinct from `ContextCitation`, which is only what the answer cited.
 * References stay honest; this powers the "what the agent read" drawer.
 */
export type PassageRef = {
  handle: string;
  paperId: string;
  pageStart: number;
  headingPath: string | null;
  chunkId: string | null;
  cited: boolean;
};

/** A persistent context item, resolved to its text. */
export type ContextItemView = {
  id: string;
  kind: "chunk" | "summary";
  chunkId: string | null;
  paperId: string | null;
  pageStart: number | null;
  headingPath: string | null;
  text: string;
  tokenEstimate: number;
  /** `"user"` or `"agent"` — who put this here (RFC 0078). */
  origin: string;
  /** The chunk no longer resolves — reported rather than silently dropped. */
  unresolved: boolean;
};

/** A raw `chat_context_items` row, as `addChatContext` returns it. */
export type ContextItem = {
  id: string;
  threadId: string;
  position: number;
  kind: string;
  chunkId: string | null;
  paperId: string | null;
  sourceStart: number | null;
  sourceEnd: number | null;
  body: string | null;
  coversThroughEntryId: string | null;
  origin: string;
  tokenEstimate: number;
  createdAt: string;
};

/** Whichever key the caller happens to hold. */
export type ContextKey = { kind: "item"; id: string } | { kind: "chunk"; id: string };

/** Parse a citation's rectangles. Bad or empty geometry yields no rectangles. */
export function citationRects(citation: ContextCitation): PageRects[] {
  try {
    const parsed: unknown = JSON.parse(citation.rectsJson || "[]");
    return Array.isArray(parsed) ? (parsed as PageRects[]) : [];
  } catch {
    return [];
  }
}
