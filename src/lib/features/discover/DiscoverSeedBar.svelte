<script lang="ts">
  import { tick } from "svelte";
  import { providerDisplayName, type DiscoverWorkspace } from "$lib/domain/discover";

  let {
    workspace,
    discoverWorkspaces,
    onNewSearch,
    onActivateSearch,
    onRunSearch,
    onImproveSearch,
  }: {
    workspace: DiscoverWorkspace;
    discoverWorkspaces: DiscoverWorkspace[];
    onNewSearch: () => void;
    onActivateSearch: (discoverId: string) => void;
    onRunSearch: (discoverId: string) => void;
    onImproveSearch: (discoverId: string) => void;
  } = $props();

  let queryInput: HTMLInputElement;
  let settingsOpen = $state(false);
  const isRunning = $derived(workspace.status === "running");
  const statusLabel = $derived(workspace.status === "idle" ? "Idle" : workspace.status);
  const resultLabel = $derived(
    workspace.status === "completed" ? `${workspace.candidates.length} candidates` : undefined,
  );
  const providerLabel = $derived(
    workspace.providers.length === 3
      ? "All providers"
      : workspace.providers.map(providerDisplayName).join(", ") || providerDisplayName(workspace.provider),
  );
  // arXiv and Semantic Scholar have no citation-count sort.
  // Semantic Scholar has no sort at all — all options fall back to relevance.
  const mostCitedDisabled = $derived(
    workspace.provider === "arxiv" || workspace.provider === "semantic_scholar"
  );
  const sortDisabled = $derived(workspace.provider === "semantic_scholar");
  const nonDefaultFilters = $derived.by(() => {
    const filters: string[] = [];
    if (workspace.providers.length !== 3) {
      filters.push(providerLabel);
    }
    if (!workspace.deep && workspace.sortBy !== "relevance") {
      filters.push(sortLabel(workspace.sortBy));
    }
    if (workspace.resultLimit !== 25) {
      filters.push(workspace.deep ? `${workspace.resultLimit} target` : `${workspace.resultLimit} max`);
    }
    if (workspace.yearFrom || workspace.yearTo) {
      filters.push(`${workspace.yearFrom || "any"}-${workspace.yearTo || "now"}`);
    }
    if (workspace.venue.trim()) {
      filters.push(workspace.venue.trim());
    }
    if (!workspace.openAccess) {
      filters.push("all access");
    }
    if (workspace.deep && workspace.deepDepth !== "standard") {
      filters.push(workspace.deepDepth);
    }
    return filters;
  });

  $effect(() => {
    workspace.id;
    tick().then(() => queryInput?.focus());
  });

  function runSearch() {
    if (!isRunning) {
      onRunSearch(workspace.id);
    }
  }

  function toggleProvider(provider: DiscoverWorkspace["provider"]) {
    if (workspace.providers.includes(provider)) {
      if (workspace.providers.length === 1) {
        return;
      }
      workspace.providers = workspace.providers.filter((item) => item !== provider);
    } else {
      workspace.providers = [...workspace.providers, provider];
    }
    workspace.provider = workspace.providers[0] ?? "open_alex";
  }

  function sortLabel(sortBy: DiscoverWorkspace["sortBy"]) {
    if (sortBy === "most_cited") {
      return "Most cited";
    }

    return sortBy === "newest" ? "Newest" : "Relevance";
  }
</script>

<header class="discover-search hair-b col">
  <div class="window-strip row" aria-label="Discover search windows">
    <button class="new-window" type="button" onclick={onNewSearch} aria-label="New search window">+</button>
    <div class="window-tabs row">
      {#each discoverWorkspaces as item}
        <button
          class:active={item.id === workspace.id}
          class="window-tab truncate"
          type="button"
          onclick={() => onActivateSearch(item.id)}
          title={item.title}
        >
          {item.title.replace("Discover: ", "")}
        </button>
      {/each}
    </div>
  </div>

  <div class="heading row">
    <div>
      <div class="label">Discover</div>
      <h1>{workspace.title}</h1>
    </div>
    <div class="status-strip row" aria-label="Search status">
      <span>{workspace.deep ? "Deep" : "Shallow"}</span>
      <span class:hot={workspace.status === "failed"}>{statusLabel}</span>
      {#if resultLabel}
        <span>{resultLabel}</span>
      {/if}
    </div>
  </div>

  <form class="search-form col" onsubmit={(event) => { event.preventDefault(); runSearch(); }}>
    <div class="query-row row">
      <span class="prompt">&gt;</span>
      <input
        bind:this={queryInput}
        bind:value={workspace.query}
        aria-label="Search papers"
        placeholder="find papers about..."
        disabled={isRunning}
      />
      <button
        class="toggle"
        class:active={workspace.deep}
        type="button"
        aria-pressed={workspace.deep}
        disabled={isRunning}
        onclick={() => (workspace.deep = !workspace.deep)}
      >
        Deep
      </button>
      <button class="btn primary" type="submit" disabled={isRunning || !workspace.query.trim()}>
        {isRunning ? "Running" : "Run"}
      </button>
      {#if workspace.candidates.length > 0}
        <button
          class="btn"
          type="button"
          disabled={isRunning}
          onclick={() => onImproveSearch(workspace.id)}
        >
          Improve
        </button>
      {/if}
      <button
        class="settings-button"
        class:active={settingsOpen}
        type="button"
        aria-expanded={settingsOpen}
        aria-label="Search settings"
        onclick={() => (settingsOpen = !settingsOpen)}
      >
        settings
      </button>
    </div>

    {#if nonDefaultFilters.length > 0 && !settingsOpen}
      <div class="filter-summary row" aria-label="Active search filters">
        <span>filters</span>
        {#each nonDefaultFilters as filter}
          <span>{filter}</span>
        {/each}
      </div>
    {/if}

    {#if settingsOpen}
      <div class="settings-panel row">
        <fieldset class="provider-set">
          <legend>Providers</legend>
          <label>
            <input
              type="checkbox"
              checked={workspace.providers.includes("open_alex")}
              disabled={isRunning}
              onchange={() => toggleProvider("open_alex")}
            />
            <span>OpenAlex</span>
          </label>
          <label>
            <input
              type="checkbox"
              checked={workspace.providers.includes("arxiv")}
              disabled={isRunning}
              onchange={() => toggleProvider("arxiv")}
            />
            <span>arXiv</span>
          </label>
          <label>
            <input
              type="checkbox"
              checked={workspace.providers.includes("semantic_scholar")}
              disabled={isRunning}
              onchange={() => toggleProvider("semantic_scholar")}
            />
            <span>Semantic Scholar</span>
          </label>
        </fieldset>
        {#if workspace.deep}
          <label>
            <span>Depth</span>
            <select bind:value={workspace.deepDepth} disabled={isRunning}>
              <option value="quick">Quick</option>
              <option value="standard">Standard</option>
              <option value="thorough">Thorough</option>
            </select>
          </label>
        {:else}
          <label>
            <span>Sort</span>
            <select bind:value={workspace.sortBy} disabled={isRunning || sortDisabled}>
              <option value="relevance">Relevance</option>
              <option value="newest" disabled={mostCitedDisabled}>Newest</option>
              <option value="most_cited" disabled={mostCitedDisabled}>Most cited</option>
            </select>
          </label>
        {/if}
        <label>
          <span>{workspace.deep ? "Target" : "Limit"}</span>
          <select bind:value={workspace.resultLimit} disabled={isRunning}>
            <option value={10}>10</option>
            <option value={25}>25</option>
            <option value={50}>50</option>
          </select>
        </label>
        <label>
          <span>From</span>
          <input
            bind:value={workspace.yearFrom}
            inputmode="numeric"
            pattern="[0-9]*"
            placeholder="any"
            disabled={isRunning}
          />
        </label>
        <label>
          <span>To</span>
          <input
            bind:value={workspace.yearTo}
            inputmode="numeric"
            pattern="[0-9]*"
            placeholder="now"
            disabled={isRunning}
          />
        </label>
        <label>
          <span>Venue</span>
          <input
            bind:value={workspace.venue}
            placeholder="any"
            disabled={isRunning}
          />
        </label>
        <label class="inline-setting">
          <input
            type="checkbox"
            bind:checked={workspace.openAccess}
            disabled={isRunning}
          />
          <span>Open access</span>
        </label>
      </div>
    {/if}
  </form>
</header>

<style>
  .discover-search {
    flex-shrink: 0;
    gap: 10px;
    padding: 10px 18px 12px;
    background: var(--bg-1);
  }

  .window-strip {
    min-width: 0;
    gap: 6px;
    align-items: center;
  }

  .new-window {
    width: 24px;
    height: 22px;
    flex-shrink: 0;
    border: 1px solid var(--border-2);
    background: var(--bg);
    color: var(--amber);
    font: inherit;
    font-size: 14px;
    cursor: pointer;
  }

  .window-tabs {
    min-width: 0;
    gap: 4px;
    overflow: hidden;
  }

  .window-tab {
    height: 22px;
    max-width: 180px;
    border: 1px solid var(--border);
    background: transparent;
    color: var(--fg-3);
    padding: 0 8px;
    font: inherit;
    font-size: 10px;
    cursor: pointer;
  }

  .window-tab.active {
    border-color: var(--amber-dim);
    background: var(--bg);
    color: var(--amber);
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
    align-items: center;
  }

  .prompt {
    flex-shrink: 0;
    color: var(--green);
    font-size: 13px;
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

  .toggle,
  .settings-button {
    height: 30px;
    border: 1px solid var(--border-2);
    background: transparent;
    color: var(--fg-2);
    padding: 0 10px;
    font: inherit;
    font-size: 10px;
    text-transform: uppercase;
    cursor: pointer;
  }

  .toggle:hover,
  .settings-button:hover,
  .new-window:hover,
  .window-tab:hover {
    border-color: var(--amber-dim);
    background: rgba(242, 169, 59, 0.05);
  }

  .toggle.active,
  .settings-button.active {
    border-color: var(--green);
    color: var(--green);
    background: rgba(138, 168, 74, 0.06);
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

  .filter-summary {
    gap: 5px;
    flex-wrap: wrap;
    padding-left: 18px;
  }

  .filter-summary span {
    border: 1px solid var(--border-2);
    padding: 1px 6px;
    color: var(--fg-3);
    font-size: 9px;
    text-transform: uppercase;
  }

  .settings-panel {
    gap: 8px;
    align-items: end;
    flex-wrap: wrap;
    min-width: 0;
    padding: 8px 0 0 18px;
  }

  .provider-set {
    display: flex;
    gap: 8px;
    align-items: center;
    min-height: 36px;
    margin: 0;
    border: 1px solid var(--border-2);
    padding: 4px 8px;
  }

  .provider-set legend {
    color: var(--fg-3);
    font-size: 9px;
    text-transform: uppercase;
  }

  .provider-set label {
    flex-direction: row;
    align-items: center;
    gap: 4px;
  }

  .provider-set input {
    width: 12px;
    height: 12px;
    min-width: 12px;
    accent-color: var(--green);
  }

  .inline-setting {
    flex-direction: row;
    align-items: center;
    min-height: 22px;
    padding-bottom: 1px;
  }

  .inline-setting input {
    width: 12px;
    height: 12px;
    min-width: 12px;
    accent-color: var(--green);
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
