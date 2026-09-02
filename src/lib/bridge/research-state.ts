import { invoke } from "@tauri-apps/api/core";
import type {
  EntryLifecycle,
  ResearchEntryDetail,
  ResearchEntryDraft,
  ResearchEntryUpdate,
  ResearchEvidenceCandidate,
  ResearchStateMutation,
  ResearchStateSnapshot,
} from "$lib/domain/research-state";

export async function getResearchState(
  projectId: string,
  revision?: number,
): Promise<ResearchStateSnapshot> {
  return invoke<ResearchStateSnapshot>("get_research_state", { projectId, revision });
}

export async function getResearchEntry(
  entryId: string,
  revision?: number,
): Promise<ResearchEntryDetail> {
  return invoke<ResearchEntryDetail>("get_research_entry", { entryId, revision });
}

export async function listResearchEvidenceCandidates(
  projectId: string,
  query?: string,
): Promise<ResearchEvidenceCandidate[]> {
  return invoke<ResearchEvidenceCandidate[]>("list_research_evidence_candidates", {
    projectId,
    query,
  });
}

export async function createResearchEntry(
  projectId: string,
  expectedRevision: number,
  draft: ResearchEntryDraft,
): Promise<ResearchStateMutation> {
  return invoke<ResearchStateMutation>("create_research_entry", {
    projectId,
    expectedRevision,
    draft,
  });
}

export async function reviseResearchEntry(
  expectedRevision: number,
  update: ResearchEntryUpdate,
): Promise<ResearchStateMutation> {
  return invoke<ResearchStateMutation>("revise_research_entry", { expectedRevision, update });
}

export async function setResearchEntryLifecycle(
  entryId: string,
  expectedRevision: number,
  lifecycle: Exclude<EntryLifecycle, "active">,
  reason: string,
): Promise<ResearchStateMutation> {
  return invoke<ResearchStateMutation>("set_research_entry_lifecycle", {
    entryId,
    expectedRevision,
    lifecycle,
    reason,
  });
}
