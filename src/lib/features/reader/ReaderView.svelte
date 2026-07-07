<script lang="ts">
  import { listen } from "@tauri-apps/api/event";
  import { onMount } from "svelte";
  import { getDiscoveryReaderDocument, getReaderDocument } from "$lib/bridge/library";
  import { listChatThreads, listPinnedChatEntries } from "$lib/bridge/chat";
  import type { ChatThreadSummary, PinnedHighlight } from "$lib/domain/chat";
  import type { Paper } from "$lib/domain/paper";
  import type { DiscoveryReaderCandidate, ReaderDocument, ReaderTextSelection } from "$lib/domain/reader";
  import ReaderFooter from "$lib/features/reader/ReaderFooter.svelte";
  import ReaderHeader from "$lib/features/reader/ReaderHeader.svelte";
  import ReaderInspector from "$lib/features/reader/ReaderInspector.svelte";
  import PdfPage from "$lib/features/reader/PdfPage.svelte";
  import { isPaperInLibrary } from "$lib/state/library-cache.svelte";

  let {
    paper,
    candidate,
  }: {
    paper: Paper;
    candidate?: DiscoveryReaderCandidate;
  } = $props();

  // Anchored chat state (RFC 0034). ReaderView owns the thread/pin lists so the
  // PDF highlight layer (pinned threads) and the inspector share one source of
  // truth, and a single reload keeps marks, the Threads list, and Pins in sync.
  let selection = $state<ReaderTextSelection | null>(null);
  let threads = $state<ChatThreadSummary[]>([]);
  let pins = $state<PinnedHighlight[]>([]);
  let requestedThreadId = $state<string | null>(null);
  let isLoadingChat = $state(false);
  let chatError = $state("");
  let chatLoadSequence = 0;
  let readerDocument = $state<ReaderDocument | null>(null);
  let docError = $state("");
  let docErrorDebug = $state("");
  let isLoadingDoc = $state(false);
  let refreshTick = $state(0);
  let documentLoadSequence = 0;

  const document = $derived<ReaderDocument | null>(readerDocument);
  const chatEnabled = $derived(isPaperInLibrary(paper.id));
  const activeCandidate = $derived(candidate && !chatEnabled ? candidate : undefined);
  const hasCachedPdf = $derived(Boolean(readerDocument?.pdfLocalPath));

  onMount(() => {
    let unlistenSource: (() => void) | undefined;
    let unlistenExtraction: (() => void) | undefined;
    let unlistenChatThread: (() => void) | undefined;

    listen("document_source_updated", (event) => {
      const payload = event.payload as { paperId?: string; paper_id?: string };
      if ((payload.paperId ?? payload.paper_id) === paper.id) {
        refreshTick += 1;
      }
    })
      .then((nextUnlisten) => {
        unlistenSource = nextUnlisten;
      })
      .catch((error) => {
        console.error("Failed to listen for document_source_updated:", error);
      });

    listen("document_extraction_updated", (event) => {
      const payload = event.payload as { paperId?: string; paper_id?: string };
      if ((payload.paperId ?? payload.paper_id) === paper.id) {
        refreshTick += 1;
      }
    })
      .then((nextUnlisten) => {
        unlistenExtraction = nextUnlisten;
      })
      .catch((error) => {
        console.error("Failed to listen for document_extraction_updated:", error);
      });

    listen("chat_thread_updated", (event) => {
      const payload = event.payload as { threadId?: string; thread_id?: string; title?: string };
      const threadId = payload.threadId ?? payload.thread_id;
      const title = payload.title;
      if (!threadId || !title) {
        return;
      }
      threads = threads.map((thread) => (thread.id === threadId ? { ...thread, title } : thread));
      pins = pins.map((pin) =>
        pin.entry.threadId === threadId ? { ...pin, threadTitle: title } : pin,
      );
    })
      .then((nextUnlisten) => {
        unlistenChatThread = nextUnlisten;
      })
      .catch((error) => {
        console.error("Failed to listen for chat_thread_updated:", error);
      });

    return () => {
      unlistenSource?.();
      unlistenExtraction?.();
      unlistenChatThread?.();
    };
  });

  // Fetch real ReaderDocument from backend whenever paper changes or refreshTick increments
  $effect(() => {
    const paperId = paper.id;
    const loadId = (documentLoadSequence += 1);
    void refreshTick;
    docError = "";
    docErrorDebug = "";
    readerDocument = null;
    isLoadingDoc = true;
    readerLog("document-load-start", { loadId, paperId, refreshTick });

    const loadDocument = activeCandidate ? getDiscoveryReaderDocument(activeCandidate) : getReaderDocument(paperId);

    loadDocument
      .then((doc) => {
        readerLog("document-load-resolved", {
          loadId,
          paperId,
          targetKind: activeCandidate ? "discovery_candidate" : "saved_paper",
          docPaperId: doc.paperId,
          sourceId: doc.sourceId,
          hasPdf: Boolean(doc.pdfLocalPath),
          pdfError: doc.pdfError,
        });
        if (paper.id === paperId) {
          readerDocument = doc;
        }
      })
      .catch((error) => {
        const detail = errorDetail(error);
        readerLog("document-load-error", { loadId, paperId, error: detail }, "error");
        if (paper.id === paperId) {
          docError = String(error);
          docErrorDebug = JSON.stringify(detail, null, 2);
        }
      })
      .finally(() => {
        readerLog("document-load-finally", { loadId, paperId, stillCurrent: paper.id === paperId });
        if (paper.id === paperId) {
          isLoadingDoc = false;
        }
      });
  });

  // Load the paper's threads + pins when it changes (once it's chat-enabled).
  $effect(() => {
    const paperId = paper.id;
    selection = null;
    requestedThreadId = null;
    chatError = "";

    if (!chatEnabled) {
      threads = [];
      pins = [];
      isLoadingChat = false;
      return;
    }

    void reloadChat(paperId);
  });

  // Reload threads + pins together, so the PDF marks, Threads list, and Pins
  // tab stay consistent after any chat mutation.
  async function reloadChat(paperId: string = paper.id) {
    if (!chatEnabled) {
      return;
    }

    const loadId = (chatLoadSequence += 1);
    isLoadingChat = true;
    try {
      const [threadList, pinList] = await Promise.all([
        listChatThreads({ kind: "paper", paperId }),
        listPinnedChatEntries({ kind: "paper", paperId }),
      ]);
      if (paper.id === paperId && loadId === chatLoadSequence) {
        threads = threadList;
        pins = pinList;
      }
    } catch (error) {
      if (paper.id === paperId) {
        chatError = String(error);
      }
    } finally {
      if (paper.id === paperId && loadId === chatLoadSequence) {
        isLoadingChat = false;
      }
    }
  }

  function selectPassage(next: ReaderTextSelection) {
    selection = next;
  }

  function clearSelection() {
    selection = null;
    window.getSelection()?.removeAllRanges();
  }

  function openThreadFromMark(threadId: string) {
    requestedThreadId = threadId;
  }

  function consumeRequestedThread() {
    requestedThreadId = null;
  }

  function readerLog(stage: string, payload: Record<string, unknown>, level: "info" | "error" = "info") {
    const message = `[reader ${new Date().toISOString()}] ${stage}`;
    if (level === "error") {
      console.error(message, payload);
      return;
    }

    console.info(message, payload);
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

<section class="reader-workspace col">
  <div class="reader-body row">
    <main class="reader-main col">
      {#if isLoadingDoc}
        <div class="loading col">
          <div class="label">Loading document…</div>
          <div class="progress-shell" aria-hidden="true">
            <div class="progress-bar"></div>
          </div>
        </div>
      {:else if docError}
        <div class="doc-error col">
          <div class="label hot">Failed to load document</div>
          <pre class="debug-block mono-dim">{docError}</pre>
          {#if docErrorDebug}
            <pre class="debug-block mono-dim">{docErrorDebug}</pre>
          {/if}
        </div>
      {:else if document}
        <ReaderHeader {document} />

        <div class="reading-surface row">
          {#if hasCachedPdf}
            <PdfPage
              pdfUrl={readerDocument!.pdfLocalPath!}
              sourceId={document.sourceId}
              {threads}
              {selection}
              {chatEnabled}
              onSelectPassage={selectPassage}
              onOpenThread={openThreadFromMark}
            />
          {:else}
            <div class="missing-pdf col">
              <div class="label hot">PDF not available</div>
              {#if readerDocument?.pdfError}
                <pre class="debug-block mono-dim">{readerDocument.pdfError}</pre>
              {:else}
                <p>This paper has no cached PDF.</p>
              {/if}
              {#if readerDocument?.pdfSourceUrl}
                <pre class="debug-block mono-dim">Source: {readerDocument.pdfSourceUrl}</pre>
              {/if}
            </div>
          {/if}
        </div>

        <ReaderFooter />
      {:else}
        <div class="missing-pdf col">
          <div class="label hot">Document not found</div>
          <p>This paper is not in the library database.</p>
        </div>
      {/if}
    </main>

    {#if document}
      <ReaderInspector
        {document}
        {chatEnabled}
        {threads}
        {pins}
        {selection}
        {requestedThreadId}
        {isLoadingChat}
        {chatError}
        onReloadChat={reloadChat}
        onClearSelection={clearSelection}
        onConsumeRequestedThread={consumeRequestedThread}
      />
    {/if}
  </div>
</section>

<style>
  .reader-workspace {
    flex: 1;
    min-width: 0;
    min-height: 0;
    background: var(--bg);
  }

  .reader-body {
    flex: 1;
    min-height: 0;
    align-items: stretch;
  }

  .reader-main {
    flex: 1;
    min-width: 0;
    min-height: 0;
  }

  .reading-surface {
    flex: 1;
    min-height: 0;
    align-items: stretch;
    overflow: hidden;
    background: var(--bg-2);
  }

  .loading,
  .doc-error,
  .missing-pdf {
    flex: 1;
    align-items: center;
    justify-content: center;
    gap: 8px;
    color: var(--fg-2);
    padding: 24px;
  }

  .loading .label,
  .doc-error .label,
  .missing-pdf .label {
    font-size: 14px;
    font-weight: 600;
  }

  .missing-pdf p {
    margin: 0;
  }

  .progress-shell {
    width: min(360px, 100%);
    height: 6px;
    overflow: hidden;
    border: 1px solid var(--border-2);
    background: var(--bg-1);
  }

  .progress-bar {
    width: 40%;
    height: 100%;
    background: var(--amber);
    animation: reader-progress 1.1s ease-in-out infinite;
  }

  .debug-block {
    width: min(900px, 100%);
    margin: 0;
    padding: 12px 14px;
    overflow: auto;
    border: 1px solid var(--border-2);
    background: var(--bg-1);
    white-space: pre-wrap;
    word-break: break-word;
    line-height: 1.45;
  }

  @keyframes reader-progress {
    0% {
      transform: translateX(-120%);
    }

    100% {
      transform: translateX(320%);
    }
  }
</style>
