// Domain types for deep-research agentic search (RFC 0037).
// Shapes mirror the Rust serde (camelCase) in src-tauri/src/domain/research.rs.

import type { DiscoverySearchResponse } from "$lib/domain/discover";

export type Depth = "quick" | "standard" | "thorough";

export type SearchRunStatus =
  | "queued"
  | "planning"
  | "searching"
  | "assessing"
  | "ranking"
  | "ready"
  | "failed"
  | "cancelled";

export interface SearchStrategy {
  depth: Depth;
  maxIterations: number;
  maxProviderQueries: number;
  maxLlmCalls: number;
}

export interface SearchConstraints {
  yearFrom?: number;
  yearTo?: number;
  providers: string[];
  openAccess: boolean;
  targetCount: number;
  venues: string[];
  authors: string[];
  fieldsOfStudy: string[];
  seedPaperIds: string[];
}

export interface SearchDraft {
  title: string;
  goal: string;
  constraints: SearchConstraints;
  strategy: SearchStrategy;
}

export interface Search {
  id: string;
  title: string;
  goal: string;
  constraints: SearchConstraints;
  strategy: SearchStrategy;
  status: string;
  stopReason?: string;
  summary?: string;
  createdAt: string;
  updatedAt: string;
}

export interface SearchRun {
  id: string;
  searchId: string;
  mode: "quick" | "deep" | "improve" | string;
  providerSet?: string;
  queryExpansions?: string;
  status: string;
  stopReason?: string;
  iteration: number;
  addedCount: number;
  totalCount: number;
  providerQueryCount: number;
  llmCallCount: number;
  inspectedCandidateCount: number;
  startedAt?: string;
  finishedAt?: string;
  error?: string;
  createdAt: string;
}

// The candidate payload is the same normalized paper shape that shallow
// discovery returns. Deep research ranks and explains these candidates; it does
// not invent a second paper model.
export type ResearchPaper = DiscoverySearchResponse["candidates"][number];

export interface SearchCandidate {
  id: string;
  searchId: string;
  firstSeenRunId: string;
  rank: number;
  score?: number;
  rationale?: string;
  rankSignalsJson?: string;
  providerHitsJson?: string;
  candidate: ResearchPaper;
  alreadyInLibrary: boolean;
  saved: boolean;
  seen: boolean;
  firstSeenAt: string;
}

export interface SearchUpdated {
  searchId: string;
  runId: string;
  status: string;
  message: string;
  iteration: number;
  found: number;
  unique: number;
  new: number;
}

export interface SearchCandidatesPreview {
  searchId: string;
  runId: string;
  candidates: ResearchPaper[];
  unique: number;
}

export interface HarnessConfiguration {
  goal: string;
  researchInstructions: string;
  scope: string;
  exclusions: string;
  preferredConcepts: string[];
  excludedConcepts: string[];
  sources: string[];
  depth: Depth;
  paperBudget: number;
  autonomy: "manual" | "propose" | "automatic";
  mayAddPapers: boolean;
  writableDocumentIds: string[];
  schedule: HarnessSchedule;
  stopConditions: HarnessStopConditions;
}

export interface HarnessSchedule {
  enabled: boolean;
  cadence: "daily" | "weekly";
  localTime: string;
  weekday?: number;
  timezone: string;
}

export interface HarnessStopConditions {
  maximumCycles?: number;
  endAt?: string;
  maximumUnproductiveRuns?: number;
  maximumRunSeconds?: number;
  maximumProviderQueries?: number;
  maximumLlmCalls?: number;
  stopOnConvergence: boolean;
}

export interface ResearchHarness {
  projectId: string;
  status: string;
  configuration: HarnessConfiguration;
  configurationVersion: number;
  nextRunAt?: string;
  lastScheduledFor?: string;
  requestedPostRunStatus: string;
  completedCycleCount: number;
  consecutiveUnproductiveRuns: number;
  terminalStopReason?: string;
  updatedAt: string;
}

export interface HarnessRun {
  id: string;
  projectId: string;
  status: string;
  configurationSnapshot: HarnessConfiguration;
  configurationVersion: number;
  policyVersion: string;
  effectiveInstructions: EffectiveInstructionStack;
  searchId: string;
  searchRunId?: string;
  startingStateRevision: number;
  resultingStateRevision?: number;
  startingVaultRevision: number;
  resultingVaultRevision?: number;
  providerQueryCount: number;
  llmCallCount: number;
  iterationCount: number;
  inspectedCandidateCount: number;
  trigger: "manual" | "scheduled" | "startup_catch_up";
  scheduledFor?: string;
  stopReason?: string;
  summary?: string;
  startedAt: string;
  finishedAt?: string;
}

export interface HarnessConfigurationVersion {
  projectId: string;
  version: number;
  configuration: HarnessConfiguration;
  actor: "researcher" | "improvement" | "migration" | string;
  sourceImprovementId?: string;
  reason: string;
  createdAt: string;
}

export interface RunContextEntry {
  id: string;
  kind: string;
  epistemicStatus: string;
  text: string;
  lifecycle: string;
}

export interface RunContextObservation {
  kind: string;
  description: string;
}

export interface EffectiveRunContext {
  startingStateRevision: number;
  activeEntries: RunContextEntry[];
  vaultId: string;
  vaultRevision: string;
  vaultPaperIds: string[];
  priorNextDirection?: string;
  priorObservations: RunContextObservation[];
  maximumProviderQueries: number;
  maximumLlmCalls: number;
  paperBudget: number;
}

export interface EffectiveInstructionStack {
  productPolicyVersion: string;
  productPolicySummary: string;
  projectResearchInstructions: string;
  structuredSettings: HarnessConfiguration;
  runContext: EffectiveRunContext;
}

export interface CandidateDecision {
  candidateId: string;
  decision: "accept" | "reject";
  reason: string;
  relevanceConfidence: number;
  withinScope: boolean;
}

export interface PlannedEvidence {
  candidateId: string;
  excerpt: string;
  supportNote?: string;
}

export interface PlannedRelation {
  target: string;
  kind: "derived_from" | "motivated_by";
}

export interface PlannedResearchEntry {
  handle: string;
  kind: "finding" | "question" | "gap" | "hypothesis" | "experiment_idea";
  epistemicStatus: "source_supported" | "agent_synthesis" | "speculative";
  text: string;
  evidence: PlannedEvidence[];
  relations: PlannedRelation[];
}

export interface RunReconciliationPlan {
  candidateDecisions: CandidateDecision[];
  entries: PlannedResearchEntry[];
  nextDirection: string;
  operationalReflection?: PlannedHarnessReflection;
}

export interface PlannedHarnessObservation {
  kind:
    | "query_quality"
    | "irrelevant_result_class"
    | "source_failure"
    | "terminology"
    | "coverage_bias"
    | "relevance_error"
    | "scope_drift"
    | "wasted_work";
  signature: string;
  severity: number;
  confidence: number;
  description: string;
  target?: "preferred_concepts" | "excluded_concepts" | "metadata_resolvers";
  proposedValue?:
    | { kind: "concepts"; items: string[] }
    | { kind: "metadata_resolvers"; items: string[] };
  proposalEligible: boolean;
}

export interface PlannedHarnessReflection {
  summary: string;
  nextDirection?: string;
  observations: PlannedHarnessObservation[];
}

export interface HarnessChangeSet {
  id: string;
  runId: string;
  projectId: string;
  startingStateRevision: number;
  status: "proposed" | "applied" | "rejected" | "superseded" | "failed";
  plan?: RunReconciliationPlan;
  consideredCandidates: SearchCandidate[];
  error?: string;
  decisionReason?: string;
  resultingStateRevision?: number;
  createdAt: string;
  decidedAt?: string;
}

export interface HarnessEvent {
  id: string;
  runId: string;
  sequence: number;
  kind: string;
  summary: string;
  detail?: Record<string, unknown>;
  phase?: string;
  progressCurrent?: number;
  progressTotal?: number;
  actor: "researcher" | "harness" | "scheduler" | "system" | string;
  occurredAt: string;
}

export interface HarnessUsage {
  providerQueries: number;
  llmCalls: number;
  iterations: number;
  inspectedCandidates: number;
}

export interface ResearchCheckpoint {
  runId: string;
  projectId: string;
  status: string;
  startingStateRevision: number;
  resultingStateRevision?: number;
  startingVaultRevision: number;
  resultingVaultRevision?: number;
  appliedChangeSetId?: string;
  acceptedCandidateCount: number;
  rejectedCandidateCount: number;
  addedPaperIds: string[];
  affectedDocuments: Array<{ documentId: string; contentRevision: number }>;
  usage: HarnessUsage;
  stopReason?: string;
  complete: boolean;
  converged: boolean;
  reflectionId?: string;
  nextDirection?: string;
  startedAt: string;
  finishedAt?: string;
  restoreAvailable: boolean;
}

export interface HarnessSnapshot {
  harness: ResearchHarness;
  runs: HarnessRun[];
  events: HarnessEvent[];
}

// Depth presets — mirror Depth::budget() in research.rs.
export function depthStrategy(depth: Depth): SearchStrategy {
  switch (depth) {
    case "quick":
      return { depth, maxIterations: 1, maxProviderQueries: 3, maxLlmCalls: 4 };
    case "thorough":
      return { depth, maxIterations: 4, maxProviderQueries: 16, maxLlmCalls: 24 };
    default:
      return { depth: "standard", maxIterations: 3, maxProviderQueries: 8, maxLlmCalls: 12 };
  }
}

export function isTerminalStatus(status: string): boolean {
  return status === "ready" || status === "failed" || status === "cancelled";
}
