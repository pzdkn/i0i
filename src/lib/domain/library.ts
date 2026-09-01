import type { Paper } from "$lib/domain/paper";

export type Project = {
  id: string;
  title: string;
  goal: string | null;
};

export type ProjectDraft = {
  title: string;
  goal?: string;
};

export type ProjectRenameDraft = {
  id: string;
  title: string;
};

export type ProjectDocumentSummary = {
  id: string;
  projectId: string;
  title: string;
  format: "markdown";
  harnessWritable: boolean;
  updatedAt: string;
};

export type ProjectDocument = ProjectDocumentSummary & {
  content: string;
  createdFromRunId: string | null;
  createdFromStateRevision: number | null;
  createdAt: string;
};

export type ProjectDocumentUpdate = {
  id: string;
  title: string;
  content: string;
  harnessWritable: boolean;
};

export type Vault = {
  id: string;
  projectId: string;
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

/**
 * A retrieval-sized slice of an extraction (RFC 0075).
 *
 * Derived data — regenerable from blocks and torn down with them.
 * `sourceStart`/`sourceEnd` index the canonical `sourceText`; `blockIds` is the
 * path back to spans and therefore to rectangles on the page.
 */
export type DocumentChunk = {
  id: string;
  paperId: string;
  sourceId: string;
  extractionId: string;
  chunkIndex: number;
  chunker: string;
  chunkVersion: number;
  pageStart: number;
  pageEnd: number;
  headingPath?: string;
  text: string;
  tokenEstimate: number;
  sourceStart: number;
  sourceEnd: number;
  blockIds: string[];
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
  projects: Project[];
  projectDocuments: ProjectDocumentSummary[];
  vaults: Vault[];
  papers: Paper[];
  vaultPapers: VaultPaper[];
  documentSources: DocumentSource[];
  documentExtractions: DocumentExtraction[];
  documentPages: DocumentPage[];
  documentAssets: DocumentAsset[];
};

/// One extraction's blocks and spans, fetched on demand (RFC 0075 R2).
///
/// Deliberately not part of `LibrarySnapshot`: carrying every paper's blocks
/// and spans there meant shipping the full text of the whole library across IPC
/// to render a list of titles, and every consumer narrowed to one extraction
/// anyway.
export type ExtractionStructure = {
  blocks: DocumentBlock[];
  spans: DocumentSpan[];
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

export type ProjectWorkspace = {
  id: string;
  title: string;
  goal: string | null;
  vault: VaultWorkspace;
  documents: ProjectDocumentSummary[];
};
