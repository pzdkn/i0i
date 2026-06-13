<script lang="ts">
  import { tick } from "svelte";
  import { providerDisplayName, type DiscoverWorkspace } from "$lib/domain/discover";

  let {
    workspace,
    onNewSearch,
    onRunSearch,
  }: {
    workspace: DiscoverWorkspace;
    onNewSearch: () => void;
    onRunSearch: (discoverId: string) => void;
  } = $props();

  let queryInput: HTMLInputElement;
  const isRunning = $derived(workspace.status === "running");
  const statusLabel = $derived(workspace.status === "idle" ? "Idle" : workspace.status);
  const resultLabel = $derived(
    workspace.status === "completed" ? `${workspace.candidates.length} candidates` : undefined,
  );
  const providerLabel = $derived(providerDisplayName(workspace.provider));
  // arXiv and Semantic Scholar have no citation-count sort.
  // Semantic Scholar has no sort at all — all options fall back to relevance.
  const mostCitedDisabled = $derived(
    workspace.provider === "arxiv" || workspace.provider === "semantic_scholar"
  );
  const sortDisabled = $derived(workspace.provider === "semantic_scholar");

  $effect(() => {
    workspace.id;
    tick().then(() => queryInput?.focus());
  });

  function runSearch() {
    if (!isRunning) {
      onRunSearch(workspace.id);
    }
  }

  function onProviderChange() {
    if (
      (workspace.provider === "arxiv" || workspace.provider === "semantic_scholar") &&
      workspace.sortBy === "most_cited"
    ) {
      workspace.sortBy = "relevance";
    }
  }

  function sortLabel(sortBy: DiscoverWorkspace["sortBy"]) {
    if (sortBy === "most_cited") {
      return "Most cited";
    }

    return sortBy === "newest" ? "Newest" : "Relevance";
  }
</script>

<header class="discover-search hair-b col">
  <div class="heading row">
    <div>
      <div class="label">Discover</div>
      <h1>{workspace.title}</h1>
    </div>
    <button class="new-search-btn" type="button" onclick={onNewSearch}>New Search</button>
  </div>

  <div class="status-strip row" aria-label="Search status">
    <span>{providerLabel}</span>
    <span>Manual run</span>
    <span>Open access</span>
    <span>{workspace.resultLimit} max</span>
    <span>{sortLabel(workspace.sortBy)}</span>
    <span class:hot={workspace.status === "failed"}>{statusLabel}</span>
    {#if resultLabel}
      <span>{resultLabel}</span>
    {/if}
  </div>

  <form class="search-form col" onsubmit={(event) => { event.preventDefault(); runSearch(); }}>
    <div class="query-row row">
      <input
        bind:this={queryInput}
        bind:value={workspace.query}
        aria-label="Search papers"
        placeholder="search {providerLabel} papers..."
        disabled={isRunning}
      />
      <button class="btn primary" type="submit" disabled={isRunning || !workspace.query.trim()}>
        {isRunning ? "Running" : "Run"}
      </button>
    </div>

    <div class="filter-row row">
      <label>
        <span>Year From</span>
        <input
          bind:value={workspace.yearFrom}
          inputmode="numeric"
          pattern="[0-9]*"
          placeholder="any"
          disabled={isRunning}
        />
      </label>
      <label>
        <span>Year To</span>
        <input
          bind:value={workspace.yearTo}
          inputmode="numeric"
          pattern="[0-9]*"
          placeholder="any"
          disabled={isRunning}
        />
      </label>
      <label>
        <span>Limit</span>
        <select bind:value={workspace.resultLimit} disabled={isRunning}>
          <option value={10}>10</option>
          <option value={25}>25</option>
          <option value={50}>50</option>
        </select>
      </label>
      <label>
        <span>Sort</span>
        <select bind:value={workspace.sortBy} disabled={isRunning || sortDisabled}>
          <option value="relevance">Relevance</option>
          <option value="newest" disabled={mostCitedDisabled}>Newest</option>
          <option value="most_cited" disabled={mostCitedDisabled}>Most cited</option>
        </select>
      </label>
      <label>
        <span>Provider</span>
        <select bind:value={workspace.provider} onchange={onProviderChange} disabled={isRunning}>
          <option value="open_alex">OpenAlex</option>
          <option value="arxiv">arXiv</option>
          <option value="semantic_scholar">Semantic Scholar</option>
        </select>
      </label>
    </div>
  </form>
</header>

<style>
  .discover-search {
    flex-shrink: 0;
    gap: 12px;
    padding: 14px 18px 12px;
    background: var(--bg-1);
  }

  .heading {
    gap: 12px;
    align-items: flex-start;
    justify-content: space-between;
  }

  h1 {
    margin: 2px 0 0;
    color: var(--amber);
    font-size: 20px;
    font-weight: 600;
  }

  .search-form {
    gap: 9px;
  }

  .query-row {
    gap: 8px;
  }

  .query-row input {
    height: 30px;
    width: min(100%, 640px);
    flex: 0 1 640px;
    min-width: 0;
    border: 1px solid var(--border-2);
    outline: none;
    background: var(--bg);
    padding: 0 10px;
    color: var(--fg);
    font: inherit;
    font-size: 12px;
  }

  .query-row input:focus {
    border-color: var(--amber-dim);
  }

  .new-search-btn {
    height: 30px;
    border: 1px solid var(--border-2);
    background: var(--bg);
    color: var(--amber);
    padding: 0 10px;
    font: inherit;
    font-size: 10px;
    text-transform: uppercase;
    cursor: pointer;
  }

  .new-search-btn:hover {
    border-color: var(--amber-dim);
    background: rgba(242, 169, 59, 0.05);
  }

  .status-strip {
    gap: 5px;
    flex-wrap: wrap;
  }

  .status-strip span {
    border: 1px solid var(--border-2);
    padding: 1px 6px;
    color: var(--fg-2);
    font-size: 9px;
    text-transform: uppercase;
  }

  .filter-row {
    gap: 8px;
    align-items: end;
    min-width: 0;
  }

  label {
    display: flex;
    flex-direction: column;
    gap: 3px;
    color: var(--fg-3);
    font-size: 9px;
    text-transform: uppercase;
  }

  label input,
  select {
    height: 22px;
    min-width: 78px;
    border: 1px solid var(--border-2);
    background: var(--bg);
    color: var(--fg-1);
    padding: 0 6px;
    font: inherit;
    font-size: 10px;
    text-transform: none;
  }

  button:disabled,
  input:disabled,
  select:disabled {
    opacity: 0.65;
    cursor: not-allowed;
  }
</style>
