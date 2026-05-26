export type ReaderMode = "TEXT" | "PDF" | "SPLIT";

export type ReaderParagraph = {
  id: string;
  kind: "heading" | "paragraph";
  text: string;
  highlight?: "soft" | "strong";
};

export type ReaderTextBlock = {
  id: string;
  kind: "title" | "authors" | "heading" | "paragraph";
  text: string;
  sourceStart: number;
  highlight?: "soft" | "strong";
};

export type ReaderTextSelection = {
  sourceId: string;
  startOffset: number;
  endOffset: number;
  selectedText: string;
};

export type ReaderMark = {
  id: string;
  paragraphId: string;
  kind: "note" | "question" | "highlight";
  body: string;
  createdLabel: string;
};

export type ReaderDocument = {
  paperId: string;
  sourceId: string;
  sourceText: string;
  title: string;
  authors: string[];
  venue: string;
  year: number;
  identifier: string;
  citationKey: string;
  tags: string[];
  textBlocks: ReaderTextBlock[];
  paragraphs: ReaderParagraph[];
  marks: ReaderMark[];
};
