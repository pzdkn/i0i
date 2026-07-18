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
  landingUrl?: string;
  finalUrl?: string;
  acquisitionMethod?: string;
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
  landingUrl?: string;
};

export type LocalPdfImport = {
  path: string;
};

export type LocalPdfImportResult = {
  snapshot: LibrarySnapshot;
  importedPaperIds: string[];
};

export type MetadataCandidate = {
  id: string;
  title: string;
  authors: string[];
  venue?: string;
  year?: number;
  doi?: string;
  arxivId?: string;
  abstractText?: string;
  providers: string[];
  confidence: number;
  evidence: string[];
};

/** Manual metadata edit payload (RFC 0049); omitted fields stay untouched. */
export type PaperMetadataUpdate = {
  title?: string;
  authors?: string[];
  venue?: string;
  year?: number;
  abstract?: string;
};

export type MetadataAutofillProgress = {
  paperId: string;
  status: string;
  stage: string;
  message: string;
  candidates: MetadataCandidate[];
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
