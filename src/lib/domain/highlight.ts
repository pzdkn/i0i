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
  color: HighlightColor;
  label: string | null;
  author: HighlightAuthor;
  createdAt: string;
  updatedAt: string;
}
