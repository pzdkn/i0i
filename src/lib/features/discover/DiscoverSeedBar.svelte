<script lang="ts">
  import { tick } from "svelte";
  import type { DiscoverWorkspace } from "$lib/domain/discover";

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

  $effect(() => {
    workspace.id;
    tick().then(() => queryInput?.focus());
  });

  function runSearch() {
    if (!isRunning) {
      onRunSearch(workspace.id);
    }
  }
</script>

<header class="discover-search hair-b col">
  <div class="heading row">
    <div>
      <div class="label">Discover</div>
      <h1>{workspace.title}</h1>
    </div>
    <span class="mono-dim">OpenAlex / transient search / explicit run</span>
  </div>

  <form class="search-form col" onsubmit={(event) => { event.preventDefault(); runSearch(); }}>
    <div class="query-row row">
      <button class="icon-btn" type="button" title="New Search" aria-label="New Search" onclick={onNewSearch}>+</button>
      <input
        bind:this={queryInput}
        bind:value={workspace.query}
        aria-label="Search papers"
        placeholder="search OpenAlex papers..."
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
        <select bind:value={workspace.sortBy} disabled={isRunning}>
          <option value="relevance">Relevance</option>
          <option value="newest">Newest</option>
          <option value="most_cited">Most cited</option>
        </select>
      </label>
      <label class="check row">
        <input bind:checked={workspace.openAccessOnly} type="checkbox" disabled={isRunning} />
        <span>Open access</span>
      </label>
      <div class="flex1"></div>
      {#if workspace.status === "completed"}
        <span class="mono-dim">{workspace.candidates.length} candidates</span>
      {:else if workspace.status === "failed"}
        <span class="mono-dim hot">run failed</span>
      {:else if workspace.status === "idle"}
        <span class="mono-dim">no run yet</span>
      {/if}
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
    align-items: baseline;
  }

  h1 {
    margin: 2px 0 0;
    color: var(--amber);
    font-size: 20px;
    font-weight: 600;
  }

  .search-form {
    gap: 8px;
  }

  .query-row {
    gap: 8px;
  }

  .query-row input {
    height: 30px;
    flex: 1;
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

  .icon-btn {
    width: 30px;
    height: 30px;
    border: 1px solid var(--border-2);
    background: var(--bg);
    color: var(--amber);
    font-size: 17px;
    cursor: pointer;
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

  .check {
    height: 22px;
    flex-direction: row;
    align-items: center;
    gap: 6px;
    margin-left: 2px;
  }

  .check input {
    min-width: 0;
    height: auto;
    padding: 0;
  }

  button:disabled,
  input:disabled,
  select:disabled {
    opacity: 0.65;
    cursor: not-allowed;
  }
</style>
