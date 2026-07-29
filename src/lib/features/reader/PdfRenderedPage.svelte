<script lang="ts">
  import { TextLayer } from "pdfjs-dist/legacy/build/pdf.mjs";
  import type { PDFDocumentProxy, PDFPageProxy, PageViewport } from "pdfjs-dist/legacy/build/pdf.mjs";
  import type { Highlight, Locator } from "$lib/domain/highlight";
  import type { PdfRect, ReaderTextSelection } from "$lib/domain/reader";
  import { ensurePdfJsRuntimeCompatibility } from "$lib/features/reader/pdfjs-compat";
  import { markFill } from "$lib/features/reader/highlight-colors";
  import { resolveQuoteInText } from "$lib/features/reader/resolve-quote-html";
  import { debugLog } from "$lib/bridge/chat";
  import { StickyNote, MessageSquare } from "@lucide/svelte";

  type PendingNote = ReaderTextSelection & { x: number; y: number };

  ensurePdfJsRuntimeCompatibility();

  let {
    pdfDocument,
    pageNumber,
    scale,
    marks,
    selection,
    chatEnabled,
    sourceId,
    onSelectPassage,
    onHighlightClick,
  }: {
    pdfDocument: PDFDocumentProxy;
    pageNumber: number;
    scale: number;
    // Highlights anchored to this PDF source — the on-page marks (RFC 0056).
    marks: Highlight[];
    selection: ReaderTextSelection | null;
    chatEnabled: boolean;
    sourceId: string;
    onSelectPassage: (selection: ReaderTextSelection) => void;
    onHighlightClick: (highlightId: string, x: number, y: number) => void;
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
  const pageMarks = $derived(
    marks.filter(
      (mark) =>
        mark.locator.kind === "pdfRect" &&
        mark.locator.pageIndex === pageIndex &&
        // Draw a mark only if the passage has a color or a note; a
        // conversation-only passage has no page mark (RFC 0061).
        (mark.color !== null || mark.note !== null),
    ),
  );
  const draftRects = $derived(selection?.pageIndex === pageIndex ? rectsFromJson(selection.rectsJson) : []);

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
    if (!chatEnabled || !pageElement) {
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

  // Flat text of the rendered text layer, in the same node walk `layerLocate`
  // uses — so a char span found in it maps back to the exact text nodes.
  function layerFullText(container: Node): string {
    const walker = document.createTreeWalker(container, NodeFilter.SHOW_TEXT);
    let text = "";
    let node: Node | null;
    while ((node = walker.nextNode())) {
      text += node.textContent ?? "";
    }
    return text;
  }

  function layerLocate(container: Node, target: number): { node: Node; offset: number } | null {
    const walker = document.createTreeWalker(container, NodeFilter.SHOW_TEXT);
    let count = 0;
    let node: Node | null;
    while ((node = walker.nextNode())) {
      const length = node.textContent?.length ?? 0;
      if (count + length >= target) {
        return { node, offset: target - count };
      }
      count += length;
    }
    return null;
  }

  // Resolve an agent quote to a tight pdfRect on THIS page by matching it in the
  // rendered text layer and taking the selection's client rects — the same path
  // manual highlighting uses, so agent marks hug the text (not full-line bands).
  // Returns null when the quote isn't on this (rendered) page (RFC 0059).
  export function resolveQuote(quote: string): Locator | null {
    const layer = textLayerElement;
    if (!layer) {
      void debugLog(`pdf page ${pageIndex}: no text layer (not rendered yet)`, "debug");
      return null;
    }
    const layerText = layerFullText(layer);
    const span = resolveQuoteInText(layerText, quote);
    if (!span) {
      void debugLog(`pdf page ${pageIndex}: quote not found in ${layerText.length} chars of text layer`, "debug");
      return null;
    }
    const start = layerLocate(layer, span.start);
    const end = layerLocate(layer, span.end);
    if (!start || !end) {
      void debugLog(`pdf page ${pageIndex}: matched span [${span.start},${span.end}] but could not map to text nodes`, "warn");
      return null;
    }
    const range = document.createRange();
    try {
      range.setStart(start.node, start.offset);
      range.setEnd(end.node, end.offset);
    } catch (error) {
      void debugLog(`pdf page ${pageIndex}: range build failed: ${String(error).slice(0, 120)}`, "warn");
      return null;
    }
    const rects = rectsFromClientRects(range.getClientRects());
    if (rects.length === 0) {
      void debugLog(`pdf page ${pageIndex}: matched but produced 0 rects`, "warn");
      return null;
    }
    void debugLog(`pdf page ${pageIndex}: matched at [${span.start},${span.end}] -> ${rects.length} rect(s)`, "debug");
    return { kind: "pdfRect", sourceId, pageIndex, rectsJson: JSON.stringify(rects) };
  }

  // Both popover actions open this passage's thread; Note vs. Ask is chosen in
  // the thread composer (RFC 0034).
  function startThread() {
    if (!pendingNote) {
      return;
    }

    onSelectPassage({
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
  }

  function markRects(mark: Highlight): PdfRect[] {
    return mark.locator.kind === "pdfRect" ? rectsFromJson(mark.locator.rectsJson) : [];
  }

  function rectsFromJson(rectsJson?: string): PdfRect[] {
    if (!rectsJson) {
      return [];
    }

    try {
      const rects = JSON.parse(rectsJson);
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
    {#each pageMarks as mark}
      {#each markRects(mark) as rect}
        <button
          class="pdf-note-anchor"
          type="button"
          aria-label="Highlight actions"
          style={`${rectStyle(rect)} background: ${markFill(mark.color)};`}
          onclick={(event) => {
            event.stopPropagation();
            onHighlightClick(mark.id, event.clientX, event.clientY);
          }}
        ></button>
      {/each}
    {/each}

    {#each draftRects as rect}
      <div class="pdf-note-draft-anchor" style={rectStyle(rect)}></div>
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
  <div
    class="selection-popover"
    style={`left: ${pendingNote.x}px; top: ${pendingNote.y}px;`}
    onmousedown={(event) => event.preventDefault()}
    role="presentation"
  >
    <button class="popover-action" type="button" onclick={startThread}>
      <StickyNote size={14} strokeWidth={1.75} aria-hidden="true" /> Note
    </button>
    <span class="popover-divider" aria-hidden="true"></span>
    <button class="popover-action" type="button" onclick={startThread}>
      <MessageSquare size={14} strokeWidth={1.75} aria-hidden="true" /> Ask
    </button>
  </div>
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

  .pdf-note-draft-anchor {
    position: absolute;
    border: 1px solid rgba(107, 160, 168, 0.95);
    background: rgba(107, 160, 168, 0.2);
    pointer-events: none;
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

  .selection-popover {
    position: fixed;
    z-index: 40;
    display: flex;
    align-items: stretch;
    border: 1px solid var(--amber);
    border-radius: 3px;
    background: var(--bg-1);
    box-shadow: 0 10px 28px rgba(0, 0, 0, 0.35);
    overflow: hidden;
  }

  .popover-action {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    border: 0;
    background: transparent;
    color: var(--amber);
    font: inherit;
    font-size: 11px;
    padding: 5px 10px;
    cursor: pointer;
    white-space: nowrap;
  }

  .popover-action:hover {
    background: rgba(242, 169, 59, 0.12);
  }

  .popover-divider {
    width: 1px;
    background: var(--amber-dim);
  }
</style>
