import type { ResearchPaper } from "$lib/domain/research";

export type VaultSuggestionOptions = {
  focus: string | null;
  yearFrom: number | null;
  yearTo: number | null;
  queryPathCount: 1 | 3 | 5;
  resultCount: 3 | 5 | 10;
  includeReviews: boolean;
};

export type VaultSuggestionQueryPath = {
  id: string;
  intent: string;
  query: string;
};

export type VaultSuggestionQueryPlan = {
  id: string;
  vaultId: string;
  vaultRevision: string;
  queries: VaultSuggestionQueryPath[];
};

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
  options: VaultSuggestionOptions;
  queryPaths: VaultSuggestionQueryPath[];
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
  sequence: number;
  phase: "planning" | "provider" | "graph" | "filtering" | "ranking" | "complete" | string;
  queryPath?: string;
  label: string;
  detail?: string;
};

export type VaultSuggestionPreview = {
  vaultId: string;
  runId: string;
  suggestions: VaultSuggestion[];
};
