<script lang="ts">
  import { Search, ZoomIn, ZoomOut, ChevronUp, ChevronDown, X, Globe, Sparkles } from "@lucide/svelte";
  import AiHighlightMenu from "$lib/features/reader/AiHighlightMenu.svelte";
  import type { AutoHighlightCategory } from "$lib/bridge/chat";

  // Persistent, document-level reader toolbar (RFC 0063). Presentational: it owns
  // no document behavior — zoom, view, and search are delegated up to ReaderView.
  let {
    contentKind,
    zoomScale,
    onZoomIn,
    onZoomOut,
    canReadAsHtml = false,
    onReadAsHtml,
    searchEnabled = false,
    searchQuery = "",
    matchCount = 0,
    activeMatch = 0,
    onSearch,
    onNextMatch,
    onPrevMatch,
    onClearSearch,
    aiEnabled = false,
    aiBusy = false,
    onAutoHighlight,
  }: {
    contentKind?: string;
    zoomScale?: number;
    onZoomIn?: () => void;
    onZoomOut?: () => void;
    canReadAsHtml?: boolean;
    onReadAsHtml?: () => void;
    searchEnabled?: boolean;
    searchQuery?: string;
    matchCount?: number;
    activeMatch?: number;
    onSearch?: (query: string) => void;
    onNextMatch?: () => void;
    onPrevMatch?: () => void;
    onClearSearch?: () => void;
    aiEnabled?: boolean;
    aiBusy?: boolean;
    onAutoHighlight?: (categories: AutoHighlightCategory[]) => void;
  } = $props();

  let aiMenuOpen = $state(false);

  function runAutoHighlight(categories: AutoHighlightCategory[]) {
    aiMenuOpen = false;
    onAutoHighlight?.(categories);
  }

  const showZoom = $derived(zoomScale !== undefined);
  const viewLabel = $derived(contentKind === "html" ? "HTML" : "PDF");

  function handleSearchKeydown(event: KeyboardEvent) {
    if (event.key === "Enter") {
      event.preventDefault();
      if (event.shiftKey) onPrevMatch?.();
      else onNextMatch?.();
    } else if (event.key === "Escape") {
      onClearSearch?.();
    }
  }
</script>

<div class="reader-toolbar row hair-b">
  <div class="row left">
    <span class="view-badge">{viewLabel}</span>
    {#if canReadAsHtml}
      <button class="tool-btn" type="button" title="Read as HTML" onclick={onReadAsHtml}>
        <Globe size={13} strokeWidth={1.75} aria-hidden="true" /> Read as HTML
      </button>
    {/if}
  </div>

  <div class="row search" class:disabled={!searchEnabled}>
    <Search size={13} strokeWidth={1.75} aria-hidden="true" />
    <input
      type="text"
      value={searchQuery}
      disabled={!searchEnabled}
      placeholder={searchEnabled ? "Search in document…" : "Search (HTML only)"}
      title={searchEnabled ? "Search in document" : "In-document search is available for HTML documents"}
      aria-label="Search in document"
      oninput={(event) => onSearch?.((event.currentTarget as HTMLInputElement).value)}
      onkeydown={handleSearchKeydown}
    />
    {#if searchEnabled && searchQuery}
      <span class="count mono-dim">{matchCount ? `${activeMatch + 1}/${matchCount}` : "0/0"}</span>
      <button class="nav-btn" type="button" title="Previous match" aria-label="Previous match" disabled={!matchCount} onclick={() => onPrevMatch?.()}>
        <ChevronUp size={13} strokeWidth={1.75} aria-hidden="true" />
      </button>
      <button class="nav-btn" type="button" title="Next match" aria-label="Next match" disabled={!matchCount} onclick={() => onNextMatch?.()}>
        <ChevronDown size={13} strokeWidth={1.75} aria-hidden="true" />
      </button>
      <button class="nav-btn" type="button" title="Clear search" aria-label="Clear search" onclick={() => onClearSearch?.()}>
        <X size={13} strokeWidth={1.75} aria-hidden="true" />
      </button>
    {/if}
  </div>

  <div class="row right">
    {#if aiEnabled}
      <button
        class="tool-btn ai"
        class:on={aiMenuOpen}
        type="button"
        title="Highlight with AI"
        onclick={() => (aiMenuOpen = !aiMenuOpen)}
      >
        <Sparkles size={13} strokeWidth={1.75} aria-hidden="true" />
        {aiBusy ? "Marking…" : "Highlight with AI"}
      </button>
      {#if aiMenuOpen}
        <AiHighlightMenu busy={aiBusy} onRun={runAutoHighlight} onClose={() => (aiMenuOpen = false)} />
      {/if}
    {/if}
    {#if showZoom}
      <div class="zoom-group row">
        <button class="tool-btn icon" type="button" title="Zoom out" aria-label="Zoom out" onclick={onZoomOut}>
          <ZoomOut size={14} strokeWidth={1.75} aria-hidden="true" />
        </button>
        <span class="zoom-label">{Math.round((zoomScale ?? 1) * 100)}%</span>
        <button class="tool-btn icon" type="button" title="Zoom in" aria-label="Zoom in" onclick={onZoomIn}>
          <ZoomIn size={14} strokeWidth={1.75} aria-hidden="true" />
        </button>
      </div>
    {/if}
  </div>
</div>

<style>
  .reader-toolbar {
    flex-shrink: 0;
    align-items: center;
    gap: 12px;
    height: 38px;
    padding: 0 14px;
    background: var(--bg-1);
  }

  .left,
  .right {
    align-items: center;
    gap: 8px;
    flex: 1;
    min-width: 0;
  }

  .right {
    justify-content: flex-end;
    position: relative;
  }

  .tool-btn.ai {
    border-color: var(--border-2);
    color: var(--amber);
  }

  .tool-btn.ai:hover,
  .tool-btn.ai.on {
    border-color: var(--amber);
    background: rgba(242, 169, 59, 0.08);
  }

  .view-badge {
    padding: 2px 7px;
    border: 1px solid var(--border-2);
    border-radius: 3px;
    color: var(--fg-3);
    font-size: 9px;
    letter-spacing: 0.08em;
  }

  .search {
    align-items: center;
    gap: 5px;
    flex: 0 1 320px;
    min-width: 0;
    padding: 0 8px;
    height: 26px;
    border: 1px solid var(--border-2);
    border-radius: 4px;
    background: var(--bg);
    color: var(--fg-3);
  }

  .search.disabled {
    opacity: 0.55;
  }

  .search input {
    flex: 1;
    min-width: 0;
    border: 0;
    outline: none;
    background: transparent;
    color: var(--fg-1);
    font: inherit;
    font-size: 11px;
  }

  .count {
    flex-shrink: 0;
    font-size: 10px;
  }

  .tool-btn,
  .nav-btn {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    border: 1px solid transparent;
    background: transparent;
    color: var(--fg-2);
    font: inherit;
    font-size: 10px;
    cursor: pointer;
    padding: 3px 6px;
    border-radius: 3px;
  }

  .nav-btn {
    padding: 2px;
  }

  .tool-btn:hover,
  .nav-btn:hover {
    color: var(--amber);
    border-color: var(--border-2);
  }

  .tool-btn:disabled,
  .nav-btn:disabled {
    opacity: 0.4;
    cursor: default;
  }

  .zoom-group {
    gap: 4px;
    align-items: center;
  }

  .zoom-label {
    width: 42px;
    color: var(--fg-3);
    font-size: 10px;
    text-align: center;
  }
</style>
