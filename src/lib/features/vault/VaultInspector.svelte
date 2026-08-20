<script lang="ts">
  import { LoaderCircle, Play, RefreshCw, Search } from "@lucide/svelte";
  import type {
    MetadataAutofillProgress,
    MetadataCandidate,
    PaperMetadataUpdate,
  } from "$lib/domain/library";
  import type { Paper } from "$lib/domain/paper";
  import MetadataPanel from "$lib/features/library/MetadataPanel.svelte";
  import type {
    VaultSuggestion,
    VaultSuggestionOptions,
    VaultSuggestionQueryPlan,
    VaultSuggestionRun,
    VaultSuggestionUpdated,
  } from "$lib/domain/vault-suggestion";

  let {
    papers,
    selectedPaper,
    metadataAutofillProgressByPaperId = {},
    autofillingMetadataPaperIds = [],
    onAutofillMetadata,
    onApplyMetadataCandidate,
    onUpdatePaperMetadata,
    activeVaultView,
    selectedSuggestion,
    suggestionCount,
    suggestionRun,
    suggestionOptions,
    suggestionPlan,
    selectedSuggestionQueryIds,
    suggestionPlanStale,
    suggestionBusy,
    planningSuggestionQueries,
    activity,
    onSuggestionOptionsChange,
    onPrepareSuggestionQueries,
    onToggleSuggestionQuery,
    onSelectAllSuggestionQueries,
    onRunSelectedSuggestionQueries,
    onOpenSuggestions,
  }: {
    papers: Paper[];
    selectedPaper: Paper | undefined;
    metadataAutofillProgressByPaperId?: Record<string, MetadataAutofillProgress>;
    autofillingMetadataPaperIds?: string[];
    onAutofillMetadata: (paperId: string) => void | Promise<void>;
    onApplyMetadataCandidate: (paperId: string, candidate: MetadataCandidate) => void | Promise<void>;
    onUpdatePaperMetadata: (paperId: string, update: PaperMetadataUpdate) => void | Promise<void>;
    activeVaultView: "papers" | "suggestions";
    selectedSuggestion: VaultSuggestion | undefined;
    suggestionCount: number;
    suggestionRun: VaultSuggestionRun | undefined;
    suggestionOptions: VaultSuggestionOptions;
    suggestionPlan: VaultSuggestionQueryPlan | undefined;
    selectedSuggestionQueryIds: string[];
    suggestionPlanStale: boolean;
    suggestionBusy: boolean;
    planningSuggestionQueries: boolean;
    activity: VaultSuggestionUpdated[];
    onSuggestionOptionsChange: (options: VaultSuggestionOptions) => void | Promise<void>;
    onPrepareSuggestionQueries: () => void | Promise<void>;
    onToggleSuggestionQuery: (queryId: string) => void;
    onSelectAllSuggestionQueries: () => void;
    onRunSelectedSuggestionQueries: () => void | Promise<void>;
    onOpenSuggestions: () => void;
  } = $props();

  const readCount = $derived(papers.filter((paper) => paper.status === "READ").length);
  const readingCount = $derived(papers.filter((paper) => paper.status === "READING").length);
  const unreadCount = $derived(papers.filter((paper) => paper.status === "UNREAD").length);
  const annotationCount = $derived(
    papers.reduce((total, paper) => total + paper.annotationCount, 0),
  );
  const yearError = $derived.by(() => {
    const { yearFrom, yearTo } = suggestionOptions;
    if ([yearFrom, yearTo].some((year) => year !== null && (year < 1000 || year > 2100))) {
      return "Years must be between 1000 and 2100";
    }
    return yearFrom !== null && yearTo !== null && yearFrom > yearTo
      ? "Start year must not exceed end year"
      : "";
  });
  const allQueriesSelected = $derived(
    Boolean(suggestionPlan) &&
      selectedSuggestionQueryIds.length === suggestionPlan?.queries.length,
  );
  const canRunSelected = $derived(
    Boolean(suggestionPlan) &&
      !suggestionPlanStale &&
      !suggestionBusy &&
      !yearError &&
      selectedSuggestionQueryIds.length > 0,
  );

  function optionalNumber(value: string): number | null {
    return value.trim() ? Number(value) : null;
  }
</script>

<aside class="inspector hair-l">
  <header class="hair-b">
    {#if activeVaultView === "suggestions" && selectedSuggestion}
      <div class="paper-name truncate">{selectedSuggestion.candidate.title}</div>
      <div class="mono-dim">suggested / {selectedSuggestion.candidate.sourceProvider}</div>
    {:else if activeVaultView === "suggestions"}
      <div class="paper-name truncate">Suggestions</div>
      <div class="mono-dim">{suggestionRun?.message ?? "No suggestion selected"}</div>
    {:else if selectedPaper}
      <div class="paper-name truncate">{selectedPaper.title}</div>
      <div class="mono-dim">{selectedPaper.year} / {selectedPaper.venue} / selected</div>
    {:else}
      <div class="paper-name truncate">No paper selected</div>
      <div class="mono-dim">Add papers to this vault</div>
    {/if}
  </header>

  {#if activeVaultView === "suggestions"}
    <div class="suggestion-controls">
      <section class="control-section hair-b">
        <div class="label hot">Search settings</div>
        <label class="field wide-field">
          <span>Focus within this vault</span>
          <input
            type="text"
            maxlength="500"
            value={suggestionOptions.focus ?? ""}
            disabled={suggestionBusy}
            placeholder="optional focus"
            oninput={(event) => onSuggestionOptionsChange({
              ...suggestionOptions,
              focus: event.currentTarget.value || null,
            })}
          />
        </label>
        <div class="control-grid">
          <label class="field">
            <span>Year from</span>
            <input
              type="number"
              min="1000"
              max="2100"
              value={suggestionOptions.yearFrom ?? ""}
              disabled={suggestionBusy}
              oninput={(event) => onSuggestionOptionsChange({
                ...suggestionOptions,
                yearFrom: optionalNumber(event.currentTarget.value),
              })}
            />
          </label>
          <label class="field">
            <span>Year to</span>
            <input
              type="number"
              min="1000"
              max="2100"
              value={suggestionOptions.yearTo ?? ""}
              disabled={suggestionBusy}
              oninput={(event) => onSuggestionOptionsChange({
                ...suggestionOptions,
                yearTo: optionalNumber(event.currentTarget.value),
              })}
            />
          </label>
          <label class="field" title="Number of search angles to prepare">
            <span>Propose</span>
            <select
              value={suggestionOptions.queryPathCount}
              disabled={suggestionBusy}
              onchange={(event) => onSuggestionOptionsChange({
                ...suggestionOptions,
                queryPathCount: Number(event.currentTarget.value) as 1 | 3 | 5,
              })}
            >
              <option value="1">1 query</option>
              <option value="3">3 queries</option>
              <option value="5">5 queries</option>
            </select>
          </label>
          <label class="field" title="Maximum number of suggestions to keep">
            <span>Results</span>
            <select
              value={suggestionOptions.resultCount}
              disabled={suggestionBusy}
              onchange={(event) => onSuggestionOptionsChange({
                ...suggestionOptions,
                resultCount: Number(event.currentTarget.value) as 3 | 5 | 10,
              })}
            >
              <option value="3">3 papers</option>
              <option value="5">5 papers</option>
              <option value="10">10 papers</option>
            </select>
          </label>
        </div>
        {#if yearError}<p class="control-error">{yearError}</p>{/if}
        <button
          class="control-command"
          type="button"
          disabled={suggestionBusy || Boolean(yearError) || papers.length === 0}
          aria-busy={planningSuggestionQueries}
          onclick={onPrepareSuggestionQueries}
        >
          {#if planningSuggestionQueries}
            <LoaderCircle size={13} class="spinning" /> Preparing
          {:else if suggestionPlan}
            <RefreshCw size={13} /> Regenerate queries
          {:else}
            <Search size={13} /> Prepare queries
          {/if}
        </button>
      </section>

      {#if suggestionPlan}
        <section class="control-section hair-b">
          <div class="section-heading">
            <div class="label hot">Proposed queries</div>
            {#if !allQueriesSelected}
              <button class="text-command" type="button" onclick={onSelectAllSuggestionQueries}>Select all</button>
            {/if}
          </div>
          {#if suggestionPlanStale}
            <p class="stale-message">Vault or focus changed. Regenerate before running.</p>
          {/if}
          <div class="query-paths">
            {#each suggestionPlan.queries as query}
              <label class="query-path">
                <input
                  type="checkbox"
                  checked={selectedSuggestionQueryIds.includes(query.id)}
                  disabled={suggestionBusy}
                  onchange={() => onToggleSuggestionQuery(query.id)}
                />
                <span><strong>{query.intent}</strong><small>{query.query}</small></span>
              </label>
            {/each}
          </div>
          <button
            class="control-command run-command"
            type="button"
            disabled={!canRunSelected}
            aria-busy={suggestionBusy && !planningSuggestionQueries}
            onclick={onRunSelectedSuggestionQueries}
          >
            {#if suggestionBusy && !planningSuggestionQueries}
              <LoaderCircle size={13} class="spinning" /> Searching
            {:else}
              <Play size={13} /> Run {selectedSuggestionQueryIds.length} selected
            {/if}
          </button>
        </section>
      {/if}

      {#if activity.length > 0}
        <section class="control-section hair-b" aria-live="polite">
          <div class="label hot">Activity</div>
          <ol class="activity-list">
            {#each activity as item}
              <li>
                <i class:active={item.phase !== "complete"}></i>
                <span><strong>{item.label}</strong>{#if item.detail}<small>{item.detail}</small>{/if}</span>
              </li>
            {/each}
          </ol>
        </section>
      {/if}

      {#if selectedSuggestion}
        <section class="control-section">
          <div class="label hot">Why this vault</div>
          <p class="suggestion-reason">{selectedSuggestion.reason}</p>
          <div class="meta">
            <div><span>year</span>{selectedSuggestion.candidate.year ?? "—"}</div>
            <div><span>venue</span>{selectedSuggestion.candidate.venue ?? "—"}</div>
            <div><span>cites</span>{selectedSuggestion.candidate.citationCount ?? 0}</div>
          </div>
        </section>
      {/if}
    </div>
  {:else if selectedPaper}
    <section class="metadata-section">
      <MetadataPanel
        paperId={selectedPaper.id}
        title={selectedPaper.title}
        authors={selectedPaper.authors}
        venue={selectedPaper.venue}
        year={selectedPaper.year}
        abstractText={selectedPaper.abstract}
        tags={selectedPaper.tags}
        progress={metadataAutofillProgressByPaperId[selectedPaper.id]}
        isAutofilling={autofillingMetadataPaperIds.includes(selectedPaper.id)}
        onAutofill={onAutofillMetadata}
        onApplyCandidate={onApplyMetadataCandidate}
        onSaveMetadata={onUpdatePaperMetadata}
      />
      <div class="meta">
        <div><span>cites</span>{selectedPaper.citations.toLocaleString()}</div>
        <div><span>highlights</span>{selectedPaper.highlightCount}</div>
        <div><span>ann</span>{selectedPaper.annotationCount}</div>
      </div>
    </section>
  {/if}

  {#if activeVaultView === "papers"}
  <section>
    <div class="label hot">Reading stats</div>
    <div class="stat-row">
      <span>read</span>
      <div class="gauge"><i style={`width: ${papers.length ? (readCount / papers.length) * 100 : 0}%`}></i></div>
      <span>{readCount} / {papers.length}</span>
    </div>
    <div class="stat-row">
      <span>reading</span>
      <div class="gauge"><i style={`width: ${papers.length ? (readingCount / papers.length) * 100 : 0}%`}></i></div>
      <span>{readingCount} / {papers.length}</span>
    </div>
    <div class="stat-row">
      <span>unread</span>
      <div class="gauge"><i style={`width: ${papers.length ? (unreadCount / papers.length) * 100 : 0}%`}></i></div>
      <span>{unreadCount} / {papers.length}</span>
    </div>
  </section>
  {/if}

  {#if activeVaultView === "papers"}
    <div class="flex1"></div>
    <section class="footer-section hair-t">
      <button class="suggestion-link" type="button" onclick={onOpenSuggestions}>
        <span>Suggestions</span><strong>{suggestionCount}</strong>
      </button>
      <div class="label">Folder health</div>
      <div class="meta">
        <div><span>papers</span>{papers.length}</div>
        <div><span>annotations</span>{annotationCount}</div>
        <div><span>bibtex</span>ready</div>
      </div>
    </section>
  {/if}
</aside>

<style>
  .inspector {
    width: 100%;
    height: 100%;
    flex-shrink: 0;
    display: flex;
    flex-direction: column;
    overflow: hidden;
    background: var(--panel);
  }

  header {
    flex-shrink: 0;
    padding: 10px 14px;
    background: var(--bg-1);
    font-size: 10px;
  }

  .paper-name {
    margin-bottom: 2px;
    color: var(--amber);
    font-size: 11px;
  }

  section {
    padding: 14px 14px 0;
  }

  .suggestion-controls {
    min-height: 0;
    overflow-y: auto;
  }

  .control-section {
    padding: 12px 14px;
  }

  .control-grid {
    display: grid;
    grid-template-columns: minmax(0, 1fr) minmax(0, 1fr);
    gap: 8px;
    margin-top: 9px;
  }

  .field {
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 4px;
    color: var(--fg-3);
    font-size: 9px;
  }

  .wide-field {
    margin-top: 9px;
  }

  .field input,
  .field select {
    width: 100%;
    min-width: 0;
    height: 25px;
    box-sizing: border-box;
    border: 1px solid var(--border-2);
    border-radius: 0;
    outline: none;
    background: var(--bg);
    color: var(--fg-2);
    padding: 0 6px;
    font: inherit;
    font-size: 10px;
  }

  .field input:focus,
  .field select:focus {
    border-color: var(--cyan);
  }

  .query-path {
    display: flex;
    align-items: flex-start;
    gap: 7px;
    color: var(--fg-2);
    font-size: 10px;
  }

  .query-path input {
    flex: 0 0 auto;
    margin: 1px 0 0;
    accent-color: var(--cyan);
  }

  .control-command {
    width: 100%;
    min-height: 27px;
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 6px;
    margin-top: 10px;
    border: 1px solid var(--border-2);
    background: var(--bg);
    color: var(--fg-2);
    font: inherit;
    font-size: 10px;
    cursor: pointer;
  }

  .control-command:hover:not(:disabled) {
    border-color: var(--cyan);
    color: var(--cyan);
  }

  .control-command:disabled {
    opacity: 0.5;
    cursor: default;
  }

  .run-command {
    color: var(--cyan);
  }

  .section-heading {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
  }

  .text-command {
    padding: 0;
    border: 0;
    background: transparent;
    color: var(--cyan);
    font: inherit;
    font-size: 9px;
    cursor: pointer;
  }

  .query-paths {
    display: flex;
    flex-direction: column;
    gap: 9px;
    margin-top: 9px;
  }

  .query-path span {
    min-width: 0;
  }

  .query-path strong,
  .query-path small,
  .activity-list strong,
  .activity-list small {
    display: block;
    overflow-wrap: anywhere;
    font-weight: 400;
    line-height: 1.35;
  }

  .query-path strong,
  .activity-list strong {
    color: var(--fg-2);
    font-size: 10px;
  }

  .query-path small,
  .activity-list small {
    margin-top: 2px;
    color: var(--fg-3);
    font-size: 9px;
  }

  .activity-list {
    display: flex;
    flex-direction: column;
    gap: 8px;
    margin: 9px 0 0;
    padding: 0;
    list-style: none;
  }

  .activity-list li {
    display: grid;
    grid-template-columns: 7px minmax(0, 1fr);
    gap: 7px;
  }

  .activity-list i {
    width: 5px;
    height: 5px;
    margin-top: 4px;
    border-radius: 50%;
    background: var(--fg-3);
  }

  .activity-list i.active {
    background: var(--cyan);
  }

  .control-error,
  .stale-message {
    margin: 8px 0 0;
    color: var(--red);
    font-size: 9px;
    line-height: 1.4;
  }

  :global(.control-command .spinning) {
    animation: inspector-spin 1s linear infinite;
  }

  .metadata-section {
    overflow-y: auto;
    max-height: 55%;
    flex-shrink: 0;
  }

  .stat-row {
    display: grid;
    grid-template-columns: 64px 1fr 52px;
    align-items: center;
    gap: 8px;
    margin-top: 8px;
    color: var(--fg-2);
    font-size: 11px;
  }

  .meta {
    display: flex;
    flex-direction: column;
    gap: 4px;
    margin-top: 8px;
    color: var(--fg-2);
    font-size: 10px;
  }

  .meta div {
    display: grid;
    grid-template-columns: 58px 1fr;
    gap: 8px;
  }

  .meta span {
    color: var(--fg-3);
  }

  .suggestion-reason {
    margin: 8px 0 0;
    color: var(--fg-2);
    font-size: 11px;
    line-height: 1.45;
  }

  .suggestion-link {
    width: 100%;
    display: flex;
    justify-content: space-between;
    margin: 0 0 10px;
    padding: 0 0 8px;
    border: 0;
    border-bottom: 1px solid var(--border);
    background: transparent;
    color: var(--fg-2);
    font: inherit;
    font-size: 10px;
    cursor: pointer;
  }

  .suggestion-link:hover {
    color: var(--cyan);
  }

  .footer-section {
    padding: 10px 14px;
    background: var(--bg-1);
  }

  @keyframes inspector-spin {
    to { transform: rotate(360deg); }
  }

  @media (prefers-reduced-motion: reduce) {
    :global(.control-command .spinning) {
      animation: none;
    }
  }
</style>
