import assert from "node:assert/strict";
import { test } from "node:test";
import type { ResearchEntrySummary } from "../../domain/research-state.ts";
import {
  allowedEpistemicStatuses,
  canonicalResearchInstructions,
  changeSetReviewSummary,
  checkpointRestoreAvailability,
  documentGenerationAvailability,
  filterResearchEntries,
  researchEntryCounts,
  simpleResearchConfiguration,
  sortResearchEntries,
} from "./research-state-ui.ts";
import type {
  HarnessChangeSet,
  HarnessConfiguration,
  ResearchCheckpoint,
} from "../../domain/research.ts";

function entry(kind: ResearchEntrySummary["kind"], text: string): ResearchEntrySummary {
  return {
    id: text,
    projectId: "project",
    kind,
    epistemicStatus: kind === "finding" ? "source_supported" : "speculative",
    text,
    lifecycle: "active",
    firstRevision: 1,
    lastRevision: 1,
    evidenceCount: 0,
    relationCount: 0,
    contextCount: 0,
    createdAt: "now",
    updatedAt: "now",
  };
}

test("kind and text filters compose", () => {
  const entries = [entry("finding", "Rank components specialize"), entry("gap", "Few ablations")];
  assert.deepEqual(filterResearchEntries(entries, "finding", "COMPONENTS"), [entries[0]]);
  assert.deepEqual(filterResearchEntries(entries, "all", "ablations"), [entries[1]]);
});

test("State counts and sorting retain lifecycle entries", () => {
  const older = entry("finding", "Older finding");
  const newer = entry("gap", "New gap");
  newer.lastRevision = 4;
  newer.lifecycle = "contested";
  assert.deepEqual(researchEntryCounts([older, newer]), {
    all: 2,
    finding: 1,
    question: 0,
    gap: 1,
    hypothesis: 0,
    experiment_idea: 0,
  });
  assert.deepEqual(sortResearchEntries([older, newer], "recent"), [newer, older]);
  assert.deepEqual(sortResearchEntries([older, newer], "kind"), [older, newer]);
});

test("speculative outputs cannot be presented as source-supported", () => {
  assert.deepEqual(allowedEpistemicStatuses("hypothesis"), ["speculative"]);
  assert.deepEqual(allowedEpistemicStatuses("experiment_idea"), ["speculative"]);
  assert.deepEqual(allowedEpistemicStatuses("gap"), ["agent_synthesis", "speculative"]);
});

test("document generation requires selection and names a historical pinned revision", () => {
  assert.deepEqual(documentGenerationAvailability(0, 4, 7), {
    enabled: false,
    description: "Select one or more Research State entries",
  });
  assert.deepEqual(documentGenerationAvailability(3, 4, 7), {
    enabled: true,
    description: "Create a Project document from historical State revision 4",
  });
});

test("change-set review counts accepted Papers and proposed State entries", () => {
  const changeSet = {
    status: "proposed",
    plan: {
      candidateDecisions: [
        { decision: "accept" },
        { decision: "reject" },
      ],
      entries: [{}, {}],
    },
  } as HarnessChangeSet;
  assert.deepEqual(changeSetReviewSummary(changeSet), {
    acceptedPapers: 1,
    proposedEntries: 2,
    reviewable: true,
  });
  changeSet.status = "applied";
  assert.equal(changeSetReviewSummary(changeSet).reviewable, false);
});

test("checkpoint restoration requires a resulting revision and the current State view", () => {
  const checkpoint = {
    restoreAvailable: true,
    resultingStateRevision: 4,
  } as ResearchCheckpoint;
  assert.deepEqual(checkpointRestoreAvailability(checkpoint, false), {
    enabled: true,
    description: "Restore State r4 as a new revision",
  });
  assert.equal(checkpointRestoreAvailability(checkpoint, true).enabled, false);
  checkpoint.resultingStateRevision = undefined;
  assert.equal(checkpointRestoreAvailability(checkpoint, false).enabled, false);
});

test("simple Research settings preserve legacy intent and enforce bounded enrichment", () => {
  const legacy: HarnessConfiguration = {
    goal: "Map LoRA mechanisms",
    researchInstructions: "Prefer causal evidence",
    scope: "Transformer adapters",
    exclusions: "Benchmark-only studies",
    preferredConcepts: ["ablation"],
    excludedConcepts: ["survey"],
    sources: ["arxiv"],
    depth: "quick",
    paperBudget: 10,
    autonomy: "manual",
    mayAddPapers: false,
    writableDocumentIds: ["document:notes"],
    schedule: {
      enabled: false,
      cadence: "daily",
      localTime: "09:00",
      timezone: "Europe/Berlin",
    },
    stopConditions: { maximumCycles: 2, stopOnConvergence: true },
  };
  const instructions = canonicalResearchInstructions(legacy);
  const simple = simpleResearchConfiguration(legacy, instructions, 500, legacy.schedule);

  for (const expected of ["Map LoRA", "causal", "Transformer", "Benchmark", "ablation", "survey"])
    assert.match(instructions, new RegExp(expected));
  assert.equal(simple.researchInstructions, instructions);
  assert.equal(simple.paperBudget, 100);
  assert.equal(simple.autonomy, "automatic");
  assert.equal(simple.mayAddPapers, true);
  assert.deepEqual(simple.writableDocumentIds, []);
  assert.deepEqual(simple.stopConditions, { stopOnConvergence: false });
});
