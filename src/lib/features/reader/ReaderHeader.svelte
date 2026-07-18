<script lang="ts">
  import type { ReaderDocument } from "$lib/domain/reader";

  let {
    document,
    layoutMode = "normal",
    threadsCollapsed = false,
    threadCount = 0,
    pinCount = 0,
    zoomScale,
    onZoomIn,
    onZoomOut,
    onToggleFocus,
    onToggleThreads,
  }: {
    document: ReaderDocument;
    layoutMode?: "normal" | "focus";
    threadsCollapsed?: boolean;
    threadCount?: number;
    pinCount?: number;
    zoomScale?: number;
    onZoomIn?: () => void;
    onZoomOut?: () => void;
    onToggleFocus: () => void;
    onToggleThreads: () => void;
  } = $props();

  const showZoom = $derived(zoomScale !== undefined);
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
    <button class="btn" type="button" title="Graph">Graph</button>
    <button class="btn" type="button" title="Ask about paper">Ask</button>

    <div class="flex1"></div>
    {#if showZoom}
      <div class="zoom-group row">
        <button class="tool" type="button" title="Zoom out" aria-label="Zoom out" onclick={onZoomOut}>-</button>
        <span class="zoom-label">{Math.round((zoomScale ?? 1) * 100)}%</span>
        <button class="tool" type="button" title="Zoom in" aria-label="Zoom in" onclick={onZoomIn}>+</button>
      </div>
    {/if}
    {#if layoutMode === "focus"}
      <button class="btn" type="button" title="Toggle threads panel" onclick={onToggleThreads}>
        {threadsCollapsed ? "Threads" : "Hide Threads"} · {threadCount}{pinCount ? ` / ${pinCount}` : ""}
      </button>
      <button class="btn primary" type="button" title="Exit reader focus" onclick={onToggleFocus}>Exit Focus</button>
    {:else}
      <button class="btn primary" type="button" title="Focus reader" onclick={onToggleFocus}>Focus</button>
    {/if}
  </div>
</header>

<style>
  .reader-header {
    display: flex;
    flex: 1;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
    overflow: hidden;
    padding: 12px 18px 10px;
    background: var(--bg-1);
  }

  .meta-line {
    flex-shrink: 0;
    gap: 8px;
    margin-bottom: 5px;
    font-size: 10px;
  }

  .title-line {
    flex: 1 1 auto;
    min-height: 0;
    overflow: hidden;
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
    flex-shrink: 0;
    gap: 10px;
    margin-top: auto;
    padding-top: 10px;
    align-items: center;
    color: var(--fg-3);
    font-size: 10px;
  }

  .zoom-group {
    gap: 6px;
    align-items: center;
  }

  .tool {
    width: 24px;
    height: 24px;
    border: 1px solid var(--border-2);
    background: var(--bg);
    color: var(--fg-2);
    font: inherit;
    cursor: pointer;
  }

  .tool:hover {
    border-color: var(--amber-dim);
    color: var(--amber);
  }

  .zoom-label {
    width: 44px;
    color: var(--fg-3);
    font-size: 10px;
    text-align: center;
  }
</style>
