<script lang="ts">
  import { listen } from "@tauri-apps/api/event";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { onMount } from "svelte";
  import {
    cancelDiscoveryPdfAcquisition,
    getDiscoveryReaderDocument,
    getReaderDocument,
    openHtmlDocument,
  } from "$lib/bridge/library";
  import { listChatThreads, listPinnedChatEntries, type HighlightIntent } from "$lib/bridge/chat";
  import {
    createAgentHighlight,
    createHighlight,
    listHighlights,
    recolorHighlight,
    removeHighlight,
  } from "$lib/bridge/highlight";
  import { getSettings } from "$lib/bridge/settings";
  import ResizableSplit from "$lib/components/layout/ResizableSplit.svelte";
  import type { ChatThreadSummary, PinnedHighlight } from "$lib/domain/chat";
  import { HIGHLIGHT_COLORS, type Highlight, type HighlightColor, type Locator } from "$lib/domain/highlight";
  import type {
    MetadataAutofillProgress,
    MetadataCandidate,
    PaperMetadataUpdate,
  } from "$lib/domain/library";
  import type { Paper } from "$lib/domain/paper";
  import type { DiscoveryReaderCandidate, ReaderDocument, ReaderTextSelection } from "$lib/domain/reader";
  import ReaderFooter from "$lib/features/reader/ReaderFooter.svelte";
  import ReaderHeader from "$lib/features/reader/ReaderHeader.svelte";
  import ReaderInspector from "$lib/features/reader/ReaderInspector.svelte";
  import PdfPage from "$lib/features/reader/PdfPage.svelte";
  import HtmlReader from "$lib/features/reader/HtmlReader.svelte";
  import HighlightPopover from "$lib/features/reader/HighlightPopover.svelte";
  import { findThreadForHighlight, hasExistingHighlight, samePassage } from "$lib/features/reader/highlight-thread-match";
  import { isPaperInLibrary } from "$lib/state/library-cache.svelte";

  let {
    paper,
    candidate,
    layoutMode = "normal",
    metadataAutofillProgress,
    isAutofillingMetadata = false,
    onAutofillMetadata,
    onApplyMetadataCandidate,
    onUpdatePaperMetadata,
    onToggleFocus,
  }: {
    paper: Paper;
    candidate?: DiscoveryReaderCandidate;
    layoutMode?: "normal" | "focus";
    metadataAutofillProgress?: MetadataAutofillProgress;
    isAutofillingMetadata?: boolean;
    onAutofillMetadata?: (paperId: string) => void | Promise<void>;
    onApplyMetadataCandidate: (paperId: string, candidate: MetadataCandidate) => void | Promise<void>;
    onUpdatePaperMetadata: (paperId: string, update: PaperMetadataUpdate) => void | Promise<void>;
    onToggleFocus: () => void;
  } = $props();

  // Anchored chat state (RFC 0034). ReaderView owns the thread/pin lists so the
  // PDF highlight layer (pinned threads) and the inspector share one source of
  // truth, and a single reload keeps marks, the Threads list, and Pins in sync.
  let selection = $state<ReaderTextSelection | null>(null);
  let threads = $state<ChatThreadSummary[]>([]);
  let pins = $state<PinnedHighlight[]>([]);
  let highlights = $state<Highlight[]>([]);
  // RFC 0058 Phase 1 (Task 9): the last color picked (swatch or note/ask) is
  // the sticky default for the next one-click highlight.
  let stickyColor = $state<HighlightColor>("yellow");
  let requestedThreadId = $state<string | null>(null);
  // Click-a-highlight popover (RFC 0058 Task 10): which mark's popover is
  // open, and where to anchor it (viewport coords from the click event).
  let popoverHighlightId = $state<string | null>(null);
  let popoverPos = $state<{ x: number; y: number } | null>(null);
  let isLoadingChat = $state(false);
  let chatError = $state("");
  let chatLoadSequence = 0;
  let readerDocument = $state<ReaderDocument | null>(null);
  let docError = $state("");
  let docErrorDebug = $state("");
  let isLoadingDoc = $state(false);
  let refreshTick = $state(0);
  let documentLoadSequence = 0;
  // RFC 0051: background PDF acquisition status for discovery opens.
  let acquisitionMessage = $state("");
  let forceNextLoad = false;
  let focusThreadsMode = $state<"open" | "collapsed">("open");
  let pdfScale = $state(1.15);
  // RFC 0059 Phase 2 (Task 8): refs to the active reader so intents can be
  // resolved wherever the content actually lives — the HTML reader resolves
  // synchronously against its rendered text; the PDF reader resolves
  // asynchronously against per-page text-content items.
  let htmlReaderRef = $state<HtmlReader | undefined>();
  let pdfPageRef = $state<PdfPage | undefined>();
  // The chat model string to attribute agent-created highlights to, resolved
  // once from settings; falls back to a generic label if unset.
  let chatModel = $state("agent");
  // RFC 0059 Phase 2 (Task 9): ids of highlights the agent created during the
  // ask turn currently in flight (or just settled) — reset at the start of
  // each turn, accumulated as each HighlightIntent resolves to a created
  // Highlight. Drives the batch Keep/Undo affordance once the turn completes.
  let turnHighlightIds = $state<string[]>([]);
  let showTurnAffordance = $state(false);

  const document = $derived<ReaderDocument | null>(readerDocument);
  const chatEnabled = $derived(isPaperInLibrary(paper.id));
  const activeCandidate = $derived(candidate && !chatEnabled ? candidate : undefined);
  const hasCachedPdf = $derived(Boolean(readerDocument?.pdfLocalPath));
  const isHtml = $derived(readerDocument?.contentKind === "html");
  const isAcquiringPdf = $derived(readerDocument?.pdfStatus === "acquiring");
  const fallbackSourceUrl = $derived(activeCandidate?.externalUrl ?? readerDocument?.pdfSourceUrl);
  const isFocusMode = $derived(layoutMode === "focus");
  const threadsCollapsed = $derived(isFocusMode && focusThreadsMode === "collapsed");
  const popoverHighlight = $derived(highlights.find((hl) => hl.id === popoverHighlightId) ?? null);
  const popoverThread = $derived(popoverHighlight ? findThreadForHighlight(threads, popoverHighlight) : undefined);

  onMount(() => {
    let unlistenSource: (() => void) | undefined;
    let unlistenExtraction: (() => void) | undefined;
    let unlistenChatThread: (() => void) | undefined;
    let unlistenMetadata: (() => void) | undefined;
    let unlistenAcquisition: (() => void) | undefined;

    // RFC 0051: background PDF acquisition progress for discovery opens.
    listen("reader_pdf_acquisition_progress", (event) => {
      const payload = event.payload as {
        sourceId?: string;
        paperId?: string;
        status?: string;
        message?: string;
        error?: string;
      };
      if (payload.paperId !== paper.id) {
        return;
      }
      readerLog("pdf-acquisition-progress", { ...payload });
      if (payload.status === "running") {
        acquisitionMessage = payload.message ?? "Fetching PDF…";
      } else if (payload.status === "ready") {
        refreshTick += 1;
      } else if (payload.status === "failed" && readerDocument) {
        readerDocument = {
          ...readerDocument,
          pdfStatus: undefined,
          pdfError: payload.error ?? payload.message,
        };
      }
    })
      .then((nextUnlisten) => {
        unlistenAcquisition = nextUnlisten;
      })
      .catch((error) => {
        console.error("Failed to listen for reader_pdf_acquisition_progress:", error);
      });

    // Applied/edited metadata must refresh the open Reader too, not just the
    // Vault rows (RFC 0049: update every visible surface).
    listen("paper_metadata_updated", (event) => {
      const payload = event.payload as { paperId?: string; paper_id?: string };
      if ((payload.paperId ?? payload.paper_id) === paper.id) {
        refreshTick += 1;
      }
    })
      .then((nextUnlisten) => {
        unlistenMetadata = nextUnlisten;
      })
      .catch((error) => {
        console.error("Failed to listen for paper_metadata_updated:", error);
      });

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

    // RFC 0059 Phase 2 (Task 8): the model string agent-created highlights
    // are attributed to. Best-effort — a settings load failure just leaves
    // the "agent" fallback.
    getSettings()
      .then((settings) => {
        const configured = settings.prefs["model.chat"]?.trim();
        if (configured) {
          chatModel = configured;
        }
      })
      .catch((error) => {
        console.error("Failed to load settings:", error);
      });

    return () => {
      unlistenSource?.();
      unlistenExtraction?.();
      unlistenChatThread?.();
      unlistenMetadata?.();
      unlistenAcquisition?.();
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

    const force = forceNextLoad;
    forceNextLoad = false;
    const loadDocument = activeCandidate
      ? getDiscoveryReaderDocument(activeCandidate, force)
      : getReaderDocument(paperId);

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
    popoverHighlightId = null;
    popoverPos = null;

    // A stale turn's affordance must not reappear over a different paper's
    // thread (Task 9 / final review fix).
    turnHighlightIds = [];
    showTurnAffordance = false;

    if (!chatEnabled) {
      threads = [];
      pins = [];
      highlights = [];
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
      const [threadList, pinList, highlightList] = await Promise.all([
        listChatThreads({ kind: "paper", paperId }),
        listPinnedChatEntries({ kind: "paper", paperId }),
        listHighlights(paperId),
      ]);
      if (paper.id === paperId && loadId === chatLoadSequence) {
        threads = threadList;
        pins = pinList;
        highlights = highlightList;
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

  // Maps the reader's live selection to the Locator the highlight bridge
  // expects — text-offset for HTML, page rect for PDF (RFC 0058 Phase 1).
  function locatorFromSelection(sel: ReaderTextSelection): Locator {
    if (sel.anchorKind === "pdf_rect") {
      return {
        kind: "pdfRect",
        sourceId: sel.sourceId,
        pageIndex: sel.pageIndex ?? 0,
        rectsJson: sel.rectsJson ?? "[]",
      };
    }
    return {
      kind: "textOffset",
      sourceId: sel.sourceId,
      startOffset: sel.startOffset,
      endOffset: sel.endOffset,
    };
  }

  // Guards createHighlight calls from both the swatch row and the
  // note/ask one-gesture path so rapid double-clicks can't create two
  // highlights for the same passage.
  let highlightActionInFlight = $state(false);

  // One-click highlight from a swatch: mark the current selection, make that
  // color sticky, refresh the marks, then clear the selection.
  async function pickColor(color: HighlightColor) {
    if (!selection || highlightActionInFlight) {
      return;
    }
    const sel = selection;
    const locator = locatorFromSelection(sel);
    // The passage may already be highlighted (e.g. reopened from the rail or
    // popover, or a race with ensureHighlightForSelection's one-gesture
    // path) — recolor the existing mark instead of creating a duplicate at
    // the same locator (RFC 0058 Task 10 / final review fix).
    const existing = hasExistingHighlight(highlights, locator)
      ? highlights.find((hl) => samePassage(hl.locator, locator))
      : undefined;
    highlightActionInFlight = true;
    try {
      if (existing) {
        await recolorHighlight(existing.id, color);
      } else {
        await createHighlight({
          paperId: paper.id,
          locator,
          excerpt: sel.selectedText,
          color,
        });
      }
      // Only sticks on success — a failed create shouldn't change the
      // default the user will get on their next attempt. Clear the
      // selection now (not after the reload below) so the swatch row
      // disappears immediately — otherwise a second swatch click during
      // the reload round-trip would still pass the `!selection` guard and
      // create a second highlight instead of a no-op.
      stickyColor = color;
      clearSelection();
    } catch (error) {
      readerLog("create-highlight-error", { error: errorDetail(error) }, "error");
    } finally {
      highlightActionInFlight = false;
    }
    await reloadChat();
  }

  // One-gesture Note/Ask: called by the inspector right before it turns a
  // fresh (virtual) selection thread into a real one, so the passage gets
  // marked with the sticky color alongside the thread (RFC 0058 Phase 1).
  // Idempotent: Task 10's "add note after the fact" path opens a virtual
  // thread for a passage that's *already* highlighted (clicked from the
  // popover or the rail), so a second createHighlight for the same locator
  // must be skipped rather than producing a duplicate mark.
  async function ensureHighlightForSelection() {
    if (!selection || highlightActionInFlight) {
      return;
    }
    const locator = locatorFromSelection(selection);
    if (hasExistingHighlight(highlights, locator)) {
      return;
    }
    highlightActionInFlight = true;
    try {
      await createHighlight({
        paperId: paper.id,
        locator,
        excerpt: selection.selectedText,
        color: stickyColor,
      });
    } catch (error) {
      readerLog("create-highlight-error", { error: errorDetail(error) }, "error");
    } finally {
      highlightActionInFlight = false;
    }
  }

  // A model-emitted `color` is normalized backend-side to one of the palette
  // colors already, but the wire type is a plain string — this just proves
  // that to the type system, falling back defensively if it somehow isn't.
  function asHighlightColor(color: string): HighlightColor {
    return (HIGHLIGHT_COLORS as string[]).includes(color) ? (color as HighlightColor) : "yellow";
  }

  // Resolves one streamed HighlightIntent to a Locator in the active reader
  // and creates the agent highlight (RFC 0059 Phase 2 / Task 8). Does not
  // reload marks itself — ReaderInspector reloads once after the whole ask
  // turn settles. Returns whether the quote was resolved and the highlight
  // created, so the caller can count unresolved passages.
  async function handleHighlightIntent(intent: HighlightIntent): Promise<boolean> {
    try {
      const locator = isHtml
        ? (htmlReaderRef?.resolveQuote(intent.quote) ?? null)
        : pdfPageRef
          ? await pdfPageRef.resolveQuote(intent.quote)
          : null;
      if (!locator) {
        return false;
      }
      const created = await createAgentHighlight({
        paperId: paper.id,
        locator,
        excerpt: intent.quote,
        color: asHighlightColor(intent.color),
        label: intent.label,
        model: chatModel,
      });
      // RFC 0059 Phase 2 (Task 9): collect this turn's created id for the
      // batch Keep/Undo affordance shown once the ask turn settles.
      turnHighlightIds = [...turnHighlightIds, created.id];
      return true;
    } catch (error) {
      // A resolve or create failure here must never surface as an ask
      // error — the ask itself may well have succeeded. Count it as
      // unresolved and move on (caller tallies this via the return value).
      readerLog("agent-highlight-intent-error", { error: errorDetail(error) }, "error");
      return false;
    }
  }

  // A new ask turn starts: dismiss any leftover affordance from a previous
  // turn and reset the id list the next handleHighlightIntent calls will
  // fill in (RFC 0059 Phase 2 / Task 9).
  function handleAskTurnStart() {
    turnHighlightIds = [];
    showTurnAffordance = false;
  }

  // The turn has settled — if the agent created at least one highlight,
  // offer the batch Keep/Undo affordance.
  function handleAskTurnComplete() {
    if (turnHighlightIds.length > 0) {
      showTurnAffordance = true;
    }
  }

  // The open thread closed (back to the list, or a different thread opened)
  // — any leftover affordance from the just-closed thread's turn must not
  // linger and reappear over whatever's opened next (Minor finding 1, final
  // review).
  function dismissTurnAffordance() {
    showTurnAffordance = false;
    turnHighlightIds = [];
  }

  // Keep: just dismiss the affordance, the marks stay.
  function keepTurnHighlights() {
    showTurnAffordance = false;
    turnHighlightIds = [];
  }

  // Undo all: remove every highlight this turn created, then reload so the
  // marks/rail/pins all drop it together.
  async function undoTurnHighlights() {
    const ids = turnHighlightIds;
    showTurnAffordance = false;
    turnHighlightIds = [];
    try {
      await Promise.all(ids.map((id) => removeHighlight(id)));
    } catch (error) {
      readerLog("undo-turn-highlights-error", { error: errorDetail(error) }, "error");
    }
    await reloadChat();
  }

  // Click-a-highlight popover (RFC 0058 Task 10).
  function openHighlightPopover(highlightId: string, x: number, y: number) {
    popoverHighlightId = highlightId;
    popoverPos = {
      x: Math.max(8, Math.min(x, window.innerWidth - 232)),
      y: Math.min(y, window.innerHeight - 170),
    };
  }

  function closeHighlightPopover() {
    popoverHighlightId = null;
    popoverPos = null;
  }

  async function recolorPopoverHighlight(color: HighlightColor) {
    if (!popoverHighlight) {
      return;
    }
    try {
      await recolorHighlight(popoverHighlight.id, color);
      await reloadChat();
    } catch (error) {
      readerLog("recolor-highlight-error", { error: errorDetail(error) }, "error");
    }
  }

  async function removePopoverHighlight() {
    if (!popoverHighlight) {
      return;
    }
    try {
      await removeHighlight(popoverHighlight.id);
      closeHighlightPopover();
      await reloadChat();
    } catch (error) {
      readerLog("remove-highlight-error", { error: errorDetail(error) }, "error");
    }
  }

  // Add note / Ask on a highlight after the fact: reuse its existing thread
  // if it has one, otherwise open a fresh note/ask composer on the same
  // locator via the normal selection flow (RFC 0058 Task 10).
  function openHighlightThread(highlight: Highlight) {
    const thread = findThreadForHighlight(threads, highlight);
    closeHighlightPopover();
    if (thread) {
      openThreadFromMark(thread.id);
      return;
    }
    const locator = highlight.locator;
    const sel: ReaderTextSelection =
      locator.kind === "pdfRect"
        ? {
            sourceId: locator.sourceId,
            startOffset: 0,
            endOffset: highlight.excerpt.length,
            selectedText: highlight.excerpt,
            anchorKind: "pdf_rect",
            pageIndex: locator.pageIndex,
            rectsJson: locator.rectsJson,
          }
        : {
            sourceId: locator.sourceId,
            startOffset: locator.startOffset,
            endOffset: locator.endOffset,
            selectedText: highlight.excerpt,
            anchorKind: "text_offset",
          };
    selectPassage(sel);
  }

  function openHighlightById(highlightId: string) {
    const highlight = highlights.find((hl) => hl.id === highlightId);
    if (highlight) {
      openHighlightThread(highlight);
    }
  }

  function retryDocumentLoad() {
    // Retry bypasses the acquisition negative cache (RFC 0051).
    forceNextLoad = true;
    refreshTick += 1;
  }

  function cancelPdfAcquisition() {
    const sourceId = readerDocument?.sourceId;
    if (!sourceId) {
      return;
    }
    cancelDiscoveryPdfAcquisition(sourceId, paper.id).catch((error) => {
      readerLog("cancel-acquisition-error", { sourceId, error: errorDetail(error) }, "error");
    });
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

  // Read the source URL in-app as extracted HTML (RFC 0056), rather than the
  // browser. Swaps the current view to the HTML document.
  async function readAsHtml() {
    if (!fallbackSourceUrl) {
      return;
    }
    try {
      readerDocument = await openHtmlDocument(fallbackSourceUrl);
    } catch (error) {
      readerLog("read-as-html-error", { url: fallbackSourceUrl, error: errorDetail(error) }, "error");
    }
  }

  function openThreadFromMark(threadId: string) {
    requestedThreadId = threadId;
    openThreadsPanel();
  }

  function consumeRequestedThread() {
    requestedThreadId = null;
  }

  function zoomIn() {
    pdfScale = Math.min(pdfScale + 0.15, 2.2);
  }

  function zoomOut() {
    pdfScale = Math.max(pdfScale - 0.15, 0.65);
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
          <ResizableSplit
            orientation="vertical"
            storageKey="i0i.reader-header-split"
            panes={[
              { id: "header", min: 64, max: 320, default: 116 },
              { id: "content", min: 240, default: 620 },
            ]}
          >
            {#snippet pane(id: string)}
              {#if id === "header"}
                <div class="header-pane">
                  <ReaderHeader
                    {document}
                    {layoutMode}
                    {threadsCollapsed}
                    threadCount={threads.length}
                    pinCount={pins.length}
                    zoomScale={hasCachedPdf ? pdfScale : undefined}
                    onZoomIn={zoomIn}
                    onZoomOut={zoomOut}
                    {onToggleFocus}
                    onToggleThreads={toggleThreadsPanel}
                  />
                </div>
              {:else}
                <div class="reader-content col">
                  <div class="reading-surface row">
                    {#if isHtml}
                      <HtmlReader
                        bind:this={htmlReaderRef}
                        sourceId={document.sourceId}
                        sourceUrl={readerDocument?.sourceUrl}
                        {highlights}
                        {chatEnabled}
                        onSelectPassage={selectPassage}
                        onHighlightClick={openHighlightPopover}
                      />
                    {:else if hasCachedPdf}
                      <PdfPage
                        bind:this={pdfPageRef}
                        pdfUrl={readerDocument!.pdfLocalPath!}
                        sourceId={document.sourceId}
                        {threads}
                        {highlights}
                        {selection}
                        {chatEnabled}
                        scale={pdfScale}
                        onSelectPassage={selectPassage}
                        onHighlightClick={openHighlightPopover}
                      />
                    {:else if isAcquiringPdf}
                      <div class="missing-pdf col">
                        <div class="label hot">Fetching PDF</div>
                        <div class="progress-shell" aria-hidden="true">
                          <div class="progress-bar"></div>
                        </div>
                        <p>{acquisitionMessage || "Fetching PDF…"}</p>
                        <div class="fallback-actions row">
                          {#if fallbackSourceUrl}
                            <button class="btn primary" type="button" onclick={() => void openSourceUrl()}>Open in Browser</button>
                          {/if}
                          <button class="btn" type="button" onclick={cancelPdfAcquisition}>Cancel</button>
                        </div>
                        {#if fallbackSourceUrl}
                          <div class="source-line mono-dim">{fallbackSourceUrl}</div>
                        {/if}
                      </div>
                    {:else}
                      <div class="missing-pdf col">
                        <div class="label hot">PDF could not be opened automatically</div>
                        <p>The publisher may require login, browser verification, or manual access.</p>
                        <div class="fallback-actions row">
                          {#if fallbackSourceUrl}
                            <button class="btn primary" type="button" onclick={() => void readAsHtml()}>Read as HTML</button>
                            <button class="btn" type="button" onclick={() => void openSourceUrl()}>Open Source</button>
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
                </div>
              {/if}
            {/snippet}
          </ResizableSplit>
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
          {highlights}
          {selection}
          {stickyColor}
          highlightBusy={highlightActionInFlight}
          {requestedThreadId}
          {isLoadingChat}
          {chatError}
          {metadataAutofillProgress}
          {isAutofillingMetadata}
          {onAutofillMetadata}
          onApplyMetadataCandidate={onApplyMetadataCandidate}
          {onUpdatePaperMetadata}
          onReloadChat={reloadChat}
          onClearSelection={clearSelection}
          onConsumeRequestedThread={consumeRequestedThread}
          onPickColor={pickColor}
          onEnsureHighlight={ensureHighlightForSelection}
          onOpenHighlight={openHighlightById}
          onHighlightIntent={handleHighlightIntent}
          onAskTurnStart={handleAskTurnStart}
          onAskTurnComplete={handleAskTurnComplete}
          onDismissTurnAffordance={dismissTurnAffordance}
          turnHighlightCount={turnHighlightIds.length}
          showTurnAffordance={showTurnAffordance}
          onKeepTurnHighlights={keepTurnHighlights}
          onUndoTurnHighlights={undoTurnHighlights}
        />
      {/if}
    {/snippet}

    {#if isFocusMode}
      {#if document && !threadsCollapsed}
        <ResizableSplit
          storageKey="i0i.reader-focus-split"
          panes={[
            { id: "reader", min: 420, default: 1180 },
            { id: "inspector", min: 240, default: 360 },
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
              { id: "reader", min: 360, default: 980 },
              { id: "inspector", min: 220, default: 340 },
            ]
          : [{ id: "reader", min: 360, default: 1200 }]}
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

  {#if popoverHighlight && popoverPos}
    <HighlightPopover
      highlight={popoverHighlight}
      hasThread={Boolean(popoverThread)}
      hasNote={Boolean(popoverThread && popoverThread.entryCount > 0)}
      x={popoverPos.x}
      y={popoverPos.y}
      onAddNote={() => openHighlightThread(popoverHighlight!)}
      onAsk={() => openHighlightThread(popoverHighlight!)}
      onRecolor={recolorPopoverHighlight}
      onRemove={removePopoverHighlight}
      onClose={closeHighlightPopover}
    />
  {/if}
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

  .header-pane {
    display: flex;
    flex: 1;
    min-width: 0;
    min-height: 0;
    overflow: hidden;
  }

  .reader-content {
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
