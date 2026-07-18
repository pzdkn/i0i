export type DiscoveryReaderCandidate = {
  id: string;
  sourceProvider?: string;
  sourceId?: string;
  title: string;
  authors: string[];
  venue: string;
  year: number;
  citations: number;
  tags: string[];
  abstract?: string;
  externalUrl?: string;
  pdfUrl?: string;
  doi?: string;
  arxivId?: string;
};

export type ReaderExtractor = "docling" | "mineru" | "marker" | "grobid" | "pdfium_basic";

export type ReaderPage = {
  pageIndex: number;
  width: number;
  height: number;
};

export type ReaderBlock = {
  id: string;
  pageIndex: number;
  blockIndex: number;
  readingOrder: number;
  kind: "title" | "authors" | "heading" | "paragraph" | "caption" | "figure" | "table" | "equation";
  text?: string;
  assetId?: string;
  sourceStart?: number;
  sourceEnd?: number;
  bbox?: [number, number, number, number];
};

export type ReaderSpan = {
  id: string;
  blockId: string;
  pageIndex: number;
  text: string;
  sourceStart: number;
  sourceEnd: number;
  bbox: [number, number, number, number];
};

export type ReaderAsset = {
  id: string;
  paperId: string;
  sourceId: string;
  extractionId: string;
  kind: "page_image" | "embedded_image" | "figure" | "table" | "equation";
  pageIndex: number;
  bbox?: [number, number, number, number];
  localPath: string;
  caption?: string;
};

export type ExtractedDocument = {
  paperId: string;
  sourceId: string;
  extractionId: string;
  annotationSourceId: string;
  extractor: ReaderExtractor;
  extractorVersion: string;
  sourceText: string;
  pages: ReaderPage[];
  blocks: ReaderBlock[];
  spans: ReaderSpan[];
  assets: ReaderAsset[];
};

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
  anchorKind?: "text_offset" | "pdf_rect";
  pageIndex?: number;
  rectsJson?: string;
  quoteContext?: string;
};

export type PdfRect = {
  x: number;
  y: number;
  width: number;
  height: number;
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
  extractionId?: string;
  annotationSourceId?: string;
  sourceText: string;
  title: string;
  authors: string[];
  venue: string;
  year: number;
  identifier: string;
  citationKey: string;
  tags: string[];
  pdfLocalPath?: string;
  pdfSourceUrl?: string;
  pdfError?: string;
  /** "acquiring" while a background download is in flight (RFC 0051). */
  pdfStatus?: string;
  pages: ReaderPage[];
  blocks: ReaderBlock[];
  spans: ReaderSpan[];
  assets: ReaderAsset[];
  textBlocks: ReaderTextBlock[];
  paragraphs: ReaderParagraph[];
  marks: ReaderMark[];
};
