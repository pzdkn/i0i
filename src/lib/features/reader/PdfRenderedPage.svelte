<script lang="ts">
  import { tick } from "svelte";
  import { TextLayer } from "pdfjs-dist/legacy/build/pdf.mjs";
  import type { PDFDocumentProxy, PDFPageProxy, PageViewport } from "pdfjs-dist/legacy/build/pdf.mjs";
  import type { PaperNote } from "$lib/domain/library";
  import type { PdfRect, ReaderTextSelection } from "$lib/domain/reader";
  import { ensurePdfJsRuntimeCompatibility } from "$lib/features/reader/pdfjs-compat";

  type PendingNote = ReaderTextSelection & { x: number; y: number };

  ensurePdfJsRuntimeCompatibility();

  let {
    pdfDocument,
    pageNumber,
    scale,
    notes,
    activeNoteId,
    notesEnabled,
    sourceId,
    onCreateNoteFromSelection,
    onActivateNote,
  }: {
    pdfDocument: PDFDocumentProxy;
    pageNumber: number;
    scale: number;
    notes: PaperNote[];
    activeNoteId: string | null;
    notesEnabled: boolean;
    sourceId: string;
    onCreateNoteFromSelection: (selection: ReaderTextSelection) => void;
    onActivateNote: (noteId: string) => void;
  } = $props();

  let pageElement = $state<HTMLElement | null>(null);
  let canvasElement = $state<HTMLCanvasElement | null>(null);
  let textLayerElement = $state<HTMLElement | null>(null);
  let pageWidth = $state(0);
  let pageHeight = $state(0);
  let isRendering = $state(false);
  let renderError = $state("");
  let pendingNote = $state<PendingNote | null>(null);

  const pageIndex = $derived(pageNumber - 1);
  const pageNotes = $derived(notes.filter((note) => note.pageIndex === pageIndex));

  $effect(() => {
    const document = pdfDocument;
    const currentScale = scale;
    const canvas = canvasElement;
    const textLayer = textLayerElement;
    let cancelled = false;
    let renderTask: { cancel: () => void; promise: Promise<unknown> } | null = null;
    let textLayerTask: TextLayer | null = null;

    if (!canvas || !textLayer) {
      return;
    }

    isRendering = true;
    renderError = "";

    document
      .getPage(pageNumber)
      .then(async (page) => {
        try {
          if (cancelled) {
            return;
          }

          const viewport = page.getViewport({ scale: currentScale });
          pageWidth = viewport.width;
          pageHeight = viewport.height;

          renderTask = renderCanvas(page, viewport, canvas);
          await renderTask.promise;

          if (cancelled) {
            return;
          }

          textLayerTask = renderTextLayer(page, viewport, textLayer);
          await textLayerTask.render();
        } finally {
          page.cleanup();
        }
      })
      .catch((nextError) => {
        if (!cancelled && !isCancelledRenderError(nextError)) {
          renderError = String(nextError);
          console.error("[pdf-render] page-error", { pageNumber, error: nextError });
        }
      })
      .finally(() => {
        if (!cancelled) {
          isRendering = false;
        }
      });

    return () => {
      cancelled = true;
      renderTask?.cancel();
      textLayerTask?.cancel();
    };
  });

  $effect(() => {
    const noteId = activeNoteId;
    const root = pageElement;
    if (!noteId || !root || !pageNotes.some((note) => note.id === noteId)) {
      return;
    }

    tick().then(() => {
      root.scrollIntoView({ behavior: "smooth", block: "center" });
    });
  });

  function renderCanvas(page: PDFPageProxy, viewport: PageViewport, canvas: HTMLCanvasElement) {
    const context = canvas.getContext("2d");
    if (!context) {
      throw new Error("Could not create PDF canvas context");
    }

    const outputScale = window.devicePixelRatio || 1;
    canvas.width = Math.floor(viewport.width * outputScale);
    canvas.height = Math.floor(viewport.height * outputScale);
    canvas.style.width = `${viewport.width}px`;
    canvas.style.height = `${viewport.height}px`;

    return page.render({
      canvas,
      canvasContext: context,
      viewport,
      transform: outputScale === 1 ? undefined : [outputScale, 0, 0, outputScale, 0, 0],
    });
  }

  function renderTextLayer(page: PDFPageProxy, viewport: PageViewport, container: HTMLElement) {
    container.replaceChildren();

    return new TextLayer({
      container,
      viewport,
      textContentSource: page.streamTextContent({ includeMarkedContent: true }),
    });
  }

  function isCancelledRenderError(value: unknown) {
    return (
      value instanceof Error &&
      (value.name === "RenderingCancelledException" || value.name === "AbortException")
    );
  }

  function updateSelectionAffordance() {
    if (!notesEnabled || !pageElement) {
      pendingNote = null;
      return;
    }

    const selection = window.getSelection();
    if (!selection || selection.rangeCount === 0 || selection.isCollapsed) {
      pendingNote = null;
      return;
    }

    const range = selection.getRangeAt(0);
    const ancestor =
      range.commonAncestorContainer.nodeType === Node.TEXT_NODE
        ? range.commonAncestorContainer.parentElement
        : (range.commonAncestorContainer as Element);

    if (!ancestor || !textLayerElement?.contains(ancestor)) {
      pendingNote = null;
      return;
    }

    const rects = rectsFromClientRects(range.getClientRects());
    const selectedText = selection.toString().trim();
    const anchorRect = range.getBoundingClientRect();

    if (!selectedText || rects.length === 0 || (!anchorRect.width && !anchorRect.height)) {
      pendingNote = null;
      return;
    }

    pendingNote = {
      sourceId,
      startOffset: 0,
      endOffset: selectedText.length,
      selectedText,
      anchorKind: "pdf_rect",
      pageIndex,
      rectsJson: JSON.stringify(rects),
      x: Math.min(anchorRect.right + 8, window.innerWidth - 40),
      y: Math.max(anchorRect.top - 4, 8),
    };
  }

  function rectsFromClientRects(clientRects: DOMRectList | DOMRect[]) {
    if (!pageElement) {
      return [];
    }

    const pageRect = pageElement.getBoundingClientRect();
    const rects: PdfRect[] = [];

    for (const rect of Array.from(clientRects)) {
      const left = Math.max(rect.left, pageRect.left);
      const top = Math.max(rect.top, pageRect.top);
      const right = Math.min(rect.right, pageRect.right);
      const bottom = Math.min(rect.bottom, pageRect.bottom);
      const width = right - left;
      const height = bottom - top;

      if (width <= 0 || height <= 0) {
        continue;
      }

      rects.push({
        x: (left - pageRect.left) / pageRect.width,
        y: (top - pageRect.top) / pageRect.height,
        width: width / pageRect.width,
        height: height / pageRect.height,
      });
    }

    return rects;
  }

  function createNote() {
    if (!pendingNote) {
      return;
    }

    onCreateNoteFromSelection({
      sourceId: pendingNote.sourceId,
      startOffset: pendingNote.startOffset,
      endOffset: pendingNote.endOffset,
      selectedText: pendingNote.selectedText,
      anchorKind: "pdf_rect",
      pageIndex: pendingNote.pageIndex,
      rectsJson: pendingNote.rectsJson,
      quoteContext: pendingNote.quoteContext,
    });
    pendingNote = null;
    window.getSelection()?.removeAllRanges();
  }

  function noteRects(note: PaperNote): PdfRect[] {
    if (!note.rectsJson) {
      return [];
    }

    try {
      const rects = JSON.parse(note.rectsJson);
      return Array.isArray(rects) ? rects : [];
    } catch {
      return [];
    }
  }

  function rectStyle(rect: PdfRect) {
    return `left: ${rect.x * 100}%; top: ${rect.y * 100}%; width: ${rect.width * 100}%; height: ${rect.height * 100}%;`;
  }

</script>

<svelte:window onmouseup={updateSelectionAffordance} onkeyup={updateSelectionAffordance} />

<article
  bind:this={pageElement}
  class="pdf-rendered-page"
  style={`width: ${pageWidth}px; height: ${pageHeight}px; --scale-factor: ${scale}; --user-unit: 1; --total-scale-factor: ${scale}; --scale-round-x: 1px; --scale-round-y: 1px;`}
>
  <canvas bind:this={canvasElement} aria-label={`PDF page ${pageNumber}`}></canvas>
  <div bind:this={textLayerElement} class="textLayer text-layer" aria-hidden="true"></div>
  <div class="annotation-layer" role="presentation">
    {#each pageNotes as note}
      {#each noteRects(note) as rect}
        <button
          class="pdf-note-anchor"
          class:active={activeNoteId === note.id}
          type="button"
          aria-label="Show note"
          style={rectStyle(rect)}
          onclick={(event) => {
            event.stopPropagation();
            onActivateNote(note.id);
          }}
        ></button>
      {/each}
    {/each}
  </div>

  {#if isRendering}
    <div class="page-loading">Rendering...</div>
  {/if}

  {#if renderError}
    <div class="page-error">{renderError}</div>
  {/if}
</article>

{#if pendingNote}
  <button
    class="comment-button"
    type="button"
    title="Add note"
    aria-label="Add note"
    style={`left: ${pendingNote.x}px; top: ${pendingNote.y}px;`}
    onmousedown={(event) => event.preventDefault()}
    onclick={createNote}
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
  .pdf-rendered-page {
    position: relative;
    flex-shrink: 0;
    overflow: hidden;
    border: 1px solid rgba(0, 0, 0, 0.28);
    background: #fff;
    box-shadow: 0 8px 32px rgba(0, 0, 0, 0.48);
  }

  canvas {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
  }

  .text-layer {
    position: absolute;
    inset: 0;
    overflow: hidden;
    opacity: 1;
    line-height: 1;
    text-align: initial;
    letter-spacing: normal;
    word-spacing: normal;
    text-size-adjust: none;
    transform-origin: 0 0;
    --min-font-size: 1;
    --text-scale-factor: calc(var(--total-scale-factor, 1) * var(--min-font-size));
    --min-font-size-inv: calc(1 / var(--min-font-size));
  }

  .text-layer :global(span),
  .text-layer :global(br) {
    position: absolute;
    color: transparent;
    white-space: pre;
    cursor: text;
    transform-origin: 0% 0%;
    user-select: text;
  }

  .text-layer :global(> :not(.markedContent)),
  .text-layer :global(.markedContent span:not(.markedContent)) {
    z-index: 1;
    --font-height: 0;
    --scale-x: 1;
    --rotate: 0deg;
    font-size: calc(var(--text-scale-factor) * var(--font-height));
    transform: rotate(var(--rotate)) scaleX(var(--scale-x)) scale(var(--min-font-size-inv));
  }

  .text-layer :global(.markedContent) {
    display: contents;
  }

  .text-layer :global(span::selection),
  .text-layer :global(br::selection) {
    background: rgba(107, 160, 168, 0.42);
    color: transparent;
  }

  .text-layer :global(span::-webkit-selection),
  .text-layer :global(br::-webkit-selection) {
    background: rgba(107, 160, 168, 0.42);
    color: transparent;
  }

  .annotation-layer {
    position: absolute;
    inset: 0;
    pointer-events: none;
  }

  .pdf-note-anchor {
    position: absolute;
    border: 1px solid rgba(242, 169, 59, 0.88);
    background: rgba(242, 169, 59, 0.2);
    padding: 0;
    cursor: pointer;
    pointer-events: auto;
  }

  .pdf-note-anchor.active {
    border-color: rgba(107, 160, 168, 0.95);
    background: rgba(107, 160, 168, 0.22);
    box-shadow: 0 0 0 2px rgba(107, 160, 168, 0.18);
  }

  .page-loading,
  .page-error {
    position: absolute;
    right: 8px;
    bottom: 8px;
    padding: 3px 6px;
    border: 1px solid var(--border);
    background: rgba(12, 12, 12, 0.8);
    color: var(--fg-2);
    font-size: 10px;
  }

  .page-error {
    color: var(--red);
  }

  .comment-button {
    position: fixed;
    z-index: 40;
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
