<script lang="ts">
  import type { VaultStatus } from "$lib/domain/vault";
  import type { ChunkHit, SearchResponse } from "$lib/domain/search";
  import { searchChunks } from "$lib/bridge/search";

  let {
    vaultStatus = null,
    currentPath = "/transformers/attention",
    vaultId = "",
    resolvePaperTitle = (paperId: string) => paperId,
    onOpenResult = (_paperId: string, _hit: ChunkHit) => {},
  }: {
    vaultStatus?: VaultStatus | null;
    currentPath?: string;
    /** Scope for the search. Empty searches the whole library. */
    vaultId?: string;
    resolvePaperTitle?: (paperId: string) => string;
    onOpenResult?: (paperId: string, hit: ChunkHit) => void;
  } = $props();

  /** Typing settles before we search — every keystroke otherwise embeds a query. */
  const DEBOUNCE_MS = 180;
  const RESULT_LIMIT = 8;

  let commandText = $state("");
  let input = $state<HTMLInputElement | null>(null);
  let response = $state<SearchResponse | null>(null);
  let searching = $state(false);
  let open = $state(false);
  let highlighted = $state(0);
  let errorText = $state("");

  let debounceTimer: ReturnType<typeof setTimeout> | undefined;
  /**
   * Responses can land out of order — a slow hybrid search started before a
   * fast one finishes after it. Only the newest query may write to `response`.
   */
  let latestQuery = 0;

  const hits = $derived(response?.hits ?? []);

  function scheduleSearch(query: string) {
    clearTimeout(debounceTimer);
    const trimmed = query.trim();

    if (!trimmed) {
      response = null;
      searching = false;
      errorText = "";
      open = false;
      return;
    }

    open = true;
    debounceTimer = setTimeout(() => void runSearch(trimmed), DEBOUNCE_MS);
  }

  async function runSearch(query: string) {
    const token = ++latestQuery;
    searching = true;
    errorText = "";

    try {
      const result = await searchChunks({
        query,
        // Defaults to the current vault. An empty vaultId means no constraint,
        // which the backend reads as the whole library.
        vaultIds: vaultId ? [vaultId] : [],
        mode: "hybrid",
        limit: RESULT_LIMIT,
      });
      if (token !== latestQuery) return;
      response = result;
      highlighted = 0;
    } catch (error) {
      if (token !== latestQuery) return;
      response = null;
      errorText = error instanceof Error ? error.message : String(error);
    } finally {
      if (token === latestQuery) searching = false;
    }
  }

  function choose(index: number) {
    const hit = hits[index];
    if (!hit) return;
    onOpenResult(hit.chunk.paperId, hit);
    close();
  }

  function close() {
    open = false;
    highlighted = 0;
  }

  function onKeydown(event: KeyboardEvent) {
    if (event.key === "Escape") {
      close();
      input?.blur();
      return;
    }
    if (!open || hits.length === 0) return;

    if (event.key === "ArrowDown") {
      event.preventDefault();
      highlighted = (highlighted + 1) % hits.length;
    } else if (event.key === "ArrowUp") {
      event.preventDefault();
      highlighted = (highlighted - 1 + hits.length) % hits.length;
    } else if (event.key === "Enter") {
      event.preventDefault();
      choose(highlighted);
    }
  }

  function onWindowKeydown(event: KeyboardEvent) {
    if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "k") {
      event.preventDefault();
      input?.focus();
      input?.select();
    }
  }

  /**
   * Semantic search is unavailable or still catching up. Worth surfacing rather
   * than quietly returning weaker results — the startup embedding sweep takes
   * minutes on a fresh library (RFC 0076).
   */
  const semanticNote = $derived.by(() => {
    const status = response?.semantic;
    if (!status) return "";
    if (status.status === "noEmbeddings") return "lexical only — still indexing";
    if (status.status === "unavailable") return "lexical only — no model";
    if (status.status === "ran" && status.coverage.embedded < status.coverage.chunks) {
      return `indexing ${status.coverage.embedded}/${status.coverage.chunks}`;
    }
    return "";
  });

  const scopeNote = $derived(
    response?.scope.emptyReason === "noIntersection"
      ? "nothing in scope"
      : response?.scope.emptyReason === "noPapers"
        ? "no papers to search"
        : "",
  );

  function snippet(hit: ChunkHit) {
    return hit.chunk.text.replace(/\s+/g, " ").trim().slice(0, 96);
  }

  function pageLabel(hit: ChunkHit) {
    const { pageStart, pageEnd } = hit.chunk;
    // Pages are 0-based in storage and 1-based to a reader.
    return pageStart === pageEnd ? `p${pageStart + 1}` : `p${pageStart + 1}-${pageEnd + 1}`;
  }
</script>

<svelte:window on:keydown={onWindowKeydown} />

<header class="titlebar row hair-b">
  <div class="path row">
    <span class="brand">i0i</span>
    <span class="sep">/</span>
    <strong>{currentPath}</strong>
  </div>

  <div class="command-wrap">
    <div class="command row" class:active={open}>
      <span class="prompt">:</span>
      <input
        bind:this={input}
        bind:value={commandText}
        oninput={() => scheduleSearch(commandText)}
        onkeydown={onKeydown}
        onfocus={() => commandText.trim() && (open = true)}
        aria-label="Search papers"
        placeholder="search this vault..."
      />
      {#if searching}
        <span class="status">…</span>
      {/if}
      <span class="key">cmd+k</span>
    </div>

    {#if open}
      <!-- Clicking away closes; the overlay sits under the panel. -->
      <button class="scrim" onclick={close} tabindex="-1" aria-label="Close search"></button>
      <div class="results">
        {#if errorText}
          <div class="note error">{errorText}</div>
        {:else if hits.length === 0}
          <div class="note">
            {searching ? "searching…" : scopeNote || "no matches"}
          </div>
        {:else}
          {#if semanticNote}
            <div class="note subtle">{semanticNote}</div>
          {/if}
          {#each hits as hit, index (hit.chunk.id)}
            <button
              class="hit"
              class:highlighted={index === highlighted}
              onclick={() => choose(index)}
              onmouseenter={() => (highlighted = index)}
            >
              <div class="hit-top row">
                <span class="page">{pageLabel(hit)}</span>
                <span class="paper">{resolvePaperTitle(hit.chunk.paperId)}</span>
                <span class="signals">
                  {#if hit.lexical}<span class="sig lex" title="matched lexically">L</span>{/if}
                  {#if hit.semantic}<span class="sig sem" title="matched semantically">S</span>{/if}
                </span>
              </div>
              {#if hit.chunk.headingPath}
                <div class="heading">{hit.chunk.headingPath}</div>
              {/if}
              <div class="snippet">{snippet(hit)}</div>
            </button>
          {/each}
        {/if}
      </div>
    {/if}
  </div>

  <div class="readout row">
    <span class="sync">online</span>
    {#if vaultStatus}
      <span>{vaultStatus.paperCount} papers</span>
      <span>{vaultStatus.unreadCount} unread</span>
    {:else}
      <span>loading vault</span>
    {/if}
  </div>
</header>

<style>
  .titlebar {
    height: 28px;
    flex-shrink: 0;
    gap: 12px;
    padding: 0 10px;
    background: var(--bg-1);
    position: relative;
    z-index: 40;
  }

  .path {
    min-width: 240px;
    gap: 6px;
    color: var(--fg-2);
    font-size: 11px;
    white-space: nowrap;
  }

  .brand,
  .path strong {
    color: var(--amber);
    font-weight: 600;
  }

  .sep {
    color: var(--fg-4);
  }

  .command-wrap {
    position: relative;
    flex: 1;
    min-width: 260px;
    max-width: 460px;
  }

  .command {
    height: 20px;
    gap: 8px;
    padding: 0 8px;
    border: 1px solid var(--border-2);
    background: var(--bg);
  }

  .command.active {
    border-color: var(--border-hot);
  }

  .prompt {
    color: var(--amber);
  }

  input {
    flex: 1;
    min-width: 0;
    border: 0;
    outline: none;
    background: transparent;
    color: var(--fg-3);
    font-size: 11px;
  }

  input::placeholder {
    color: var(--fg-3);
  }

  .status,
  .key {
    color: var(--fg-4);
    font-size: 10px;
  }

  /* Sits behind the panel so an outside click dismisses it. */
  .scrim {
    position: fixed;
    inset: 0;
    z-index: 1;
    border: 0;
    padding: 0;
    background: transparent;
    cursor: default;
  }

  .results {
    position: absolute;
    top: calc(100% + 3px);
    left: 0;
    right: 0;
    z-index: 2;
    max-height: 60vh;
    overflow-y: auto;
    border: 1px solid var(--border-hot);
    background: var(--panel);
    box-shadow: 0 8px 24px rgb(0 0 0 / 60%);
  }

  .note {
    padding: 6px 8px;
    color: var(--fg-3);
    font-size: 10px;
  }

  .note.error {
    color: var(--red);
  }

  .note.subtle {
    color: var(--amber-dim);
    border-bottom: 1px solid var(--border);
  }

  .hit {
    display: block;
    width: 100%;
    padding: 5px 8px;
    border: 0;
    border-bottom: 1px solid var(--border);
    background: transparent;
    text-align: left;
    cursor: pointer;
  }

  .hit:last-child {
    border-bottom: 0;
  }

  .hit.highlighted {
    background: var(--bg-2);
  }

  .hit-top {
    gap: 6px;
    font-size: 10px;
  }

  .page {
    flex-shrink: 0;
    color: var(--amber-mid);
  }

  .paper {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    color: var(--fg-2);
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .signals {
    flex-shrink: 0;
    display: flex;
    gap: 3px;
  }

  /* Which signal found this. The honest alternative to a relevance percentage,
     which would imply a precision the score does not have. */
  .sig {
    width: 11px;
    color: var(--bg);
    font-size: 9px;
    line-height: 11px;
    text-align: center;
  }

  .sig.lex {
    background: var(--amber-dim);
  }

  .sig.sem {
    background: var(--cyan);
  }

  .heading {
    overflow: hidden;
    margin-top: 2px;
    color: var(--amber);
    font-size: 10px;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .snippet {
    display: -webkit-box;
    overflow: hidden;
    margin-top: 2px;
    color: var(--fg-3);
    font-size: 10px;
    line-height: 1.35;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 2;
    line-clamp: 2;
  }
</style>
