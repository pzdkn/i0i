export type ResearchDocumentShape =
  | "survey"
  | "related_work"
  | "research_gap_analysis"
  | "hypothesis_report"
  | "experiment_plan"
  | "custom";

export type CreateFromResearchRequest = {
  projectId: string;
  stateRevision: number;
  selectedEntryIds: string[];
  shape: ResearchDocumentShape;
  title: string;
  customInstruction?: string;
  originatingRunId?: string;
  includeNonActive: boolean;
};

export type ResearchDocumentGeneration = {
  id: string;
  projectId: string;
  status: "queued" | "generating" | "ready" | "failed" | "cancelled";
  shape: ResearchDocumentShape;
  title: string;
  customInstruction: string | null;
  stateRevision: number;
  selectedEntryIds: string[];
  includeNonActive: boolean;
  originatingRunId: string | null;
  policyVersion: string;
  modelIdentifier: string;
  resultingDocumentId: string | null;
  retryOfId: string | null;
  error: string | null;
  cancellationRequested: boolean;
  inputEntryCount: number;
  citationCount: number;
  createdAt: string;
  startedAt: string | null;
  finishedAt: string | null;
};
