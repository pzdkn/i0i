<script lang="ts">
  import type { ReaderDocument, ReaderMode } from "$lib/domain/reader";

  let {
    document,
    mode,
    onModeChange,
  }: {
    document: ReaderDocument;
    mode: ReaderMode;
    onModeChange: (mode: ReaderMode) => void;
  } = $props();

  const viewModes: ReaderMode[] = ["TEXT", "PDF", "SPLIT"];
</script>

<header class="reader-header hair-b">
  <div class="row meta-line">
    <span class="label">Paper</span>
    <span class="mono-dim">{document.identifier} / {document.venue} {document.year}</span>
    {#each document.tags as tag}
      <span class="chip">{tag}</span>
    {/each}
    <div class="flex1"></div>
  </div>

  <div class="row title-line">
    <h1>{document.title}</h1>
    <span class="authors truncate">{document.authors.join(" / ")}</span>
  </div>

  <div class="row action-line">
    <div class="view-toggle row">
      {#each viewModes as viewMode}
        <button
          class:active={mode === viewMode}
          type="button"
          title={viewMode === "TEXT" ? "Text view" : `${viewMode} view placeholder`}
          onclick={() => onModeChange(viewMode)}
        >
          {viewMode}
          <span class="key">{viewMode[0]}</span>
        </button>
      {/each}
    </div>

    <button class="btn" type="button">Notes</button>
    <button class="btn" type="button">Annotate</button>
    <button class="btn" type="button">Graph</button>
    <button class="btn" type="button">Ask</button>

    <div class="flex1"></div>
    <span class="mono-dim">[h] highlight / [n] note / [q] question</span>
  </div>
</header>

<style>
  .reader-header {
    flex-shrink: 0;
    padding: 12px 18px 10px;
    background: var(--bg-1);
  }

  .meta-line {
    gap: 8px;
    margin-bottom: 5px;
    font-size: 10px;
  }

  .title-line {
    gap: 12px;
    align-items: baseline;
  }

  h1 {
    margin: 0;
    color: var(--amber);
    font-size: 18px;
    font-weight: 600;
  }

  .authors {
    min-width: 0;
    color: var(--fg-2);
    font-size: 11px;
  }

  .action-line {
    gap: 10px;
    margin-top: 10px;
    color: var(--fg-3);
    font-size: 10px;
  }

  .view-toggle {
    border: 1px solid var(--border-2);
  }

  .view-toggle button {
    height: 22px;
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 0 10px;
    border: 0;
    border-right: 1px solid var(--border-2);
    background: transparent;
    color: var(--fg-2);
    font-size: 10px;
    letter-spacing: 0.08em;
  }

  .view-toggle button:last-child {
    border-right: 0;
  }

  .view-toggle button.active {
    background: var(--amber);
    color: var(--bg);
  }

  .view-toggle button.active .key {
    border-color: var(--bg);
    color: var(--bg);
  }
</style>
