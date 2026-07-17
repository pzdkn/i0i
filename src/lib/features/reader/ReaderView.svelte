<script lang="ts">
  import { listen } from "@tauri-apps/api/event";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { onMount } from "svelte";
  import { getDiscoveryReaderDocument, getReaderDocument } from "$lib/bridge/library";
  import { listChatThreads, listPinnedChatEntries } from "$lib/bridge/chat";
  import ResizableSplit from "$lib/components/layout/ResizableSplit.svelte";
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
    layoutMode = "normal",
    onToggleFocus,
  }: {
    paper: Paper;
    candidate?: DiscoveryReaderCandidate;
    layoutMode?: "normal" | "focus";
    onToggleFocus: () => void;
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
  let focusThreadsMode = $state<"open" | "collapsed">("open");

  const document = $derived<ReaderDocument | null>(readerDocument);
  const chatEnabled = $derived(isPaperInLibrary(paper.id));
  const activeCandidate = $derived(candidate && !chatEnabled ? candidate : undefined);
  const hasCachedPdf = $derived(Boolean(readerDocument?.pdfLocalPath));
  const fallbackSourceUrl = $derived(activeCandidate?.externalUrl ?? readerDocument?.pdfSourceUrl);
  const isFocusMode = $derived(layoutMode === "focus");
  const threadsCollapsed = $derived(isFocusMode && focusThreadsMode === "collapsed");

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
    openThreadsPanel();
  }

  function clearSelection() {
    selection = null;
    window.getSelection()?.removeAllRanges();
  }

  function retryDocumentLoad() {
    refreshTick += 1;
  }

  async function openSourceUrl() {
    if (!fallbackSourceUrl) {
      return;
    }

    try {
      await openUrl(fallbackSourceUrl);
    } catch (error) {
      readerLog("open-source-error", { url: fallbackSourceUrl, error: errorDetail(error) }, "error");
    }
  }

  function openThreadFromMark(threadId: string) {
    requestedThreadId = threadId;
    openThreadsPanel();
  }

  function consumeRequestedThread() {
    requestedThreadId = null;
  }

  function toggleThreadsPanel() {
    focusThreadsMode = focusThreadsMode === "open" ? "collapsed" : "open";
  }

  function openThreadsPanel() {
    if (isFocusMode) {
      focusThreadsMode = "open";
    }
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
    {#snippet readerPane()}
      <main class="reader-main col">
        {#if isFocusMode && !document}
          <div class="focus-fallback-toolbar row hair-b">
            <span class="label hot">Reader Focus</span>
            <div class="flex1"></div>
            <button class="btn primary" type="button" title="Exit reader focus" onclick={onToggleFocus}>
              Exit Focus
            </button>
          </div>
        {/if}

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
          <ReaderHeader
            {document}
            {layoutMode}
            {threadsCollapsed}
            threadCount={threads.length}
            pinCount={pins.length}
            {onToggleFocus}
            onToggleThreads={toggleThreadsPanel}
          />

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
                <div class="label hot">PDF could not be opened automatically</div>
                <p>The publisher may require login, browser verification, or manual access.</p>
                <div class="fallback-actions row">
                  {#if fallbackSourceUrl}
                    <button class="btn primary" type="button" onclick={() => void openSourceUrl()}>Open Source</button>
                  {/if}
                  <button class="btn" type="button" onclick={retryDocumentLoad}>Retry</button>
                </div>
                {#if fallbackSourceUrl}
                  <div class="source-line mono-dim">{fallbackSourceUrl}</div>
                {/if}
                {#if readerDocument?.pdfError}
                  <details class="error-details">
                    <summary>Details</summary>
                    <pre class="debug-block mono-dim">{readerDocument.pdfError}</pre>
                  </details>
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
    {/snippet}

    {#snippet inspectorPane()}
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
    {/snippet}

    {#if isFocusMode}
      {#if document && !threadsCollapsed}
        <ResizableSplit
          storageKey="i0i.reader-focus-split"
          panes={[
            { id: "reader", min: 680, default: 1180 },
            { id: "inspector", min: 300, max: 520, default: 360 },
          ]}
        >
          {#snippet pane(id: string)}
            {#if id === "reader"}
              {@render readerPane()}
            {:else}
              {@render inspectorPane()}
            {/if}
          {/snippet}
        </ResizableSplit>
      {:else}
        <div class="focus-collapsed-layout">
          {@render readerPane()}
          {#if document}
            <button
              class="threads-rail hair-l"
              type="button"
              aria-label="Open threads panel"
              title="Open threads"
              onclick={openThreadsPanel}
            >
              <span>Threads</span>
              <strong>{threads.length}</strong>
              {#if pins.length}
                <em>{pins.length}</em>
              {/if}
            </button>
          {/if}
        </div>
      {/if}
    {:else}
      <ResizableSplit
        storageKey="i0i.reader-split"
        panes={document
          ? [
              { id: "reader", min: 520, default: 980 },
              { id: "inspector", min: 280, max: 560, default: 340 },
            ]
          : [{ id: "reader", min: 520, default: 1200 }]}
      >
        {#snippet pane(id: string)}
          {#if id === "reader"}
            {@render readerPane()}
          {:else}
            {@render inspectorPane()}
          {/if}
        {/snippet}
      </ResizableSplit>
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

  .focus-fallback-toolbar {
    height: 42px;
    flex-shrink: 0;
    gap: 10px;
    padding: 0 14px;
    background: var(--bg-1);
  }

  .focus-collapsed-layout {
    flex: 1;
    min-width: 0;
    min-height: 0;
    display: grid;
    grid-template-columns: minmax(0, 1fr) 42px;
    overflow: hidden;
  }

  .threads-rail {
    width: 42px;
    min-width: 42px;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 8px;
    padding: 12px 0;
    border-top: 0;
    border-right: 0;
    border-bottom: 0;
    background: var(--panel);
    color: var(--fg-3);
    font: inherit;
    cursor: pointer;
  }

  .threads-rail:hover,
  .threads-rail:focus-visible {
    background: rgba(242, 169, 59, 0.08);
    color: var(--amber);
    outline: none;
  }

  .threads-rail span {
    writing-mode: vertical-rl;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    font-size: 9px;
  }

  .threads-rail strong,
  .threads-rail em {
    width: 22px;
    height: 22px;
    display: grid;
    place-items: center;
    border: 1px solid var(--border-2);
    border-radius: 50%;
    color: var(--fg-2);
    font-size: 10px;
    font-style: normal;
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
    max-width: 460px;
    text-align: center;
    line-height: 1.45;
  }

  .fallback-actions {
    gap: 8px;
    align-items: center;
  }

  .source-line {
    max-width: min(680px, 100%);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 12px;
  }

  .error-details {
    width: min(760px, 100%);
  }

  .error-details summary {
    cursor: pointer;
    color: var(--fg-2);
    font-size: 12px;
    text-align: center;
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
