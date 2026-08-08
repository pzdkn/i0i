// Mirrors the Rust chat domain types (src-tauri/src/domain/chat.rs).
// `model` and `contextSummary` arrive as null (not omitted) for non-answer
// entries, matching serde's Option serialization.

import type { ContextCitation, PassageRef } from "$lib/domain/context";

export type ChatScope = { kind: "paper"; paperId: string };

export type ThreadAnchor =
  | { kind: "document" }
  | { kind: "textOffset"; sourceId: string; startOffset: number; endOffset: number; selectedText: string }
  | { kind: "pdfRect"; sourceId: string; pageIndex: number; rectsJson: string; selectedText: string };

export type ChatContextSummary = {
  paperTitle: string;
  includedChars: number;
  truncated: boolean;
  /**
   * RFC 0077. These arrive as 0/false/[] for answers written before the
   * ContextManager existed — the Rust side defaults them, so they are never
   * undefined.
   */
  contextItems: number;
  droppedItems: number;
  unresolvedItems: number;
  compacted: boolean;
  /** RFC 0078: the retrieval loop hit a bound and stopped short. */
  retrievalCapped: boolean;
  /** Every passage the model was shown this turn, cited or not (RFC 0078). */
  passages: PassageRef[];
  /** Only the passages the answer actually cited. */
  citations: ContextCitation[];
};

export type ChatEntryKind = "note" | "question" | "answer";

export type ChatEntry = {
  id: string;
  threadId: string;
  kind: ChatEntryKind;
  body: string;
  model: string | null;
  contextSummary: ChatContextSummary | null;
  pinned: boolean;
  createdAt: string;
};

export type ChatThread = {
  id: string;
  anchor: ThreadAnchor;
  title: string;
  createdAt: string;
  updatedAt: string;
};

export type ChatThreadSummary = {
  id: string;
  anchor: ThreadAnchor;
  title: string;
  entryCount: number;
  pinnedCount: number;
  updatedAt: string;
};

export type ChatThreadView = {
  thread: ChatThread;
  entries: ChatEntry[];
};

export type PinnedHighlight = {
  entry: ChatEntry;
  threadTitle: string;
  anchor: ThreadAnchor;
};

export function anchorSelectedText(anchor: ThreadAnchor): string | null {
  return anchor.kind === "document" ? null : anchor.selectedText;
}
