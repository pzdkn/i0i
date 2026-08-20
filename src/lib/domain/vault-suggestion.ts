import type { ResearchPaper } from "$lib/domain/research";

export type VaultSuggestion = {
  id: string;
  vaultId: string;
  runId: string;
  paperRef: string;
  candidate: ResearchPaper;
  reason: string;
  score: number;
  state: "pending" | "added" | "dismissed";
  createdAt: string;
  updatedAt: string;
};

export type VaultSuggestionRun = {
  id: string;
  vaultId: string;
  status: string;
  message: string;
  stopReason?: string;
  error?: string;
  resultCount: number;
  startedAt?: string;
  finishedAt?: string;
  createdAt: string;
};

export type VaultSuggestionSnapshot = {
  suggestions: VaultSuggestion[];
  latestRun?: VaultSuggestionRun;
};

export type VaultSuggestionUpdated = {
  vaultId: string;
  runId: string;
  status: string;
  message: string;
  found: number;
};

export type VaultSuggestionPreview = {
  vaultId: string;
  runId: string;
  suggestions: VaultSuggestion[];
};
