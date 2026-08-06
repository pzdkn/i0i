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

  // RFC 0073 Phase 0: the canvas backing store is allocated at
  // `viewport × devicePixelRatio`. On a Retina display that is 4× the pixels —
  // ~9.8MB for a single letter page at scale 1.15. Cap the multiplier: the
  // sharpness cost is small, the memory saving is ~1.8×.
  const MAX_CANVAS_DPR = 1.5;

  // RFC 0073 Phase 1: how far outside the viewport a page still renders its
  // bitmap, as a fraction of viewport height. 1.5 keeps roughly three screens
  // of pages hot, so ordinary scrolling never waits on a render.
  const RENDER_MARGIN = "150% 0px";

  let {
    pdfDocument,
    pageNumber,
    scale,
    baseWidth,
    baseHeight,
    marks,
    conversationIds,
    selection,
    chatEnabled,
    sourceId,
    onSelectPassage,
    onHighlightClick,
  }: {
    pdfDocument: PDFDocumentProxy;
    pageNumber: number;
    scale: number;
    // Unscaled page size, measured once by the parent (RFC 0073 Phase 1). The
    // page box is sized from this *before* anything renders, so a page whose
    // bitmap is released still holds its place in the scroll height.
    baseWidth: number;
    baseHeight: number;
    // Highlights anchored to this PDF source — the on-page marks (RFC 0056).
    marks: Highlight[];
    // Highlight ids with a conversation (RFC 0067): they draw the neutral marker.
    conversationIds?: Set<string>;
    selection: ReaderTextSelection | null;
    chatEnabled: boolean;
    sourceId: string;
    // RFC 0073: the popover says which pane the passage should open in. Without
    // it the landing section is whatever the rail was last left on, so "Ask"
    // opened the note editor.
    onSelectPassage: (selection: ReaderTextSelection, intent?: "notes" | "chat") => void;
    onHighlightClick: (highlightId: string, x: number, y: number) => void;
  } = $props();

  let pageElement = $state<HTMLElement | null>(null);
  let canvasElement = $state<HTMLCanvasElement | null>(null);
  let textLayerElement = $state<HTMLElement | null>(null);
  let isRendering = $state(false);
  let renderError = $state("");
  let pendingNote = $state<PendingNote | null>(null);
  // RFC 0073 Phase 1: whether this page is near enough to the viewport to hold a
  // rendered bitmap. Driven by the observer below; the page element itself stays
  // mounted (and so do its text layer and marks) whatever this says.
  let isNear = $state(false);

  // Page geometry no longer waits on `getPage()` — the parent measures every
  // page up front, so the box has its true size from first paint (RFC 0073).
  const pageWidth = $derived(baseWidth * scale);
  const pageHeight = $derived(baseHeight * scale);

  export function getElement(): HTMLElement | null {
    return pageElement;
  }

  // RFC 0069: readiness gate. `resolveQuote` reads the rendered DOM text layer,
  // which is empty until the async render below finishes. `whenTextReady()` lets
  // the parent await that before matching agent quotes. Flipped false at the
  // start of every (re)render and true in the render `finally` — on success or
  // error — so a failed page never hangs the await (it just yields no match).
  let textReady = false;
  let readyWaiters: Array<() => void> = [];
  function markTextReady() {
    textReady = true;
    const waiters = readyWaiters;
    readyWaiters = [];
    for (const resolve of waiters) resolve();
  }
  export function whenTextReady(): Promise<void> {
    return textReady ? Promise.resolve() : new Promise((resolve) => readyWaiters.push(resolve));
  }

  const pageIndex = $derived(pageNumber - 1);
  const pageMarks = $derived(
    marks.filter(
      (mark) =>
        mark.locator.kind === "pdfRect" &&
        mark.locator.pageIndex === pageIndex &&
        // Draw a mark if the passage has a color, a note, or a conversation
        // (RFC 0061 + RFC 0067: ask leaves the neutral marker).
        (mark.color !== null || mark.note !== null || Boolean(conversationIds?.has(mark.id))),
    ),
  );
  const draftRects = $derived(selection?.pageIndex === pageIndex ? rectsFromJson(selection.rectsJson) : []);

  // RFC 0073 Phase 1: the text layer and the canvas bitmap are now rendered by
  // SEPARATE effects. They used to share one, which meant that re-rendering a
  // bitmap on scroll would also tear down and rebuild the text layer — flipping
  // `textReady` false under an in-flight `resolveQuote` (RFC 0069) and
  // destroying the DOM ranges a live text selection points at.
  //
  // The text layer belongs to the document, not to the viewport: it renders once
  // per (document, scale) and stays. Only `textReady` is touched here.
  $effect(() => {
    const document = pdfDocument;
    const currentScale = scale;
    const textLayer = textLayerElement;
    let cancelled = false;
    let textLayerTask: TextLayer | null = null;

    // A fresh render invalidates the previous text layer; invalidate readiness
    // before the early-return guard so no run can leave `textReady` stale-true
    // while the layer is being torn down/rebuilt (RFC 0069).
    textReady = false;

    if (!textLayer) {
      return;
    }

    document
      .getPage(pageNumber)
      .then(async (page) => {
        if (cancelled) {
          return;
        }
        const viewport = page.getViewport({ scale: currentScale });
        textLayerTask = renderTextLayer(page, viewport, textLayer);
        await textLayerTask.render();
      })
      .catch((nextError) => {
        if (!cancelled && !isCancelledRenderError(nextError)) {
          console.error("[pdf-render] text-layer-error", { pageNumber, error: nextError });
        }
      })
      .finally(() => {
        if (!cancelled) {
          // Text layer is rendered (or this page errored out and never will be);
          // either way, release quote-resolution awaiters (RFC 0069).
          markTextReady();
        }
      });

    return () => {
      cancelled = true;
      textLayerTask?.cancel();
    };
  });

  // The bitmap, in contrast, is viewport-scoped: painted when the page comes
  // near, released when it leaves. Releasing is what bounds memory by window
  // size instead of by page count.
  $effect(() => {
    const document = pdfDocument;
    const currentScale = scale;
    const canvas = canvasElement;
    const near = isNear;
    let cancelled = false;
    let renderTask: { cancel: () => void; promise: Promise<unknown> } | null = null;

    if (!canvas) {
      return;
    }

    if (!near) {
      releaseCanvas(canvas);
      // Drop the page's parsed operator list too — the heavy half of a rendered
      // page. Only once the text layer is done with it, since both share the
      // same `PDFPageProxy`.
      if (textReady) {
        void document.getPage(pageNumber).then((page) => page.cleanup());
      }
      return;
    }

    isRendering = true;
    renderError = "";

    document
      .getPage(pageNumber)
      .then(async (page) => {
        if (cancelled) {
          return;
        }
        const viewport = page.getViewport({ scale: currentScale });
        renderTask = renderCanvas(page, viewport, canvas);
        await renderTask.promise;
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
    };
  });

  // Drive `isNear` from the page's own position in the scroll container. One
  // observer per page, each with a single target — cheaper than any scroll
  // handler, and it needs no coordination with the parent.
  $effect(() => {
    const element = pageElement;
    if (!element || typeof IntersectionObserver === "undefined") {
      // No observer (SSR, ancient runtime): render eagerly rather than never.
      isNear = true;
      return;
    }

    const observer = new IntersectionObserver(
      (entries) => {
        for (const entry of entries) {
          isNear = entry.isIntersecting;
        }
      },
      { root: element.closest(".pdf-scroll"), rootMargin: RENDER_MARGIN },
    );
    observer.observe(element);

    return () => observer.disconnect();
  });

  function renderCanvas(page: PDFPageProxy, viewport: PageViewport, canvas: HTMLCanvasElement) {
    const context = canvas.getContext("2d");
    if (!context) {
      throw new Error("Could not create PDF canvas context");
    }

    // RFC 0073 Phase 0: capped, not raw DPR — see MAX_CANVAS_DPR.
    const outputScale = Math.min(window.devicePixelRatio || 1, MAX_CANVAS_DPR);
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

  // Free a page's pixels without disturbing its box: the element keeps its CSS
  // size (driven by `baseWidth`/`baseHeight`), so the scroll height is stable
  // whether or not the bitmap exists (RFC 0073 Phase 1).
  function releaseCanvas(canvas: HTMLCanvasElement) {
    if (canvas.width === 0 && canvas.height === 0) {
      return;
    }
    canvas.width = 0;
    canvas.height = 0;
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

  // RFC 0073: the two popover actions used to share one intent-less handler, so
  // the pane you landed in was decided by the rail's sticky section rather than
  // by the button you clicked. Each now says what it means.
  function startThread(intent: "notes" | "chat") {
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
    }, intent);
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
    <button class="popover-action" type="button" onclick={() => startThread("notes")}>
      <StickyNote size={14} strokeWidth={1.75} aria-hidden="true" /> Note
    </button>
    <span class="popover-divider" aria-hidden="true"></span>
    <button class="popover-action" type="button" onclick={() => startThread("chat")}>
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
    /* RFC 0073 Phase 0: was `0 8px 32px` — a 32px blur on every page of a long
       document is real compositing work on each scroll frame. */
    box-shadow: 0 1px 4px rgba(0, 0, 0, 0.45);
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
