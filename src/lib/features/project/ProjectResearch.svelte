<script lang="ts">
  import { onMount, tick } from "svelte";
  import {
    applyHarnessChangeSet,
    cancelProjectResearch,
    editHarnessChangeSet,
    getHarnessChangeSet,
    getHarnessRunInstructions,
    getResearchHarness,
    listHarnessConfigurationVersions,
    listResearchCheckpoints,
    listenSearchUpdated,
    pauseResearchHarness,
    resumeResearchHarness,
    rejectHarnessChangeSet,
    restoreResearchCheckpoint,
    runProjectResearch,
    saveResearchHarness,
    stopResearchHarness,
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
    Depth,
    EffectiveInstructionStack,
    HarnessChangeSet,
    HarnessConfiguration,
    HarnessConfigurationVersion,
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
    changeSetReviewSummary,
    checkpointRestoreAvailability,
    documentGenerationAvailability,
    filterResearchEntries,
    researchEntryCounts,
    sortResearchEntries,
  } from "$lib/features/project/research-state-ui";

  let {
    projectId,
    projectTitle,
    projectGoal,
    documents = [],
    initialRevision,
    onOpenDocument,
    onLibraryChanged,
  }: {
    projectId: string;
    projectTitle: string;
    projectGoal?: string | null;
    documents?: ProjectDocumentSummary[];
    initialRevision?: number;
    onOpenDocument?: (documentId: string) => void;
    onLibraryChanged?: () => void | Promise<void>;
  } = $props();

  let snapshot = $state<HarnessSnapshot | null>(null);
  let researchState = $state<ResearchStateSnapshot | null>(null);
  let selectedEntry = $state<ResearchEntryDetail | null>(null);
  let evidenceCandidates = $state<ResearchEvidenceCandidate[]>([]);
  let improvements = $state<HarnessImprovement[]>([]);
  let configurationVersions = $state<HarnessConfigurationVersion[]>([]);
  let inspectedInstructions = $state<EffectiveInstructionStack | null>(null);
  let changeSets = $state<Record<string, HarnessChangeSet>>({});
  let checkpoints = $state<Record<string, ResearchCheckpoint>>({});
  let inspectorTab = $state<"details" | "activity" | "settings">("activity");
  let loading = $state(true);
  let working = $state(false);
  let error = $state("");
  let loadedProjectKey = "";

  let goal = $state("");
  let researchInstructions = $state("");
  let scope = $state("");
  let exclusions = $state("");
  let preferredConcepts = $state("");
  let excludedConcepts = $state("");
  let sources = $state<string[]>([]);
  let depth = $state<Depth>("standard");
  let paperBudget = $state(10);
  let autonomy = $state<"manual" | "propose" | "automatic">("manual");
  let mayAddPapers = $state(false);
  let writableDocumentIds = $state<string[]>([]);
  let scheduleEnabled = $state(false);
  let scheduleCadence = $state<"daily" | "weekly">("daily");
  let scheduleTime = $state("09:00");
  let scheduleWeekday = $state(1);
  let scheduleTimezone = $state(Intl.DateTimeFormat().resolvedOptions().timeZone || "UTC");
  let maximumCycles = $state<number | undefined>();
  let maximumUnproductiveRuns = $state<number | undefined>();
  let maximumRunMinutes = $state<number | undefined>();
  let maximumProviderQueries = $state<number | undefined>();
  let maximumLlmCalls = $state<number | undefined>();
  let endAt = $state("");
  let stopOnConvergence = $state(false);
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
  let entryButtons: Record<string, HTMLButtonElement> = {};

  const activeRun = $derived(
    snapshot?.runs.find((run) =>
      ["queued", "planning", "searching", "assessing", "ranking", "reconciling"].includes(run.status),
    ),
  );
  const activeProgress = $derived(
    activeRun
      ? snapshot?.events.find((event) => event.runId === activeRun.id && event.phase)
      : undefined,
  );
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
    let unlisten: (() => void) | undefined;
    let unlistenGeneration: (() => void) | undefined;
    listenSearchUpdated((event) => {
      if (snapshot?.runs.some((run) => run.searchId === event.searchId)) void refreshHarness();
    }).then((stop) => (unlisten = stop));
    listenResearchDocumentGenerationUpdated((updated) => {
      if (updated.projectId !== projectId) return;
      generation = updated;
      if (updated.status === "ready" && updated.resultingDocumentId) {
        generationDialogOpen = false;
        generationSelection = [];
        onOpenDocument?.(updated.resultingDocumentId);
      }
    }).then((stop) => (unlistenGeneration = stop));
    return () => {
      unlisten?.();
      unlistenGeneration?.();
    };
  });

  async function loadProject(key: string) {
    loading = true;
    error = "";
    loadedProjectKey = key;
    try {
      const [next, nextState, nextImprovements, nextVersions, nextCheckpoints] = await Promise.all([
        getResearchHarness(projectId),
        getResearchState(projectId, initialRevision),
        listHarnessImprovements(projectId),
        listHarnessConfigurationVersions(projectId),
        listResearchCheckpoints(projectId),
      ]);
      if (loadedProjectKey !== key) return;
      snapshot = next;
      researchState = nextState;
      improvements = nextImprovements;
      configurationVersions = nextVersions;
      checkpoints = Object.fromEntries(nextCheckpoints.map((checkpoint) => [checkpoint.runId, checkpoint]));
      changeSets = await loadChangeSets(next.runs);
      inspectedInstructions = null;
      selectedEntry = null;
      generationSelection = [];
      applyConfiguration(next.harness.configuration);
    } catch (caught) {
      error = String(caught);
    } finally {
      loading = false;
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
      selectedEntry = await getResearchEntry(entryId, researchState?.revision);
      inspectorTab = "details";
      await tick();
      detailHeading?.focus();
    } catch (caught) {
      error = String(caught);
    }
  }

  async function closeDetails() {
    const entryId = originatingEntryId;
    selectedEntry = null;
    inspectorTab = "activity";
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
      inspectorTab = "details";
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

  $effect(() => {
    if (autonomy === "manual") scheduleEnabled = false;
  });

  async function refreshHarness() {
    try {
      const [nextSnapshot, nextImprovements, nextCheckpoints] = await Promise.all([
        getResearchHarness(projectId),
        listHarnessImprovements(projectId),
        listResearchCheckpoints(projectId),
      ]);
      snapshot = nextSnapshot;
      improvements = nextImprovements;
      checkpoints = Object.fromEntries(nextCheckpoints.map((checkpoint) => [checkpoint.runId, checkpoint]));
      configurationVersions = await listHarnessConfigurationVersions(projectId);
      changeSets = await loadChangeSets(snapshot?.runs ?? []);
      if (!historical) researchState = await getResearchState(projectId);
    } catch (caught) {
      error = String(caught);
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
    goal = configuration.goal;
    researchInstructions = configuration.researchInstructions;
    scope = configuration.scope;
    exclusions = configuration.exclusions;
    preferredConcepts = configuration.preferredConcepts.join(", ");
    excludedConcepts = configuration.excludedConcepts.join(", ");
    sources = [...configuration.sources];
    depth = configuration.depth;
    paperBudget = configuration.paperBudget;
    autonomy = configuration.autonomy;
    mayAddPapers = configuration.mayAddPapers;
    writableDocumentIds = [...configuration.writableDocumentIds];
    scheduleEnabled = configuration.schedule.enabled;
    scheduleCadence = configuration.schedule.cadence;
    scheduleTime = configuration.schedule.localTime;
    scheduleWeekday = configuration.schedule.weekday ?? 1;
    scheduleTimezone = configuration.schedule.timezone;
    maximumCycles = configuration.stopConditions.maximumCycles;
    maximumUnproductiveRuns = configuration.stopConditions.maximumUnproductiveRuns;
    maximumRunMinutes = configuration.stopConditions.maximumRunSeconds
      ? Math.ceil(configuration.stopConditions.maximumRunSeconds / 60)
      : undefined;
    maximumProviderQueries = configuration.stopConditions.maximumProviderQueries;
    maximumLlmCalls = configuration.stopConditions.maximumLlmCalls;
    endAt = configuration.stopConditions.endAt?.slice(0, 16) ?? "";
    stopOnConvergence = configuration.stopConditions.stopOnConvergence;
  }

  async function inspectInstructions(runId: string) {
    error = "";
    try {
      inspectedInstructions = await getHarnessRunInstructions(runId);
      inspectorTab = "activity";
    } catch (caught) {
      error = String(caught);
    }
  }

  function conceptList(value: string): string[] {
    return value
      .split(",")
      .map((item) => item.trim())
      .filter(Boolean);
  }

  function toggleSource(source: string, enabled: boolean) {
    sources = enabled
      ? [...new Set([...sources, source])]
      : sources.filter((candidate) => candidate !== source);
  }

  async function saveSettings() {
    working = true;
    error = "";
    try {
      snapshot = await saveResearchHarness(projectId, {
        goal,
        researchInstructions,
        scope,
        exclusions,
        preferredConcepts: conceptList(preferredConcepts),
        excludedConcepts: conceptList(excludedConcepts),
        sources,
        depth,
        paperBudget: Number(paperBudget),
        autonomy,
        mayAddPapers,
        writableDocumentIds,
        schedule: {
          enabled: scheduleEnabled,
          cadence: scheduleCadence,
          localTime: scheduleTime,
          weekday: scheduleCadence === "weekly" ? Number(scheduleWeekday) : undefined,
          timezone: scheduleTimezone,
        },
        stopConditions: {
          maximumCycles: maximumCycles ? Number(maximumCycles) : undefined,
          endAt: endAt ? new Date(endAt).toISOString() : undefined,
          maximumUnproductiveRuns: maximumUnproductiveRuns
            ? Number(maximumUnproductiveRuns)
            : undefined,
          maximumRunSeconds: maximumRunMinutes ? Number(maximumRunMinutes) * 60 : undefined,
          maximumProviderQueries: maximumProviderQueries
            ? Number(maximumProviderQueries)
            : undefined,
          maximumLlmCalls: maximumLlmCalls ? Number(maximumLlmCalls) : undefined,
          stopOnConvergence,
        },
      });
      applyConfiguration(snapshot.harness.configuration);
    } catch (caught) {
      error = String(caught);
    } finally {
      working = false;
    }
  }

  async function runNow() {
    working = true;
    error = "";
    try {
      await runProjectResearch(projectId);
      await refreshHarness();
      inspectorTab = "activity";
    } catch (caught) {
      error = String(caught);
    } finally {
      working = false;
    }
  }

  async function cancelRun() {
    working = true;
    error = "";
    try {
      await cancelProjectResearch(projectId);
      await refreshHarness();
    } catch (caught) {
      error = String(caught);
    } finally {
      working = false;
    }
  }

  async function setHarnessLifecycle(action: "pause" | "resume" | "stop") {
    working = true;
    error = "";
    try {
      snapshot = await (action === "pause"
        ? pauseResearchHarness(projectId)
        : action === "resume"
          ? resumeResearchHarness(projectId)
          : stopResearchHarness(projectId));
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
        <p class="project-goal">{projectGoal?.trim() || goal || "Define a research goal in Harness Settings."}</p>
        <small class="next-run">Cycle {snapshot?.harness.completedCycleCount ?? 0} · Harness {activeRun?.status ?? snapshot?.harness.status ?? "inactive"}</small>
        {#if snapshot?.harness.nextRunAt}<small class="next-run">Next while i0i is open: {new Date(snapshot.harness.nextRunAt).toLocaleString()}</small>{/if}
        {#if snapshot?.harness.terminalStopReason}<small class="next-run">Stopped: {snapshot.harness.terminalStopReason.replaceAll("_", " ")}</small>{/if}
      </div>
      <span class:running={Boolean(activeRun)} class="status">
        {activeRun?.status ?? snapshot?.harness.status ?? "inactive"}
      </span>
    </header>

    {#if error}<div class="error hair-b">{error}</div>{/if}
    {#if loading}
      <div class="empty-state">Loading Research Harness…</div>
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
          <div class="empty-state">{(researchState?.entries.length ?? 0) === 0 ? "Research State is empty. Seed the Project Vault, configure and run the Harness, or create a manual Research Entry." : "No Research State entries match these filters."}</div>
        {/each}
      </section>
    {/if}
  </main>

  <aside class="harness hair-l">
    <header class="harness-header hair-b">
      <div class="row">
        <span class="label hot">Research Harness</span>
        <div class="flex1"></div>
        <span class="version">v{snapshot?.harness.configurationVersion ?? 1}</span>
      </div>
      <div class="actions row">
        <button class="primary" type="button" disabled={working || Boolean(activeRun) || !goal.trim() || snapshot?.harness.status === "stopped"} onclick={() => void runNow()}>
          Run now
        </button>
        {#if snapshot?.harness.status === "paused" || snapshot?.harness.status === "stopped"}
          <button type="button" disabled={working} onclick={() => void setHarnessLifecycle("resume")}>Resume</button>
        {:else}
          <button type="button" disabled={working} onclick={() => void setHarnessLifecycle("pause")}>{activeRun ? "Pause after Run" : "Pause"}</button>
        {/if}
        <button type="button" disabled={working} onclick={() => void setHarnessLifecycle("stop")}>{activeRun ? "Stop and cancel" : "Stop"}</button>
        {#if activeRun}<button type="button" disabled={working || !activeRun.searchRunId} onclick={() => void cancelRun()}>Cancel Run</button>{/if}
      </div>
      {#if activeProgress}<small class="live-progress">{activeProgress.phase} · {activeProgress.summary}{activeProgress.progressCurrent !== undefined ? ` · ${activeProgress.progressCurrent}${activeProgress.progressTotal !== undefined ? `/${activeProgress.progressTotal}` : ""}` : ""}</small>{/if}
    </header>

    <nav class="tabs hair-b" aria-label="Research Harness inspector">
      <button class:active={inspectorTab === "details"} type="button" disabled={!selectedEntry} onclick={() => (inspectorTab = "details")}>Details</button>
      <button class:active={inspectorTab === "activity"} type="button" onclick={() => (inspectorTab = "activity")}>Activity{#if improvements.some((item) => item.status === "proposed")} · {improvements.filter((item) => item.status === "proposed").length}{/if}</button>
      <button class:active={inspectorTab === "settings"} type="button" onclick={() => (inspectorTab = "settings")}>Settings</button>
    </nav>

    {#if inspectorTab === "details" && selectedEntry}
      <div class="panel details">
        <div class="row detail-heading"><h2 bind:this={detailHeading} tabindex="-1">Research Entry details</h2><button type="button" onclick={() => void closeDetails()}>Close</button></div>
        <div class="detail-meta">{selectedEntry.entry.kind.replaceAll("_", " ")} · {selectedEntry.entry.epistemicStatus.replaceAll("_", " ")} · {selectedEntry.entry.lifecycle}</div>
        <small>Origin: {runLabel(selectedEntry.entry.originRunId)} · first recorded r{selectedEntry.entry.firstRevision}</small>
        <p class="detail-text">{selectedEntry.entry.text}</p>
        {#if !historical}
          <div class="actions"><button type="button" onclick={() => void startEdit()}>Revise</button><button type="button" onclick={() => void changeLifecycle("contested")}>Contest</button><button type="button" onclick={() => void changeLifecycle("superseded")}>Supersede</button>{#if selectedEntry.entry.kind === "experiment_idea"}<button class="primary" type="button" onclick={promoteExperiment}>Promote to document</button>{/if}</div>
        {/if}
        <section><h3>Source evidence</h3>{#each selectedEntry.evidence as link}<article class="provenance"><strong>Paper {link.paperId} · pp. {link.pageStart + 1}–{link.pageEnd + 1}</strong><p>“{link.excerpt}”</p>{#if link.supportNote}<small>{link.supportNote}</small>{/if}</article>{:else}<p class="empty-list">No direct source evidence. This entry is not presented as a sourced quotation.</p>{/each}</section>
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
        {#each improvements as improvement}
          <article class:pending={improvement.status === "proposed"} class="improvement-card">
            <div class="row"><span class="event-kind">{improvement.status === "proposed" ? "Needs review" : improvement.status}</span><span class="sequence">configuration v{improvement.baseConfigurationVersion}</span></div>
            <h3>Improve future research</h3>
            <small>Target: {improvement.target.replaceAll("_", " ")} · observed in {improvement.runIds.length} Runs</small>
            <p>{improvement.rationale}</p>
            <details><summary>Contributing Run telemetry</summary>{#each improvement.observations as observation}<article class="proposal-observation"><strong>{observation.kind.replaceAll("_", " ")} · {Math.round(observation.confidence * 100)}% confidence</strong><p>{observation.description}</p><code>{observation.metricsJson}</code></article>{/each}</details>
            <div class="proposal-values"><div><strong>Before</strong><span>{improvement.beforeValue.items.join(", ") || "None"}</span></div><div><strong>After</strong><span>{improvement.proposedValue.items.join(", ") || "None"}</span></div></div>
            <div class="proposal-preview"><strong>Expected effect</strong><p>{improvement.expectedEffect}</p></div>
            <small>Operational analysis only — not Research State evidence. Active Runs keep their snapshot.</small>
            {#if improvement.status === "proposed"}<div class="actions"><button type="button" disabled={working} onclick={() => void decideImprovement(improvement, "reject")}>Reject</button><button type="button" disabled={working} onclick={() => void decideImprovement(improvement, "edit")}>Edit</button><button class="primary" type="button" disabled={working} onclick={() => void decideImprovement(improvement, "accept")}>Accept next Run</button></div>{/if}
          </article>
        {/each}
        {#each snapshot?.runs ?? [] as run}
          <article class="improvement-card run-card">
            <div class="row"><span class="event-kind">Run · {run.status}</span><span class="sequence">configuration v{run.configurationVersion}</span></div>
            <p>{run.summary ?? run.stopReason ?? "No Run summary yet."}</p>
            <small>Started from State r{run.startingStateRevision} · {run.trigger.replaceAll("_", " ")}</small>
            <div class="actions"><button type="button" onclick={() => void inspectInstructions(run.id)}>View effective instructions</button></div>
            {#if checkpoints[run.id]}
              {@const checkpoint = checkpoints[run.id]}
              {@const restoreAvailability = checkpointRestoreAvailability(checkpoint, historical)}
              <details class="checkpoint">
                <summary>Checkpoint · {checkpoint.complete ? "complete" : "stopped early"}</summary>
                <p>
                  State r{checkpoint.startingStateRevision} → {checkpoint.resultingStateRevision === undefined ? "unchanged" : `r${checkpoint.resultingStateRevision}`}
                  · Vault v{checkpoint.startingVaultRevision} → {checkpoint.resultingVaultRevision === undefined ? "unchanged" : `v${checkpoint.resultingVaultRevision}`}
                </p>
                <p>{checkpoint.acceptedCandidateCount} accepted · {checkpoint.rejectedCandidateCount} rejected · {checkpoint.addedPaperIds.length} Papers added</p>
                <p>{checkpoint.usage.providerQueries} provider queries · {checkpoint.usage.llmCalls} model calls · {checkpoint.usage.iterations} iterations · {checkpoint.usage.inspectedCandidates} candidates inspected</p>
                <p><strong>Stop</strong> · {checkpoint.stopReason?.replaceAll("_", " ") ?? checkpoint.status}</p>
                {#if checkpoint.nextDirection}<p><strong>Next direction</strong><br />{checkpoint.nextDirection}</p>{/if}
                {#if checkpoint.restoreAvailable}
                  <button type="button" disabled={working || !restoreAvailability.enabled} title={restoreAvailability.description} onclick={() => void restoreCheckpoint(checkpoint)}>Restore Research State</button>
                  <small>Creates a new State revision. Papers and Documents are not restored.</small>
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
                  <details><summary>Candidate decisions</summary>{#each changeSet.plan.candidateDecisions as decision}<p><strong>{decision.decision}</strong> · {changeSet.consideredCandidates.find((candidate) => candidate.id === decision.candidateId)?.candidate.title ?? decision.candidateId}<br />{decision.reason} · {Math.round(decision.relevanceConfidence * 100)}%</p>{/each}</details>
                  <details><summary>Proposed Research State</summary>{#each changeSet.plan.entries as entry}<p><strong>{entry.kind.replaceAll("_", " ")} · {entry.epistemicStatus.replaceAll("_", " ")}</strong><br />{entry.text}</p>{/each}<p><strong>Next direction</strong><br />{changeSet.plan.nextDirection}</p></details>
                {/if}
                {#if changeSet.error}<p class="error-text">{changeSet.error}</p>{/if}
                {#if changeSet.decisionReason}<p>{changeSet.decisionReason}</p>{/if}
                {#if reviewSummary.reviewable}<div class="actions"><button type="button" disabled={working} onclick={() => void decideChangeSet(changeSet, "reject")}>Reject</button><button type="button" disabled={working} onclick={() => void decideChangeSet(changeSet, "edit")}>Edit JSON</button><button class="primary" type="button" disabled={working} onclick={() => void decideChangeSet(changeSet, "apply")}>Apply changes</button></div>{/if}
              </div>
            {/if}
            <details class="run-events">
              <summary>Run Activity · {(snapshot?.events ?? []).filter((event) => event.runId === run.id).length}</summary>
              {#each (snapshot?.events ?? []).filter((event) => event.runId === run.id) as event}
                <article class="event compact-event">
                  <div class="row"><span class="event-kind">{event.kind.replaceAll("_", " ")}</span><span class="sequence">#{event.sequence}</span></div>
                  <p>{event.summary}</p>
                  <small>{event.actor}{event.phase ? ` · ${event.phase}` : ""}{event.progressCurrent !== undefined ? ` · ${event.progressCurrent}${event.progressTotal !== undefined ? `/${event.progressTotal}` : ""}` : ""}</small>
                  {#if event.detail}<details><summary>Structured detail</summary><code>{JSON.stringify(event.detail, null, 2)}</code></details>{/if}
                </article>
              {/each}
            </details>
          </article>
        {/each}
        {#if inspectedInstructions}
          <article class="instruction-stack">
            <div class="row"><strong>Effective Run instructions</strong><button type="button" onclick={() => (inspectedInstructions = null)}>Close</button></div>
            <section><h3>Product policy · {inspectedInstructions.productPolicyVersion}</h3><p>{inspectedInstructions.productPolicySummary}</p></section>
            <section><h3>Researcher instructions</h3><p>{inspectedInstructions.projectResearchInstructions || "None"}</p></section>
            <section><h3>Structured authority</h3><p>{inspectedInstructions.structuredSettings.autonomy} · {inspectedInstructions.structuredSettings.mayAddPapers ? "may add Papers" : "cannot add Papers"} · {inspectedInstructions.structuredSettings.writableDocumentIds.length} writable Documents</p></section>
            <section><h3>Bounded context</h3><p>State r{inspectedInstructions.runContext.startingStateRevision} · {inspectedInstructions.runContext.activeEntries.length} active entries · {inspectedInstructions.runContext.priorObservations.length} operational observations · {inspectedInstructions.runContext.priorNextDirection ? "previous next direction included" : "no previous next direction"} · Vault {inspectedInstructions.runContext.vaultId} revision {inspectedInstructions.runContext.vaultRevision} · {inspectedInstructions.runContext.vaultPaperIds.length} Papers · {inspectedInstructions.runContext.maximumProviderQueries} provider queries · {inspectedInstructions.runContext.maximumLlmCalls} model calls</p></section>
          </article>
        {/if}
        {#each (snapshot?.events ?? []).filter((event) => !event.runId) as event}
          <article class="event">
            <div class="row">
              <span class="event-kind">{event.kind.replaceAll("_", " ")}</span>
              <span class="sequence">#{event.sequence}</span>
            </div>
            <p>{event.summary}</p>
            <small>{event.actor}{event.phase ? ` · ${event.phase}` : ""}{event.progressCurrent !== undefined ? ` · ${event.progressCurrent}${event.progressTotal !== undefined ? `/${event.progressTotal}` : ""}` : ""}</small>
            {#if event.detail}<details><summary>Structured detail</summary><code>{JSON.stringify(event.detail, null, 2)}</code></details>{/if}
            <time>{event.occurredAt}</time>
          </article>
        {:else}
          <div class="empty-list">Activity is recorded when a Run starts.</div>
        {/each}
      </div>
    {:else}
      <form class="panel settings" onsubmit={(event) => { event.preventDefault(); void saveSettings(); }}>
        <label>
          <span>Research goal</span>
          <textarea bind:value={goal} rows="3" placeholder="Improve LoRA interpretability"></textarea>
        </label>
        <label>
          <span>Researcher instructions</span>
          <textarea bind:value={researchInstructions} rows="5" placeholder="What should the Harness prioritize or question?"></textarea>
        </label>
        <label>
          <span>Scope</span>
          <textarea bind:value={scope} rows="3" placeholder="Questions, methods, populations, or periods that belong in this Project"></textarea>
        </label>
        <label>
          <span>Exclusions</span>
          <textarea bind:value={exclusions} rows="3" placeholder="Explicitly out-of-scope research"></textarea>
        </label>
        <label>
          <span>Preferred concepts</span>
          <input bind:value={preferredConcepts} placeholder="mechanistic analysis, causal evidence" />
        </label>
        <label>
          <span>Excluded concepts</span>
          <input bind:value={excludedConcepts} placeholder="application-only studies" />
        </label>
        <fieldset>
          <legend>Sources</legend>
          <label class="check" title="Research Runs use the browser-first scholarly discovery policy">
            <input type="checkbox" checked disabled />
            Browser discovery (required)
          </label>
          {#each [["open_alex", "OpenAlex metadata"], ["arxiv", "arXiv metadata"]] as source}
            <label class="check">
              <input
                type="checkbox"
                checked={sources.includes(source[0])}
                onchange={(event) => toggleSource(source[0], event.currentTarget.checked)}
              />
              {source[1]}
            </label>
          {/each}
        </fieldset>
        <div class="settings-row">
          <label>
            <span>Depth</span>
            <select bind:value={depth}>
              <option value="quick">Quick</option>
              <option value="standard">Standard</option>
              <option value="thorough">Thorough</option>
            </select>
          </label>
          <label>
            <span>Paper budget</span>
            <input type="number" min="1" max="100" bind:value={paperBudget} />
          </label>
        </div>
        <fieldset>
          <legend>Authority</legend>
          <label><span>Autonomy</span><select bind:value={autonomy}><option value="manual">Manual · Run now only</option><option value="propose">Propose · review changes</option><option value="automatic">Automatic · apply allowed changes</option></select></label>
          <label class="check"><input type="checkbox" bind:checked={mayAddPapers} /> May add accepted Papers to this Project's Vault</label>
          <p class="setting-note">Automatic authority remains bounded by the selected Vault and Documents. Product evidence and budget policy is never editable here.</p>
          {#if documents.length}
            <div class="document-authority">
              <strong>Writable Documents</strong>
              {#each documents as document}
                <label class="check" title={document.harnessWritable ? "Allow this Harness to update the Document" : "Enable Harness writing from the Document editor first"}>
                  <input type="checkbox" disabled={!document.harnessWritable} checked={writableDocumentIds.includes(document.id)} onchange={(event) => (writableDocumentIds = toggleSelection(writableDocumentIds, document.id, event.currentTarget.checked))} />
                  {document.title}{document.harnessWritable ? "" : " · Document opt-in required"}
                </label>
              {/each}
            </div>
          {/if}
        </fieldset>
        <fieldset>
          <legend>Schedule · runs only while i0i is open</legend>
          <label class="check"><input type="checkbox" bind:checked={scheduleEnabled} disabled={autonomy === "manual"} /> Enable local schedule</label>
          {#if autonomy === "manual"}<p class="setting-note">Choose Propose or Automatic autonomy to schedule Runs.</p>{/if}
          {#if scheduleEnabled}
            <div class="settings-row schedule-row">
              <label><span>Cadence</span><select bind:value={scheduleCadence}><option value="daily">Daily</option><option value="weekly">Weekly</option></select></label>
              {#if scheduleCadence === "weekly"}<label><span>Weekday</span><select bind:value={scheduleWeekday}><option value={1}>Monday</option><option value={2}>Tuesday</option><option value={3}>Wednesday</option><option value={4}>Thursday</option><option value={5}>Friday</option><option value={6}>Saturday</option><option value={7}>Sunday</option></select></label>{/if}
              <label><span>Local time</span><input type="time" bind:value={scheduleTime} /></label>
            </div>
            <p class="setting-note">Timezone {scheduleTimezone}. Missed intervals produce at most one startup catch-up.</p>
          {/if}
        </fieldset>
        <fieldset>
          <legend>Per-Run limits</legend>
          <div class="settings-row">
            <label><span>Provider queries</span><input type="number" min="1" bind:value={maximumProviderQueries} placeholder="Depth default" /></label>
            <label><span>Model calls</span><input type="number" min="4" bind:value={maximumLlmCalls} placeholder="Depth default" /></label>
            <label><span>Wall time · minutes</span><input type="number" min="1" bind:value={maximumRunMinutes} placeholder="No extra limit" /></label>
          </div>
        </fieldset>
        <fieldset>
          <legend>Stop when</legend>
          <div class="settings-row">
            <label><span>Completed cycles</span><input type="number" min="1" bind:value={maximumCycles} placeholder="No limit" /></label>
            <label><span>Unproductive Runs</span><input type="number" min="1" bind:value={maximumUnproductiveRuns} placeholder="No limit" /></label>
            <label><span>End date</span><input type="datetime-local" bind:value={endAt} /></label>
          </div>
          <label class="check"><input type="checkbox" bind:checked={stopOnConvergence} /> Stop after a completed Run converges</label>
          <p class="setting-note">{snapshot?.harness.completedCycleCount ?? 0} cycles · {snapshot?.harness.consecutiveUnproductiveRuns ?? 0} consecutive unproductive</p>
        </fieldset>
        <section class="policy">
          <div class="row"><strong>Product research policy</strong><span>read-only</span></div>
          <p>
            The application owns planning, evidence-ranking, budget, and safety instructions.
            Project settings are labelled researcher context and cannot replace that policy.
          </p>
          <code>{snapshot?.runs[0]?.policyVersion ?? "project-research-v1"}</code>
        </section>
        <details class="configuration-history">
          <summary>Configuration history · {configurationVersions.length} versions</summary>
          {#each configurationVersions as version}
            <article><strong>v{version.version} · {version.actor}</strong><span>{version.reason}</span><time>{version.createdAt}</time></article>
          {/each}
        </details>
        <button class="primary save" type="submit" disabled={working || !goal.trim() || sources.length === 0}>
          {working ? "Saving…" : "Save settings"}
        </button>
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
  .version,
  time,
  .sequence {
    color: var(--fg-3);
    font-size: 10px;
    text-transform: uppercase;
    letter-spacing: 0.06em;
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

  .policy p,
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

  .instruction-stack,
  .configuration-history {
    margin-bottom: 10px;
    padding: 10px;
    border: 1px solid var(--amber-mid);
    background: var(--bg-1);
  }

  .change-set-summary {
    margin-top: 10px;
    padding-top: 10px;
    border-top: 1px solid var(--border-1);
  }

  .change-set-summary details {
    margin-top: 8px;
  }

  .change-set-summary details p {
    margin-top: 7px;
    padding: 7px;
    border: 1px solid var(--border-1);
  }

  .instruction-stack > .row {
    justify-content: space-between;
  }

  .instruction-stack section {
    margin-top: 10px;
  }

  .instruction-stack h3,
  .document-authority > strong {
    display: block;
    margin: 0 0 4px;
    color: var(--fg-3);
    font-size: 10px;
    text-transform: uppercase;
  }

  .document-authority {
    margin-top: 10px;
  }

  .document-authority .check {
    display: flex;
    margin-top: 6px;
  }

  .configuration-history article {
    display: grid;
    gap: 3px;
    padding: 8px 0;
    border-bottom: 1px solid var(--border-1);
  }

  .improvement-card h3 {
    margin: 7px 0 3px;
    font-size: 13px;
  }

  .improvement-card > small {
    color: var(--fg-3);
    font-size: 10px;
  }

  .proposal-values {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 6px;
    margin: 9px 0;
  }

  .improvement-card details {
    margin: 8px 0;
    color: var(--fg-3);
    font-size: 10px;
  }

  .proposal-observation {
    margin-top: 5px;
    padding: 7px;
    border: 1px solid var(--border-1);
    color: var(--fg-2);
  }

  .proposal-observation code {
    display: block;
    overflow: hidden;
    color: var(--fg-3);
    white-space: nowrap;
    text-overflow: ellipsis;
  }

  .proposal-values > div,
  .proposal-preview {
    padding: 7px;
    border: 1px solid var(--border-1);
  }

  .proposal-values strong,
  .proposal-values span,
  .proposal-preview strong {
    display: block;
    font-size: 10px;
  }

  .proposal-values span {
    margin-top: 4px;
    color: var(--fg-2);
  }

  .event .row {
    justify-content: space-between;
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

  .settings > label,
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

  .policy {
    padding: 10px;
    border: 1px solid var(--border-2);
    background: var(--bg-1);
  }

  .policy .row {
    justify-content: space-between;
  }

  .policy .row span {
    color: var(--fg-3);
    font-size: 10px;
    text-transform: uppercase;
  }

  .policy p {
    margin: 7px 0;
    font-size: 11px;
  }

  .policy code {
    color: var(--amber);
    font-size: 10px;
  }

  .save {
    align-self: flex-start;
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
