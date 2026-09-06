// Mirrors the Rust chat domain types (src-tauri/src/domain/chat.rs).
// `model` and `contextSummary` arrive as null (not omitted) for non-answer
// entries, matching serde's Option serialization.

import type { ContextCitation, ExternalCitation, PassageRef } from "$lib/domain/context";

export type ChatScope = { kind: "paper"; paperId: string };

export type ThreadAnchor =
  | { kind: "document" }
  | { kind: "textOffset"; sourceId: string; startOffset: number; endOffset: number; selectedText: string }
  | { kind: "pdfRect"; sourceId: string; pageIndex: number; rectsJson: string; selectedText: string }
  | { kind: "sourcePassage"; sourceId: string; pageIndex: number | null; startOffset: number; endOffset: number; selectedText: string };

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
  /** External sources actually cited by this answer. */
  externalCitations: ExternalCitation[];
  /** Outcome of the bounded web lookup attempted for this answer. */
  webLookup: WebLookupOutcome;
  /** Background research runs started by this turn. */
  researchActivities: ResearchActivity[];
  /**
   * RFC 0079: what retrieval searched for this turn, oldest first. Stored with
   * the answer, so a past turn with no references can still be read back.
   */
  retrievalQueries: string[];
  /**
   * RFC 0079: false when the paper has no chunks to retrieve from. The
   * difference between "nothing matched" and "never indexed" — only the second
   * is the reader's to fix. Defaults true for answers written before 0079.
   */
  paperIndexed: boolean;
};

export type WebLookupOutcome =
  | { status: "not_requested" }
  | { status: "succeeded"; sourceCount: number }
  | { status: "no_evidence" }
  | { status: "unavailable"; message: string };

export type ResearchActivity = {
  searchId: string;
  runId: string;
  title: string;
  status: string;
};

export type ChatProgress =
  | { event: "deciding"; turnId: string; threadId: string | null }
  | { event: "searchingPaper"; turnId: string; threadId: string | null; query: string }
  | { event: "searchingLibrary"; turnId: string; threadId: string | null; query: string }
  | { event: "searchingWeb"; turnId: string; threadId: string | null; query: string }
  | { event: "readingSource"; turnId: string; threadId: string | null; title: string }
  | { event: "startingDeepResearch"; turnId: string; threadId: string | null; title: string }
  | { event: "retrieved"; turnId: string; threadId: string | null; count: number };

export type ChatEntryKind = "note" | "question" | "answer";

export type ChatEntry = {
  id: string;
  threadId: string;
  kind: ChatEntryKind;
  body: string;
  model: string | null;
  contextSummary: ChatContextSummary | null;
  pinned: boolean;
  authorKind: "user" | "agent";
  authorId: string | null;
  runId: string | null;
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
