<script lang="ts">
  import { listen } from "@tauri-apps/api/event";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { onMount } from "svelte";
  import {
    cancelDiscoveryPdfAcquisition,
    extractPaperDocument,
    getDiscoveryReaderDocument,
    getReaderDocument,
    openHtmlDocument,
  } from "$lib/bridge/library";
  import {
    autoHighlight,
    debugLog,
    listChatThreads,
    listPinnedChatEntries,
    type AutoHighlightCategory,
    type HighlightIntent,
  } from "$lib/bridge/chat";
  import {
    createAgentHighlight,
    createHighlight,
    listHighlights,
    recolorHighlight,
    removeAnnotation,
    removeHighlight,
    setHighlightNote,
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
  import { citationRects, type ContextCitation } from "$lib/domain/context";
  import type { DiscoveryReaderCandidate, PdfRect, ReaderDocument, ReaderTextSelection } from "$lib/domain/reader";
  import ReaderFooter from "$lib/features/reader/ReaderFooter.svelte";
  import ReaderHeader from "$lib/features/reader/ReaderHeader.svelte";
  import ReaderInspector from "$lib/features/reader/ReaderInspector.svelte";
  import PdfPage from "$lib/features/reader/PdfPage.svelte";
  import HtmlReader from "$lib/features/reader/HtmlReader.svelte";
  import ReaderToolbar from "$lib/features/reader/ReaderToolbar.svelte";
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
  // Declared before its first use, which is `loadActiveColor()` in the
  // `stickyColor` initializer below. A `const` is hoisted but stays in the
  // temporal dead zone until its own declaration runs, so moving this back down
  // makes every ReaderView mount throw "Cannot access 'TOOL_COLOR_KEY' before
  // initialization" — and a reader that throws on init renders nothing at all.
  const TOOL_COLOR_KEY = "i0i.reader-active-color";

  // RFC 0058 Phase 1 (Task 9): the last color picked (swatch or note/ask) is
  // the sticky default for the next one-click highlight. RFC 0074 promotes it
  // to the *active color* shared by the tools, the swatch row, and the popover.
  let stickyColor = $state<HighlightColor>(loadActiveColor());
  // RFC 0074: the active annotation tool. `null` = the reader behaves as it
  // always has (a selection raises the Note/Ask/Highlight popover).
  let activeTool = $state<"highlight" | "note" | null>(null);

  function loadActiveColor(): HighlightColor {
    if (typeof localStorage === "undefined") {
      return "yellow";
    }
    const stored = localStorage.getItem(TOOL_COLOR_KEY);
    return (HIGHLIGHT_COLORS as string[]).includes(stored ?? "")
      ? (stored as HighlightColor)
      : "yellow";
  }

  // The tool is deliberately NOT persisted: a mode restored on open is a mode
  // you did not choose. The color is, since it is a preference (RFC 0074).
  function setActiveColor(color: HighlightColor) {
    stickyColor = color;
    if (typeof localStorage !== "undefined") {
      localStorage.setItem(TOOL_COLOR_KEY, color);
    }
  }

  function setActiveTool(tool: "highlight" | "note" | null) {
    activeTool = activeTool === tool ? null : tool;
  }

  // Escape leaves the mode. A tool you cannot see is a tool you cannot leave.
  //
  // RFC 0079 R4.4: and then it leaves focus mode, which had exactly one exit —
  // a button in a toolbar that focus mode is meant to get out of the way. The
  // tool wins when both are active: Escape backs out one layer at a time.
  function handleToolKeydown(event: KeyboardEvent) {
    if (event.key !== "Escape") {
      return;
    }
    if (activeTool) {
      activeTool = null;
      return;
    }
    if (isFocusMode) {
      onToggleFocus();
    }
  }

  // RFC 0074: the Highlight tool marks the selection outright, then clears it —
  // `updateSelectionAffordance` runs on every window mouseup, so a live
  // selection would be re-marked on the next click anywhere in the document.
  async function highlightFromTool(passage: ReaderTextSelection) {
    selection = passage;
    await pickColor(stickyColor);
  }

  // RFC 0074: place a standalone sticky note at a point on a PDF page, then open
  // its editor so it can be typed into immediately.
  async function placeNote(
    pageIndex: number,
    x: number,
    y: number,
    clientX: number,
    clientY: number,
  ) {
    if (!document || highlightActionInFlight) {
      return;
    }
    const locator: Locator = {
      kind: "pdfPoint",
      sourceId: document.sourceId,
      pageIndex,
      x,
      y,
    };
    highlightActionInFlight = true;
    try {
      const created = await createHighlight({
        paperId: paper.id,
        locator,
        // A sticky is anchored to a spot, not to a passage — there is nothing to
        // quote, and `excerpt` is non-null in storage.
        excerpt: "",
        color: stickyColor,
      });
      // Seed the new sticky locally before opening its editor: the popover looks
      // its target up in `highlights`, and a reload that gets superseded (paper
      // switch, concurrent refresh) would leave the id pointing at nothing.
      if (!highlights.some((hl) => hl.id === created.id)) {
        highlights = [...highlights, created];
      }
      openHighlightPopover(created.id, clientX, clientY);
      // Eligible for cleanup until it is typed into (R2.2).
      pendingStickyId = created.id;
      await reloadChat();
    } catch (error) {
      readerLog("place-note-error", { error: errorDetail(error) }, "error");
    } finally {
      highlightActionInFlight = false;
    }
  }

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
  // RFC 0072: papers this reader session has already nudged into extraction.
  // `document_extraction_updated` bumps `refreshTick`, so an unguarded retry
  // would loop forever on an extraction that keeps failing.
  const extractionRequested = new Set<string>();
  // RFC 0051: background PDF acquisition status for discovery opens.
  let acquisitionMessage = $state("");
  let forceNextLoad = false;
  // RFC 0079 R4.1: focus mode starts with the paper and nothing else. It used
  // to hide the app chrome and then open the rail on top of the page, which
  // traded one panel for another rather than clearing the desk.
  let focusThreadsMode = $state<"open" | "collapsed">("collapsed");
  let pdfScale = $state(1.15);
  // RFC 0059 Phase 2 (Task 8): refs to the active reader so intents can be
  // resolved wherever the content actually lives — the HTML reader resolves
  // synchronously against its rendered text; the PDF reader resolves
  // asynchronously against per-page text-content items.
  let htmlReaderRef = $state<HtmlReader | undefined>();
  let pdfPageRef = $state<PdfPage | undefined>();
  // RFC 0077: the passage a citation points at, painted briefly so the eye can
  // find it after the scroll. Transient by design — a persistent highlight
  // would be indistinguishable from one the user made.
  const CITATION_FLASH_MS = 2600;
  let citationFlash = $state<{ pageIndex: number; rects: PdfRect[] } | null>(null);
  let citationFlashTimer: ReturnType<typeof setTimeout> | undefined;
  // The chat model string to attribute agent-created highlights to, resolved
  // once from settings; falls back to a generic label if unset.
  let chatModel = $state("agent");
  // RFC 0059 Phase 2 (Task 9): ids of highlights the agent created during the
  // ask turn currently in flight (or just settled) — reset at the start of
  // each turn, accumulated as each HighlightIntent resolves to a created
  // Highlight. Drives the batch Keep/Undo affordance once the turn completes.
  let turnHighlightIds = $state<string[]>([]);
  let showTurnAffordance = $state(false);
  // RFC 0064: explicit AI auto-highlight (toolbar command). `aiBusy` drives the
  // toolbar "Marking…" state; the created highlights reuse the per-turn id list
  // and the Keep/Undo affordance, surfaced here in a bar under the toolbar.
  let aiBusy = $state(false);
  let aiMarkedCount = $state(0);
  let aiUnresolvedCount = $state(0);
  let aiError = $state("");
  // RFC 0072: a run that completed but marked nothing. Distinct from `aiError`
  // (the run failed) and from `aiUnresolvedCount` (passages came back but
  // couldn't be located in the rendered document) — without it, a zero-passage
  // run leaves every flag false and the bar silently unmounts.
  let aiNotice = $state("");
  // RFC 0063: in-document search state (HTML reader in v1). ReaderView owns it;
  // the toolbar is presentational and the HtmlReader does the painting.
  let searchQuery = $state("");
  let matchCount = $state(0);
  let activeMatch = $state(0);

  // RFC 0071: the inspector can collapse to a full-width reading surface. The
  // choice is remembered so reopening a paper keeps it.
  const INSPECTOR_COLLAPSED_KEY = "i0i.reader-inspector-collapsed";
  let inspectorCollapsed = $state(loadInspectorCollapsed());

  function loadInspectorCollapsed(): boolean {
    return typeof localStorage !== "undefined" && localStorage.getItem(INSPECTOR_COLLAPSED_KEY) === "1";
  }

  function toggleInspector() {
    inspectorCollapsed = !inspectorCollapsed;
    if (typeof localStorage !== "undefined") {
      localStorage.setItem(INSPECTOR_COLLAPSED_KEY, inspectorCollapsed ? "1" : "0");
    }
  }

  // RFC 0071: the toolbar's Note / Chat buttons ask the inspector to show a
  // section, revealing the panel first if collapsed; ReaderInspector consumes
  // the request. Highlight acts directly on the current selection.
  let requestedSection = $state<"info" | "notes" | "chat" | null>(null);

  function revealSection(section: "notes" | "chat") {
    requestedSection = section;
    openThreadsPanel();
  }

  function consumeRequestedSection() {
    requestedSection = null;
  }

  function highlightSelection() {
    void pickColor(stickyColor);
  }

  const document = $derived<ReaderDocument | null>(readerDocument);
  const chatEnabled = $derived(isPaperInLibrary(paper.id));
  const activeCandidate = $derived(candidate && !chatEnabled ? candidate : undefined);
  const hasCachedPdf = $derived(Boolean(readerDocument?.pdfLocalPath));
  const isHtml = $derived(readerDocument?.contentKind === "html");
  const isAcquiringPdf = $derived(readerDocument?.pdfStatus === "acquiring");
  const fallbackSourceUrl = $derived(activeCandidate?.externalUrl ?? readerDocument?.pdfSourceUrl);
  // RFC 0066 (R6a): "Read as HTML" fetches a web page, so it only applies to a
  // real http(s) source — never a local PDF's `local://sha256/…` pseudo-URL,
  // which `open_html_document` can't fetch.
  const isWebUrl = (url: string | undefined): boolean =>
    typeof url === "string" && /^https?:\/\//i.test(url);
  const canReadAsHtml = $derived(isWebUrl(fallbackSourceUrl) && !isHtml);
  const isFocusMode = $derived(layoutMode === "focus");
  const threadsCollapsed = $derived(isFocusMode && focusThreadsMode === "collapsed");

  // RFC 0079 R4.2: the focus-mode toolbar is revealed by pointing at the top of
  // the reader, and hides again when the pointer leaves. Never while a tool is
  // armed — a mode you cannot see is a mode you cannot leave, and the toolbar is
  // where you see it.
  let toolbarHovered = $state(false);
  const toolbarRevealed = $derived(toolbarHovered || activeTool !== null);

  // RFC 0079 R4.1/R4.3: every *entry* into focus mode starts collapsed. Opening
  // the rail while in focus keeps it open for as long as you stay — leaving and
  // coming back is a fresh request for the paper alone.
  let wasFocusMode = false;
  $effect(() => {
    if (isFocusMode && !wasFocusMode) {
      focusThreadsMode = "collapsed";
    }
    wasFocusMode = isFocusMode;
  });
  // RFC 0067 (R2): highlight ids that have a conversation, so an ask-only
  // passage (no color, no note) still draws the neutral marker on the page.
  const conversationIds = $derived(
    new Set(
      threads
        .filter((thread) => thread.anchor.kind !== "document" && thread.entryCount > 0)
        .map((thread) => highlights.find((hl) => samePassage(thread.anchor, hl.locator))?.id)
        .filter((id): id is string => Boolean(id)),
    ),
  );
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
      const payload = event.payload as { paperId?: string; paper_id?: string; status?: string };
      // RFC 0072: only a *ready* extraction changes what the reader can show.
      // The manager also emits on "extracting" (start), and a refresh there
      // nulls `readerDocument` mid-read — unmounting PdfPage and re-fetching
      // the whole PDF. Harmless while extraction only ran at startup; not once
      // opening a paper can trigger it.
      if ((payload.paperId ?? payload.paper_id) === paper.id && payload.status === "ready") {
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
          requestExtractionIfMissing(doc, paperId);
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

  // A cached PDF with no *ready* extraction has an empty `source_text`, so chat
  // and AI marking silently do nothing (RFC 0072). Queue the extraction the
  // import path missed; the `document_extraction_updated` listener above
  // refreshes the reader when the text lands.
  //
  // The trigger is `!doc.extractionId`, which means "no READY extraction" —
  // `get_saved_reader_document` only accepts `status == "ready"`, so rows stuck
  // in `failed`/`extracting` also yield a null id and also need re-queueing. Do
  // not tighten this into a check for whether an extraction row exists.
  function requestExtractionIfMissing(doc: ReaderDocument, paperId: string) {
    if (activeCandidate || doc.contentKind !== "pdf" || doc.extractionId || !doc.pdfLocalPath) {
      return;
    }
    if (extractionRequested.has(paperId)) {
      return;
    }
    extractionRequested.add(paperId);
    readerLog("extract-request", { paperId, sourceId: doc.sourceId });
    void extractPaperDocument(paperId).catch((error) => {
      readerLog("extract-request-error", { paperId, error: errorDetail(error) }, "error");
    });
  }

  // Load the paper's threads + pins when it changes (once it's chat-enabled).
  $effect(() => {
    const paperId = paper.id;
    selection = null;
    requestedThreadId = null;
    chatError = "";
    popoverHighlightId = null;
    popoverPos = null;
    // RFC 0063: a new paper starts with a clean search. (Any leftover painted
    // ranges from the previous document point at removed nodes and are ignored
    // by the browser; we deliberately don't read `htmlReaderRef` here — that
    // would subscribe this effect to the ref and trigger a redundant reload on
    // every reader mount.)
    searchQuery = "";
    matchCount = 0;
    activeMatch = 0;

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

  // Lightweight reload of just the highlight marks (no threads/pins), so an
  // agent mark can pop in the instant it is created — background marking
  // reloads per-mark instead of one batch at the end (RFC 0059 progress UX).
  async function reloadHighlightsOnly(paperId: string = paper.id) {
    if (!chatEnabled) {
      return;
    }
    try {
      const next = await listHighlights(paperId);
      if (paper.id === paperId) {
        highlights = next;
      }
    } catch {
      // Best-effort repaint; the next full reloadChat will reconcile.
    }
  }

  // RFC 0073: `intent` is what the user actually clicked — the PDF popover's
  // Note vs. Ask. Without one (the HTML reader, which has no popover) the rail
  // keeps its sticky section, which is the pre-existing behavior.
  function selectPassage(next: ReaderTextSelection, intent?: "notes" | "chat") {
    selection = next;
    if (intent) {
      revealSection(intent);
    }
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

  // One-gesture Note/Ask: called by the inspector to guarantee the current
  // selection has an annotated-passage row (a `highlights` row) it can attach a
  // note or conversation to. Per RFC 0061 the passage is created **color-less**
  // — note/ask no longer force a colored mark (a note renders a neutral marker,
  // a conversation-only passage renders nothing). Returns the passage's id so
  // the note path can `set_highlight_note` on it.
  // Idempotent: if the passage is already highlighted (reopened from the popover
  // or rail), returns the existing id rather than creating a duplicate.
  async function ensureHighlightForSelection(): Promise<string | null> {
    if (!selection || highlightActionInFlight) {
      return null;
    }
    const locator = locatorFromSelection(selection);
    const existing = highlights.find((hl) => samePassage(hl.locator, locator));
    if (existing) {
      return existing.id;
    }
    highlightActionInFlight = true;
    try {
      const created = await createHighlight({
        paperId: paper.id,
        locator,
        excerpt: selection.selectedText,
        color: null,
      });
      return created.id;
    } catch (error) {
      readerLog("create-highlight-error", { error: errorDetail(error) }, "error");
      // RFC 0072: `null` now means only "there was nothing to mark". A genuine
      // backend failure carries its message to the caller, which decides
      // whether it is fatal — swallowing it here is what turned a NOT NULL
      // constraint error into an unexplained "Couldn't attach the note".
      throw error;
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
      void debugLog(
        `intent isHtml=${isHtml} hasHtmlRef=${!!htmlReaderRef} hasPdfRef=${!!pdfPageRef} quote_len=${intent.quote.length} quote="${intent.quote.slice(0, 60)}"`,
      );
      const locator = isHtml
        ? (htmlReaderRef?.resolveQuote(intent.quote) ?? null)
        : pdfPageRef
          ? await pdfPageRef.resolveQuote(intent.quote)
          : null;
      if (!locator) {
        void debugLog(`resolve FAILED: quote not located in the rendered document`, "warn");
        return false;
      }
      const where =
        locator.kind === "pdfRect"
          ? `pdfRect page=${locator.pageIndex} rects=${JSON.parse(locator.rectsJson || "[]").length}`
          : locator.kind === "textOffset"
            ? `textOffset [${locator.startOffset}, ${locator.endOffset}]`
            : locator.kind;
      void debugLog(`resolve OK -> ${where}`);
      const created = await createAgentHighlight({
        paperId: paper.id,
        locator,
        excerpt: intent.quote,
        color: asHighlightColor(intent.color),
        label: intent.label,
        model: chatModel,
      });
      void debugLog(`created highlight id=${created.id}`);
      // RFC 0059 Phase 2 (Task 9): collect this turn's created id for the
      // batch Keep/Undo affordance shown once the ask turn settles.
      turnHighlightIds = [...turnHighlightIds, created.id];
      // Repaint immediately so the mark appears the moment it lands, rather
      // than after the whole batch settles.
      void reloadHighlightsOnly();
      return true;
    } catch (error) {
      // A resolve or create failure here must never surface as an ask
      // error — the ask itself may well have succeeded. Count it as
      // unresolved and move on (caller tallies this via the return value).
      readerLog("agent-highlight-intent-error", { error: errorDetail(error) }, "error");
      void debugLog(`intent ERROR ${String(error).slice(0, 200)}`);
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

  // RFC 0064: run the toolbar auto-highlight command. Fetches one structured
  // list of passages, resolves + creates each via the shipped intent path, and
  // reuses the per-turn Keep/Undo affordance. No conversation, no prose.
  async function runAutoHighlight(categories: AutoHighlightCategory[]) {
    if (aiBusy || !chatEnabled) {
      return;
    }
    aiBusy = true;
    aiError = "";
    aiNotice = "";
    aiMarkedCount = 0;
    aiUnresolvedCount = 0;
    handleAskTurnStart();
    try {
      const intents = await autoHighlight({ kind: "paper", paperId: paper.id }, categories);
      if (intents.length === 0) {
        aiNotice = "The AI found no passages to mark in this document.";
      }
      for (const intent of intents) {
        const resolved = await handleHighlightIntent(intent);
        if (resolved) {
          aiMarkedCount += 1;
        } else {
          aiUnresolvedCount += 1;
        }
      }
    } catch (error) {
      aiError = String(error);
      readerLog("auto-highlight-error", { error: errorDetail(error) }, "error");
    } finally {
      aiBusy = false;
      handleAskTurnComplete();
    }
  }

  // Dismiss the AI bar keeping any marks (also clears the error / unresolved
  // notice so the bar closes).
  function keepAi() {
    aiError = "";
    aiNotice = "";
    aiUnresolvedCount = 0;
    keepTurnHighlights();
  }

  // Undo every mark this auto-highlight run created, then close the bar.
  async function undoAi() {
    aiError = "";
    aiNotice = "";
    aiUnresolvedCount = 0;
    await undoTurnHighlights();
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

  // RFC 0079 R2.2: a sticky placed and then dismissed without a word is a
  // misfire, not a record — clean it up rather than leaving an empty glyph on
  // the page and an empty row in the list. Only the sticky *this* placement
  // created is eligible: an existing one you opened and closed stays put.
  let pendingStickyId: string | null = null;

  function closeHighlightPopover() {
    const abandoned = pendingStickyId;
    const target = abandoned ? highlights.find((hl) => hl.id === abandoned) : null;
    pendingStickyId = null;
    popoverHighlightId = null;
    popoverPos = null;
    if (target && !target.note?.trim()) {
      void discardEmptySticky(target.id);
    }
  }

  async function discardEmptySticky(id: string) {
    try {
      await removeHighlight(id);
      await reloadChat();
    } catch (error) {
      readerLog("discard-empty-sticky-error", { error: errorDetail(error) }, "error");
    }
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
    closeHighlightPopover();
    await deleteAnnotation(popoverHighlight.id);
  }

  // RFC 0079 R1.3/R1.4: deleting a passage takes its note and its conversation
  // with it — the old `removeHighlight` left the thread alive with no row
  // rendering it, which is unreachable rather than deleted.
  //
  // The confirm is asymmetric on purpose: a colored mark is one gesture to
  // redo, so it goes silently; a note or a conversation is work, so it asks
  // once. Confirming everything teaches you to confirm without reading.
  async function deleteAnnotation(id: string) {
    const target = highlights.find((hl) => hl.id === id);
    if (!target) {
      return;
    }
    const hasNote = Boolean(target.note?.trim());
    const hasThread = Boolean(findThreadForHighlight(threads, target));
    if (hasNote || hasThread) {
      const what = hasThread && hasNote ? "note and conversation" : hasThread ? "conversation" : "note";
      const excerpt = target.excerpt.trim().slice(0, 60);
      const subject = excerpt ? `“${excerpt}${target.excerpt.trim().length > 60 ? "…" : ""}”` : "this passage";
      if (!window.confirm(`Delete ${subject} and its ${what}? This cannot be undone.`)) {
        return;
      }
    }
    try {
      await removeAnnotation(id);
      await reloadChat();
    } catch (error) {
      readerLog("remove-annotation-error", { error: errorDetail(error) }, "error");
      chatError = String(error);
    }
  }

  // Save/clear a passage's note from the popover's inline note field (RFC
  // 0061). An empty note clears it; the reload repaints the neutral note
  // marker (or removes it, if the passage now has neither color nor note).
  async function saveNoteForPopoverHighlight(note: string | null) {
    if (!popoverHighlight) {
      return;
    }
    const trimmed = note?.trim() ?? "";
    try {
      await setHighlightNote(popoverHighlight.id, trimmed.length ? trimmed : null);
      // Typed into, so no longer a misfire (R2.2).
      if (trimmed.length && pendingStickyId === popoverHighlight.id) {
        pendingStickyId = null;
      }
      await reloadChat();
    } catch (error) {
      readerLog("set-highlight-note-error", { error: errorDetail(error) }, "error");
    }
  }

  // Add note / Ask on a highlight after the fact: reuse its existing thread
  // if it has one, otherwise open a fresh note/ask composer on the same
  // locator via the normal selection flow (RFC 0058 Task 10).
  // RFC 0073: `intent` is passed only by the mark popover's Ask/Open-thread
  // action. The Marks list uses the same entry point and deliberately passes
  // nothing — clicking a row there must stay in Notes, not jump to Chat.
  function openHighlightThread(highlight: Highlight, intent?: "notes" | "chat") {
    const thread = findThreadForHighlight(threads, highlight);
    closeHighlightPopover();
    if (intent) {
      revealSection(intent);
    }
    scrollToHighlight(highlight);
    if (thread) {
      openThreadFromMark(thread.id);
      return;
    }
    const locator = highlight.locator;
    // RFC 0074: a sticky note anchors to a point, so there is no passage to
    // select — its editor is the popover, which the caller already opened.
    if (locator.kind === "pdfPoint" || locator.kind === "textPoint") {
      return;
    }
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

  // RFC 0073 (R2.2): opening a mark from the Marks list moves the reader to it.
  // Deliberately not `scrollIntoView` — that scrolls every scrollable ancestor,
  // including the inspector panel the list now lives in.
  function scrollToHighlight(highlight: Highlight) {
    const locator = highlight.locator;
    if (locator.kind === "pdfRect" || locator.kind === "pdfPoint") {
      pdfPageRef?.scrollToPage(locator.pageIndex);
      return;
    }
    if (locator.kind === "textOffset") {
      htmlReaderRef?.focusOffsets(locator.startOffset, locator.endOffset);
      return;
    }
    htmlReaderRef?.focusOffsets(locator.offset, locator.offset);
  }

  // RFC 0077: a `[C1]` marker in an answer, clicked. The chunk resolves to
  // whole blocks, so the jump lands on the paragraph rather than the sentence.
  // Rectangles can legitimately be empty (blocks written before RFC 0075 carry
  // no geometry, and a re-resolved passage may lose it) — the page jump still
  // works, which is why the flash is separate from the scroll.
  function openCitation(citation: ContextCitation) {
    pdfPageRef?.scrollToPage(citation.pageStart);

    const pages = citationRects(citation);
    const page = pages.find((entry) => entry.pageIndex === citation.pageStart) ?? pages[0];
    if (!page || page.rects.length === 0) {
      citationFlash = null;
      return;
    }
    citationFlash = { pageIndex: page.pageIndex, rects: page.rects };
    clearTimeout(citationFlashTimer);
    citationFlashTimer = setTimeout(() => (citationFlash = null), CITATION_FLASH_MS);
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
    // Defense in depth (R6a): only a real web page can be fetched as HTML.
    if (!isWebUrl(fallbackSourceUrl)) {
      return;
    }
    try {
      readerDocument = await openHtmlDocument(fallbackSourceUrl!);
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

  // In-document search (RFC 0063). v1 targets the HTML reader; the toolbar
  // disables the box for PDF.
  function runSearch(query: string) {
    searchQuery = query;
    if (!htmlReaderRef) {
      return;
    }
    // Require 2+ chars: a single-character query matches thousands of ranges on
    // a real article, one rebuild per keystroke.
    if (query.trim().length < 2) {
      htmlReaderRef.clearSearch();
      matchCount = 0;
      activeMatch = 0;
      return;
    }
    matchCount = htmlReaderRef.search(query);
    activeMatch = 0;
    if (matchCount > 0) {
      htmlReaderRef.focusMatch(0);
    }
  }

  function nextMatch() {
    if (!htmlReaderRef || matchCount === 0) {
      return;
    }
    activeMatch = (activeMatch + 1) % matchCount;
    htmlReaderRef.focusMatch(activeMatch);
  }

  function prevMatch() {
    if (!htmlReaderRef || matchCount === 0) {
      return;
    }
    activeMatch = (activeMatch - 1 + matchCount) % matchCount;
    htmlReaderRef.focusMatch(activeMatch);
  }

  function clearReaderSearch() {
    searchQuery = "";
    matchCount = 0;
    activeMatch = 0;
    htmlReaderRef?.clearSearch();
  }

  function toggleThreadsPanel() {
    focusThreadsMode = focusThreadsMode === "open" ? "collapsed" : "open";
  }

  function openThreadsPanel() {
    if (isFocusMode) {
      focusThreadsMode = "open";
    } else if (inspectorCollapsed) {
      // Annotating or asking reveals the panel for this session, but must not
      // overwrite the saved collapse preference — only the toolbar toggle does
      // that (RFC 0071). Otherwise an incidental text selection would erase it.
      inspectorCollapsed = false;
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

<svelte:window onkeydown={handleToolKeydown} />

<section class="reader-workspace col">
  <!-- RFC 0071: one tool panel. In normal mode it spans the full width above the
       split (so the collapse toggle stays put); in focus mode it lives inside the
       reader pane. Defined once here, rendered in exactly one place per mode. -->
  {#snippet toolbarStrip()}
    {#if document}
      <div class="toolbar-strip col">
        <ReaderToolbar
          contentKind={document.contentKind}
          zoomScale={hasCachedPdf ? pdfScale : undefined}
          onZoomIn={zoomIn}
          onZoomOut={zoomOut}
          {canReadAsHtml}
          onReadAsHtml={() => void readAsHtml()}
          searchEnabled={isHtml}
          {searchQuery}
          {matchCount}
          {activeMatch}
          onSearch={runSearch}
          onNextMatch={nextMatch}
          onPrevMatch={prevMatch}
          onClearSearch={clearReaderSearch}
          aiEnabled={chatEnabled}
          {aiBusy}
          onAutoHighlight={runAutoHighlight}
          hasSelection={selection !== null}
          onHighlight={chatEnabled ? highlightSelection : undefined}
          toolsEnabled={chatEnabled && !isHtml}
          {activeTool}
          activeColor={stickyColor}
          onSelectTool={setActiveTool}
          onSelectColor={setActiveColor}
          onNote={chatEnabled ? () => revealSection("notes") : undefined}
          onChat={chatEnabled ? () => revealSection("chat") : undefined}
          {isFocusMode}
          {onToggleFocus}
          {inspectorCollapsed}
          onToggleInspector={isFocusMode ? undefined : toggleInspector}
        />
        {#if aiBusy || showTurnAffordance || aiError || aiNotice || aiUnresolvedCount > 0}
          <div class="ai-bar row hair-b">
            {#if aiBusy}
              <span class="ai-dot"></span>
              <span>Marking…{aiMarkedCount > 0 ? ` ${aiMarkedCount} added` : ""}</span>
            {:else if aiError}
              <span class="ai-error">{aiError}</span>
              <div class="flex1"></div>
              <button class="ai-link" type="button" onclick={keepAi}>Dismiss</button>
            {:else if turnHighlightIds.length > 0}
              <span>
                AI added {turnHighlightIds.length} highlight{turnHighlightIds.length === 1 ? "" : "s"}{aiUnresolvedCount > 0 ? ` · ${aiUnresolvedCount} not found` : ""}
              </span>
              <div class="flex1"></div>
              <button class="ai-link" type="button" onclick={keepAi}>Keep</button>
              <span class="mono-dim">·</span>
              <button class="ai-link" type="button" onclick={() => void undoAi()}>Undo all</button>
            {:else if aiNotice}
              <!-- Must precede the unresolved-count branch: a run that returned
                   zero passages has aiUnresolvedCount === 0 and would otherwise
                   render "Couldn't locate 0 passages" (RFC 0072). -->
              <span>{aiNotice}</span>
              <div class="flex1"></div>
              <button class="ai-link" type="button" onclick={keepAi}>Dismiss</button>
            {:else}
              <span class="ai-error">Couldn't locate {aiUnresolvedCount} passage{aiUnresolvedCount === 1 ? "" : "s"} in this document.</span>
              <div class="flex1"></div>
              <button class="ai-link" type="button" onclick={keepAi}>Dismiss</button>
            {/if}
          </div>
        {/if}
      </div>
    {/if}
  {/snippet}

  {#if !isFocusMode}
    {@render toolbarStrip()}
  {/if}

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
              // RFC 0072: min/default must fit the meta row, TWO clamped title
              // lines, and (in focus mode) the action row — the old 64/116 left
              // ~39px for a 26px line box, so a wrapped title was cut mid-glyph.
              // Raising `min` also lifts already-persisted panes back above the
              // floor, via ResizableSplit's clampSizes.
              { id: "header", min: 132, max: 320, default: 132 },
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
                    onToggleThreads={toggleThreadsPanel}
                  />
                </div>
              {:else}
                <div class="reader-content col">
                  <!-- RFC 0079 R4.2: in focus mode the toolbar gets out of the
                       way and comes back when you reach for it. It occupies no
                       layout height while hidden, so the page really does get
                       the room; the hover strip above it is the reveal. -->
                  {#if isFocusMode}
                    <div
                      class="focus-toolbar-zone"
                      class:revealed={toolbarRevealed}
                      role="presentation"
                      onpointerenter={() => (toolbarHovered = true)}
                      onpointerleave={() => (toolbarHovered = false)}
                    >
                      {@render toolbarStrip()}
                    </div>
                  {/if}
                  <div class="reading-surface row">
                    {#if isHtml}
                      <HtmlReader
                        bind:this={htmlReaderRef}
                        sourceId={document.sourceId}
                        sourceUrl={readerDocument?.sourceUrl}
                        {highlights}
                        {conversationIds}
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
                        {conversationIds}
                        {selection}
                        {chatEnabled}
                        scale={pdfScale}
                        {activeTool}
                        {citationFlash}
                        onSelectPassage={selectPassage}
                        onHighlightClick={openHighlightPopover}
                        onToolHighlight={(passage) => void highlightFromTool(passage)}
                        onPlaceNote={(pageIndex, x, y, clientX, clientY) =>
                          void placeNote(pageIndex, x, y, clientX, clientY)}
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
          {requestedSection}
          onConsumeRequestedSection={consumeRequestedSection}
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
          onRemoveHighlight={deleteAnnotation}
          onOpenCitation={openCitation}
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
        panes={document && !inspectorCollapsed
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
      x={popoverPos.x}
      y={popoverPos.y}
      onSaveNote={saveNoteForPopoverHighlight}
      onAsk={() => openHighlightThread(popoverHighlight!, "chat")}
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

  .toolbar-strip {
    flex-shrink: 0;
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

  /* RFC 0079 R4.2: a thin always-live hover strip, with the toolbar itself
     collapsed above the fold until the pointer arrives. Height rather than
     visibility, so the paper actually gains the space. */
  .focus-toolbar-zone {
    flex-shrink: 0;
    max-height: 6px;
    overflow: hidden;
    transition: max-height 120ms ease-out;
  }

  .focus-toolbar-zone.revealed {
    max-height: 200px;
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

  /* RFC 0064: AI auto-highlight progress / Keep-Undo bar. */
  .ai-bar {
    flex-shrink: 0;
    align-items: center;
    gap: 8px;
    height: 30px;
    padding: 0 14px;
    background: rgba(242, 169, 59, 0.06);
    color: var(--fg-2);
    font-size: 11px;
  }

  .ai-dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--amber, #f2a93b);
    animation: ai-pulse 1s ease-in-out infinite;
  }

  @keyframes ai-pulse {
    0%,
    100% {
      opacity: 0.35;
    }
    50% {
      opacity: 1;
    }
  }

  .ai-error {
    color: var(--red);
  }

  .ai-link {
    border: 0;
    background: transparent;
    color: var(--amber);
    font: inherit;
    font-size: 11px;
    cursor: pointer;
    padding: 0;
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
