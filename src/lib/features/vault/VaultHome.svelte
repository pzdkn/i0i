<script lang="ts">
  import { onMount, tick } from "svelte";
  import { LoaderCircle, RefreshCw, Search } from "@lucide/svelte";
  import { open, save } from "@tauri-apps/plugin-dialog";
  import { exportVaultBibtex } from "$lib/bridge/library";
  import {
    dismissVaultSuggestion,
    getVaultSuggestionOptions,
    getVaultSuggestions,
    listenVaultSuggestionPreview,
    listenVaultSuggestionUpdated,
    planVaultSuggestionQueries,
    runVaultSuggestions,
    saveVaultSuggestionOptions,
    undoVaultSuggestionDismissal,
  } from "$lib/bridge/vault-suggestions";
  import ResizableSplit from "$lib/components/layout/ResizableSplit.svelte";
  import PaperList from "$lib/features/vault/PaperList.svelte";
  import VaultSuggestionList from "$lib/features/vault/VaultSuggestionList.svelte";
  import VaultInspector from "$lib/features/vault/VaultInspector.svelte";
  import type {
    MetadataAutofillProgress,
    MetadataCandidate,
    PaperMetadataUpdate,
    VaultWorkspace,
  } from "$lib/domain/library";
  import type {
    VaultSuggestion,
    VaultSuggestionOptions,
    VaultSuggestionQueryPlan,
    VaultSuggestionRun,
    VaultSuggestionUpdated,
  } from "$lib/domain/vault-suggestion";

  let {
    workspace,
    onOpenPaper,
    onImportPdfs,
    onAddHtmlUrl,
    onAutofillMetadata,
    autofillingMetadataPaperIds,
    metadataAutofillProgressByPaperId = {},
    onApplyMetadataCandidate,
    onUpdatePaperMetadata,
    onRemovePaperFromVault,
    onRemovePaperFromLibrary,
    onOpenSuggestion,
    onAddSuggestion,
  }: {
    workspace: VaultWorkspace;
    onOpenPaper: (paperId: string) => void;
    onImportPdfs: (vaultId: string, paths: string[]) => void | Promise<void>;
    onAddHtmlUrl: (vaultId: string, url: string) => void | Promise<void>;
    onAutofillMetadata: (paperId: string) => void | Promise<void>;
    autofillingMetadataPaperIds: string[];
    metadataAutofillProgressByPaperId?: Record<string, MetadataAutofillProgress>;
    onApplyMetadataCandidate: (paperId: string, candidate: MetadataCandidate) => void | Promise<void>;
    onUpdatePaperMetadata: (paperId: string, update: PaperMetadataUpdate) => void | Promise<void>;
    onRemovePaperFromVault: (vaultId: string, paperId: string) => void;
    onRemovePaperFromLibrary: (paperId: string) => void;
    onOpenSuggestion: (suggestion: VaultSuggestion) => void;
    onAddSuggestion: (suggestion: VaultSuggestion) => void | Promise<void>;
  } = $props();

  let selectedPaperId = $state("");
  let localFilter = $state("");
  let isImporting = $state(false);
  // RFC 0070: export the vault's papers as a BibTeX file.
  let isExporting = $state(false);
  // RFC 0065: add a web page to this vault by URL.
  let showUrlInput = $state(false);
  let urlDraft = $state("");
  let isAddingUrl = $state(false);
  let urlError = $state("");
  let activeVaultView = $state<"papers" | "suggestions">("papers");
  let suggestions = $state<VaultSuggestion[]>([]);
  let latestSuggestionRun = $state<VaultSuggestionRun | undefined>();
  let activeSuggestionRunId = $state("");
  let suggestionStatus = $state("");
  let suggestionError = $state("");
  let selectedSuggestionId = $state("");
  let suggestionBusyIds = $state<string[]>([]);
  let dismissedSuggestion = $state<VaultSuggestion | undefined>();
  let refreshInProgress = $state(false);
  let planningSuggestionQueries = $state(false);
  let suggestionOptions = $state<VaultSuggestionOptions>({
    focus: null,
    yearFrom: null,
    yearTo: null,
    queryPathCount: 3,
    resultCount: 5,
    includeReviews: false,
  });
  let suggestionPlan = $state<VaultSuggestionQueryPlan | undefined>();
  let selectedSuggestionQueryIds = $state<string[]>([]);
  let suggestionActivity = $state<VaultSuggestionUpdated[]>([]);
  let preparedFocus = $state<string | null>(null);
  let preparedQueryPathCount = $state<number | undefined>();
  let preparedPaperKey = $state("");
  let suggestionLoadSequence = 0;
  // RFC 0087 R2: the filter box was bound to `localFilter` and the value was
  // never read. Substring over the three columns the list already shows —
  // a filter that reordered results would be a search.
  const visiblePapers = $derived.by(() => {
    const query = localFilter.trim().toLowerCase();
    if (!query) {
      return workspace.papers;
    }
    return workspace.papers.filter((paper) =>
      [paper.title, paper.authors.join(" "), String(paper.year)]
        .join(" ")
        .toLowerCase()
        .includes(query),
    );
  });

  const selectedPaper = $derived(
    workspace.papers.find((paper) => paper.id === selectedPaperId) ?? workspace.papers[0],
  );
  const selectedSuggestion = $derived(
    suggestions.find((suggestion) => suggestion.id === selectedSuggestionId),
  );
  const suggestionRunning = $derived(
    ["queued", "planning", "searching", "assessing", "ranking"].includes(
      latestSuggestionRun?.status ?? "",
    ) || Boolean(activeSuggestionRunId),
  );
  const suggestionBusy = $derived(planningSuggestionQueries || suggestionRunning);
  const suggestionPaperKey = $derived(
    workspace.papers.map((paper) => paper.id).sort().join("\n"),
  );
  const suggestionPlanStale = $derived(
    Boolean(suggestionPlan) &&
      (preparedFocus !== (suggestionOptions.focus?.trim() || null) ||
        preparedQueryPathCount !== suggestionOptions.queryPathCount ||
        preparedPaperKey !== suggestionPaperKey),
  );
  const suggestionToolbarStatus = $derived(
    planningSuggestionQueries
      ? "Preparing query paths"
      : suggestionRunning
      ? suggestionStatus || latestSuggestionRun?.message || "Searching"
      : suggestionPlanStale
        ? "Queries need to be regenerated"
        : suggestionPlan
          ? `${selectedSuggestionQueryIds.length} of ${suggestionPlan.queries.length} queries selected`
      : latestSuggestionRun?.finishedAt
        ? `${latestSuggestionRun.message} · ${latestSuggestionRun.finishedAt}`
        : latestSuggestionRun?.message || "Not run yet",
  );

  // R2.3: the filter is view state, and a vault switch is a new view.
  $effect(() => {
    void workspace.id;
    localFilter = "";
    activeVaultView = "papers";
    suggestionPlan = undefined;
    selectedSuggestionQueryIds = [];
    suggestionActivity = [];
    void loadSuggestions(workspace.id);
    void loadSuggestionOptions(workspace.id);
  });

  $effect(() => {
    if (!workspace.papers.some((paper) => paper.id === selectedPaperId)) {
      selectedPaperId = workspace.papers[0]?.id ?? "";
    }
  });

  async function chooseLocalPdfs() {
    if (isImporting) {
      return;
    }

    const selected = await open({
      multiple: true,
      filters: [{ name: "PDF", extensions: ["pdf"] }],
    });
    const paths = Array.isArray(selected) ? selected : selected ? [selected] : [];
    if (paths.length === 0) {
      return;
    }

    isImporting = true;
    try {
      await onImportPdfs(workspace.id, paths);
    } finally {
      isImporting = false;
    }
  }

  async function exportCitations() {
    if (isExporting || workspace.papers.length === 0) {
      return;
    }

    const path = await save({
      defaultPath: `${workspace.path}/${workspace.title}.bib`,
      filters: [{ name: "BibTeX", extensions: ["bib"] }],
    });
    if (!path) {
      return;
    }

    isExporting = true;
    try {
      await exportVaultBibtex(workspace.id, path);
    } finally {
      isExporting = false;
    }
  }

  function toggleUrlInput() {
    showUrlInput = !showUrlInput;
    urlError = "";
    if (!showUrlInput) {
      urlDraft = "";
    }
  }

  async function submitUrl() {
    const url = urlDraft.trim();
    if (!url || isAddingUrl) {
      return;
    }
    isAddingUrl = true;
    urlError = "";
    try {
      await onAddHtmlUrl(workspace.id, url);
      urlDraft = "";
      showUrlInput = false;
    } catch (error) {
      urlError = String(error);
    } finally {
      isAddingUrl = false;
    }
  }

  async function loadSuggestions(vaultId: string) {
    const sequence = ++suggestionLoadSequence;
    try {
      const snapshot = await getVaultSuggestions(vaultId);
      if (sequence !== suggestionLoadSequence || workspace.id !== vaultId) return;
      suggestions = snapshot.suggestions;
      latestSuggestionRun = snapshot.latestRun;
      selectedSuggestionId = suggestions[0]?.id ?? "";
      suggestionError = snapshot.latestRun?.error ?? "";
    } catch (error) {
      if (sequence === suggestionLoadSequence) suggestionError = String(error);
    }
  }

  async function loadSuggestionOptions(vaultId: string) {
    try {
      const options = await getVaultSuggestionOptions(vaultId);
      if (workspace.id === vaultId) suggestionOptions = options;
    } catch (error) {
      if (workspace.id === vaultId) suggestionError = String(error);
    }
  }

  async function updateSuggestionOptions(options: VaultSuggestionOptions) {
    suggestionOptions = options;
    suggestionError = "";
    try {
      await saveVaultSuggestionOptions(workspace.id, options);
    } catch (error) {
      suggestionError = String(error);
    }
  }

  async function prepareSuggestionQueries() {
    if (suggestionBusy || workspace.papers.length === 0) return;
    planningSuggestionQueries = true;
    suggestionError = "";
    suggestionStatus = "Preparing query paths";
    try {
      const plan = await planVaultSuggestionQueries(workspace.id, suggestionOptions);
      suggestionPlan = plan;
      selectedSuggestionQueryIds = plan.queries.map((query) => query.id);
      preparedFocus = suggestionOptions.focus?.trim() || null;
      preparedQueryPathCount = suggestionOptions.queryPathCount;
      preparedPaperKey = suggestionPaperKey;
      suggestionStatus = `${plan.queries.length} queries ready`;
    } catch (error) {
      suggestionError = String(error);
    } finally {
      planningSuggestionQueries = false;
    }
  }

  function toggleSuggestionQuery(queryId: string) {
    selectedSuggestionQueryIds = selectedSuggestionQueryIds.includes(queryId)
      ? selectedSuggestionQueryIds.filter((id) => id !== queryId)
      : [...selectedSuggestionQueryIds, queryId];
  }

  function selectAllSuggestionQueries() {
    selectedSuggestionQueryIds = suggestionPlan?.queries.map((query) => query.id) ?? [];
  }

  onMount(() => {
    const unlisteners: Array<() => void> = [];
    void listenVaultSuggestionUpdated((event) => {
      if (event.vaultId !== workspace.id) return;
      if (event.sequence > 0) {
        suggestionActivity = [...suggestionActivity, event]
          .sort((left, right) => left.sequence - right.sequence)
          .slice(-8);
      }
      activeSuggestionRunId = event.status === "ready" || event.status === "failed" || event.status === "cancelled" ? "" : event.runId;
      suggestionStatus = event.message;
      latestSuggestionRun = {
        id: event.runId,
        vaultId: event.vaultId,
        status: event.status,
        message: event.message,
        resultCount: event.found,
        createdAt: latestSuggestionRun?.createdAt ?? "",
        options: latestSuggestionRun?.options ?? suggestionOptions,
        queryPaths: latestSuggestionRun?.queryPaths ?? suggestionPlan?.queries ?? [],
      };
      if (event.status === "ready") {
        refreshInProgress = false;
        void loadSuggestions(workspace.id);
      } else if (event.status === "failed" || event.status === "cancelled") {
        const wasRefresh = refreshInProgress;
        refreshInProgress = false;
        suggestionError = event.status === "failed" ? event.message : "";
        if (!wasRefresh) suggestions = [];
      }
    }).then((unlisten) => unlisteners.push(unlisten));
    void listenVaultSuggestionPreview((event) => {
      if (event.vaultId !== workspace.id || event.runId !== activeSuggestionRunId || refreshInProgress) return;
      const known = new Set(suggestions.map((suggestion) => suggestion.paperRef));
      const incoming = event.suggestions.filter((suggestion) => !known.has(suggestion.paperRef));
      suggestions = [...suggestions, ...incoming].slice(0, suggestionOptions.resultCount);
      selectedSuggestionId ||= suggestions[0]?.id ?? "";
    }).then((unlisten) => unlisteners.push(unlisten));
    return () => unlisteners.forEach((unlisten) => unlisten());
  });

  async function runSelectedSuggestionQueries() {
    if (
      suggestionBusy ||
      !suggestionPlan ||
      suggestionPlanStale ||
      selectedSuggestionQueryIds.length === 0
    ) return;
    suggestionError = "";
    suggestionStatus = "Queued";
    suggestionActivity = [];
    refreshInProgress = suggestions.length > 0;
    if (!refreshInProgress) suggestions = [];
    try {
      activeSuggestionRunId = await runVaultSuggestions(
        workspace.id,
        suggestionPlan.id,
        selectedSuggestionQueryIds,
        suggestionOptions,
      );
    } catch (error) {
      refreshInProgress = false;
      suggestionError = String(error);
    }
  }

  async function addSuggestion(suggestion: VaultSuggestion) {
    suggestionBusyIds = [...suggestionBusyIds, suggestion.id];
    const index = suggestions.findIndex((item) => item.id === suggestion.id);
    try {
      await onAddSuggestion(suggestion);
      suggestions = suggestions.filter((item) => item.id !== suggestion.id);
      await focusSuggestionAfterRemoval(index);
    } catch (error) {
      suggestionError = String(error);
    } finally {
      suggestionBusyIds = suggestionBusyIds.filter((id) => id !== suggestion.id);
    }
  }

  async function dismissSuggestion(suggestion: VaultSuggestion, keyboard: boolean) {
    const index = suggestions.findIndex((item) => item.id === suggestion.id);
    suggestionBusyIds = [...suggestionBusyIds, suggestion.id];
    try {
      await dismissVaultSuggestion(suggestion);
      dismissedSuggestion = suggestion;
      suggestions = suggestions.filter((item) => item.id !== suggestion.id);
      await focusSuggestionAfterRemoval(index);
      if (keyboard) {
        await tick();
        document.querySelector<HTMLButtonElement>(".undo-dismiss")?.focus();
      }
    } catch (error) {
      suggestionError = String(error);
    } finally {
      suggestionBusyIds = suggestionBusyIds.filter((id) => id !== suggestion.id);
    }
  }

  async function undoDismissal() {
    if (!dismissedSuggestion) return;
    const snapshot = await undoVaultSuggestionDismissal(dismissedSuggestion.id);
    suggestions = snapshot.suggestions;
    selectedSuggestionId = dismissedSuggestion.id;
    dismissedSuggestion = undefined;
  }

  async function focusSuggestionAfterRemoval(index: number) {
    selectedSuggestionId = suggestions[Math.min(index, suggestions.length - 1)]?.id ?? "";
    await tick();
    if (suggestions.length > 0) {
      document.querySelector<HTMLButtonElement>(".suggestion-row.selected .suggestion-primary")?.focus();
    } else {
      document.querySelector<HTMLButtonElement>('[role="tab"][aria-controls="vault-suggestions-panel"]')?.focus();
    }
  }

  function activateVaultView(view: "papers" | "suggestions") {
    activeVaultView = view;
  }

  function handleTabKeydown(event: KeyboardEvent, view: "papers" | "suggestions") {
    if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return;
    event.preventDefault();
    const next = view === "papers" ? "suggestions" : "papers";
    activeVaultView = next;
    void tick().then(() => document.querySelector<HTMLButtonElement>(`#vault-${next}-tab`)?.focus());
  }
</script>

<section class="workspace col">
  <div class="vault-home row">
    <ResizableSplit
      storageKey="i0i.vault-split"
      panes={[
        { id: "papers", min: 360, default: 900 },
        { id: "inspector", min: 220, default: 320 },
      ]}
    >
      {#snippet pane(id: string)}
        {#if id === "papers"}
          <main class="vault-main col">
            <header class="folder-header hair-b">
              <div class="row heading-line">
                <div>
                  <div class="label">Folder</div>
                  <h1>{workspace.title}</h1>
                </div>
                <span class="mono-dim">{workspace.summary}</span>
                <div class="flex1"></div>
                <div class="actions row">
                  <button class="btn" type="button" disabled={isImporting} onclick={chooseLocalPdfs}>
                    {isImporting ? "Importing" : "Import PDF"}
                  </button>
                  <button class="btn" type="button" class:active={showUrlInput} onclick={toggleUrlInput}>
                    Add web page
                  </button>
                  <button
                    class="btn"
                    type="button"
                    disabled={isExporting || workspace.papers.length === 0}
                    onclick={exportCitations}
                  >
                    {isExporting ? "Exporting" : "Export .bib"}
                  </button>
                </div>
              </div>

              {#if showUrlInput}
                <div class="url-bar row">
                  <input
                    class="url-input"
                    type="url"
                    bind:value={urlDraft}
                    disabled={isAddingUrl}
                    placeholder="https://example.com/article"
                    aria-label="Web page URL"
                    onkeydown={(event) => {
                      if (event.key === "Enter") void submitUrl();
                      if (event.key === "Escape") toggleUrlInput();
                    }}
                  />
                  <button class="btn primary" type="button" disabled={isAddingUrl || !urlDraft.trim()} onclick={() => void submitUrl()}>
                    {isAddingUrl ? "Adding…" : "Add"}
                  </button>
                  <button class="btn" type="button" disabled={isAddingUrl} onclick={toggleUrlInput}>Cancel</button>
                </div>
                {#if urlError}
                  <p class="url-error">{urlError}</p>
                {/if}
              {/if}

              <div class="row chip-row">
                {#each workspace.chips as chip, index}
                  <span class:hot={index === 0} class="chip">{chip}</span>
                {/each}
                <div class="flex1"></div>
                {#if activeVaultView === "papers"}
                  <label class="inline-filter row">
                    <span>filter</span>
                    <input bind:value={localFilter} aria-label="Paper filter" placeholder="title, author, year" />
                    {#if localFilter.trim()}
                      <span class="mono-dim">{visiblePapers.length}/{workspace.papers.length}</span>
                    {/if}
                  </label>
                  <span class="mono-dim">sort recent</span>
                {/if}
              </div>
            </header>

            <div class="view-tabs row hair-b" role="tablist" aria-label="Vault views">
              <button
                id="vault-papers-tab"
                type="button"
                role="tab"
                aria-selected={activeVaultView === "papers"}
                aria-controls="vault-papers-panel"
                tabindex={activeVaultView === "papers" ? 0 : -1}
                class:active={activeVaultView === "papers"}
                onclick={() => activateVaultView("papers")}
                onkeydown={(event) => handleTabKeydown(event, "papers")}
              >Papers <strong>{workspace.papers.length}</strong></button>
              <button
                id="vault-suggestions-tab"
                type="button"
                role="tab"
                aria-selected={activeVaultView === "suggestions"}
                aria-controls="vault-suggestions-panel"
                tabindex={activeVaultView === "suggestions" ? 0 : -1}
                class:active={activeVaultView === "suggestions"}
                onclick={() => activateVaultView("suggestions")}
                onkeydown={(event) => handleTabKeydown(event, "suggestions")}
              >Suggestions <strong>{suggestions.length}</strong>{#if suggestionBusy}<LoaderCircle size={11} class="tab-spinner" />{/if}</button>
            </div>

            {#if activeVaultView === "papers"}
              <div id="vault-papers-panel" role="tabpanel" aria-labelledby="vault-papers-tab" class="view-panel">
                <PaperList
                  papers={visiblePapers}
                  {selectedPaperId}
                  {autofillingMetadataPaperIds}
                  {metadataAutofillProgressByPaperId}
                  onSelect={(paperId) => (selectedPaperId = paperId)}
                  onOpen={(paperId) => {
                    selectedPaperId = paperId;
                    onOpenPaper(paperId);
                  }}
                  {onAutofillMetadata}
                  onRemoveFromVault={(paperId) => onRemovePaperFromVault(workspace.id, paperId)}
                  onRemoveFromLibrary={onRemovePaperFromLibrary}
                />
                {#if localFilter.trim() && visiblePapers.length === 0}
                  <p class="filter-empty">No papers match “{localFilter.trim()}”. <button class="link-btn" type="button" onclick={() => (localFilter = "")}>Clear</button></p>
                {/if}
              </div>
            {:else}
              <div id="vault-suggestions-panel" role="tabpanel" aria-labelledby="vault-suggestions-tab" class="view-panel suggestions-panel">
                <div class="suggestion-toolbar row hair-b">
                  <span class="mono-dim" aria-live="polite">
                    {suggestionToolbarStatus}
                  </span>
                  <div class="flex1"></div>
                  {#if workspace.papers.length > 0}
                    <button
                      class="suggestion-run"
                      type="button"
                      disabled={suggestionBusy}
                      aria-busy={suggestionBusy}
                      title={suggestionPlan ? "Regenerate suggestion queries" : "Prepare suggestion queries"}
                      aria-label={suggestionPlan ? "Regenerate suggestion queries" : "Prepare suggestion queries"}
                      onclick={prepareSuggestionQueries}
                    >
                      {#if suggestionBusy}
                        <LoaderCircle size={14} class="spinning" />
                      {:else if suggestionPlan}
                        <RefreshCw size={14} />
                      {:else}
                        <Search size={14} />
                      {/if}
                    </button>
                  {/if}
                </div>
                {#if dismissedSuggestion}
                  <div class="undo-bar" role="status">Dismissed · <button class="undo-dismiss" type="button" onclick={undoDismissal}>Undo</button></div>
                {/if}
                {#if suggestionError}
                  <div class="suggestion-error-banner" role="alert">{suggestionError}</div>
                {/if}
                {#if workspace.papers.length === 0}
                  <div class="suggestion-empty">No papers to match yet</div>
                {:else if suggestions.length > 0}
                  <VaultSuggestionList
                    {suggestions}
                    selectedId={selectedSuggestionId}
                    busyIds={suggestionBusyIds}
                    onSelect={(suggestion) => (selectedSuggestionId = suggestion.id)}
                    onOpen={onOpenSuggestion}
                    onAdd={addSuggestion}
                    onDismiss={dismissSuggestion}
                  />
                {:else if suggestionRunning}
                  <div class="suggestion-empty">{suggestionStatus || "Searching"}</div>
                {:else if suggestionPlan}
                  <div class="suggestion-empty">Review the proposed queries in the inspector.</div>
                {:else if latestSuggestionRun?.status === "ready"}
                  <div class="suggestion-empty">No new suggestions</div>
                {:else}
                  <div class="suggestion-empty"><button class="link-btn" type="button" onclick={prepareSuggestionQueries}>Prepare suggestion queries</button></div>
                {/if}
                {#if workspace.papers.length > 0 && workspace.papers.length < 3}
                  <p class="small-basis">Based on {workspace.papers.length} paper{workspace.papers.length === 1 ? "" : "s"}; suggestions may be broad.</p>
                {/if}
              </div>
            {/if}
          </main>
        {:else}
          <VaultInspector
            papers={workspace.papers}
            {selectedPaper}
            {activeVaultView}
            {selectedSuggestion}
            suggestionCount={suggestions.length}
            suggestionRun={latestSuggestionRun}
            {suggestionOptions}
            {suggestionPlan}
            selectedSuggestionQueryIds={selectedSuggestionQueryIds}
            suggestionPlanStale={suggestionPlanStale}
            suggestionBusy={suggestionBusy}
            planningSuggestionQueries={planningSuggestionQueries}
            activity={suggestionActivity}
            onSuggestionOptionsChange={updateSuggestionOptions}
            onPrepareSuggestionQueries={prepareSuggestionQueries}
            onToggleSuggestionQuery={toggleSuggestionQuery}
            onSelectAllSuggestionQueries={selectAllSuggestionQueries}
            onRunSelectedSuggestionQueries={runSelectedSuggestionQueries}
            onOpenSuggestions={() => (activeVaultView = "suggestions")}
            {metadataAutofillProgressByPaperId}
            {autofillingMetadataPaperIds}
            {onAutofillMetadata}
            {onApplyMetadataCandidate}
            {onUpdatePaperMetadata}
          />
        {/if}
      {/snippet}
    </ResizableSplit>
  </div>
</section>

<style>
  .workspace {
    flex: 1;
    min-width: 0;
    min-height: 0;
    background: var(--bg);
  }

  .vault-home {
    flex: 1;
    min-height: 0;
    align-items: stretch;
  }

  .vault-main {
    flex: 1;
    min-width: 0;
    min-height: 0;
    background: var(--bg);
  }

  .folder-header {
    flex-shrink: 0;
    padding: 14px 18px 12px;
    background: var(--bg-1);
  }

  .heading-line {
    gap: 12px;
    align-items: baseline;
  }

  h1 {
    margin: 2px 0 0;
    color: var(--amber);
    font-size: 22px;
    font-weight: 600;
  }

  .actions {
    gap: 6px;
  }

  .btn.active {
    border-color: var(--amber);
    color: var(--amber);
  }

  .url-bar {
    gap: 6px;
    align-items: center;
    margin-top: 10px;
  }

  .url-input {
    flex: 1;
    min-width: 0;
    height: 28px;
    border: 1px solid var(--border-2);
    outline: none;
    background: var(--bg);
    color: var(--fg-1);
    padding: 0 9px;
    font: inherit;
    font-size: 12px;
  }

  .url-input:focus {
    border-color: var(--cyan);
  }

  .url-error {
    margin: 6px 0 0;
    color: var(--red);
    font-size: 11px;
    line-height: 1.45;
  }

  .chip-row {
    gap: 6px;
    margin-top: 12px;
    overflow: hidden;
    color: var(--fg-3);
    font-size: 10px;
    white-space: nowrap;
  }

  .filter-empty {
    margin: 10px 12px;
    color: var(--fg-3);
    font-size: 11px;
  }

  .link-btn {
    padding: 0;
    border: 0;
    background: transparent;
    color: var(--amber);
    font: inherit;
    font-size: 11px;
    cursor: pointer;
  }

  .inline-filter {
    gap: 6px;
    height: 20px;
    color: var(--fg-3);
  }

  .inline-filter input {
    width: 80px;
    border: 1px solid var(--border-2);
    outline: none;
    background: var(--bg);
    color: var(--fg-2);
    font: inherit;
    font-size: 10px;
  }

  .view-tabs {
    height: 28px;
    flex-shrink: 0;
    gap: 18px;
    padding: 0 18px;
    background: var(--bg-1);
    color: var(--fg-2);
    font-size: 11px;
  }

  .view-tabs button {
    height: 100%;
    display: inline-flex;
    align-items: center;
    gap: 4px;
    padding: 0;
    border: 0;
    border-bottom: 1px solid transparent;
    outline: none;
    background: transparent;
    color: inherit;
    font: inherit;
    white-space: nowrap;
    cursor: pointer;
  }

  .view-tabs button.active {
    border-bottom: 1px solid var(--amber);
    color: var(--amber);
  }

  .view-tabs button:focus-visible {
    outline: 1px solid var(--cyan);
    outline-offset: -2px;
  }

  .view-tabs strong {
    color: var(--amber-mid);
    font-weight: 500;
  }

  .view-panel {
    min-height: 0;
    flex: 1;
    display: flex;
    flex-direction: column;
  }

  .suggestion-toolbar {
    min-height: 34px;
    flex-shrink: 0;
    align-items: center;
    padding: 0 12px 0 14px;
    background: var(--bg-1);
  }

  .suggestion-run {
    width: 24px;
    height: 24px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    padding: 0;
    border: 1px solid var(--border-2);
    background: var(--bg);
    color: var(--fg-2);
    font: inherit;
    font-size: 10px;
    cursor: pointer;
  }

  .suggestion-run:hover:not(:disabled) {
    border-color: var(--cyan);
    color: var(--cyan);
  }

  .suggestion-run:disabled {
    opacity: 0.65;
  }

  :global(.suggestion-run .spinning),
  :global(.tab-spinner) {
    animation: spin 1s linear infinite;
  }

  .undo-bar {
    flex-shrink: 0;
    padding: 5px 14px;
    border-bottom: 1px solid var(--border);
    background: rgba(242, 169, 59, 0.05);
    color: var(--fg-2);
    font-size: 10px;
  }

  .undo-dismiss {
    padding: 0;
    border: 0;
    background: transparent;
    color: var(--amber);
    font: inherit;
    cursor: pointer;
  }

  .suggestion-empty {
    min-height: 120px;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 7px;
    padding: 20px;
    color: var(--fg-3);
    font-size: 11px;
    text-align: center;
  }

  .suggestion-error-banner {
    flex-shrink: 0;
    padding: 6px 14px;
    border-bottom: 1px solid var(--border);
    background: rgba(217, 92, 76, 0.08);
    color: var(--red);
    font-size: 10px;
    line-height: 1.4;
    overflow-wrap: anywhere;
  }

  .small-basis {
    margin: auto 14px 10px;
    color: var(--fg-3);
    font-size: 10px;
  }

  @keyframes spin {
    to { transform: rotate(360deg); }
  }

  @media (prefers-reduced-motion: reduce) {
    :global(.suggestion-run .spinning),
    :global(.tab-spinner) {
      animation: none;
    }
  }
</style>
