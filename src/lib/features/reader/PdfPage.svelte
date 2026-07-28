<script lang="ts">
  import * as pdfjsLib from "pdfjs-dist/legacy/build/pdf.mjs";
  import workerUrl from "pdfjs-dist/legacy/build/pdf.worker.mjs?url";
  import type { PDFDocumentProxy, PageViewport } from "pdfjs-dist/legacy/build/pdf.mjs";
  import { getReaderPdfBytes } from "$lib/bridge/library";
  import type { ChatThreadSummary } from "$lib/domain/chat";
  import type { Highlight, Locator } from "$lib/domain/highlight";
  import type { ReaderTextSelection } from "$lib/domain/reader";
  import { ensurePdfJsRuntimeCompatibility } from "$lib/features/reader/pdfjs-compat";
  import { coveringRectsForQuote, type PdfTextItem } from "$lib/features/reader/resolve-quote-pdf";
  import PdfRenderedPage from "$lib/features/reader/PdfRenderedPage.svelte";

  ensurePdfJsRuntimeCompatibility();

  pdfjsLib.GlobalWorkerOptions.workerSrc = workerUrl;

  let {
    pdfUrl,
    sourceId,
    threads,
    highlights,
    selection,
    chatEnabled,
    scale = 1.15,
    onSelectPassage,
    onHighlightClick,
  }: {
    pdfUrl: string;
    sourceId: string;
    threads: ChatThreadSummary[];
    highlights: Highlight[];
    selection: ReaderTextSelection | null;
    chatEnabled: boolean;
    scale?: number;
    onSelectPassage: (selection: ReaderTextSelection) => void;
    onHighlightClick: (highlightId: string, x: number, y: number) => void;
  } = $props();

  let pdfDocument = $state<PDFDocumentProxy | null>(null);
  let pageNumbers = $state<number[]>([]);
  let isLoading = $state(false);
  let error = $state("");
  let renderSessionSequence = 0;

  // Highlights anchored to this PDF source become the on-page marks (RFC 0056;
  // no longer gated on pinnedCount — asks persist their highlight too).
  const pdfMarks = $derived(
    highlights.filter((hl) => hl.locator.kind === "pdfRect" && hl.locator.sourceId === sourceId),
  );

  $effect(() => {
    const currentSourceId = sourceId;
    const sessionId = (renderSessionSequence += 1);
    let cancelled = false;
    let loadingTask: ReturnType<typeof pdfjsLib.getDocument> | null = null;

    pdfDocument = null;
    pageNumbers = [];
    error = "";
    isLoading = true;

    async function loadPdf() {
      try {
        const bytes = await getReaderPdfBytes(currentSourceId);
        if (cancelled) {
          return;
        }

        loadingTask = pdfjsLib.getDocument({ data: new Uint8Array(bytes) });
        const document = await loadingTask.promise;
        if (cancelled) {
          void document.cleanup();
          return;
        }

        pdfDocument = document;
        pageNumbers = Array.from({ length: document.numPages }, (_, index) => index + 1);
      } catch (nextError) {
        if (!cancelled) {
          error = String(nextError);
          console.error("[pdf-render] load-error", {
            sessionId,
            sourceId: currentSourceId,
            pdfUrl,
            error,
            detail: errorDetail(nextError),
          });
        }
      } finally {
        if (!cancelled) {
          isLoading = false;
        }
      }
    }

    void loadPdf();
    return () => {
      cancelled = true;
      void loadingTask?.destroy();
    };
  });

  // Best-effort text-item extraction for quote resolution (RFC 0059 Phase 2 /
  // Task 8) — NEEDS MANUAL VERIFICATION against a live rendered PDF. Derives
  // each item's on-page rect from PDF.js's own transform matrix, the same way
  // its text-layer builder positions spans, rather than reading the rendered
  // DOM text layer (`resolve-quote-pdf.ts` flagged that DOM approach as messy
  // — rotated text, marked-content wrapper spans, `<br>`s). This sidesteps
  // that by working from `getTextContent()` items directly, but the rect math
  // (in particular for rotated or vertically-scaled text) is unverified;
  // horizontal, unrotated text (the common case) should be correct.
  // PDF.js's `getTextContent()` types (`TextItem`/`TextMarkedContent`) aren't
  // re-exported from the package's aggregate type entry, so we shape only the
  // fields used here rather than deep-importing internal type paths.
  type RawTextContentItem = { str?: string; transform?: number[]; width?: number };

  function extractPageTextItems(viewport: PageViewport, items: RawTextContentItem[]): PdfTextItem[] {
    const result: PdfTextItem[] = [];
    for (const item of items) {
      if (!item.str || !item.transform || typeof item.width !== "number") {
        continue;
      }
      const tx = pdfjsLib.Util.transform(viewport.transform, item.transform) as number[];
      const fontHeight = Math.hypot(tx[2], tx[3]);
      const scaleX = Math.hypot(tx[0], tx[1]);
      const width = scaleX * item.width;
      if (fontHeight <= 0 || width <= 0) {
        continue;
      }
      const left = tx[4];
      const top = tx[5] - fontHeight;
      result.push({
        str: item.str,
        rect: {
          x: left / viewport.width,
          y: top / viewport.height,
          width: width / viewport.width,
          height: fontHeight / viewport.height,
        },
      });
    }
    return result;
  }

  // Resolves an agent-provided verbatim quote to a pdfRect Locator by scanning
  // pages in order and matching against each page's extracted text items.
  // Exposed to ReaderView via `bind:this`. Best-effort — see
  // `extractPageTextItems` above.
  export async function resolveQuote(quote: string): Promise<Locator | null> {
    const document = pdfDocument;
    if (!document) {
      return null;
    }
    for (let pageNumber = 1; pageNumber <= document.numPages; pageNumber++) {
      // `getPage` returns the document's cached proxy — the same instance
      // `PdfRenderedPage` may be actively rendering — so this deliberately
      // does NOT call `page.cleanup()`; doing so here raced with (and could
      // tear down resources under) that in-progress render.
      const page = await document.getPage(pageNumber);
      const viewport = page.getViewport({ scale: 1 });
      const textContent = await page.getTextContent();
      const items = extractPageTextItems(viewport, textContent.items as RawTextContentItem[]);
      const rects = coveringRectsForQuote(items, quote);
      if (rects) {
        return { kind: "pdfRect", sourceId, pageIndex: pageNumber - 1, rectsJson: JSON.stringify(rects) };
      }
    }
    return null;
  }

  function errorDetail(value: unknown) {
    if (value instanceof Error) {
      return {
        name: value.name,
        message: value.message,
        stack: value.stack,
      };
    }

    return {
      type: typeof value,
      value: String(value),
    };
  }
</script>

<section class="pdf-reader col">
  <div class="pdf-scroll">
    {#if isLoading}
      <div class="pdf-state col">
        <div class="label">Loading PDF...</div>
      </div>
    {:else if error}
      <div class="pdf-state col">
        <div class="label hot">Failed to render PDF</div>
        <p class="mono-dim">{error}</p>
      </div>
    {:else if pdfDocument}
      <div class="page-stack col">
        {#each pageNumbers as pageNumber}
          <PdfRenderedPage
            {pdfDocument}
            {pageNumber}
            {scale}
            marks={pdfMarks}
            {selection}
            {chatEnabled}
            {sourceId}
            {onSelectPassage}
            {onHighlightClick}
          />
        {/each}
      </div>
    {/if}
  </div>
</section>

<style>
  .pdf-reader {
    min-width: 0;
    min-height: 0;
    flex: 1;
    background: #151515;
  }

  .pdf-scroll {
    min-height: 0;
    flex: 1;
    overflow: auto;
    padding: 22px 24px;
  }

  .page-stack {
    align-items: center;
    gap: 18px;
  }

  .pdf-state {
    min-height: 360px;
    align-items: center;
    justify-content: center;
    gap: 8px;
    color: var(--fg-2);
  }
</style>
