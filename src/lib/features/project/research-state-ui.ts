import type {
  EpistemicStatus,
  ResearchEntryKind,
  ResearchEntrySummary,
} from "../../domain/research-state.ts";
import type {
  HarnessChangeSet,
  HarnessConfiguration,
  HarnessSchedule,
  ResearchCheckpoint,
} from "../../domain/research.ts";

/** Preserves every legacy researcher field in one readable instruction. */
export function canonicalResearchInstructions(configuration: HarnessConfiguration): string {
  const hasLegacyFields = Boolean(
    configuration.goal.trim() ||
      configuration.scope.trim() ||
      configuration.exclusions.trim() ||
      configuration.preferredConcepts.length ||
      configuration.excludedConcepts.length,
  );
  if (!hasLegacyFields) return configuration.researchInstructions.trim();
  const sections = [
    ["Goal", configuration.goal.trim()],
    ["Instructions", configuration.researchInstructions.trim()],
    ["Scope", configuration.scope.trim()],
    ["Avoid", configuration.exclusions.trim()],
    ["Prioritize", configuration.preferredConcepts.join(", ")],
    ["Avoid concepts", configuration.excludedConcepts.join(", ")],
  ];
  return sections
    .filter(([, value]) => value)
    .map(([heading, value]) => `${heading}:\n${value}`)
    .join("\n\n");
}

/** Builds the bounded persisted configuration behind the simple controls. */
export function simpleResearchConfiguration(
  current: HarnessConfiguration,
  instructions: string,
  paperBudget: number,
  schedule: HarnessSchedule,
): HarnessConfiguration {
  return {
    ...current,
    goal: "",
    researchInstructions: instructions.trim(),
    scope: "",
    exclusions: "",
    preferredConcepts: [],
    excludedConcepts: [],
    sources: ["browser", "open_alex", "arxiv"],
    depth: "standard",
    paperBudget: Math.min(100, Math.max(1, Math.round(paperBudget))),
    autonomy: "automatic",
    mayAddPapers: true,
    writableDocumentIds: [],
    schedule,
    stopConditions: { stopOnConvergence: false },
  };
}

/** Returns the epistemic statuses the editor may offer for one semantic kind. */
export function allowedEpistemicStatuses(kind: ResearchEntryKind): EpistemicStatus[] {
  if (kind === "hypothesis" || kind === "experiment_idea") return ["speculative"];
  if (kind === "gap") return ["agent_synthesis", "speculative"];
  if (kind === "finding") {
    return ["source_supported", "agent_synthesis", "researcher_context", "speculative"];
  }
  return ["agent_synthesis", "researcher_context", "speculative"];
}

/** Applies the visible kind and case-insensitive text filters to State entries. */
export function filterResearchEntries(
  entries: ResearchEntrySummary[],
  kind: ResearchEntryKind | "all",
  query: string,
): ResearchEntrySummary[] {
  const normalizedQuery = query.trim().toLowerCase();
  return entries.filter(
    (entry) =>
      (kind === "all" || entry.kind === kind) &&
      (!normalizedQuery || entry.text.toLowerCase().includes(normalizedQuery)),
  );
}

export type ResearchEntryCounts = Record<ResearchEntryKind | "all", number>;

/** Counts every semantic kind without hiding non-active lifecycle states. */
export function researchEntryCounts(entries: ResearchEntrySummary[]): ResearchEntryCounts {
  const counts: ResearchEntryCounts = {
    all: entries.length,
    finding: 0,
    question: 0,
    gap: 0,
    hypothesis: 0,
    experiment_idea: 0,
  };
  for (const entry of entries) counts[entry.kind] += 1;
  return counts;
}

/** Sorts a copy of State entries by newest change or stable semantic grouping. */
export function sortResearchEntries(
  entries: ResearchEntrySummary[],
  sort: "recent" | "kind",
): ResearchEntrySummary[] {
  return [...entries].sort((left, right) => {
    if (sort === "recent") {
      return right.lastRevision - left.lastRevision || left.id.localeCompare(right.id);
    }
    return left.kind.localeCompare(right.kind) || right.lastRevision - left.lastRevision;
  });
}

/** Describes whether Create-from is available and which immutable revision it will pin. */
export function documentGenerationAvailability(
  selectedCount: number,
  revision: number,
  currentRevision: number,
): { enabled: boolean; description: string } {
  if (selectedCount === 0) {
    return { enabled: false, description: "Select one or more Research State entries" };
  }
  const historical = revision !== currentRevision ? " historical" : "";
  return {
    enabled: true,
    description: `Create a Project document from${historical} State revision ${revision}`,
  };
}

/** Produces the stable review counts shown on a Harness Change Set card. */
export function changeSetReviewSummary(changeSet: HarnessChangeSet): {
  acceptedPapers: number;
  proposedEntries: number;
  reviewable: boolean;
} {
  return {
    acceptedPapers:
      changeSet.plan?.candidateDecisions.filter((decision) => decision.decision === "accept")
        .length ?? 0,
    proposedEntries: changeSet.plan?.entries.length ?? 0,
    reviewable: changeSet.status === "proposed" && Boolean(changeSet.plan),
  };
}

/** Explains whether append-only checkpoint restoration is currently safe to offer. */
export function checkpointRestoreAvailability(
  checkpoint: ResearchCheckpoint,
  viewingHistoricalState: boolean,
): { enabled: boolean; description: string } {
  if (!checkpoint.restoreAvailable || checkpoint.resultingStateRevision === undefined) {
    return { enabled: false, description: "This Run has no restorable Research State" };
  }
  if (viewingHistoricalState) {
    return { enabled: false, description: "Return to current Research State before restoring" };
  }
  return {
    enabled: true,
    description: `Restore State r${checkpoint.resultingStateRevision} as a new revision`,
  };
}
