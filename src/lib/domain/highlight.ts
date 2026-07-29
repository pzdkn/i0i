export type HighlightColor = "yellow" | "green" | "blue" | "red" | "purple" | "orange";
export const HIGHLIGHT_COLORS: HighlightColor[] = ["yellow", "green", "blue", "red", "purple", "orange"];

export type Locator =
  | { kind: "textOffset"; sourceId: string; startOffset: number; endOffset: number }
  | { kind: "pdfRect"; sourceId: string; pageIndex: number; rectsJson: string };

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
