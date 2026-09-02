// Revisioned Project Research State (RFC 0112). Shapes mirror Rust serde.

export type ResearchEntryKind =
  | "finding"
  | "question"
  | "gap"
  | "hypothesis"
  | "experiment_idea";

export type EpistemicStatus =
  | "source_supported"
  | "agent_synthesis"
  | "researcher_context"
  | "speculative";

export type EntryLifecycle = "active" | "contested" | "superseded";
export type EntryRelationKind = "derived_from" | "motivated_by" | "contests" | "supersedes";
export type ResearchContextKind =
  | "reader_note"
  | "chat_turn"
  | "project_document"
  | "project_instruction";

export interface ResearchStateRevision {
  projectId: string;
  revision: number;
  runId?: string;
  reason: string;
  createdAt: string;
}

export interface ResearchEntrySummary {
  id: string;
  projectId: string;
  kind: ResearchEntryKind;
  epistemicStatus: EpistemicStatus;
  text: string;
  lifecycle: EntryLifecycle;
  firstRevision: number;
  lastRevision: number;
  originRunId?: string;
  evidenceCount: number;
  relationCount: number;
  contextCount: number;
  createdAt: string;
  updatedAt: string;
}

export interface ResearchStateSnapshot {
  projectId: string;
  revision: number;
  currentRevision: number;
  revisions: ResearchStateRevision[];
  entries: ResearchEntrySummary[];
}

export interface ResearchEvidenceLink {
  id: string;
  entryId: string;
  stateRevision: number;
  paperId: string;
  sourceId: string;
  extractionId: string;
  chunkId: string;
  excerpt: string;
  sourceStart: number;
  sourceEnd: number;
  pageStart: number;
  pageEnd: number;
  supportNote?: string;
}

export interface EntryRelation {
  id: string;
  entryId: string;
  stateRevision: number;
  targetEntryId: string;
  kind: EntryRelationKind;
}

export interface ResearchContextLink {
  id: string;
  entryId: string;
  stateRevision: number;
  kind: ResearchContextKind;
  contextId: string;
  label: string;
}

export interface ResearchEntryVersion {
  entryId: string;
  stateRevision: number;
  kind: ResearchEntryKind;
  epistemicStatus: EpistemicStatus;
  text: string;
  lifecycle: EntryLifecycle;
  originRunId?: string;
  reason: string;
  createdAt: string;
}

export interface ResearchEntryDetail {
  entry: ResearchEntrySummary;
  evidence: ResearchEvidenceLink[];
  relations: EntryRelation[];
  context: ResearchContextLink[];
  history: ResearchEntryVersion[];
}

export interface EvidenceLinkDraft {
  chunkId: string;
  excerpt?: string;
  supportNote?: string;
}

export interface EntryRelationDraft {
  targetEntryId: string;
  kind: EntryRelationKind;
}

export interface ResearchContextLinkDraft {
  kind: ResearchContextKind;
  contextId: string;
  label: string;
}

export interface ResearchEntryDraft {
  kind: ResearchEntryKind;
  epistemicStatus: EpistemicStatus;
  text: string;
  evidence: EvidenceLinkDraft[];
  relations: EntryRelationDraft[];
  context: ResearchContextLinkDraft[];
  reason?: string;
}

export interface ResearchEntryUpdate extends Omit<ResearchEntryDraft, "kind"> {
  id: string;
}

export interface ResearchStateMutation {
  state: ResearchStateSnapshot;
  entry: ResearchEntryDetail;
}

export interface ResearchEvidenceCandidate {
  chunkId: string;
  paperId: string;
  paperTitle: string;
  pageStart: number;
  pageEnd: number;
  headingPath?: string;
  text: string;
}
