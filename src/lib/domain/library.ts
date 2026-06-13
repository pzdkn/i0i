import type { Paper } from "$lib/domain/paper";

export type Vault = {
  id: string;
  title: string;
  path: string;
};

export type VaultDraft = {
  path: string;
};

export type VaultRenameDraft = {
  id: string;
  path: string;
};

export type VaultPaper = {
  vaultId: string;
  paperId: string;
};

export type DocumentSource = {
  id: string;
  paperId: string;
  sourceKind: string;
  sourceUrl?: string;
  localPath?: string;
  status: string;
  error?: string;
  createdAt: string;
  updatedAt: string;
};

export type DocumentExtraction = {
  id: string;
  paperId: string;
  sourceId: string;
  extractor: string;
  extractorVersion: string;
  annotationSourceId: string;
  status: string;
  error?: string;
  createdAt: string;
  updatedAt: string;
};

export type DocumentPage = {
  id: string;
  paperId: string;
  sourceId: string;
  extractionId: string;
  pageIndex: number;
  width: number;
  height: number;
};

export type DocumentBlock = {
  id: string;
  paperId: string;
  sourceId: string;
  extractionId: string;
  pageIndex: number;
  blockIndex: number;
  readingOrder: number;
  kind: string;
  text?: string;
  assetId?: string;
  sourceStart?: number;
  sourceEnd?: number;
  bboxJson?: string;
};

export type DocumentSpan = {
  id: string;
  paperId: string;
  sourceId: string;
  extractionId: string;
  blockId: string;
  pageIndex: number;
  text: string;
  sourceStart: number;
  sourceEnd: number;
  bboxJson: string;
};

export type DocumentAsset = {
  id: string;
  paperId: string;
  sourceId: string;
  extractionId: string;
  assetKind: string;
  pageIndex: number;
  bboxJson?: string;
  localPath: string;
  caption?: string;
  createdAt: string;
  updatedAt: string;
};

export type PaperDraft = {
  id: string;
  title: string;
  authors: string[];
  venue: string;
  year: number;
  citations: number;
  tags: string[];
  status: Paper["status"];
  abstract?: string;
  sources?: PaperSourceDraft[];
};

export type PaperSourceDraft = {
  sourceKind: "pdf";
  sourceUrl: string;
};

export type PaperNote = {
  id: string;
  paperId: string;
  sourceId: string;
  startOffset: number;
  endOffset: number;
  selectedText: string;
  anchorKind: "text_offset" | "pdf_rect";
  pageIndex?: number;
  rectsJson?: string;
  quoteContext?: string;
  body: string;
  createdAt: string;
  updatedAt: string;
};

export type PaperNoteDraft = {
  paperId: string;
  sourceId: string;
  startOffset: number;
  endOffset: number;
  selectedText: string;
  anchorKind?: "text_offset" | "pdf_rect";
  pageIndex?: number;
  rectsJson?: string;
  quoteContext?: string;
  body: string;
};

export type LibrarySnapshot = {
  vaults: Vault[];
  papers: Paper[];
  vaultPapers: VaultPaper[];
  documentSources: DocumentSource[];
  documentExtractions: DocumentExtraction[];
  documentPages: DocumentPage[];
  documentBlocks: DocumentBlock[];
  documentSpans: DocumentSpan[];
  documentAssets: DocumentAsset[];
};

export type VaultWorkspace = {
  id: string;
  title: string;
  path: string;
  summary: string;
  tabs: Array<{ label: string; count?: number }>;
  chips: string[];
  papers: Paper[];
};
