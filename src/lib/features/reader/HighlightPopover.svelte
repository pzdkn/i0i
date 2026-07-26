<script lang="ts">
  import type { Highlight, HighlightColor } from "$lib/domain/highlight";
  import { HIGHLIGHT_COLORS } from "$lib/domain/highlight";
  import { highlightFill } from "$lib/features/reader/highlight-colors";

  // Click-a-highlight popover (RFC 0058 Task 10): the after-the-fact actions
  // for a mark that's already on the page — recolor, remove, or open/start
  // the passage's thread. Rendered by ReaderView, anchored at the click point
  // (viewport coordinates, same space as MouseEvent.clientX/Y).
  let {
    highlight,
    hasNote = false,
    hasThread = false,
    x,
    y,
    onAddNote,
    onAsk,
    onRecolor,
    onRemove,
    onClose,
  }: {
    highlight: Highlight;
    hasNote?: boolean;
    hasThread?: boolean;
    x: number;
    y: number;
    onAddNote: () => void;
    onAsk: () => void;
    onRecolor: (color: HighlightColor) => void | Promise<void>;
    onRemove: () => void | Promise<void>;
    onClose: () => void;
  } = $props();

  let root: HTMLElement | undefined = $state();

  // Dismiss on any click outside the popover, or Escape — standard popover
  // behavior. The mark's own click handler runs its onclick after this
  // mousedown, so clicking a *different* mark closes this one and opens the
  // next in the same gesture.
  function handleWindowMousedown(event: MouseEvent) {
    if (root && !root.contains(event.target as Node)) {
      onClose();
    }
  }

  function handleWindowKeydown(event: KeyboardEvent) {
    if (event.key === "Escape") {
      onClose();
    }
  }
</script>

<svelte:window onmousedown={handleWindowMousedown} onkeydown={handleWindowKeydown} />

<div
  bind:this={root}
  class="highlight-popover"
  style={`left: ${x}px; top: ${y}px;`}
  role="dialog"
  aria-label="Highlight actions"
  tabindex="-1"
  onmousedown={(event) => event.stopPropagation()}
>
  <div class="row swatch-row" role="group" aria-label="Recolor highlight">
    {#each HIGHLIGHT_COLORS as color}
      <button
        class="swatch"
        class:active={color === highlight.color}
        type="button"
        style={`background:${highlightFill(color)}`}
        aria-label={`Recolor ${color}`}
        title={`Recolor ${color}`}
        onclick={() => void onRecolor(color)}
      ></button>
    {/each}
  </div>

  {#if highlight.excerpt}
    <blockquote class="excerpt">{highlight.excerpt}</blockquote>
  {/if}

  <div class="actions">
    <button class="action" type="button" onclick={onAddNote}>
      📝 {hasNote ? "Note" : "Add note"}
    </button>
    <button class="action" type="button" onclick={onAsk}>
      💬 {hasThread ? "Open" : "Ask"}
    </button>
    <span class="flex1"></span>
    <button class="action remove" type="button" onclick={() => void onRemove()}>Remove</button>
  </div>
</div>

<style>
  .highlight-popover {
    position: fixed;
    z-index: 40;
    width: 220px;
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 10px;
    border: 1px solid var(--amber);
    border-radius: 4px;
    background: var(--bg-1);
    box-shadow: 0 10px 28px rgba(0, 0, 0, 0.35);
  }

  .swatch-row {
    gap: 6px;
    align-items: center;
  }

  .swatch {
    width: 18px;
    height: 18px;
    flex-shrink: 0;
    border: 1px solid var(--border-2);
    border-radius: 50%;
    padding: 0;
    cursor: pointer;
  }

  .swatch:hover {
    border-color: var(--fg-3);
  }

  .swatch.active {
    border-color: var(--fg-1);
    box-shadow: 0 0 0 1px var(--fg-1);
  }

  .excerpt {
    margin: 0;
    padding: 0 0 0 8px;
    border-left: 2px solid var(--amber-mid);
    color: var(--fg-2);
    font-size: 10.5px;
    line-height: 1.4;
    max-height: 4.2em;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .actions {
    display: flex;
    align-items: center;
    gap: 4px;
  }

  .action {
    border: 1px solid var(--border-2);
    background: transparent;
    color: var(--fg-1);
    font: inherit;
    font-size: 11px;
    padding: 4px 8px;
    cursor: pointer;
    white-space: nowrap;
  }

  .action:hover {
    border-color: var(--amber-dim);
    background: rgba(242, 169, 59, 0.08);
    color: var(--amber);
  }

  .action.remove:hover {
    border-color: var(--border-2);
    background: rgba(227, 88, 74, 0.08);
    color: var(--red);
  }

  .flex1 {
    flex: 1;
  }
</style>
