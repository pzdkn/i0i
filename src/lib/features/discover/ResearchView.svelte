<script lang="ts">
  import { onMount } from "svelte";
  import type { UnlistenFn } from "@tauri-apps/api/event";
  import {
    createSearch,
    listSearches,
    listSearchCandidates,
    listenSearchUpdated,
    markSearchCandidatesSeen,
    runSearch,
    cancelSearchRun,
  } from "$lib/bridge/research";
  import {
    depthStrategy,
    isTerminalStatus,
    type Depth,
    type Search,
    type SearchCandidate,
    type SearchUpdated,
  } from "$lib/domain/research";

  let searches = $state<Search[]>([]);
  let activeSearchId = $state<string | null>(null);
  let candidates = $state<SearchCandidate[]>([]);

  // Seed-bar form.
  let goal = $state("");
  let yearFrom = $state("");
  let targetCount = $state(20);
  let depth = $state<Depth>("standard");
  let useOpenAlex = $state(true);
  let useArxiv = $state(true);

  // Run state.
  let running = $state(false);
  let activeRunId = $state<string | null>(null);
  let progress = $state<string[]>([]);
  let error = $state<string | null>(null);

  const PROGRESS_CAP = 80;

  onMount(() => {
    let unlisten: UnlistenFn | undefined;
    void refreshSearches();
    listenSearchUpdated(handleEvent).then((fn) => (unlisten = fn));
    return () => unlisten?.();
  });

  async function refreshSearches() {
    try {
      searches = await listSearches();
    } catch (e) {
      error = String(e);
    }
  }

  function handleEvent(payload: SearchUpdated) {
    if (payload.searchId !== activeSearchId) return;
    progress = [...progress.slice(-(PROGRESS_CAP - 1)), payload.message];
    if (isTerminalStatus(payload.status)) {
      running = false;
      activeRunId = null;
      void afterRun();
    }
  }

  async function afterRun() {
    if (!activeSearchId) return;
    candidates = await listSearchCandidates(activeSearchId);
    await refreshSearches();
  }

  function providers(): string[] {
    const list: string[] = [];
    if (useOpenAlex) list.push("open_alex");
    if (useArxiv) list.push("arxiv");
    return list;
  }

  function newSearch() {
    activeSearchId = null;
    candidates = [];
    progress = [];
    goal = "";
    error = null;
  }

  async function selectSearch(search: Search) {
    activeSearchId = search.id;
    goal = search.goal;
    progress = [];
    try {
      candidates = await listSearchCandidates(search.id);
      await markSearchCandidatesSeen(search.id);
      await refreshSearches();
    } catch (e) {
      error = String(e);
    }
  }

  async function run() {
    const trimmed = goal.trim();
    if (!trimmed || running) return;
    error = null;
    progress = [];
    candidates = [];
    try {
      const search = await createSearch({
        title: trimmed.slice(0, 60),
        goal: trimmed,
        constraints: {
          yearFrom: yearFrom.trim() ? Number(yearFrom) : undefined,
          providers: providers(),
          openAccess: true,
          targetCount,
          venues: [],
          authors: [],
          fieldsOfStudy: [],
          seedPaperIds: [],
        },
        strategy: depthStrategy(depth),
      });
      activeSearchId = search.id;
      running = true;
      activeRunId = await runSearch(search.id);
      await refreshSearches();
    } catch (e) {
      running = false;
      error = String(e);
    }
  }

  async function cancel() {
    if (activeRunId) await cancelSearchRun(activeRunId);
  }

  function unreadCount(search: Search): number {
    // Cheap surfacing: ready searches show their summary "new" count.
    if (!search.summary) return 0;
    try {
      return (JSON.parse(search.summary).new as number) ?? 0;
    } catch {
      return 0;
    }
  }
</script>

<section class="research row">
  <aside class="rail col hair-r">
    <button class="new-btn" type="button" onclick={newSearch}>+ New search</button>
    <ul class="searches col">
      {#each searches as search (search.id)}
        <li>
          <button
            class="search-item col"
            class:active={search.id === activeSearchId}
            type="button"
            onclick={() => selectSearch(search)}
          >
            <span class="title">{search.title}</span>
            <span class="meta">
              {search.status}{#if unreadCount(search) > 0}<em>+{unreadCount(search)}</em>{/if}
            </span>
          </button>
        </li>
      {/each}
      {#if searches.length === 0}
        <li class="empty">No searches yet.</li>
      {/if}
    </ul>
  </aside>

  <div class="main col">
    <header class="seed col hair-b">
      <textarea
        bind:value={goal}
        rows="2"
        placeholder="Describe what to find — e.g. recent explainable-AI papers relevant to my interpretability work, methods not surveys"
        disabled={running}
      ></textarea>
      <div class="controls row">
        <label>Year from<input bind:value={yearFrom} inputmode="numeric" placeholder="any" disabled={running} /></label>
        <label>Target<input type="number" bind:value={targetCount} min="5" max="50" disabled={running} /></label>
        <label>Depth
          <select bind:value={depth} disabled={running}>
            <option value="quick">Quick</option>
            <option value="standard">Standard</option>
            <option value="thorough">Thorough</option>
          </select>
        </label>
        <label class="chk"><input type="checkbox" bind:checked={useOpenAlex} disabled={running} />OpenAlex</label>
        <label class="chk"><input type="checkbox" bind:checked={useArxiv} disabled={running} />arXiv</label>
        {#if running}
          <button class="btn" type="button" onclick={cancel}>Cancel</button>
        {:else}
          <button class="btn primary" type="button" onclick={run} disabled={!goal.trim()}>Run deep research</button>
        {/if}
      </div>
      {#if error}<div class="error">{error}</div>{/if}
    </header>

    <div class="results col">
      {#if candidates.length === 0 && !running}
        <p class="hint">Write a goal and run deep research, or pick a saved search.</p>
      {/if}
      {#each candidates as item (item.id)}
        <article class="candidate col" class:unseen={!item.seen}>
          <div class="row head">
            <span class="rank">{item.rank}</span>
            <span class="ctitle">{item.candidate.title}</span>
            {#if item.alreadyInLibrary}<span class="badge">in library</span>{/if}
          </div>
          <div class="row sub">
            {#if item.candidate.year}<span>{item.candidate.year}</span>{/if}
            {#if item.candidate.venue}<span class="truncate" title={item.candidate.venue}>{item.candidate.venue}</span>{/if}
            {#if item.score != null}<span>score {item.score.toFixed(2)}</span>{/if}
            {#if item.candidate.externalUrl}<a href={item.candidate.externalUrl} target="_blank" rel="noreferrer">open</a>{/if}
          </div>
          {#if item.rationale}<p class="rationale">{item.rationale}</p>{/if}
        </article>
      {/each}
    </div>
  </div>

  {#if running || progress.length > 0}
    <aside class="trace col hair-l">
      <div class="trace-head row">
        <span>Progress</span>
        {#if running}<span class="spin">running</span>{/if}
      </div>
      <ol class="trace-lines col">
        {#each progress as line, i (i)}
          <li>{line}</li>
        {/each}
      </ol>
    </aside>
  {/if}
</section>

<style>
  .research {
    flex: 1;
    min-width: 0;
    min-height: 0;
    align-items: stretch;
  }
  .rail {
    width: 200px;
    flex-shrink: 0;
    background: var(--bg-1);
    gap: 8px;
    padding: 10px;
    overflow-y: auto;
  }
  .new-btn {
    height: 28px;
    border: 1px solid var(--border-2);
    background: var(--bg);
    color: var(--amber);
    font: inherit;
    font-size: 11px;
    cursor: pointer;
  }
  .searches {
    gap: 4px;
  }
  .search-item {
    width: 100%;
    gap: 2px;
    align-items: flex-start;
    border: 1px solid transparent;
    background: none;
    color: var(--fg-1);
    padding: 6px 8px;
    text-align: left;
    cursor: pointer;
  }
  .search-item:hover {
    border-color: var(--border-2);
  }
  .search-item.active {
    border-color: var(--amber-dim);
    background: rgba(242, 169, 59, 0.05);
  }
  .search-item .title {
    font-size: 12px;
  }
  .search-item .meta {
    font-size: 9px;
    color: var(--fg-3);
    text-transform: uppercase;
  }
  .search-item .meta em {
    color: var(--amber);
    font-style: normal;
    margin-left: 4px;
  }
  .empty,
  .hint {
    color: var(--fg-3);
    font-size: 11px;
    padding: 8px;
  }
  .main {
    flex: 1;
    min-width: 0;
    min-height: 0;
  }
  .seed {
    flex-shrink: 0;
    gap: 8px;
    padding: 12px 14px;
    background: var(--bg-1);
  }
  textarea {
    width: 100%;
    border: 1px solid var(--border-2);
    background: var(--bg);
    color: var(--fg);
    font: inherit;
    font-size: 12px;
    padding: 8px;
    resize: vertical;
  }
  textarea:focus {
    outline: none;
    border-color: var(--amber-dim);
  }
  .controls {
    gap: 10px;
    align-items: end;
    flex-wrap: wrap;
  }
  label {
    display: flex;
    flex-direction: column;
    gap: 3px;
    color: var(--fg-3);
    font-size: 9px;
    text-transform: uppercase;
  }
  label.chk {
    flex-direction: row;
    align-items: center;
    gap: 4px;
    text-transform: none;
    font-size: 11px;
    color: var(--fg-1);
  }
  label input,
  label select {
    height: 24px;
    border: 1px solid var(--border-2);
    background: var(--bg);
    color: var(--fg-1);
    padding: 0 6px;
    font: inherit;
    font-size: 11px;
  }
  label.chk input {
    height: auto;
  }
  .btn {
    height: 28px;
    border: 1px solid var(--border-2);
    background: var(--bg);
    color: var(--fg-1);
    font: inherit;
    font-size: 11px;
    padding: 0 12px;
    cursor: pointer;
  }
  .btn.primary {
    color: var(--amber);
    border-color: var(--amber-dim);
  }
  .btn:disabled {
    opacity: 0.6;
    cursor: not-allowed;
  }
  .error {
    color: var(--red, #e06c75);
    font-size: 11px;
  }
  .results {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    gap: 8px;
    padding: 12px 14px;
  }
  .candidate {
    gap: 4px;
    border: 1px solid var(--border-2);
    padding: 8px 10px;
    background: var(--bg-1);
  }
  .candidate.unseen {
    border-left: 2px solid var(--amber);
  }
  .head {
    gap: 8px;
    align-items: baseline;
  }
  .rank {
    color: var(--fg-3);
    font-size: 11px;
  }
  .ctitle {
    flex: 1;
    min-width: 0;
    color: var(--fg);
    font-size: 13px;
  }
  .badge {
    border: 1px solid var(--border-2);
    color: var(--fg-3);
    font-size: 9px;
    padding: 1px 5px;
    text-transform: uppercase;
  }
  .sub {
    gap: 8px;
    color: var(--fg-3);
    font-size: 10px;
  }
  .sub a {
    color: var(--amber);
  }
  .rationale {
    margin: 2px 0 0;
    color: var(--fg-2);
    font-size: 11px;
  }
  .trace {
    width: 240px;
    flex-shrink: 0;
    background: var(--bg-1);
    padding: 10px;
    gap: 6px;
    overflow-y: auto;
  }
  .trace-head {
    justify-content: space-between;
    color: var(--fg-3);
    font-size: 10px;
    text-transform: uppercase;
  }
  .trace-head .spin {
    color: var(--amber);
  }
  .trace-lines {
    gap: 3px;
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .trace-lines li {
    color: var(--fg-2);
    font-size: 10px;
    font-family: var(--mono, monospace);
    word-break: break-word;
  }
</style>
