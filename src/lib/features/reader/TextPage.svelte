<script lang="ts">
  import { tick } from "svelte";
  import type { PaperNote } from "$lib/domain/library";
  import type { ReaderDocument, ReaderTextBlock, ReaderTextSelection } from "$lib/domain/reader";

  type TextSegment = {
    text: string;
    noteId?: string;
    active?: boolean;
  };

  let {
    document,
    notes,
    activeNoteId,
    notesEnabled,
    onCreateNoteFromSelection,
  }: {
    document: ReaderDocument;
    notes: PaperNote[];
    activeNoteId: string | null;
    notesEnabled: boolean;
    onCreateNoteFromSelection: (selection: ReaderTextSelection) => void;
  } = $props();

  let pageElement = $state<HTMLElement | null>(null);
  let commentButton = $state<(ReaderTextSelection & { x: number; y: number }) | null>(null);
  const visibleAnchors = $derived(noteAnchors(notes, document));

  $effect(() => {
    const noteId = activeNoteId;
    const root = pageElement;
    if (!noteId || !root) {
      return;
    }

    tick().then(() => {
      const noteElement = [...root.querySelectorAll<HTMLElement>("[data-note-id]")].find(
        (element) => element.dataset.noteId === noteId,
      );

      noteElement?.scrollIntoView({
        behavior: "smooth",
        block: "center",
      });
    });
  });

  function paragraphNumber(id: string) {
    return id.startsWith("p") ? id.replace("p", "") : "";
  }

  function blockEnd(block: ReaderTextBlock) {
    return block.sourceStart + block.text.length;
  }

  function noteAnchors(currentNotes: PaperNote[], currentDocument: ReaderDocument) {
    const anchors: PaperNote[] = [];
    let lastEnd = -1;

    for (const note of currentNotes
      .filter((item) => item.anchorKind === "text_offset")
      .filter((item) => item.sourceId === currentDocument.sourceId)
      .filter((item) => item.startOffset >= 0 && item.endOffset > item.startOffset)
      .filter((item) => item.endOffset <= currentDocument.sourceText.length)
      .sort((left, right) => left.startOffset - right.startOffset || left.endOffset - right.endOffset)) {
      if (note.startOffset < lastEnd) {
        continue;
      }

      anchors.push(note);
      lastEnd = note.endOffset;
    }

    return anchors;
  }

  function segmentsForBlock(block: ReaderTextBlock): TextSegment[] {
    const start = block.sourceStart;
    const end = blockEnd(block);
    const segments: TextSegment[] = [];
    let cursor = start;

    for (const note of visibleAnchors) {
      const segmentStart = Math.max(note.startOffset, start);
      const segmentEnd = Math.min(note.endOffset, end);

      if (segmentStart >= segmentEnd) {
        continue;
      }

      if (segmentStart > cursor) {
        segments.push({
          text: block.text.slice(cursor - start, segmentStart - start),
        });
      }

      segments.push({
        text: block.text.slice(segmentStart - start, segmentEnd - start),
        noteId: note.id,
        active: note.id === activeNoteId,
      });
      cursor = segmentEnd;
    }

    if (cursor < end) {
      segments.push({
        text: block.text.slice(cursor - start),
      });
    }

    return segments.length ? segments : [{ text: block.text }];
  }

  function blockClass(block: ReaderTextBlock) {
    return {
      soft: block.highlight === "soft",
      strong: block.highlight === "strong",
    };
  }

  function sourceOffsetFromBoundary(container: Node, offset: number) {
    const element = container.nodeType === Node.TEXT_NODE ? container.parentElement : (container as Element);
    const block = element?.closest<HTMLElement>("[data-source-start]");
    if (!block || !pageElement?.contains(block)) {
      return null;
    }

    const localOffset = localTextOffset(block, container, offset);
    if (localOffset === null) {
      return null;
    }

    return Number(block.dataset.sourceStart) + localOffset;
  }

  function localTextOffset(block: HTMLElement, container: Node, offset: number) {
    const sourceElement = block.querySelector<HTMLElement>("[data-source-text]") ?? block;
    const range = window.document.createRange();
    range.selectNodeContents(sourceElement);

    try {
      range.setEnd(container, offset);
    } catch {
      return null;
    }

    return range.toString().length;
  }

  function updateSelectionAffordance() {
    if (!notesEnabled) {
      commentButton = null;
      return;
    }

    const selection = window.getSelection();
    if (!selection || selection.rangeCount === 0 || selection.isCollapsed) {
      commentButton = null;
      return;
    }

    const range = selection.getRangeAt(0);
    const commonAncestor =
      range.commonAncestorContainer.nodeType === Node.TEXT_NODE
        ? range.commonAncestorContainer.parentElement
        : (range.commonAncestorContainer as Element);

    if (!commonAncestor || !pageElement?.contains(commonAncestor)) {
      commentButton = null;
      return;
    }

    const startOffset = sourceOffsetFromBoundary(range.startContainer, range.startOffset);
    const endOffset = sourceOffsetFromBoundary(range.endContainer, range.endOffset);
    if (startOffset === null || endOffset === null) {
      commentButton = null;
      return;
    }

    const start = Math.min(startOffset, endOffset);
    const end = Math.max(startOffset, endOffset);
    const selectedText = document.sourceText.slice(start, end);
    const rect = range.getBoundingClientRect();
    if (!selectedText.trim() || end <= start || (!rect.width && !rect.height)) {
      commentButton = null;
      return;
    }

    commentButton = {
      sourceId: document.sourceId,
      startOffset: start,
      endOffset: end,
      selectedText,
      x: Math.min(rect.right + 8, window.innerWidth - 40),
      y: Math.max(rect.top - 4, 8),
    };
  }

  function createNoteFromSelection() {
    if (!commentButton) {
      return;
    }

    onCreateNoteFromSelection({
      sourceId: commentButton.sourceId,
      startOffset: commentButton.startOffset,
      endOffset: commentButton.endOffset,
      selectedText: commentButton.selectedText,
    });
    commentButton = null;
    window.getSelection()?.removeAllRanges();
  }

</script>

<svelte:document onselectionchange={updateSelectionAffordance} />

{#snippet textSegment(segment: TextSegment)}
  {#if segment.noteId}
    <mark
      class="note-anchor"
      class:active={segment.active}
      data-note-id={segment.noteId}
    >
      {segment.text}
    </mark>
  {:else}
    {segment.text}
  {/if}
{/snippet}

<article bind:this={pageElement} class="paper-page">
  <div class="page-kicker">Extracted text / {document.identifier} / p.1-2</div>

  {#each document.textBlocks as block}
    {#if block.kind === "title"}
      <h2 data-source-start={block.sourceStart} data-source-end={blockEnd(block)}>
        <span data-source-text>
          {#each segmentsForBlock(block) as segment}
            {@render textSegment(segment)}
          {/each}
        </span>
      </h2>
    {:else if block.kind === "authors"}
      <p class="paper-authors" data-source-start={block.sourceStart} data-source-end={blockEnd(block)}>
        <span data-source-text>
          {#each segmentsForBlock(block) as segment}
            {@render textSegment(segment)}
          {/each}
        </span>
      </p>
    {:else if block.kind === "heading"}
      <h3 data-source-start={block.sourceStart} data-source-end={blockEnd(block)}>
        <span data-source-text>
          {#each segmentsForBlock(block) as segment}
            {@render textSegment(segment)}
          {/each}
        </span>
      </h3>
    {:else}
      <p
        class:soft={blockClass(block).soft}
        class:strong={blockClass(block).strong}
        data-source-start={block.sourceStart}
        data-source-end={blockEnd(block)}
      >
        <span class="paragraph-number">¶{paragraphNumber(block.id)}</span>
        <span data-source-text>
          {#each segmentsForBlock(block) as segment}
            {@render textSegment(segment)}
          {/each}
        </span>
      </p>
    {/if}
  {/each}

  <div class="page-footer">Page 2 of 15</div>
</article>

{#if commentButton}
  <button
    class="comment-button"
    type="button"
    title="Add note"
    aria-label="Add note"
    style={`left: ${commentButton.x}px; top: ${commentButton.y}px;`}
    onmousedown={(event) => event.preventDefault()}
    onclick={createNoteFromSelection}
  >
    <svg viewBox="0 0 24 24" aria-hidden="true">
      <path
        d="M6 6.5h12v8H9.8L6 18.2V6.5Zm1.5 1.5v6.6l1.7-1.6h7.3V8h-9Z"
        fill="currentColor"
      />
    </svg>
  </button>
{/if}

<style>
  .paper-page {
    width: 580px;
    max-width: 100%;
    min-height: 760px;
    flex-shrink: 0;
    padding: 36px 44px;
    border: 1px solid rgba(0, 0, 0, 0.22);
    background: #f0e6c8;
    color: #1a1208;
    box-shadow: 0 8px 32px rgba(0, 0, 0, 0.6);
    font-family: "IBM Plex Mono", ui-monospace, Menlo, Consolas, monospace;
    font-size: 10.5px;
    line-height: 1.65;
  }

  .page-kicker {
    margin-bottom: 18px;
    color: #7a5a30;
    font-size: 8.5px;
    letter-spacing: 0.14em;
    text-transform: uppercase;
  }

  h2 {
    margin: 0 0 8px;
    font-size: 18px;
    line-height: 1.2;
    text-align: center;
  }

  .paper-authors {
    margin: 0 0 24px;
    padding-left: 0;
    color: #5a4630;
    font-size: 9.5px;
    text-align: center;
  }

  h3 {
    margin: 18px 0 8px;
    font-size: 11px;
  }

  p {
    position: relative;
    margin: 0 0 10px;
    padding-left: 24px;
  }

  p.soft {
    padding: 4px 10px 4px 28px;
    background: rgba(220, 160, 60, 0.35);
  }

  p.strong {
    padding: 4px 10px 4px 28px;
    background: rgba(220, 160, 60, 0.6);
    font-weight: 500;
  }

  .paragraph-number {
    position: absolute;
    top: 2px;
    left: 0;
    color: #7a5a30;
    font-size: 8px;
    user-select: none;
  }

  .note-anchor {
    display: inline;
    padding: 0;
    border-bottom: 1px solid rgba(166, 110, 37, 0.85);
    border-top: 0;
    border-right: 0;
    border-left: 0;
    background: rgba(242, 169, 59, 0.28);
    color: inherit;
    font: inherit;
  }

  .note-anchor.active {
    border-bottom-color: #8a4d12;
    background: rgba(242, 169, 59, 0.55);
    box-shadow: 0 0 0 2px rgba(242, 169, 59, 0.18);
  }

  p.soft .paragraph-number,
  p.strong .paragraph-number {
    left: 6px;
  }

  .page-footer {
    margin-top: 22px;
    color: #7a5a30;
    font-size: 8px;
    letter-spacing: 0.2em;
    text-align: center;
    text-transform: uppercase;
  }

  .comment-button {
    position: fixed;
    z-index: 30;
    width: 28px;
    height: 28px;
    display: grid;
    place-items: center;
    border: 1px solid var(--amber);
    border-radius: 3px;
    background: var(--bg-1);
    color: var(--amber);
    box-shadow: 0 10px 28px rgba(0, 0, 0, 0.35);
    cursor: pointer;
  }

  .comment-button:hover {
    background: rgba(242, 169, 59, 0.12);
  }

  .comment-button svg {
    width: 17px;
    height: 17px;
  }
</style>
