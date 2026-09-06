<script lang="ts">
  import { ExternalLink, LoaderCircle, Play, Square } from "@lucide/svelte";
  import { onMount, tick } from "svelte";
  import {
    applyHarnessChangeSet,
    cancelProjectResearch,
    clearHarnessConfigurationHistory,
    editHarnessChangeSet,
    getHarnessChangeSet,
    getResearchHarness,
    listResearchCheckpoints,
    listenResearchHarnessUpdated,
    listenResearchLibraryUpdated,
    listenResearchStateUpdated,
    listenSearchUpdated,
    rejectHarnessChangeSet,
    restoreResearchCheckpoint,
    runProjectResearch,
    saveResearchHarness,
  } from "$lib/bridge/research";
  import {
    createResearchEntry,
    getResearchEntry,
    getResearchState,
    listResearchEvidenceCandidates,
    reviseResearchEntry,
    setResearchEntryLifecycle,
  } from "$lib/bridge/research-state";
  import {
    acceptHarnessImprovement,
    editHarnessImprovement,
    listHarnessImprovements,
    rejectHarnessImprovement,
  } from "$lib/bridge/harness-improvement";
  import {
    cancelResearchDocumentGeneration,
    createDocumentFromResearch,
    listenResearchDocumentGenerationUpdated,
    retryResearchDocumentGeneration,
  } from "$lib/bridge/research-document";
  import type { HarnessImprovement } from "$lib/domain/harness-improvement";
  import type {
    ResearchDocumentGeneration,
    ResearchDocumentShape,
  } from "$lib/domain/research-document";
  import type { ProjectDocumentSummary } from "$lib/domain/library";
  import type {
    HarnessChangeSet,
    HarnessConfiguration,
    HarnessSnapshot,
    RunReconciliationPlan,
    ResearchCheckpoint,
  } from "$lib/domain/research";
  import type {
    EpistemicStatus,
    ResearchEntryDetail,
    ResearchEntryKind,
    ResearchEvidenceCandidate,
    ResearchStateSnapshot,
  } from "$lib/domain/research-state";
  import {
    allowedEpistemicStatuses,
    actionableResearchError,
    changeSetReviewSummary,
    checkpointRestoreAvailability,
    canonicalResearchInstructions,
    documentGenerationAvailability,
    filterResearchEntries,
    isActiveResearchRun,
    latestResearchActivity,
    latestResearchFailure,
    researchEntryCounts,
    researchProgressLabel,
    simpleResearchConfiguration,
    sortResearchEntries,
  } from "$lib/features/project/research-state-ui";

  let {
    projectId,
    projectTitle,
    projectGoal,
    documents = [],
    initialRevision,
    onOpenDocument,
    onOpenPaper,
    onLibraryChanged,
  }: {
    projectId: string;
    projectTitle: string;
    projectGoal?: string | null;
    documents?: ProjectDocumentSummary[];
    initialRevision?: number;
    onOpenDocument?: (documentId: string) => void;
    onOpenPaper?: (paperId: string, pageIndex?: number) => void;
    onLibraryChanged?: () => void | Promise<void>;
  } = $props();

  let snapshot = $state<HarnessSnapshot | null>(null);
  let researchState = $state<ResearchStateSnapshot | null>(null);
  let selectedEntry = $state<ResearchEntryDetail | null>(null);
  let evidenceCandidates = $state<ResearchEvidenceCandidate[]>([]);
  let improvements = $state<HarnessImprovement[]>([]);
  let changeSets = $state<Record<string, HarnessChangeSet>>({});
  let checkpoints = $state<Record<string, ResearchCheckpoint>>({});
  let inspectorTab = $state<"activity" | "settings">("activity");
  let loading = $state(true);
  let working = $state(false);
  let startingRun = $state(false);
  let cancellingRun = $state(false);
  let error = $state("");
  let loadedProjectKey = "";
  let refreshRequest = 0;
  let lastEventSequence = 0;

  let researchInstructions = $state("");
  let paperBudget = $state(10);
  let scheduleEnabled = $state(false);
  let scheduleCadence = $state<"daily" | "weekly">("daily");
  let scheduleTime = $state("09:00");
  let scheduleWeekday = $state(1);
  let scheduleTimezone = $state(Intl.DateTimeFormat().resolvedOptions().timeZone || "UTC");
  let search = $state("");
  let kindFilter = $state<ResearchEntryKind | "all">("all");
  let runFilter = $state("all");
  let stateSort = $state<"recent" | "kind">("recent");
  let editorOpen = $state(false);
  let editingEntryId = $state<string | null>(null);
  let entryKind = $state<ResearchEntryKind>("finding");
  let epistemicStatus = $state<EpistemicStatus>("source_supported");
  let entryText = $state("");
  let entryReason = $state("");
  let selectedEvidenceIds = $state<string[]>([]);
  let selectedRelationIds = $state<string[]>([]);
  let selectedDocumentIds = $state<string[]>([]);
  let includeProjectInstructions = $state(false);
  let generationSelection = $state<string[]>([]);
  let generationDialogOpen = $state(false);
  let generationShape = $state<ResearchDocumentShape>("survey");
  let generationTitle = $state("");
  let generationDirection = $state("");
  let includeNonActive = $state(false);
  let generation = $state<ResearchDocumentGeneration | null>(null);
  let detailHeading = $state<HTMLHeadingElement>();
  let originatingEntryId: string | null = null;
  let previousInspectorTab: "activity" | "settings" = "activity";
  let entryButtons: Record<string, HTMLButtonElement> = {};

  const activeRun = $derived(
    snapshot?.runs.find(isActiveResearchRun),
  );
  const activeProgress = $derived(
    activeRun ? latestResearchActivity(snapshot?.events ?? [], activeRun.id) : undefined,
  );
  const featuredRun = $derived(activeRun ?? snapshot?.runs[0]);
  const featuredFailure = $derived(
    featuredRun ? latestResearchFailure(snapshot?.events ?? [], featuredRun.id) : undefined,
  );
  const olderRuns = $derived((snapshot?.runs ?? []).filter((run) => run.id !== featuredRun?.id));
  const historical = $derived(
    Boolean(researchState && researchState.revision !== researchState.currentRevision),
  );
  const entryCounts = $derived(researchEntryCounts(researchState?.entries ?? []));
  const filteredEntries = $derived(
    sortResearchEntries(
      filterResearchEntries(researchState?.entries ?? [], kindFilter, search).filter(
        (entry) => runFilter === "all" || entry.originRunId === runFilter,
      ),
      stateSort,
    ),
  );
  const generationAvailability = $derived(
    documentGenerationAvailability(
      generationSelection.length,
      researchState?.revision ?? 0,
      researchState?.currentRevision ?? 0,
    ),
  );

  $effect(() => {
    const key = `${projectId}:${initialRevision ?? "current"}`;
    if (projectId && key !== loadedProjectKey) void loadProject(key);
  });

  onMount(() => {
    const unlisteners: Array<() => void> = [];
    let disposed = false;
    const register = (listener: Promise<() => void>): void => {
      void listener
        .then((stop) => {
          if (disposed) stop();
          else unlisteners.push(stop);
        })
        .catch((caught) => {
          if (!disposed) error = actionableResearchError(caught);
        });
    };
    register(
      listenSearchUpdated((event) => {
        if (!snapshot?.runs.some((run) => run.searchId === event.searchId)) return;
        void refreshHarness().then(() => {
          if (["ready", "failed", "cancelled"].includes(event.status)) {
            void onLibraryChanged?.();
          }
        });
      }),
    );
    register(
      listenResearchHarnessUpdated((event) => {
        if (event.projectId !== projectId && !snapshot?.runs.some((run) => run.id === event.runId)) {
          return;
        }
        void refreshHarness();
      }),
    );
    register(
      listenResearchStateUpdated((event) => {
        if (event.projectId === projectId) void refreshHarness();
      }),
    );
    register(
      listenResearchLibraryUpdated((event) => {
        if (event.projectId !== projectId) return;
        void refreshHarness();
        void onLibraryChanged?.();
      }),
    );
    register(
      listenResearchDocumentGenerationUpdated((updated) => {
        if (updated.projectId !== projectId) return;
        generation = updated;
        if (updated.status === "ready" && updated.resultingDocumentId) {
          generationDialogOpen = false;
          generationSelection = [];
          onOpenDocument?.(updated.resultingDocumentId);
        }
      }),
    );
    return () => {
      disposed = true;
      for (const unlisten of unlisteners) unlisten();
    };
  });

  async function loadProject(key: string) {
    refreshRequest += 1;
    loading = true;
    error = "";
    loadedProjectKey = key;
    try {
      const [next, nextState, nextImprovements, nextCheckpoints] = await Promise.all([
        getResearchHarness(projectId),
        getResearchState(projectId, initialRevision),
        listHarnessImprovements(projectId),
        listResearchCheckpoints(projectId),
      ]);
      if (loadedProjectKey !== key) return;
      const nextChangeSets = await loadChangeSets(next.runs);
      if (loadedProjectKey !== key) return;
      snapshot = next;
      lastEventSequence = Math.max(0, ...next.events.map((event) => event.sequence));
      researchState = nextState;
      improvements = nextImprovements;
      checkpoints = Object.fromEntries(nextCheckpoints.map((checkpoint) => [checkpoint.runId, checkpoint]));
      changeSets = nextChangeSets;
      selectedEntry = null;
      generationSelection = [];
      applyConfiguration(next.harness.configuration);
    } catch (caught) {
      if (loadedProjectKey === key) error = actionableResearchError(caught);
    } finally {
      if (loadedProjectKey === key) loading = false;
    }
  }

  async function switchRevision(value: string) {
    error = "";
    try {
      researchState = await getResearchState(
        projectId,
        value === "current" ? undefined : Number(value),
      );
      selectedEntry = null;
      editorOpen = false;
      generationSelection = [];
    } catch (caught) {
      error = String(caught);
    }
  }

  function openGenerationDialog(shape: ResearchDocumentShape = "survey", entryIds = generationSelection) {
    if (!researchState || entryIds.length === 0) return;
    generationSelection = [...new Set(entryIds)];
    generationShape = shape;
    generationTitle = `${projectTitle} ${shape.replaceAll("_", " ")}`;
    generationDirection = "";
    includeNonActive = false;
    generationDialogOpen = true;
  }

  function promoteExperiment() {
    if (!selectedEntry) return;
    openGenerationDialog("experiment_plan", [
      selectedEntry.entry.id,
      ...selectedEntry.relations.map((relation) => relation.targetEntryId),
    ]);
  }

  async function createGeneratedDocument() {
    if (!researchState || !generationTitle.trim() || generationSelection.length === 0) return;
    working = true;
    error = "";
    try {
      generation = await createDocumentFromResearch({
        projectId,
        stateRevision: researchState.revision,
        selectedEntryIds: generationSelection,
        shape: generationShape,
        title: generationTitle.trim(),
        customInstruction: generationDirection.trim() || undefined,
        includeNonActive,
      });
    } catch (caught) {
      error = String(caught);
    } finally {
      working = false;
    }
  }

  async function cancelGeneration() {
    if (!generation) return;
    generation = await cancelResearchDocumentGeneration(generation.id);
  }

  async function retryGeneration() {
    if (!generation) return;
    generation = await retryResearchDocumentGeneration(generation.id);
  }

  async function openEntry(entryId: string) {
    try {
      originatingEntryId = entryId;
      previousInspectorTab = inspectorTab;
      selectedEntry = await getResearchEntry(entryId, researchState?.revision);
      await tick();
      detailHeading?.focus();
    } catch (caught) {
      error = String(caught);
    }
  }

  async function closeDetails() {
    const entryId = originatingEntryId;
    selectedEntry = null;
    inspectorTab = previousInspectorTab;
    await tick();
    if (entryId) entryButtons[entryId]?.focus();
  }

  async function startCreate() {
    editingEntryId = null;
    entryKind = "finding";
    epistemicStatus = "source_supported";
    entryText = "";
    entryReason = "";
    selectedEvidenceIds = [];
    selectedRelationIds = [];
    selectedDocumentIds = [];
    includeProjectInstructions = false;
    evidenceCandidates = await listResearchEvidenceCandidates(projectId);
    editorOpen = true;
  }

  async function startEdit() {
    if (!selectedEntry) return;
    editingEntryId = selectedEntry.entry.id;
    entryKind = selectedEntry.entry.kind;
    epistemicStatus = selectedEntry.entry.epistemicStatus;
    entryText = selectedEntry.entry.text;
    entryReason = "";
    selectedEvidenceIds = selectedEntry.evidence.map((item) => item.chunkId);
    selectedRelationIds = selectedEntry.relations
      .filter((item) => item.kind === "derived_from")
      .map((item) => item.targetEntryId);
    selectedDocumentIds = selectedEntry.context
      .filter((item) => item.kind === "project_document")
      .map((item) => item.contextId);
    includeProjectInstructions = selectedEntry.context.some(
      (item) => item.kind === "project_instruction",
    );
    evidenceCandidates = await listResearchEvidenceCandidates(projectId);
    editorOpen = true;
  }

  function toggleSelection(values: string[], value: string, enabled: boolean): string[] {
    return enabled ? [...new Set([...values, value])] : values.filter((item) => item !== value);
  }

  function runLabel(runId?: string): string {
    if (!runId || !snapshot) return "Manual";
    const index = snapshot.runs.findIndex((run) => run.id === runId);
    return index < 0 ? "Run" : `Cycle ${snapshot.runs.length - index}`;
  }

  async function saveEntry() {
    if (!researchState) return;
    working = true;
    error = "";
    const evidence = selectedEvidenceIds.map((chunkId) => ({ chunkId }));
    const relations = selectedRelationIds.map((targetEntryId) => ({
      targetEntryId,
      kind: "derived_from" as const,
    }));
    const context = [
      ...selectedDocumentIds.map((contextId) => ({
        kind: "project_document" as const,
        contextId,
        label: documents.find((document) => document.id === contextId)?.title ?? "Project document",
      })),
      ...(includeProjectInstructions
        ? [{ kind: "project_instruction" as const, contextId: projectId, label: "Project instructions" }]
        : []),
    ];
    try {
      const mutation = editingEntryId
        ? await reviseResearchEntry(researchState.currentRevision, {
            id: editingEntryId,
            epistemicStatus,
            text: entryText,
            evidence,
            relations,
            context,
            reason: entryReason || undefined,
          })
        : await createResearchEntry(projectId, researchState.currentRevision, {
            kind: entryKind,
            epistemicStatus,
            text: entryText,
            evidence,
            relations,
            context,
            reason: entryReason || undefined,
          });
      researchState = mutation.state;
      selectedEntry = mutation.entry;
      editorOpen = false;
    } catch (caught) {
      error = String(caught);
    } finally {
      working = false;
    }
  }

  async function changeLifecycle(lifecycle: "contested" | "superseded") {
    if (!selectedEntry || !researchState) return;
    const reason = window.prompt(`Reason this entry is ${lifecycle}:`)?.trim();
    if (!reason) return;
    try {
      const mutation = await setResearchEntryLifecycle(
        selectedEntry.entry.id,
        researchState.currentRevision,
        lifecycle,
        reason,
      );
      researchState = mutation.state;
      selectedEntry = mutation.entry;
    } catch (caught) {
      error = String(caught);
    }
  }

  $effect(() => {
    const options = allowedEpistemicStatuses(entryKind);
    if (!options.includes(epistemicStatus)) epistemicStatus = options[0];
  });

  async function refreshHarness() {
    const request = ++refreshRequest;
    const requestedProjectId = projectId;
    try {
      const [nextSnapshot, nextImprovements, nextCheckpoints] = await Promise.all([
        getResearchHarness(requestedProjectId),
        listHarnessImprovements(requestedProjectId),
        listResearchCheckpoints(requestedProjectId),
      ]);
      const nextChangeSets = await loadChangeSets(nextSnapshot.runs);
      const nextState = !historical ? await getResearchState(requestedProjectId) : undefined;
      if (request !== refreshRequest || projectId !== requestedProjectId) return;
      const nextSequence = Math.max(0, ...nextSnapshot.events.map((event) => event.sequence));
      if (nextSequence < lastEventSequence) return;
      lastEventSequence = nextSequence;
      snapshot = nextSnapshot;
      improvements = nextImprovements;
      checkpoints = Object.fromEntries(nextCheckpoints.map((checkpoint) => [checkpoint.runId, checkpoint]));
      changeSets = nextChangeSets;
      if (nextState) researchState = nextState;
      error = "";
    } catch (caught) {
      if (request === refreshRequest) error = actionableResearchError(caught);
    }
  }

  async function restoreCheckpoint(checkpoint: ResearchCheckpoint) {
    if (!researchState || checkpoint.resultingStateRevision === undefined) return;
    const confirmed = window.confirm(
      `Restore Research State r${checkpoint.resultingStateRevision} as a new revision? Later active entries will be superseded. Papers, Paper metadata, and Documents will not be changed. No history will be deleted.`,
    );
    if (!confirmed) return;
    working = true;
    error = "";
    try {
      researchState = await restoreResearchCheckpoint(
        projectId,
        checkpoint.runId,
        researchState.currentRevision,
      );
      selectedEntry = null;
      await refreshHarness();
    } catch (caught) {
      error = String(caught);
    } finally {
      working = false;
    }
  }

  async function loadChangeSets(runs: HarnessSnapshot["runs"]): Promise<Record<string, HarnessChangeSet>> {
    const pairs = await Promise.all(
      runs.map(async (run) => {
        try {
          return [run.id, await getHarnessChangeSet(run.id)] as const;
        } catch {
          return null;
        }
      }),
    );
    return Object.fromEntries(pairs.filter((pair): pair is readonly [string, HarnessChangeSet] => pair !== null));
  }

  async function decideChangeSet(changeSet: HarnessChangeSet, action: "apply" | "edit" | "reject") {
    working = true;
    error = "";
    try {
      if (action === "apply") {
        await applyHarnessChangeSet(changeSet.id);
        await onLibraryChanged?.();
      } else if (action === "reject") {
        const reason = window.prompt("Why should these proposed Project changes be rejected?")?.trim();
        if (!reason) return;
        await rejectHarnessChangeSet(changeSet.id, reason);
      } else {
        if (!changeSet.plan) return;
        const raw = window.prompt(
          "Edit the bounded reconciliation plan as JSON. Evidence handles and excerpts are revalidated.",
          JSON.stringify(changeSet.plan, null, 2),
        );
        if (raw === null) return;
        await editHarnessChangeSet(changeSet.id, JSON.parse(raw) as RunReconciliationPlan);
      }
      await refreshHarness();
    } catch (caught) {
      error = String(caught);
    } finally {
      working = false;
    }
  }

  async function decideImprovement(
    improvement: HarnessImprovement,
    action: "edit" | "accept" | "reject",
  ) {
    working = true;
    error = "";
    try {
      if (action === "edit") {
        const value = window.prompt(
          "Comma-separated proposed values. Product policy, goals, schedules, budgets, and evidence rules cannot be changed here.",
          improvement.proposedValue.items.join(", "),
        );
        if (value === null) return;
        await editHarnessImprovement(improvement.id, {
          kind: improvement.proposedValue.kind,
          items: conceptList(value),
        });
      } else if (action === "accept") {
        await acceptHarnessImprovement(improvement.id);
      } else {
        const reason = window.prompt("Optional rejection reason:") ?? undefined;
        await rejectHarnessImprovement(improvement.id, reason);
      }
      await refreshHarness();
    } catch (caught) {
      error = String(caught);
    } finally {
      working = false;
    }
  }

  function applyConfiguration(configuration: HarnessConfiguration) {
    researchInstructions = canonicalResearchInstructions(configuration);
    paperBudget = configuration.paperBudget;
    scheduleEnabled = configuration.schedule.enabled;
    scheduleCadence = configuration.schedule.cadence;
    scheduleTime = configuration.schedule.localTime;
    scheduleWeekday = configuration.schedule.weekday ?? 1;
    scheduleTimezone = configuration.schedule.timezone;
  }

  function conceptList(value: string): string[] {
    return value
      .split(",")
      .map((item) => item.trim())
      .filter(Boolean);
  }

  function editedConfiguration(): HarnessConfiguration | null {
    if (!snapshot) return null;
    return simpleResearchConfiguration(
      snapshot.harness.configuration,
      researchInstructions,
      Number(paperBudget),
      {
        enabled: scheduleEnabled,
        cadence: scheduleCadence,
        localTime: scheduleTime,
        weekday: scheduleCadence === "weekly" ? Number(scheduleWeekday) : undefined,
        timezone: scheduleTimezone,
      },
    );
  }

  async function persistSettings(): Promise<void> {
    const configuration = editedConfiguration();
    if (!configuration || JSON.stringify(configuration) === JSON.stringify(snapshot?.harness.configuration)) return;
    snapshot = await saveResearchHarness(projectId, configuration);
    applyConfiguration(snapshot.harness.configuration);
  }

  async function saveSettings() {
    working = true;
    error = "";
    try {
      await persistSettings();
    } catch (caught) {
      error = String(caught);
    } finally {
      working = false;
    }
  }

  async function runNow() {
    working = true;
    startingRun = true;
    error = "";
    try {
      await persistSettings();
      await runProjectResearch(projectId);
      await refreshHarness();
      inspectorTab = "activity";
    } catch (caught) {
      error = actionableResearchError(caught);
    } finally {
      working = false;
      startingRun = false;
    }
  }

  async function cancelRun() {
    working = true;
    cancellingRun = true;
    error = "";
    try {
      await cancelProjectResearch(projectId);
      await refreshHarness();
    } catch (caught) {
      error = actionableResearchError(caught);
    } finally {
      working = false;
      cancellingRun = false;
    }
  }

  async function clearConfigurationHistory() {
    if (!window.confirm("Clear older Research configuration history? Existing Runs and their snapshots will remain unchanged.")) return;
    working = true;
    error = "";
    try {
      await clearHarnessConfigurationHistory(projectId);
    } catch (caught) {
      error = String(caught);
    } finally {
      working = false;
    }
  }
</script>

<section class="research-workspace">
  <main class="research-state">
    <header class="project-header hair-b">
      <div>
        <span class="eyebrow">Project Research</span>
        <h1>{projectTitle}</h1>
        <p class="project-goal">{projectGoal?.trim() || researchInstructions.split("\n").find(Boolean) || "Add research instructions to begin."}</p>
        <small class="next-run">{snapshot?.harness.completedCycleCount ?? 0} completed Runs{activeRun ? ` · ${activeRun.status}` : ""}</small>
        {#if snapshot?.harness.nextRunAt}<small class="next-run">Next while i0i is open: {new Date(snapshot.harness.nextRunAt).toLocaleString()}</small>{/if}
      </div>
      {#if activeRun}<span class="status running">{activeRun.status}</span>{/if}
    </header>

    {#if error}<div class="error hair-b">{error}</div>{/if}
    {#if loading}
      <div class="empty-state">Loading research…</div>
    {:else}
      <section class="state-toolbar hair-b">
        <div class="state-heading row">
          <div>
            <span class="eyebrow">Research State</span>
            <strong>Revision {researchState?.revision ?? 0}</strong>
            {#if historical}<span class="read-only">Historical · read-only</span>{/if}
          </div>
          <select
            aria-label="Research State revision"
            value={historical ? String(researchState?.revision) : "current"}
            onchange={(event) => void switchRevision(event.currentTarget.value)}
          >
            <option value="current">Current · r{researchState?.currentRevision ?? 0}</option>
            {#each researchState?.revisions ?? [] as revision}
              {#if revision.revision !== researchState?.currentRevision}
                <option value={revision.revision}>r{revision.revision} · {revision.reason}</option>
              {/if}
            {/each}
          </select>
          <button class="primary" type="button" disabled={historical} onclick={() => void startCreate()}>
            New entry
          </button>
          <button
            type="button"
            disabled={!generationAvailability.enabled}
            title={generationAvailability.description}
            onclick={() => openGenerationDialog()}
          >
            Create from…{#if generationSelection.length} · {generationSelection.length}{/if}
          </button>
        </div>
        <input bind:value={search} placeholder="Search current understanding…" aria-label="Search Research State" />
        <nav class="kind-filters" aria-label="Research Entry kind">
          {#each [["all", "All"], ["finding", "Findings"], ["question", "Questions"], ["gap", "Gaps"], ["hypothesis", "Hypotheses"], ["experiment_idea", "Experiment ideas"]] as filter}
            <button class:active={kindFilter === filter[0]} type="button" onclick={() => (kindFilter = filter[0] as ResearchEntryKind | "all")}>{filter[1]} · {entryCounts[filter[0] as ResearchEntryKind | "all"]}</button>
          {/each}
        </nav>
        <div class="state-view-controls">
          <label><span>Run / cycle</span><select bind:value={runFilter}><option value="all">All Runs</option>{#each snapshot?.runs ?? [] as run, index}<option value={run.id}>Cycle {(snapshot?.runs.length ?? 0) - index} · {run.status}</option>{/each}</select></label>
          <label><span>Sort</span><select bind:value={stateSort}><option value="recent">Recent changes</option><option value="kind">Semantic kind</option></select></label>
        </div>
      </section>

      {#if editorOpen}
        <form class="entry-editor" onsubmit={(event) => { event.preventDefault(); void saveEntry(); }}>
          <div class="row editor-title">
            <strong>{editingEntryId ? "Revise entry" : "New Research Entry"}</strong>
            <button type="button" onclick={() => (editorOpen = false)}>Close</button>
          </div>
          <div class="editor-grid">
            <label>
              <span>Kind</span>
              <select bind:value={entryKind} disabled={Boolean(editingEntryId)}>
                <option value="finding">Finding</option><option value="question">Question</option>
                <option value="gap">Gap</option><option value="hypothesis">Hypothesis</option>
                <option value="experiment_idea">Experiment idea</option>
              </select>
            </label>
            <label>
              <span>Epistemic status</span>
              <select bind:value={epistemicStatus}>
                {#each allowedEpistemicStatuses(entryKind) as option}
                  <option value={option}>{option.replaceAll("_", " ")}</option>
                {/each}
              </select>
            </label>
          </div>
          <label><span>Entry</span><textarea bind:value={entryText} rows="4" required></textarea></label>
          {#if epistemicStatus === "source_supported"}
            <fieldset>
              <legend>Evidence · canonical Project Vault passages</legend>
              {#each evidenceCandidates as candidate}
                <label class="candidate">
                  <input type="checkbox" checked={selectedEvidenceIds.includes(candidate.chunkId)} onchange={(event) => (selectedEvidenceIds = toggleSelection(selectedEvidenceIds, candidate.chunkId, event.currentTarget.checked))} />
                  <span><strong>{candidate.paperTitle}</strong> · pp. {candidate.pageStart + 1}–{candidate.pageEnd + 1}<small>{candidate.text}</small></span>
                </label>
              {:else}<p class="empty-list">No extracted passages are available in this Project Vault.</p>{/each}
            </fieldset>
          {/if}
          {#if epistemicStatus === "agent_synthesis"}
            <fieldset>
              <legend>Derived from Research Entries</legend>
              {#each researchState?.entries.filter((entry) => entry.id !== editingEntryId) ?? [] as entry}
                <label class="candidate"><input type="checkbox" checked={selectedRelationIds.includes(entry.id)} onchange={(event) => (selectedRelationIds = toggleSelection(selectedRelationIds, entry.id, event.currentTarget.checked))} /><span>{entry.text}</span></label>
              {:else}<p class="empty-list">Create a premise before recording an agent synthesis.</p>{/each}
            </fieldset>
          {/if}
          <fieldset>
            <legend>Motivation / working context · not evidence</legend>
            <label class="candidate"><input type="checkbox" bind:checked={includeProjectInstructions} /><span>Project instructions</span></label>
            {#each documents as document}
              <label class="candidate"><input type="checkbox" checked={selectedDocumentIds.includes(document.id)} onchange={(event) => (selectedDocumentIds = toggleSelection(selectedDocumentIds, document.id, event.currentTarget.checked))} /><span>{document.title}</span></label>
            {/each}
          </fieldset>
          <label><span>Revision reason</span><input bind:value={entryReason} placeholder="Why this belongs in the State" /></label>
          <button class="primary" type="submit" disabled={working || !entryText.trim()}>{working ? "Saving…" : "Commit revision"}</button>
        </form>
      {/if}

      {#if generationDialogOpen && researchState}
        <form class="entry-editor generation-dialog" onsubmit={(event) => { event.preventDefault(); void createGeneratedDocument(); }}>
          <div class="row editor-title">
            <div><strong>Create from Research</strong><small>{generationSelection.length} entries · State revision {researchState.revision}{historical ? " · historical" : ""}</small></div>
            <button type="button" onclick={() => (generationDialogOpen = false)}>Close</button>
          </div>
          <div class="editor-grid">
            <label><span>Type</span><select bind:value={generationShape}><option value="survey">Survey</option><option value="related_work">Related-work section</option><option value="research_gap_analysis">Research-gap analysis</option><option value="hypothesis_report">Hypothesis report</option><option value="experiment_plan">Experiment plan</option><option value="custom">Custom document</option></select></label>
            <label><span>Title</span><input bind:value={generationTitle} required /></label>
          </div>
          <label><span>Direction</span><textarea bind:value={generationDirection} rows="3" maxlength="1000" placeholder="Audience, emphasis, or ordering…"></textarea></label>
          <label class="candidate"><input type="checkbox" bind:checked={includeNonActive} /><span>Include selected contested or superseded entries, visibly qualified</span></label>
          <p class="setting-note">Epistemic labels and source citations are always preserved. The generated Markdown is authored output, not source evidence.</p>
          <button class="primary" type="submit" disabled={working || !generationTitle.trim()}>{working ? "Queuing…" : "Create document"}</button>
        </form>
      {/if}

      <section class="state-list">
        {#each filteredEntries as entry}
          <article class:selected={selectedEntry?.entry.id === entry.id} class="entry-row">
            <input
              class="generation-check"
              type="checkbox"
              aria-label={`Select Research Entry for document generation: ${entry.text}`}
              checked={generationSelection.includes(entry.id)}
              onchange={(event) => (generationSelection = toggleSelection(generationSelection, entry.id, event.currentTarget.checked))}
            />
            <button bind:this={entryButtons[entry.id]} class="entry-content" type="button" onclick={() => void openEntry(entry.id)}>
              <div class="entry-labels"><span>{entry.kind.replaceAll("_", " ")}</span><span>{entry.epistemicStatus.replaceAll("_", " ")}</span><span>{entry.lifecycle}</span></div>
              <p>{entry.text}</p>
              <small>{entry.evidenceCount} evidence links · {entry.relationCount} premises · {entry.contextCount} working-context links · {runLabel(entry.originRunId)} · changed r{entry.lastRevision}</small>
            </button>
          </article>
        {:else}
          <div class="empty-state">{(researchState?.entries.length ?? 0) === 0 ? "Research State is empty. Add instructions and run research, or create a manual Research Entry." : "No Research State entries match these filters."}</div>
        {/each}
      </section>
    {/if}
  </main>

  <aside class="harness hair-l">
    <header class="harness-header hair-b">
      <span class="label hot">Research</span>
      <label class="instruction-input">
        <span>Research instructions</span>
        <textarea bind:value={researchInstructions} rows="5" placeholder="What should i0i investigate, prioritize, include, or avoid?"></textarea>
      </label>
      <div class="run-controls">
        <label>
          <span>Papers to find</span>
          <input type="number" min="1" max="100" step="1" bind:value={paperBudget} />
        </label>
        {#if activeRun}
          <button
            class="primary command-button"
            type="button"
            disabled={working || activeRun.status === "canceling"}
            onclick={() => void cancelRun()}
          >
            {#if cancellingRun || activeRun.status === "canceling"}<span class="spin"><LoaderCircle size={14} aria-hidden="true" /></span>{:else}<Square size={13} aria-hidden="true" />{/if}
            {activeRun.status === "canceling" ? "Stopping" : "Cancel"}
          </button>
        {:else}
          <button class="primary command-button" type="button" disabled={working || !researchInstructions.trim()} onclick={() => void runNow()}>
            {#if startingRun}<span class="spin"><LoaderCircle size={14} aria-hidden="true" /></span>{:else}<Play size={14} aria-hidden="true" />{/if}
            {startingRun ? "Starting" : "Run research"}
          </button>
        {/if}
      </div>
      {#if activeRun}
        <small class="live-progress" aria-live="polite">
          <span class="spin"><LoaderCircle size={13} aria-hidden="true" /></span>
          <span>{researchProgressLabel(activeRun, activeProgress)}{activeProgress?.progressCurrent !== undefined ? ` · ${activeProgress.progressCurrent}${activeProgress.progressTotal !== undefined ? `/${activeProgress.progressTotal}` : ""}` : ""}</span>
        </small>
      {/if}
    </header>

    {#if !selectedEntry}
      <nav class="tabs hair-b" aria-label="Research inspector">
        <button class:active={inspectorTab === "activity"} type="button" onclick={() => (inspectorTab = "activity")}>Activity</button>
        <button class:active={inspectorTab === "settings"} type="button" onclick={() => (inspectorTab = "settings")}>Settings</button>
      </nav>
    {/if}

    {#if selectedEntry}
      <div class="panel details">
        <div class="row detail-heading"><button type="button" onclick={() => void closeDetails()}>Back</button><h2 bind:this={detailHeading} tabindex="-1">Research Entry details</h2></div>
        <div class="detail-meta">{selectedEntry.entry.kind.replaceAll("_", " ")} · {selectedEntry.entry.epistemicStatus.replaceAll("_", " ")} · {selectedEntry.entry.lifecycle}</div>
        <small>Origin: {runLabel(selectedEntry.entry.originRunId)} · first recorded r{selectedEntry.entry.firstRevision}</small>
        <p class="detail-text">{selectedEntry.entry.text}</p>
        {#if !historical}
          <div class="actions"><button type="button" onclick={() => void startEdit()}>Revise</button><button type="button" onclick={() => void changeLifecycle("contested")}>Contest</button><button type="button" onclick={() => void changeLifecycle("superseded")}>Supersede</button>{#if selectedEntry.entry.kind === "experiment_idea"}<button class="primary" type="button" onclick={promoteExperiment}>Promote to document</button>{/if}</div>
        {/if}
        <section><h3>Source evidence</h3>{#each selectedEntry.evidence as link}<button class="provenance provenance-link" type="button" onclick={() => onOpenPaper?.(link.paperId, link.pageStart)}><span><strong>Paper {link.paperId} · pp. {link.pageStart + 1}–{link.pageEnd + 1}</strong><ExternalLink size={12} aria-hidden="true" /></span><p>“{link.excerpt}”</p>{#if link.supportNote}<small>{link.supportNote}</small>{/if}</button>{:else}<p class="empty-list">No direct source evidence. This entry is not presented as a sourced quotation.</p>{/each}</section>
        <section><h3>Derivation</h3>{#each selectedEntry.relations as link}<p class="provenance">{link.kind.replaceAll("_", " ")} · {link.targetEntryId}</p>{:else}<p class="empty-list">No entry derivations.</p>{/each}</section>
        <section><h3>Researcher context</h3>{#each selectedEntry.context as link}<p class="provenance">{link.kind.replaceAll("_", " ")} · {link.label}</p>{:else}<p class="empty-list">No working context attached.</p>{/each}</section>
        <section><h3>Immutable history</h3>{#each selectedEntry.history as version}<article class="history"><strong>r{version.stateRevision} · {version.lifecycle}</strong><p>{version.reason}</p><time>{version.createdAt}</time></article>{/each}</section>
      </div>
    {:else if inspectorTab === "activity"}
      <div class="panel activity">
        {#if generation}
          <article class:pending={["queued", "generating"].includes(generation.status)} class="improvement-card generation-card">
            <div class="row"><span class="event-kind">Document · {generation.status}</span><span class="sequence">State r{generation.stateRevision}</span></div>
            <h3>{generation.title}</h3>
            <p>{generation.selectedEntryIds.length} selected entries · {generation.shape.replaceAll("_", " ")}</p>
            {#if generation.error}<p class="error-text">{generation.error}</p>{/if}
            <div class="actions">
              {#if ["queued", "generating"].includes(generation.status)}<button type="button" onclick={() => void cancelGeneration()}>Cancel</button>{/if}
              {#if ["failed", "cancelled"].includes(generation.status)}<button type="button" onclick={() => void retryGeneration()}>Retry</button>{/if}
              {#if generation.status === "ready" && generation.resultingDocumentId}<button class="primary" type="button" onclick={() => onOpenDocument?.(generation?.resultingDocumentId ?? "")}>Open document</button>{/if}
            </div>
          </article>
        {/if}
        {#if featuredRun}
          {@const run = featuredRun}
          <article class="improvement-card run-card">
            <div class="row"><span class="event-kind">{activeRun ? "Current Run" : "Last Run"} · {run.status}</span><time>{run.finishedAt ?? run.startedAt}</time></div>
            {#if featuredFailure && ["failed", "cancelled"].includes(run.status)}
              <p class="error-text">{actionableResearchError(featuredFailure.summary)}</p>
            {:else if checkpoints[run.id]?.outcome}
              <p>{checkpoints[run.id].outcome?.summary}</p>
            {:else}
              <p>{run.summary ?? run.stopReason ?? "Research is starting."}</p>
            {/if}
            {#if checkpoints[run.id]}
              {@const checkpoint = checkpoints[run.id]}
              {@const restoreAvailability = checkpointRestoreAvailability(checkpoint, historical)}
              <p class="result-summary">{checkpoint.addedPaperIds.length} papers added to Vault · State {checkpoint.resultingStateRevision === undefined ? "unchanged" : `advanced to r${checkpoint.resultingStateRevision}`}</p>
              {#if checkpoint.outcome?.unansweredQuestions[0]}<p><strong>Still open</strong><br />{checkpoint.outcome.unansweredQuestions[0]}</p>{/if}
              {#if checkpoint.nextDirection}<p><strong>Next</strong><br />{checkpoint.nextDirection}</p>{/if}
              <details class="checkpoint">
                <summary>Technical activity</summary>
                <p>{checkpoint.acceptedCandidateCount} accepted · {checkpoint.rejectedCandidateCount} rejected · {checkpoint.usage.providerQueries} queries · {checkpoint.usage.llmCalls} model calls</p>
                {#each (snapshot?.events ?? []).filter((event) => event.runId === run.id) as event}
                  <article class="event compact-event"><span class="event-kind">{event.kind.replaceAll("_", " ")}</span><p>{event.summary}</p></article>
                {/each}
                {#if checkpoint.restoreAvailable}
                  <button type="button" disabled={working || !restoreAvailability.enabled} title={restoreAvailability.description} onclick={() => void restoreCheckpoint(checkpoint)}>Restore Research State</button>
                {/if}
              </details>
            {/if}
            {#if changeSets[run.id]}
              {@const changeSet = changeSets[run.id]}
              {@const reviewSummary = changeSetReviewSummary(changeSet)}
              <div class="change-set-summary">
                <strong>Project changes · {changeSet.status}</strong>
                {#if changeSet.plan}
                  <p>{reviewSummary.acceptedPapers} accepted Papers · {reviewSummary.proposedEntries} proposed State entries</p>
                {/if}
                {#if changeSet.error}<p class="error-text">{changeSet.error}</p>{/if}
                {#if changeSet.decisionReason}<p>{changeSet.decisionReason}</p>{/if}
                {#if reviewSummary.reviewable}<div class="actions"><button type="button" disabled={working} onclick={() => void decideChangeSet(changeSet, "reject")}>Reject</button><button type="button" disabled={working} onclick={() => void decideChangeSet(changeSet, "edit")}>Edit JSON</button><button class="primary" type="button" disabled={working} onclick={() => void decideChangeSet(changeSet, "apply")}>Apply changes</button></div>{/if}
              </div>
            {/if}
          </article>
        {:else}
          <div class="empty-list">Run research to enrich this Project.</div>
        {/if}

        {#if olderRuns.length || improvements.length}
          <details class="history-list">
            <summary>History ({olderRuns.length})</summary>
            {#each olderRuns as run}
              <article class="history-run"><strong>{run.status}</strong><time>{run.finishedAt ?? run.startedAt}</time><p>{run.summary ?? run.stopReason ?? "No summary."}</p><details><summary>Technical activity</summary>{#each (snapshot?.events ?? []).filter((event) => event.runId === run.id) as event}<p>{event.summary}</p>{/each}</details></article>
            {/each}
            {#each improvements as improvement}
              <article class:pending={improvement.status === "proposed"} class="improvement-card">
                <strong>Research improvement · {improvement.status}</strong><p>{improvement.rationale}</p>
                {#if improvement.status === "proposed"}<div class="actions"><button type="button" disabled={working} onclick={() => void decideImprovement(improvement, "reject")}>Reject</button><button type="button" disabled={working} onclick={() => void decideImprovement(improvement, "edit")}>Edit</button><button class="primary" type="button" disabled={working} onclick={() => void decideImprovement(improvement, "accept")}>Accept</button></div>{/if}
              </article>
            {/each}
          </details>
        {/if}
      </div>
    {:else}
      <form class="panel settings" onsubmit={(event) => { event.preventDefault(); void saveSettings(); }}>
        <details>
          <summary>Schedule</summary>
          <label class="check"><input type="checkbox" bind:checked={scheduleEnabled} /> Run automatically while i0i is open</label>
          {#if scheduleEnabled}
            <div class="settings-row schedule-row">
              <label><span>Cadence</span><select bind:value={scheduleCadence}><option value="daily">Daily</option><option value="weekly">Weekly</option></select></label>
              {#if scheduleCadence === "weekly"}<label><span>Weekday</span><select bind:value={scheduleWeekday}><option value={1}>Monday</option><option value={2}>Tuesday</option><option value={3}>Wednesday</option><option value={4}>Thursday</option><option value={5}>Friday</option><option value={6}>Saturday</option><option value={7}>Sunday</option></select></label>{/if}
              <label><span>Local time</span><input type="time" bind:value={scheduleTime} /></label>
            </div>
            <p class="setting-note">Timezone {scheduleTimezone}. Missed intervals produce at most one startup catch-up.</p>
          {/if}
        </details>
        <button class="primary save" type="submit" disabled={working}>{working ? "Saving…" : "Save schedule"}</button>
        <details class="danger-zone"><summary>More</summary><button type="button" disabled={working} onclick={() => void clearConfigurationHistory()}>Clear configuration history</button></details>
      </form>
    {/if}
  </aside>
</section>

<style>
  .research-workspace {
    flex: 1;
    min-height: 0;
    display: grid;
    grid-template-columns: minmax(0, 1fr) 360px;
    background: var(--bg);
  }

  .research-state,
  .harness {
    min-height: 0;
    overflow: auto;
  }

  .project-header,
  .harness-header {
    padding: 14px 16px;
    background: var(--bg-1);
  }

  .harness-header {
    position: sticky;
    top: 0;
    z-index: 2;
    display: grid;
    gap: 10px;
  }

  .instruction-input {
    display: grid;
    gap: 5px;
  }

  .instruction-input > span,
  .run-controls label > span {
    color: var(--fg-3);
    font-size: 10px;
    text-transform: uppercase;
  }

  .run-controls {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    gap: 8px;
    align-items: end;
  }

  .run-controls label {
    display: grid;
    grid-template-columns: 1fr 72px;
    gap: 8px;
    align-items: center;
  }

  .run-controls button {
    min-height: 32px;
  }

  .command-button,
  .live-progress {
    display: inline-flex;
    align-items: center;
    gap: 6px;
  }

  .command-button {
    justify-content: center;
    white-space: nowrap;
  }

  .live-progress {
    min-width: 0;
    color: var(--fg-2);
    line-height: 1.35;
  }

  .live-progress span {
    min-width: 0;
    overflow-wrap: anywhere;
  }

  .spin {
    flex: 0 0 auto;
    animation: research-spin 850ms linear infinite;
  }

  .project-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }

  h1,
  p {
    margin: 0;
  }

  h1 {
    margin-top: 3px;
    font-size: 18px;
    font-weight: 600;
  }

  .next-run,
  .setting-note {
    display: block;
    margin-top: 4px;
    color: var(--fg-3);
    font-size: 10px;
  }

  .project-goal {
    max-width: 64ch;
    margin-top: 5px;
    color: var(--fg-2);
    line-height: 1.35;
  }

  .eyebrow,
  time,
  .sequence {
    color: var(--fg-3);
    font-size: 10px;
    text-transform: uppercase;
    letter-spacing: 0;
  }

  .status {
    padding: 3px 7px;
    border: 1px solid var(--border-2);
    color: var(--fg-3);
    font-size: 10px;
    text-transform: uppercase;
  }

  .status.running {
    color: var(--amber);
    border-color: var(--amber-mid);
  }

  .state-toolbar {
    padding: 12px 16px;
    background: var(--bg-1);
  }

  .state-heading {
    align-items: center;
    gap: 9px;
    margin-bottom: 9px;
  }

  .state-heading > div {
    display: flex;
    align-items: baseline;
    gap: 8px;
    flex: 1;
  }

  .read-only {
    color: var(--amber);
    font-size: 10px;
    text-transform: uppercase;
  }

  .kind-filters {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
    margin-top: 8px;
  }

  .kind-filters button.active {
    color: var(--amber);
    border-color: var(--amber-mid);
  }

  .state-view-controls {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    margin-top: 8px;
  }

  .state-view-controls label {
    display: flex;
    align-items: center;
    gap: 6px;
    color: var(--fg-3);
    font-size: 10px;
    text-transform: uppercase;
  }

  .state-list {
    padding: 10px 16px 30px;
  }

  .entry-row {
    display: flex;
    align-items: flex-start;
    width: 100%;
    background: var(--bg-1);
    border: 1px solid var(--border-2);
  }

  .entry-row + .entry-row {
    border-top: 0;
  }

  .entry-row.selected {
    border-color: var(--amber-mid);
  }

  .generation-check {
    width: auto;
    margin: 15px 0 0 13px;
  }

  .entry-content {
    flex: 1;
    min-width: 0;
    padding: 13px;
    border: 0;
    text-align: left;
    background: transparent;
  }

  .entry-row p {
    margin: 7px 0;
    line-height: 1.45;
  }

  .generation-dialog small {
    display: block;
    margin-top: 4px;
    color: var(--fg-3);
    font-weight: 400;
  }

  .entry-row small,
  .candidate small {
    display: block;
    color: var(--fg-3);
  }

  .entry-labels {
    display: flex;
    gap: 8px;
    color: var(--amber);
    font-size: 10px;
    text-transform: uppercase;
  }

  .entry-editor {
    display: flex;
    flex-direction: column;
    gap: 11px;
    margin: 14px 16px;
    padding: 14px;
    border: 1px solid var(--amber-mid);
    background: var(--bg-1);
  }

  .entry-editor > label,
  .editor-grid label {
    display: flex;
    flex-direction: column;
    gap: 5px;
  }

  .entry-editor label > span {
    color: var(--fg-3);
    font-size: 10px;
    text-transform: uppercase;
  }

  .editor-title {
    justify-content: space-between;
  }

  .editor-grid {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 8px;
  }

  .candidate {
    display: flex;
    gap: 7px;
    align-items: flex-start;
    margin: 6px 0;
  }

  .candidate input {
    width: auto;
    margin-top: 2px;
  }

  .candidate > span {
    color: var(--fg-2) !important;
    font-size: 11px !important;
    text-transform: none !important;
  }

  .candidate small {
    margin-top: 3px;
    line-height: 1.35;
    max-height: 3.9em;
    overflow: hidden;
  }

  .detail-meta {
    color: var(--amber);
    font-size: 10px;
    text-transform: uppercase;
  }

  .detail-heading {
    justify-content: space-between;
    align-items: center;
    margin-bottom: 10px;
  }

  .detail-heading h2 {
    margin: 0;
    font-size: 13px;
  }

  .detail-heading h2:focus-visible {
    outline: 2px solid var(--amber);
    outline-offset: 3px;
  }

  .detail-text {
    margin: 9px 0 13px;
    line-height: 1.5;
  }

  .details h3 {
    margin: 16px 0 7px;
    color: var(--fg-3);
    font-size: 10px;
    text-transform: uppercase;
  }

  .provenance,
  .history {
    padding: 8px;
    border: 1px solid var(--border-1);
    background: var(--bg-1);
    font-size: 11px;
  }

  .provenance + .provenance,
  .history + .history {
    margin-top: 5px;
  }

  .provenance p,
  .history p {
    margin-top: 5px;
    color: var(--fg-2);
    line-height: 1.4;
  }

  .provenance-link {
    display: block;
    width: 100%;
    text-align: left;
  }

  .provenance-link > span {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 8px;
  }

  @keyframes research-spin {
    to {
      transform: rotate(360deg);
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .spin {
      animation: none;
    }
  }

  .event p {
    color: var(--fg-2);
    line-height: 1.5;
  }

  .actions {
    margin-top: 12px;
    gap: 6px;
  }

  button,
  input,
  textarea,
  select {
    border: 1px solid var(--border-2);
    background: var(--bg);
    color: var(--fg);
    font: inherit;
  }

  button {
    padding: 5px 9px;
    cursor: pointer;
  }

  button:hover:not(:disabled) {
    color: var(--amber);
    border-color: var(--amber-mid);
  }

  button:disabled {
    opacity: 0.4;
    cursor: default;
  }

  button.primary {
    color: var(--amber);
    border-color: var(--amber-mid);
  }

  .tabs {
    display: flex;
    padding: 0 10px;
  }

  .tabs button {
    border: 0;
    border-bottom: 2px solid transparent;
    background: transparent;
    padding: 9px 8px 7px;
    color: var(--fg-3);
  }

  .tabs button.active {
    color: var(--amber);
    border-bottom-color: var(--amber);
  }

  .panel {
    padding: 12px;
  }

  .event {
    padding: 10px 0;
    border-bottom: 1px solid var(--border-1);
  }

  .improvement-card {
    margin-bottom: 10px;
    padding: 11px;
    border: 1px solid var(--border-2);
    background: var(--bg-1);
  }

  .improvement-card.pending {
    border-color: var(--amber-mid);
  }

  .change-set-summary {
    margin-top: 10px;
    padding-top: 10px;
    border-top: 1px solid var(--border-1);
  }

  .improvement-card details {
    margin: 8px 0;
    color: var(--fg-3);
    font-size: 10px;
  }

  .event-kind {
    color: var(--amber);
    text-transform: uppercase;
    font-size: 10px;
  }

  .event p {
    margin: 5px 0;
  }

  .settings {
    display: flex;
    flex-direction: column;
    gap: 13px;
  }

  .settings-row label {
    display: flex;
    flex-direction: column;
    gap: 5px;
  }

  .settings label > span,
  legend {
    color: var(--fg-3);
    font-size: 10px;
    text-transform: uppercase;
  }

  input,
  textarea,
  select {
    width: 100%;
    box-sizing: border-box;
    padding: 7px;
  }

  textarea {
    resize: vertical;
    line-height: 1.4;
  }

  fieldset {
    margin: 0;
    padding: 8px;
    border: 1px solid var(--border-2);
  }

  .check {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    margin-right: 12px;
  }

  .check input {
    width: auto;
  }

  .settings-row {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 8px;
  }

  .schedule-row {
    margin-top: 8px;
    grid-template-columns: repeat(3, 1fr);
  }

  .save {
    align-self: flex-start;
  }

  .result-summary {
    margin-top: 8px;
    color: var(--fg-2);
    line-height: 1.45;
  }

  .history-list,
  .danger-zone {
    margin-top: 12px;
    border-top: 1px solid var(--border-1);
    padding-top: 10px;
  }

  .history-run {
    padding: 10px 0;
    border-bottom: 1px solid var(--border-1);
  }

  .history-run time {
    float: right;
  }

  .history-run p {
    margin-top: 5px;
    color: var(--fg-2);
    line-height: 1.4;
  }

  .error,
  .empty-list,
  .empty-state {
    padding: 14px;
    color: var(--fg-3);
  }

  .error {
    color: var(--red, #d26b6b);
  }

  @media (max-width: 900px) {
    .research-workspace {
      grid-template-columns: minmax(0, 1fr);
      grid-template-rows: minmax(55vh, 1fr) auto;
    }

    .harness {
      max-height: 48vh;
      border-top: 1px solid var(--border-2);
      border-left: 0;
    }
  }
</style>
