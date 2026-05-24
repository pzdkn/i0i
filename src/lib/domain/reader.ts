export type ReaderMode = "TEXT" | "PDF" | "SPLIT";

export type ReaderParagraph = {
  id: string;
  kind: "heading" | "paragraph";
  text: string;
  highlight?: "soft" | "strong";
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
  title: string;
  authors: string[];
  venue: string;
  year: number;
  identifier: string;
  citationKey: string;
  tags: string[];
  paragraphs: ReaderParagraph[];
  marks: ReaderMark[];
};
