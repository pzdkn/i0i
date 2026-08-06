export type HighlightColor = "yellow" | "green" | "blue" | "red" | "purple" | "orange";
export const HIGHLIGHT_COLORS: HighlightColor[] = ["yellow", "green", "blue", "red", "purple", "orange"];

export type Locator =
  | { kind: "textOffset"; sourceId: string; startOffset: number; endOffset: number }
  | { kind: "pdfRect"; sourceId: string; pageIndex: number; rectsJson: string }
  // RFC 0074: a position rather than a range — where a standalone sticky note
  // lives. x/y are normalized to the page box (0..1) so they survive zoom.
  | { kind: "pdfPoint"; sourceId: string; pageIndex: number; x: number; y: number }
  | { kind: "textPoint"; sourceId: string; offset: number };

/// A point-anchored annotation is a sticky note; a range-anchored one is a mark
/// on a passage (RFC 0074). This is the only thing that distinguishes them —
/// there is no `kind` column.
export function isStickyNote(locator: Locator): boolean {
  return locator.kind === "pdfPoint" || locator.kind === "textPoint";
}

export type HighlightAuthor = { kind: "user" } | { kind: "agent"; model: string };

export interface Highlight {
  id: string;
  paperId: string;
  sourceId: string;
  locator: Locator;
  excerpt: string;
  // Optional (RFC 0061): null = an annotated passage with no color mark
  // (note-only or conversation-only).
  color: HighlightColor | null;
  // The passage's note text, if any (RFC 0061) — distinct from its conversation.
  note: string | null;
  label: string | null;
  author: HighlightAuthor;
  createdAt: string;
  updatedAt: string;
}
