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
};

export type PaperNote = {
  id: string;
  paperId: string;
  sourceId: string;
  startOffset: number;
  endOffset: number;
  selectedText: string;
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
  body: string;
};

export type LibrarySnapshot = {
  vaults: Vault[];
  papers: Paper[];
  vaultPapers: VaultPaper[];
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
