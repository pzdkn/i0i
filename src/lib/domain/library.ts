import type { Paper } from "$lib/domain/paper";

export type Vault = {
  id: string;
  title: string;
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
