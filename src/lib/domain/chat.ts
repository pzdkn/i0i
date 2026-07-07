// Mirrors the Rust chat domain types (src-tauri/src/domain/chat.rs).
// `model` and `contextSummary` arrive as null (not omitted) for non-answer
// entries, matching serde's Option serialization.

export type ChatScope = { kind: "paper"; paperId: string };

export type ThreadAnchor =
  | { kind: "document" }
  | { kind: "textOffset"; sourceId: string; startOffset: number; endOffset: number; selectedText: string }
  | { kind: "pdfRect"; sourceId: string; pageIndex: number; rectsJson: string; selectedText: string };

export type ChatContextSummary = {
  paperTitle: string;
  includedChars: number;
  truncated: boolean;
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
