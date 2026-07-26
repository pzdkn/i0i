<script lang="ts">
  import * as pdfjsLib from "pdfjs-dist/legacy/build/pdf.mjs";
  import workerUrl from "pdfjs-dist/legacy/build/pdf.worker.mjs?url";
  import type { PDFDocumentProxy } from "pdfjs-dist/legacy/build/pdf.mjs";
  import { getReaderPdfBytes } from "$lib/bridge/library";
  import type { ChatThreadSummary } from "$lib/domain/chat";
  import type { Highlight } from "$lib/domain/highlight";
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
