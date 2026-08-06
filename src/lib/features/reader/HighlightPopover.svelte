<script lang="ts">
  import type { Highlight, HighlightColor } from "$lib/domain/highlight";
  import { HIGHLIGHT_COLORS, isStickyNote } from "$lib/domain/highlight";
  import { highlightFill } from "$lib/features/reader/highlight-colors";

  // Click-a-highlight popover (RFC 0058 Task 10): the after-the-fact actions
  // for a mark that's already on the page — recolor, remove, or open/start the
  // passage's thread. Rendered by ReaderView, anchored at the click point
  // (viewport coordinates, same space as MouseEvent.clientX/Y).
  let {
    highlight,
    hasThread = false,
    x,
    y,
    onSaveNote,
    onAsk,
    onRecolor,
    onRemove,
    onClose,
  }: {
    highlight: Highlight;
    hasThread?: boolean;
    x: number;
    y: number;
    onSaveNote: (note: string | null) => void | Promise<void>;
    onAsk: () => void;
    onRecolor: (color: HighlightColor) => void | Promise<void>;
    onRemove: () => void | Promise<void>;
    onClose: () => void;
  } = $props();

  let root: HTMLElement | undefined = $state();

  // Inline, editable note (RFC 0061). Seeded from the passage's stored note and
  // re-seeded whenever the popover targets a different highlight. Enter saves;
  // Shift+Enter inserts a newline — never asks the AI.
  let noteDraft = $state("");
  let noteFor = "";
  $effect(() => {
    if (highlight.id !== noteFor) {
      noteFor = highlight.id;
      noteDraft = highlight.note ?? "";
    }
  });
  const noteDirty = $derived(noteDraft.trim() !== (highlight.note ?? "").trim());
  // RFC 0074: a sticky note is anchored to a point, so there is no passage to
  // quote — asking about it would send the model an empty excerpt.
  const isSticky = $derived(isStickyNote(highlight.locator));

  function saveNote() {
    void onSaveNote(noteDraft.trim().length ? noteDraft.trim() : null);
  }

  function handleNoteKeydown(event: KeyboardEvent) {
    if (event.key === "Enter" && !event.shiftKey) {
      event.preventDefault();
      saveNote();
    }
  }

  // Dismiss on any click outside the popover, or Escape. The mark's own click
  // handler runs its onclick after this mousedown, so clicking a *different*
  // mark closes this one and opens the next in the same gesture.
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
  class="hp"
  style={`left: ${x}px; top: ${y}px;`}
  role="dialog"
  aria-label="Highlight actions"
  tabindex="-1"
  onmousedown={(event) => event.stopPropagation()}
>
  {#if highlight.excerpt}
    <div class="hp-quote-row">
      {#if highlight.author.kind === "agent"}
        <span class="hp-tag" title={`AI-added (${highlight.author.model})`}>AI</span>
      {/if}
      <p class="hp-quote">{highlight.excerpt}</p>
    </div>
  {/if}

  <div class="hp-swatches" role="group" aria-label="Recolor highlight">
    {#each HIGHLIGHT_COLORS as color}
      <button
        class="hp-swatch"
        class:active={color === highlight.color}
        type="button"
        style={`background:${highlightFill(color)}`}
        aria-label={`Recolor ${color}`}
        title={color}
        onclick={() => void onRecolor(color)}
      ></button>
    {/each}
  </div>

  <div class="hp-note">
    <textarea
      class="hp-note-input"
      bind:value={noteDraft}
      aria-label="Note"
      placeholder={isSticky ? "Write your note… (Enter saves)" : "Add a note… (Enter saves)"}
      rows="2"
      onkeydown={handleNoteKeydown}
    ></textarea>
    {#if noteDirty}
      <button class="hp-btn hp-save-note" type="button" onclick={saveNote}>Save note</button>
    {/if}
  </div>

  <div class="hp-actions">
    {#if !isSticky}
      <button class="hp-btn" type="button" onclick={onAsk}>{hasThread ? "Open thread" : "Ask"}</button>
    {/if}
    <button class="hp-btn hp-remove" type="button" aria-label="Remove highlight" title="Remove" onclick={() => void onRemove()}>
      Remove
    </button>
  </div>
</div>

<style>
  .hp {
    position: fixed;
    z-index: 40;
    width: 232px;
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding: 12px;
    border: 1px solid var(--hair, var(--border-2));
    border-radius: 8px;
    background: var(--bg-1);
    box-shadow: 0 12px 32px rgba(0, 0, 0, 0.4);
  }

  /* Quoted passage — subtle, clamped to two lines with a real ellipsis. */
  .hp-quote-row {
    display: flex;
    align-items: flex-start;
    gap: 6px;
  }

  .hp-tag {
    flex-shrink: 0;
    margin-top: 1px;
    padding: 1px 5px;
    border-radius: 999px;
    background: rgba(242, 169, 59, 0.16);
    color: var(--amber, #f2a93b);
    font-size: 8.5px;
    font-weight: 600;
    letter-spacing: 0.06em;
  }

  .hp-quote {
    margin: 0;
    color: var(--fg-2);
    font-size: 11px;
    line-height: 1.45;
    font-style: italic;
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
  }

  .hp-quote::before {
    content: "\201C";
    color: var(--fg-3);
  }
  .hp-quote::after {
    content: "\201D";
    color: var(--fg-3);
  }

  /* Recolor swatches. */
  .hp-swatches {
    display: flex;
    gap: 7px;
    align-items: center;
  }

  .hp-swatch {
    width: 17px;
    height: 17px;
    flex-shrink: 0;
    border: 1px solid transparent;
    border-radius: 50%;
    padding: 0;
    cursor: pointer;
    transition: transform 0.08s ease;
  }

  .hp-swatch:hover {
    transform: scale(1.15);
  }

  .hp-swatch.active {
    border-color: var(--fg-1);
    box-shadow: 0 0 0 2px var(--bg-1), 0 0 0 3px var(--fg-1);
  }

  /* Inline editable note. */
  .hp-note {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .hp-note-input {
    width: 100%;
    min-height: 44px;
    resize: vertical;
    padding: 6px 7px;
    border: 1px solid var(--border-2);
    border-radius: 4px;
    outline: none;
    background: var(--bg);
    color: var(--fg-1);
    font: inherit;
    font-size: 11px;
    line-height: 1.45;
  }

  .hp-note-input:focus {
    border-color: var(--cyan);
  }

  .hp-save-note {
    align-self: flex-start;
    color: var(--amber, #f2a93b);
  }

  /* Actions — flat text buttons, Remove pushed to the right. */
  .hp-actions {
    display: flex;
    align-items: center;
    gap: 6px;
    border-top: 1px solid var(--hair, var(--border-2));
    padding-top: 10px;
  }

  .hp-btn {
    border: none;
    background: transparent;
    color: var(--fg-2);
    font: inherit;
    font-size: 11.5px;
    padding: 3px 6px;
    border-radius: 4px;
    cursor: pointer;
    white-space: nowrap;
  }

  .hp-btn:hover {
    background: rgba(242, 169, 59, 0.1);
    color: var(--amber, #f2a93b);
  }

  .hp-remove {
    margin-left: auto;
    color: var(--fg-3);
  }

  .hp-remove:hover {
    background: rgba(227, 88, 74, 0.1);
    color: var(--red, #e3584a);
  }
</style>
