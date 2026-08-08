<script lang="ts">
  import * as pdfjsLib from "pdfjs-dist/legacy/build/pdf.mjs";
  import workerUrl from "pdfjs-dist/legacy/build/pdf.worker.mjs?url";
  import type { PDFDocumentProxy } from "pdfjs-dist/legacy/build/pdf.mjs";
  import { getReaderPdfBytes } from "$lib/bridge/library";
  import { debugLog } from "$lib/bridge/chat";
  import type { ChatThreadSummary } from "$lib/domain/chat";
  import type { Highlight, Locator } from "$lib/domain/highlight";
  import type { ReaderTextSelection } from "$lib/domain/reader";
  import { ensurePdfJsRuntimeCompatibility } from "$lib/features/reader/pdfjs-compat";
  import PdfRenderedPage from "$lib/features/reader/PdfRenderedPage.svelte";

  ensurePdfJsRuntimeCompatibility();

  pdfjsLib.GlobalWorkerOptions.workerSrc = workerUrl;

  let {
    pdfUrl,
    sourceId,
    threads,
    highlights,
    conversationIds,
    selection,
    chatEnabled,
    scale = 1.15,
    activeTool = null,
    onSelectPassage,
    onHighlightClick,
    onToolHighlight,
    onPlaceNote,
  }: {
    pdfUrl: string;
    sourceId: string;
    threads: ChatThreadSummary[];
    highlights: Highlight[];
    conversationIds?: Set<string>;
    selection: ReaderTextSelection | null;
    chatEnabled: boolean;
    scale?: number;
    // RFC 0073: forwarded verbatim to every page — the intent must survive this
    // hop, or Ask works from some pages and not others (an optional parameter
    // dropped here still type-checks).
    // RFC 0074: the active annotation tool, forwarded to every page.
    activeTool?: "highlight" | "note" | null;
    onSelectPassage: (selection: ReaderTextSelection, intent?: "notes" | "chat") => void;
    onHighlightClick: (highlightId: string, x: number, y: number) => void;
    onToolHighlight?: (selection: ReaderTextSelection) => void;
    onPlaceNote?: (pageIndex: number, x: number, y: number, clientX: number, clientY: number) => void;
  } = $props();

  let pdfDocument = $state<PDFDocumentProxy | null>(null);
  let pageNumbers = $state<number[]>([]);
  // Unscaled page sizes, measured once per document (RFC 0073 Phase 1). Pages
  // size their box from these, so a page whose bitmap has been released still
  // occupies its true height and the scroll bar never jumps.
  let pageSizes = $state<Array<{ width: number; height: number }>>([]);
  let scrollElement = $state<HTMLElement | null>(null);
  let isLoading = $state(false);
  let error = $state("");
  let renderSessionSequence = 0;

  // Highlights anchored to this PDF source become the on-page marks (RFC 0056;
  // no longer gated on pinnedCount — asks persist their highlight too).
  // RFC 0074: `pdfPoint` rides along here — sticky notes are annotations on this
  // source too, and filtering them out at this hop would make every placed note
  // invisible no matter what the page does with it.
  const pdfMarks = $derived(
    highlights.filter(
      (hl) =>
        (hl.locator.kind === "pdfRect" || hl.locator.kind === "pdfPoint") &&
        hl.locator.sourceId === sourceId,
    ),
  );

  $effect(() => {
    const currentSourceId = sourceId;
    const sessionId = (renderSessionSequence += 1);
    let cancelled = false;
    let loadingTask: ReturnType<typeof pdfjsLib.getDocument> | null = null;

    pdfDocument = null;
    pageNumbers = [];
    pageSizes = [];
    error = "";
    isLoading = true;

    async function loadPdf() {
      try {
        const bytes = await getReaderPdfBytes(currentSourceId);
        if (cancelled) {
          return;
        }

        // `bytes` is an ArrayBuffer straight off the IPC, so this wraps rather
        // than copies. pdf.js takes ownership and detaches the buffer — nothing
        // may read `bytes` after this line.
        loadingTask = pdfjsLib.getDocument({ data: new Uint8Array(bytes) });
        const document = await loadingTask.promise;
        if (cancelled) {
          void document.cleanup();
          return;
        }

        // Pages need a box before they render (RFC 0073 Phase 1), but measuring
        // all of them *before* publishing the document would serialize 43 page
        // parses ahead of first paint — worse open latency than the eager render
        // this replaces. So: measure page 1, publish immediately using it as the
        // provisional box for every page, then refine in parallel.
        const firstPage = await document.getPage(1);
        if (cancelled) {
          void document.cleanup();
          return;
        }
        const firstViewport = firstPage.getViewport({ scale: 1 });
        const provisional = { width: firstViewport.width, height: firstViewport.height };
        const numbers = Array.from({ length: document.numPages }, (_, index) => index + 1);

        pdfDocument = document;
        pageSizes = numbers.map(() => provisional);
        pageNumbers = numbers;

        // Correct any page that isn't shaped like page 1 (mixed-orientation
        // documents). Page dictionaries only — no content streams.
        void Promise.all(
          numbers.map(async (number) => {
            const page = await document.getPage(number);
            const viewport = page.getViewport({ scale: 1 });
            return { width: viewport.width, height: viewport.height };
          }),
        )
          .then((sizes) => {
            if (!cancelled) {
              pageSizes = sizes;
            }
          })
          .catch(() => {
            // Provisional sizes stand; a page that renders corrects its own box.
          });
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

  // Child page component refs, so a quote can be resolved against each page's
  // rendered text layer (RFC 0059 follow-up).
  let pageRefs = $state<
    Array<
      | {
          resolveQuote: (quote: string) => Locator | null;
          whenTextReady: () => Promise<void>;
          getElement: () => HTMLElement | null;
        }
      | undefined
    >
  >([]);

  // RFC 0073 (R2.2 / Phase 1.5): centre a page in the reader column, so opening
  // a mark from the Marks list moves the document to it. Deliberately not
  // `scrollIntoView`, which would scroll every scrollable ancestor — including
  // the inspector panel the Marks list lives in. Same one-scroller geometry as
  // `HtmlReader.focusMatch`.
  export function scrollToPage(pageIndex: number) {
    const target = pageRefs[pageIndex]?.getElement();
    const scroller = scrollElement;
    if (!target || !scroller) {
      return;
    }
    const delta =
      target.getBoundingClientRect().top -
      scroller.getBoundingClientRect().top -
      scroller.clientHeight / 2 +
      target.clientHeight / 2;
    // A smooth scroll travels through every page in between, and each one
    // crossing the render margin queues a canvas render that is cancelled a
    // frame later. Fine for a neighbouring page; wasteful for a jump to page 30,
    // so long jumps land instantly (RFC 0073 Phase 1).
    const behavior = Math.abs(delta) > scroller.clientHeight * 2 ? "auto" : "smooth";
    scroller.scrollBy({ top: delta, behavior });
  }

  // Resolves an agent quote to a pdfRect by asking each rendered page to match
  // it against its own text layer (tight, selection-accurate rects). Exposed to
  // ReaderView via `bind:this`; returns the first page that contains the quote.
  //
  // RFC 0069: the text layers render asynchronously, so we must await each
  // page's readiness before matching — otherwise the matcher is handed an empty
  // string and every quote "isn't found." Awaiting readiness (work already in
  // flight, since every page renders eagerly) is what makes auto-highlight land.
  export async function resolveQuote(quote: string): Promise<Locator | null> {
    // Cold open: the document may still be loading and `pageRefs` empty. Resolving
    // against zero pages would falsely report "not located", so wait for the doc
    // + all page components to mount first (bounded, so a stuck load can't hang
    // resolution forever) — RFC 0069.
    const deadline = Date.now() + 15000;
    while (Date.now() < deadline) {
      if (error) break;
      if (pdfDocument && pageNumbers.length > 0 && pageRefs.filter(Boolean).length >= pageNumbers.length) {
        break;
      }
      await new Promise((resolve) => setTimeout(resolve, 50));
    }

    const refs = pageRefs.filter((ref): ref is NonNullable<typeof ref> => Boolean(ref));
    if (refs.length === 0) {
      void debugLog(`pdf resolveQuote: no pages mounted (loading=${isLoading} error=${Boolean(error)})`, "warn");
      return null;
    }
    void debugLog(`pdf resolveQuote: awaiting text layers on ${refs.length}/${pageNumbers.length} page(s)`, "debug");
    await Promise.all(refs.map((ref) => ref.whenTextReady()));
    for (const ref of refs) {
      const locator = ref.resolveQuote(quote);
      if (locator) {
        return locator;
      }
    }
    void debugLog(`pdf resolveQuote: no page matched the quote`, "debug");
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
  <div class="pdf-scroll" bind:this={scrollElement}>
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
        {#each pageNumbers as pageNumber, index}
          <PdfRenderedPage
            bind:this={pageRefs[index]}
            {pdfDocument}
            {pageNumber}
            {scale}
            baseWidth={pageSizes[index]?.width ?? 0}
            baseHeight={pageSizes[index]?.height ?? 0}
            marks={pdfMarks}
            {conversationIds}
            {selection}
            {chatEnabled}
            {sourceId}
            {activeTool}
            {onSelectPassage}
            {onHighlightClick}
            {onToolHighlight}
            {onPlaceNote}
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
