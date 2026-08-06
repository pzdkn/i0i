import type { Highlight, Locator } from "$lib/domain/highlight";
import type { ChatThreadSummary, ThreadAnchor } from "$lib/domain/chat";

// Threads and highlights are separate rows with no shared foreign key exposed
// over IPC (`chat_threads.highlight_id` is a backend-internal migration
// artifact — see library_store.rs — not serialized to ChatThreadSummary), so
// pairing a thread with its highlight is done structurally: same source and
// same offsets/rect. RFC 0058's migration gives every anchored thread a
// paired highlight, so in practice this pairs 1:1.
//
// `ThreadAnchor` and `Locator` share the same `kind` tags ("textOffset" /
// "pdfRect") and field names for source/offsets/rect, so one structural
// comparison serves both "does this thread match this highlight" and "does
// this new locator collide with an existing highlight" callers.
type Passage = {
  kind: string;
  sourceId?: string;
  startOffset?: number;
  endOffset?: number;
  pageIndex?: number;
  rectsJson?: string;
  x?: number;
  y?: number;
  offset?: number;
};

export function samePassage(a: Passage, b: Passage): boolean {
  if (a.kind !== b.kind || a.kind === "document" || a.sourceId !== b.sourceId) return false;
  if (a.kind === "textOffset") return a.startOffset === b.startOffset && a.endOffset === b.endOffset;
  if (a.kind === "pdfRect") return a.pageIndex === b.pageIndex && a.rectsJson === b.rectsJson;
  // RFC 0074: sticky notes compare on their exact placement. Two notes a pixel
  // apart are deliberately two notes — but without these arms they would fall
  // through to `false`, and every "does this collide with an existing mark"
  // caller would create a duplicate row instead of reusing one.
  if (a.kind === "pdfPoint") return a.pageIndex === b.pageIndex && a.x === b.x && a.y === b.y;
  if (a.kind === "textPoint") return a.offset === b.offset;
  return false;
}

export function findThreadForHighlight(
  threads: ChatThreadSummary[],
  highlight: Highlight,
): ChatThreadSummary | undefined {
  return threads.find((thread) => samePassage(thread.anchor, highlight.locator));
}

export function findHighlightForThread(
  highlights: Highlight[],
  anchor: ThreadAnchor,
): Highlight | undefined {
  return highlights.find((hl) => samePassage(anchor, hl.locator));
}

// The highlight (if any) whose text-offset range covers `offset` in `sourceId`
// — the click target for a plain click on HTML reader text (RFC 0058 Task 10).
export function findHighlightForOffset(
  highlights: Highlight[],
  sourceId: string,
  offset: number,
): Highlight | undefined {
  return highlights.find(
    (hl) =>
      hl.locator.kind === "textOffset" &&
      hl.locator.sourceId === sourceId &&
      offset >= hl.locator.startOffset &&
      offset < hl.locator.endOffset,
  );
}

export function hasExistingHighlight(highlights: Highlight[], locator: Locator): boolean {
  return highlights.some((hl) => samePassage(hl.locator, locator));
}
