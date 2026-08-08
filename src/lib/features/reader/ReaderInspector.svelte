<script lang="ts">
  import { Pencil, Trash2, Sparkles, MessageSquare, StickyNote, Star, SlidersHorizontal, Info, ChevronDown, ChevronRight } from "@lucide/svelte";
  import { onMount } from "svelte";
  import { listen } from "@tauri-apps/api/event";
  import CitedAnswer from "$lib/features/reader/CitedAnswer.svelte";
  import { citationLabel } from "$lib/features/reader/cited-answer";
  import {
    addChatContext,
    compactChatContext,
    deleteChatContext,
    listChatContext,
  } from "$lib/bridge/context";
  import type { ContextCitation, ContextItemView, PassageRef } from "$lib/domain/context";
  import {
    askAtAnchorStreamed,
    askChatThreadStreamed,
    deleteChatThread,
    getChatThread,
    renameChatThread,
    setChatEntryPinned,
    type HighlightIntent,
  } from "$lib/bridge/chat";
  import { setHighlightNote } from "$lib/bridge/highlight";
  import {
    anchorSelectedText,
    type ChatEntry,
    type ChatScope,
    type ChatThreadSummary,
    type ChatThreadView,
    type PinnedHighlight,
    type ThreadAnchor,
  } from "$lib/domain/chat";
  import { HIGHLIGHT_COLORS, isStickyNote, type Highlight, type HighlightColor } from "$lib/domain/highlight";
  import type {
    MetadataAutofillProgress,
    MetadataCandidate,
    PaperMetadataUpdate,
  } from "$lib/domain/library";
  import type { ReaderDocument, ReaderTextSelection } from "$lib/domain/reader";
  import { highlightFill, markFill } from "$lib/features/reader/highlight-colors";
  import StickyGlyph from "$lib/features/reader/StickyGlyph.svelte";
  import { samePassage } from "$lib/features/reader/highlight-thread-match";
  import MetadataPanel from "$lib/features/library/MetadataPanel.svelte";

  // RFC 0071: the reader inspector is a Zotero-style sidebar whose right-edge
  // icon rail switches between three sections.
  type InspectorSection = "info" | "notes" | "chat";
  // Annotations-list filters (RFC 0062). Pins → the `starred` filter; Threads →
  // the `conversation` filter.
  type AuthorFilter = "all" | "me" | "ai";

  let {
    document,
    chatEnabled,
    threads,
    pins,
    highlights,
    selection,
    stickyColor,
    highlightBusy = false,
    requestedThreadId,
    requestedSection = null,
    onConsumeRequestedSection,
    isLoadingChat,
    chatError,
    metadataAutofillProgress,
    isAutofillingMetadata = false,
    onAutofillMetadata,
    onApplyMetadataCandidate,
    onUpdatePaperMetadata,
    onReloadChat,
    onClearSelection,
    onConsumeRequestedThread,
    onPickColor,
    onEnsureHighlight,
    onOpenHighlight,
    onHighlightIntent,
    onAskTurnStart,
    onAskTurnComplete,
    turnHighlightCount = 0,
    showTurnAffordance = false,
    onKeepTurnHighlights,
    onUndoTurnHighlights,
    onDismissTurnAffordance,
    onOpenCitation = () => {},
  }: {
    document: ReaderDocument;
    chatEnabled: boolean;
    threads: ChatThreadSummary[];
    pins: PinnedHighlight[];
    highlights: Highlight[];
    selection: ReaderTextSelection | null;
    stickyColor: HighlightColor;
    highlightBusy?: boolean;
    requestedThreadId: string | null;
    requestedSection?: InspectorSection | null;
    onConsumeRequestedSection?: () => void;
    isLoadingChat: boolean;
    chatError: string;
    metadataAutofillProgress?: MetadataAutofillProgress;
    isAutofillingMetadata?: boolean;
    onAutofillMetadata?: (paperId: string) => void | Promise<void>;
    onApplyMetadataCandidate: (paperId: string, candidate: MetadataCandidate) => void | Promise<void>;
    onUpdatePaperMetadata: (paperId: string, update: PaperMetadataUpdate) => void | Promise<void>;
    onReloadChat: () => void | Promise<void>;
    onClearSelection: () => void;
    onConsumeRequestedThread: () => void;
    onPickColor: (color: HighlightColor) => void | Promise<void>;
    onEnsureHighlight: () => Promise<string | null>;
    onOpenHighlight: (highlightId: string) => void;
    onHighlightIntent?: (intent: HighlightIntent) => Promise<boolean>;
    // RFC 0059 Phase 2 (Task 9): lifecycle hooks around a single ask turn so
    // ReaderView can collect the highlight ids the agent creates during that
    // turn (via onHighlightIntent) and offer a batch Keep/Undo once it settles.
    onAskTurnStart?: () => void;
    onAskTurnComplete?: () => void;
    turnHighlightCount?: number;
    showTurnAffordance?: boolean;
    /// RFC 0077: jump the reader to the passage behind a `[C1]` marker.
    onOpenCitation?: (citation: ContextCitation) => void;
    onKeepTurnHighlights?: () => void;
    onUndoTurnHighlights?: () => void | Promise<void>;
    // Fired at the thread-close chokepoints (back to list, opening a
    // different thread) so ReaderView can drop a stale turn's affordance
    // before it can reappear over the next thread (Minor finding 1, final
    // review).
    onDismissTurnAffordance?: () => void;
  } = $props();

  // Persisted so the rail choice survives the inspector unmounting when the
  // panel collapses (RFC 0071), and carries across paper switches.
  const SECTION_KEY = "i0i.reader-inspector-section";
  let activeSection = $state<InspectorSection>(loadActiveSection());

  function loadActiveSection(): InspectorSection {
    if (typeof localStorage === "undefined") {
      return "notes";
    }
    const stored = localStorage.getItem(SECTION_KEY);
    return stored === "info" || stored === "notes" || stored === "chat" ? stored : "notes";
  }

  $effect(() => {
    if (typeof localStorage !== "undefined") {
      localStorage.setItem(SECTION_KEY, activeSection);
    }
  });

  // RFC 0073 (R2.4): with a thread open, the Chat section shows the conversation
  // *and* the list of other conversations. The list collapses so the composer
  // keeps the height, and the choice is remembered — an unremembered default
  // would re-collapse on every thread you open.
  const CHAT_LIST_KEY = "i0i.reader-inspector-chat-list-collapsed";
  let chatListCollapsed = $state(loadChatListCollapsed());

  function loadChatListCollapsed(): boolean {
    if (typeof localStorage === "undefined") {
      return true;
    }
    // Default collapsed: opening a thread means you want to read it.
    return localStorage.getItem(CHAT_LIST_KEY) !== "false";
  }

  function toggleChatList() {
    chatListCollapsed = !chatListCollapsed;
    if (typeof localStorage !== "undefined") {
      localStorage.setItem(CHAT_LIST_KEY, String(chatListCollapsed));
    }
  }

  // Active annotation-list filters (RFC 0062); collapsed behind a labeled Filter
  // control (RFC 0066 R4).
  let filtersOpen = $state(false);
  let filterAuthor = $state<AuthorFilter>("all");
  let filterColor = $state<HighlightColor | null>(null);
  let filterHasNote = $state(false);
  let filterHasConversation = $state(false);
  let filterStarred = $state(false);
  // A thread with an empty id is *virtual*: it shows a passage's composer before
  // the first Note/Ask creates the row (RFC 0034 lazy threads).
  let openThread = $state<ChatThreadView | null>(null);
  let chatInput = $state("");
  let isBusy = $state(false);
  // RFC 0061: the passage's note is a highlight attachment, edited in its own
  // field (never the ask composer). `noteDraft` holds the editable text;
  // `isSavingNote` guards the save; `noteLoadedFor` tracks which passage's note
  // is currently in the draft so a background reload never clobbers typing.
  let noteDraft = $state("");
  let isSavingNote = $state(false);
  let noteLoadedFor = "";
  let pendingQuestion = $state<string | null>(null);
  let streamingAnswer = $state<string | null>(null);
  // RFC 0078: phase 1 can take up to three round trips before the first word of
  // prose. Without a live indicator the panel looks hung rather than thinking —
  // and a static line looks hung too, so the glyph has to move.
  let retrievalProgress = $state("");
  let retrievalCount = $state(0);
  const SPINNER = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
  let spinnerFrame = $state(0);

  $effect(() => {
    if (!isBusy || streamingAnswer) {
      return;
    }
    const timer = setInterval(() => {
      spinnerFrame = (spinnerFrame + 1) % SPINNER.length;
    }, 90);
    return () => clearInterval(timer);
  });
  let error = $state("");
  // RFC 0072: marking the passage is a bonus on the ask path — a failure there
  // must not lose the user's question, so it surfaces as its own non-fatal
  // notice instead of aborting the turn.
  let passageWarning = $state("");
  let renaming = $state(false);
  let renameTitle = $state("");
  // Guards streamed deltas against paper switches / superseded asks.
  let askSequence = 0;
  let activeAskId = $state(0);

  const scope = $derived<ChatScope>({ kind: "paper", paperId: document.paperId });
  // Whole-paper conversations. Plural since RFC 0078 follow-up: "Ask about
  // this paper" starts a new one rather than reopening January's.
  const paperChats = $derived(
    threads.filter((thread) => thread.anchor.kind === "document" && thread.entryCount > 0),
  );
  const isVirtual = $derived(Boolean(openThread) && openThread!.thread.id === "");
  const openPassage = $derived(openThread ? anchorSelectedText(openThread.thread.anchor) : null);
  // The annotated-passage row backing the open thread (RFC 0061). A passage
  // may exist without a thread (note-only / color-only) and a virtual selection
  // may have no row yet — the note field creates one on first save.
  const currentHighlight = $derived(
    openThread && openThread.thread.anchor.kind !== "document"
      ? highlights.find((hl) => samePassage(openThread!.thread.anchor, hl.locator))
      : undefined,
  );
  // For a selection thread the title defaults to the passage, so the quote block
  // alone says it — only show the heading when it adds something (a renamed
  // thread, or the whole-paper thread that has no passage).
  const showTitle = $derived(
    !openPassage || (openThread?.thread.title.trim() ?? "") !== openPassage.trim(),
  );
  const canSubmit = $derived(Boolean(openThread && chatInput.trim() && !isBusy));
  const visibleError = $derived(error || passageWarning || chatError);

  // RFC 0062: the annotation index. Each annotated passage (a `highlights` row)
  // becomes one row, paired with its conversation thread (if any) for the
  // conversation/starred badges. Newest first.
  type AnnotationRow = {
    highlight: Highlight;
    thread?: ChatThreadSummary;
    hasNote: boolean;
    hasConversation: boolean;
    isAgent: boolean;
    starred: boolean;
  };
  const annotationRows = $derived<AnnotationRow[]>(
    highlights
      .map((highlight) => {
        const thread = threads.find((t) => samePassage(t.anchor, highlight.locator));
        return {
          highlight,
          thread,
          hasNote: Boolean(highlight.note && highlight.note.trim()),
          hasConversation: Boolean(thread && thread.entryCount > 0),
          isAgent: highlight.author.kind === "agent",
          // v1: "starred" reuses thread pins (no highlight-level star column yet).
          starred: Boolean(thread && thread.pinnedCount > 0),
        };
      })
      .reverse(),
  );
  // RFC 0067 (R1): the Marks list is "everything I highlighted/noted" — rows
  // with a color or a note. A *pure* conversation passage (asked, no color/note)
  // lives under Chats, not here (though it's still marked on the page). The
  // `!hasConversation` clause keeps orphan rows reachable: a highlight with no
  // color, no note, and no conversation (e.g. a failed ask, a whitespace-only
  // note) would otherwise fall out of both lists and render nowhere. Net
  // invariant: every highlight appears in Marks ∪ Chats exactly once.
  const markRows = $derived(
    annotationRows.filter(
      (row) =>
        // RFC 0074: a sticky note is always a mark, even before it has been
        // typed into — right after placement it has no note and no color rule
        // should be able to hide it.
        isStickyNote(row.highlight.locator) ||
        row.highlight.color !== null ||
        row.hasNote ||
        !row.hasConversation,
    ),
  );
  // RFC 0067 (R1): the Chats section lists every passage conversation; the
  // whole-paper "Ask about this paper" row heads it separately. The count folds
  // in the whole-paper conversation when it has entries.
  const passageChats = $derived(
    threads.filter((thread) => thread.anchor.kind !== "document" && thread.entryCount > 0),
  );
  const chatCount = $derived(paperChats.length + passageChats.length);
  const activeFilterCount = $derived(
    (filterAuthor !== "all" ? 1 : 0) +
      (filterColor !== null ? 1 : 0) +
      (filterHasNote ? 1 : 0) +
      (filterHasConversation ? 1 : 0) +
      (filterStarred ? 1 : 0),
  );
  const anyFilterActive = $derived(activeFilterCount > 0);
  const filteredAnnotations = $derived(
    markRows.filter((row) => {
      if (filterAuthor === "me" && row.isAgent) return false;
      if (filterAuthor === "ai" && !row.isAgent) return false;
      if (filterColor !== null && row.highlight.color !== filterColor) return false;
      if (filterHasNote && !row.hasNote) return false;
      if (filterHasConversation && !row.hasConversation) return false;
      if (filterStarred && !row.starred) return false;
      return true;
    }),
  );

  function clearFilters() {
    filterAuthor = "all";
    filterColor = null;
    filterHasNote = false;
    filterHasConversation = false;
    filterStarred = false;
  }

  // Reset the open conversation when the paper changes.
  $effect(() => {
    void document.paperId;
    openThread = null;
    chatInput = "";
    error = "";
    renaming = false;
    // RFC 0066 (R4): don't carry filters onto a different paper — a collapsed
    // "Starred" filter would silently empty the new paper's Marks list.
    clearFilters();
    activeAskId = (askSequence += 1);
  });

  // A new reader selection opens that passage's (virtual) thread.
  $effect(() => {
    if (!selection) {
      return;
    }
    openThread = virtualThread(anchorFromSelection(selection), selection.selectedText.trim() || "New thread");
    error = "";
    renaming = false;
    activeSection = "notes";
  });

  // A clicked margin mark (or pin) requests a specific thread to open.
  $effect(() => {
    const threadId = requestedThreadId;
    if (!threadId) {
      return;
    }
    activeSection = sectionForThread(threadId);
    void openThreadById(threadId);
    onConsumeRequestedThread();
  });

  // RFC 0071: the toolbar's Note/Chat buttons request a section through
  // ReaderView; apply it and clear the request. Fires on mount too, so revealing
  // a collapsed panel onto a requested section lands correctly.
  $effect(() => {
    const section = requestedSection;
    if (!section) {
      return;
    }
    activeSection = section;
    onConsumeRequestedSection?.();
  });

  // RFC 0073 (R2.3): in Notes the marks list now sits below the open passage, so
  // a long list could leave the note field scrolled out of view when a passage
  // opens. Anchor the panel to the detail — the thing the user just acted on.
  let tabPanelElement = $state<HTMLElement | null>(null);
  $effect(() => {
    const passage = openPassage;
    if (activeSection === "notes" && passage && tabPanelElement) {
      tabPanelElement.scrollTop = 0;
    }
  });

  // Background title generation updates the parent `threads` list first; keep
  // the open thread header in sync without refetching its entries.
  $effect(() => {
    if (!openThread || openThread.thread.id === "") {
      return;
    }
    const updated = threads.find((thread) => thread.id === openThread!.thread.id);
    if (updated && updated.title !== openThread.thread.title) {
      openThread = {
        ...openThread,
        thread: {
          ...openThread.thread,
          title: updated.title,
          updatedAt: updated.updatedAt,
        },
      };
      if (renaming) {
        renameTitle = updated.title;
      }
    }
  });

  function anchorFromSelection(sel: ReaderTextSelection): ThreadAnchor {
    if (sel.anchorKind === "pdf_rect") {
      return {
        kind: "pdfRect",
        sourceId: sel.sourceId,
        pageIndex: sel.pageIndex ?? 0,
        rectsJson: sel.rectsJson ?? "[]",
        selectedText: sel.selectedText,
      };
    }
    return {
      kind: "textOffset",
      sourceId: sel.sourceId,
      startOffset: sel.startOffset,
      endOffset: sel.endOffset,
      selectedText: sel.selectedText,
    };
  }

  function virtualThread(anchor: ThreadAnchor, title: string): ChatThreadView {
    return {
      thread: { id: "", anchor, title, createdAt: "", updatedAt: "" },
      entries: [],
    };
  }

  // Stable identity for a passage, so the note-load effect re-seeds the draft
  // only when the *passage* changes — not on every `highlights` reload.
  function passageKey(anchor: ThreadAnchor): string {
    if (anchor.kind === "document") return "document";
    if (anchor.kind === "pdfRect") return `pdf:${anchor.sourceId}:${anchor.pageIndex}:${anchor.rectsJson}`;
    return `txt:${anchor.sourceId}:${anchor.startOffset}:${anchor.endOffset}`;
  }

  // Seed the note draft from the open passage's stored note, once per passage.
  $effect(() => {
    if (!openThread) {
      noteLoadedFor = "";
      noteDraft = "";
      return;
    }
    const key = passageKey(openThread.thread.anchor);
    if (key !== noteLoadedFor) {
      noteLoadedFor = key;
      noteDraft = currentHighlight?.note ?? "";
    }
  });

  // The rail section a reopened thread should land in: Notes if the passage has
  // a note, Chat if it has a conversation, else Notes (RFC 0071, replacing the
  // old note/chat detail switch). Whole-paper threads always open in Chat.
  function sectionForThread(threadId: string): InspectorSection {
    const thread = threads.find((t) => t.id === threadId);
    if (thread?.anchor.kind === "document") {
      return "chat";
    }
    const highlight = thread
      ? highlights.find((hl) => samePassage(thread.anchor, hl.locator))
      : undefined;
    if (highlight?.note?.trim()) {
      return "notes";
    }
    return thread && thread.entryCount > 0 ? "chat" : "notes";
  }

  /// Start a new whole-paper conversation. Always fresh: a new question about
  /// the paper is usually a new subject, and appending it to a months-old
  /// thread both buries it and drags that history into every prompt.
  /// Past conversations stay in the Chats list below.
  function openWholePaper() {
    activeSection = "chat";
    error = "";
    openThread = virtualThread({ kind: "document" }, "New chat");
  }

  async function openThreadById(threadId: string) {
    error = "";
    // Switching to a specific existing thread (a mark click, a pin, a thread
    // row) invalidates any pending selection — otherwise the swatch row for
    // the old selection would stay visible over the newly-opened thread and
    // a swatch click would mark the wrong (stale) passage.
    onClearSelection();
    onDismissTurnAffordance?.();
    try {
      openThread = await getChatThread(threadId);
    } catch (caught) {
      error = String(caught);
    }
  }

  function backToThreadList() {
    openThread = null;
    renaming = false;
    onClearSelection();
    onDismissTurnAffordance?.();
  }

  async function refreshOpenThread() {
    if (!openThread || openThread.thread.id === "") {
      return;
    }
    const threadId = openThread.thread.id;
    const view = await getChatThread(threadId);
    if (openThread?.thread.id === threadId) {
      openThread = view;
    }
    await refreshContext();
  }

  function handleComposerKeydown(event: KeyboardEvent) {
    if (event.key === "Enter" && !event.shiftKey) {
      event.preventDefault();
      void ask();
    }
  }

  async function ask() {
    const body = chatInput.trim();
    if (!body || isBusy || !openThread) {
      return;
    }

    const virtual = openThread.thread.id === "";
    const anchor = openThread.thread.anchor;
    const threadId = openThread.thread.id;
    const askId = (askSequence += 1);
    activeAskId = askId;
    isBusy = true;
    error = "";
    passageWarning = "";
    pendingQuestion = body;
    streamingAnswer = "";
    retrievalProgress = "";
    retrievalCount = 0;
    chatInput = "";
    try {
      if (virtual) {
        // RFC 0058 Phase 1 (Task 9): mark the passage alongside the thread,
        // one gesture. Reuses the reload the ask itself triggers below.
        // RFC 0072: the conversation still has value if the passage row can't
        // be created — report it, don't abandon the ask.
        try {
          await onEnsureHighlight();
        } catch (caught) {
          passageWarning = `Couldn't mark this passage: ${String(caught)}`;
        }
      }
      // RFC 0064: asking is purely a conversation now. AI marking is the
      // explicit toolbar "Highlight with AI" command — never a side effect of a
      // chat message (no keyword gate, no prose-plus-marks).
      const onDelta = (text: string) => {
        if (activeAskId === askId) {
          // The first token of prose means phase 1 is over.
          retrievalProgress = "";
          retrievalCount = 0;
          streamingAnswer = (streamingAnswer ?? "") + text;
        }
      };
      const view = virtual
        ? // A virtual whole-paper thread is a new chat by construction — the
          // backend would otherwise fold it into the existing one.
          await askAtAnchorStreamed(scope, anchor, body, onDelta, anchor.kind === "document")
        : await askChatThreadStreamed(threadId, body, onDelta);
      // The ANSWER is done: show it and unblock the composer immediately (the
      // `finally` below clears `isBusy`).
      if (activeAskId === askId) {
        openThread = view;
        onReloadChat();
        if (virtual) {
          onClearSelection();
        }
      }
    } catch (caught) {
      if (activeAskId === askId) {
        error = String(caught);
        chatInput = body;
      }
    } finally {
      if (activeAskId === askId) {
        isBusy = false;
        pendingQuestion = null;
        streamingAnswer = null;
        retrievalProgress = "";
        retrievalCount = 0;
      }
    }
  }

  function handleNoteKeydown(event: KeyboardEvent) {
    if (event.key === "Enter" && !event.shiftKey) {
      event.preventDefault();
      void saveNote();
    }
  }

  // RFC 0061: the Note field saves the passage's note (never asks the AI).
  // It attaches to the annotated-passage row — creating a color-less one for a
  // fresh selection — and writes `set_highlight_note`. An empty draft clears
  // the note. It creates no thread; a note-only passage renders as the neutral
  // marker.
  async function saveNote() {
    if (!openThread || isSavingNote) {
      return;
    }
    const body = noteDraft.trim();
    // Nothing to save and nothing to clear — don't conjure a phantom passage
    // from a stray Enter on a fresh selection (RFC 0061; the Note field is now
    // the first input, inheriting the old composer's "type, Enter" reflex).
    if (!body.length && !currentHighlight) {
      return;
    }
    isSavingNote = true;
    error = "";
    passageWarning = "";
    try {
      let id = currentHighlight?.id ?? null;
      if (!id) {
        // RFC 0072: a backend failure now throws and lands in the catch below
        // with its real message; `null` means only "there was nothing to mark".
        id = await onEnsureHighlight();
      }
      if (!id) {
        error = "Couldn't attach the note to this passage.";
        return;
      }
      await setHighlightNote(id, body.length ? body : null);
      await onReloadChat();
      if (isVirtual) {
        onClearSelection();
      }
    } catch (caught) {
      error = String(caught);
    } finally {
      isSavingNote = false;
    }
  }

  async function togglePin(entry: ChatEntry) {
    error = "";
    try {
      await setChatEntryPinned(entry.id, !entry.pinned);
      await refreshOpenThread();
      onReloadChat();
    } catch (caught) {
      error = String(caught);
    }
  }

  async function deleteOpenThread() {
    if (!openThread) {
      return;
    }
    if (openThread.thread.id === "") {
      backToThreadList();
      return;
    }
    const threadId = openThread.thread.id;
    try {
      await deleteChatThread(threadId);
      openThread = null;
      onReloadChat();
    } catch (caught) {
      error = String(caught);
    }
  }

  function startRename() {
    if (openThread && openThread.thread.id !== "") {
      renameTitle = openThread.thread.title;
      renaming = true;
    }
  }

  async function commitRename() {
    if (!openThread || openThread.thread.id === "" || !renameTitle.trim()) {
      renaming = false;
      return;
    }
    const threadId = openThread.thread.id;
    try {
      await renameChatThread(threadId, renameTitle);
      await refreshOpenThread();
      onReloadChat();
    } catch (caught) {
      error = String(caught);
    } finally {
      renaming = false;
    }
  }

  function entryAuthor(entry: ChatEntry) {
    if (entry.kind === "answer") {
      return "AI";
    }
    return entry.kind === "note" ? "Note" : "You";
  }

  let compacting = $state(false);
  let contextNote = $state("");
  // RFC 0077: the thread's *persistent* context. The current selection and this
  // turn's retrieval are ephemeral and deliberately absent — they are assembled
  // at ask time and never stored, so there is nothing here to remove.
  let contextItems = $state<ContextItemView[]>([]);

  const contextTokens = $derived(
    contextItems.reduce((total, item) => total + item.tokenEstimate, 0),
  );
  const unresolvedCount = $derived(contextItems.filter((item) => item.unresolved).length);
  // RFC 0078: closed is the resting state — context is plumbing, and plumbing
  // you must look at to read an answer is a leak. The one exception: a hole
  // opens itself, because a hole you never see is the failure mode RFC 0076 and
  // RFC 0077 were both written against.
  let contextOpen = $state(false);
  const contextExpanded = $derived(contextOpen || unresolvedCount > 0);

  // Keyed on the thread id, so switching threads never leaves the previous
  // thread's context on screen. Cheap: one indexed read per open.
  $effect(() => {
    const threadId = openThread?.thread.id ?? "";
    contextNote = "";
    contextOpen = false;
    if (!threadId) {
      contextItems = [];
      return;
    }
    void refreshContext();
  });

  // RFC 0078: a global event, not a per-ask channel — the reader has one
  // conversation open at a time, and the ask channel is committed to deltas.
  onMount(() => {
    const unlisten = listen<{ event: string; query?: string; count?: number }>(
      "chat://progress",
      ({ payload }) => {
        if (!isBusy) {
          return;
        }
        // "deciding" fires before the first round trip, so the panel says
        // something even on a turn that ends up searching nothing.
        if (payload.event === "deciding") {
          retrievalCount = 0;
          retrievalProgress = "";
        } else if (payload.event === "searching") {
          retrievalProgress = payload.query ?? "";
        } else {
          retrievalCount = payload.count ?? 0;
        }
      },
    );
    return () => void unlisten.then((stop) => stop());
  });

  async function refreshContext() {
    if (!openThread || isVirtual) {
      contextItems = [];
      return;
    }
    const threadId = openThread.thread.id;
    try {
      const items = await listChatContext(threadId);
      if (openThread?.thread.id === threadId) {
        contextItems = items;
      }
    } catch (caught) {
      error = String(caught);
    }
  }

  /** Keep a passage the agent cited, so later turns carry it deliberately. */
  async function keepCitation(citation: ContextCitation) {
    if (!openThread || isVirtual || !citation.chunkId) {
      return;
    }
    try {
      await addChatContext(openThread.thread.id, citation.chunkId);
      contextNote = `Kept ${citation.handle}`;
      await refreshContext();
    } catch (caught) {
      error = String(caught);
    }
  }

  async function dropContextItem(item: ContextItemView) {
    if (!openThread) {
      return;
    }
    try {
      await deleteChatContext(openThread.thread.id, { kind: "item", id: item.id });
      contextNote = "";
      await refreshContext();
    } catch (caught) {
      error = String(caught);
    }
  }

  // RFC 0078: which answers have their "what the agent read" drawer open.
  // Per entry, and closed by default — references are the answer's evidence,
  // this is the audit trail behind them.
  let openPassageDrawers = $state<Record<string, boolean>>({});

  function togglePassageDrawer(entryId: string) {
    openPassageDrawers = { ...openPassageDrawers, [entryId]: !openPassageDrawers[entryId] };
  }

  function passageLabel(passage: PassageRef) {
    const page = `p${passage.pageStart + 1}`;
    return passage.headingPath ? `${page} · ${passage.headingPath}` : page;
  }

  function contextItemLabel(item: ContextItemView) {
    if (item.kind === "summary") {
      return "Summary of earlier context";
    }
    if (item.unresolved) {
      return "Passage no longer in the document";
    }
    const page = item.pageStart === null ? "" : `p${item.pageStart + 1}`;
    return item.headingPath ? `${page} · ${item.headingPath}` : page || "Passage";
  }

  /**
   * Compact the open thread's context (RFC 0077).
   *
   * Explicit, never automatic: it costs a model round-trip, and assembling a
   * prompt has to stay fast and predictable. Nothing in the conversation is
   * deleted — only what the next prompt carries shrinks.
   */
  async function compactOpenContext() {
    if (!openThread || compacting) {
      return;
    }
    compacting = true;
    try {
      await compactChatContext(openThread.thread.id);
      // The conversation is untouched by design, so the thread re-renders
      // identically — without a word here, a successful compaction looks like
      // a button that did nothing.
      contextNote = "Context compacted — the conversation above is unchanged.";
      await refreshOpenThread();
    } catch (caught) {
      error = String(caught);
    } finally {
      compacting = false;
    }
  }

  function chatContextLabel(entry: ChatEntry) {
    const summary = entry.contextSummary;
    if (!summary) {
      return "";
    }
    // RFC 0077: passages the model was actually given, and the ones it was
    // not. A silent drop would be indistinguishable from a thin answer.
    const parts: string[] = [];
    if (summary.contextItems > 0) {
      parts.push(`${summary.contextItems} passage${summary.contextItems === 1 ? "" : "s"}`);
    }
    if (summary.compacted) {
      parts.push("compacted");
    }
    if (summary.droppedItems > 0) {
      parts.push(`${summary.droppedItems} over budget`);
    }
    if (summary.unresolvedItems > 0) {
      parts.push(`${summary.unresolvedItems} unresolved`);
    }
    // RFC 0078: the loop stopped at a bound. Said out loud, because a capped
    // turn otherwise reads as an agent that decided it had enough.
    if (summary.retrievalCapped) {
      parts.push("search capped");
    }
    if (parts.length) {
      return `Context: ${parts.join(" · ")}`;
    }
    // `includedChars` counts the paper body text; the foregrounded passage (for
    // an anchored thread) is sent separately, so reflect that rather than
    // claiming there was no context.
    if (summary.includedChars > 0) {
      const chars = summary.includedChars.toLocaleString();
      return `Context: ${chars} chars${summary.truncated ? " · truncated" : ""}`;
    }
    return openPassage ? "Context: selected passage only" : "Context: title + metadata only";
  }

</script>

{#snippet passageDetail()}
  {#if openThread}
              <!-- A focused passage (or the whole-paper thread) is open. The rail
                   section decides the lens: Notes edits its note, Chat holds its
                   conversation. `openThread` persists across a section switch. -->
              <div class="row section-title">
                <button class="link-btn" type="button" onclick={backToThreadList}>‹ {activeSection === "chat" ? "Chat" : "Marks"}</button>
                <div class="flex1"></div>
                {#if activeSection === "chat"}
                  {#if !isVirtual}
                    <button class="note-icon" type="button" aria-label="rename thread" onclick={startRename}><Pencil size={13} strokeWidth={1.75} aria-hidden="true" /></button>
                  {/if}
                  <!-- Start another conversation without going back to the
                       list. The current one is already saved. -->
                  <button
                    class="link-btn"
                    type="button"
                    title="Start a new chat about this paper"
                    onclick={openWholePaper}
                  >+ new</button>
                  {#if !isVirtual}
                    <!-- RFC 0077: explicit, because it costs a model call and
                         is lossy. The conversation itself is untouched. -->
                    <button
                      class="link-btn"
                      type="button"
                      disabled={compacting}
                      title="Summarize this thread's context so later turns carry less"
                      onclick={() => void compactOpenContext()}
                    >
                      {compacting ? "compacting…" : "compact"}
                    </button>
                  {/if}
                  <button class="note-icon remove" type="button" aria-label="delete thread" onclick={() => void deleteOpenThread()}><Trash2 size={13} strokeWidth={1.75} aria-hidden="true" /></button>
                {/if}
              </div>

              {#if renaming}
                <input
                  class="rename-input"
                  bind:value={renameTitle}
                  aria-label="Thread title"
                  onkeydown={(event) => {
                    if (event.key === "Enter") void commitRename();
                    if (event.key === "Escape") renaming = false;
                  }}
                />
                <div class="row note-actions">
                  <button class="btn primary" type="button" onclick={() => void commitRename()}>Rename</button>
                </div>
              {:else if showTitle}
                <h3 class="thread-title">{openThread.thread.title}</h3>
              {/if}

              {#if openPassage}
                <blockquote>{openPassage}</blockquote>
              {/if}

              {#if activeSection === "notes"}
                {#if selection}
                  <div class="row swatch-row" role="group" aria-label="Highlight color">
                    {#each HIGHLIGHT_COLORS as color}
                      <button
                        class="swatch"
                        class:active={color === stickyColor}
                        type="button"
                        disabled={highlightBusy}
                        style={`background:${highlightFill(color)}`}
                        aria-label={`Highlight ${color}`}
                        title={`Highlight ${color}`}
                        onclick={() => void onPickColor(color)}
                      ></button>
                    {/each}
                  </div>
                {/if}

                <div class="note-field">
                  <div class="row note-field-head">
                    <span class="label">Note</span>
                    {#if noteDraft.trim() !== (currentHighlight?.note ?? "").trim()}
                      <button
                        class="link-btn"
                        type="button"
                        disabled={isSavingNote}
                        onclick={() => void saveNote()}
                      >
                        {isSavingNote ? "Saving…" : "Save"}
                      </button>
                    {/if}
                  </div>
                  <textarea
                    bind:value={noteDraft}
                    aria-label="Note on this passage"
                    placeholder="Jot a note on this passage… (Enter saves, Shift+Enter newline)"
                    rows="2"
                    disabled={isSavingNote}
                    onkeydown={handleNoteKeydown}
                  ></textarea>
                </div>
              {:else}
                {#if contextItems.length || contextNote}
                  <div class="context-panel">
                    <button
                      class="row context-head"
                      type="button"
                      aria-expanded={contextExpanded}
                      onclick={() => (contextOpen = !contextOpen)}
                    >
                      {#if contextExpanded}
                        <ChevronDown size={11} strokeWidth={1.75} aria-hidden="true" />
                      {:else}
                        <ChevronRight size={11} strokeWidth={1.75} aria-hidden="true" />
                      {/if}
                      <span class="label">Context ({contextItems.length})</span>
                      {#if unresolvedCount}
                        <span class="context-warn">· {unresolvedCount} unresolved</span>
                      {/if}
                      <div class="flex1"></div>
                      {#if contextItems.length}
                        <span class="mono-dim">~{contextTokens} tok</span>
                      {/if}
                    </button>
                    {#if contextNote}
                      <div class="context-note mono-dim">{contextNote}</div>
                    {/if}
                    {#if contextExpanded}
                      {#each contextItems as item (item.id)}
                        <div class="row context-item" class:unresolved={item.unresolved}>
                          <span class="context-label">{contextItemLabel(item)}</span>
                          {#if item.origin === "agent"}
                            <span class="origin-tag" title="Added by the AI">agent</span>
                          {/if}
                          <div class="flex1"></div>
                          <button
                            class="note-icon remove"
                            type="button"
                            aria-label="remove from context"
                            onclick={() => void dropContextItem(item)}
                          >×</button>
                        </div>
                      {/each}
                    {/if}
                  </div>
                {/if}

                <div class="thread-view">
                  {#each openThread.entries as entry}
                    <div class="entry {entry.kind}">
                      <div class="row entry-head">
                        <span class="label">{entryAuthor(entry)}</span>
                        <div class="flex1"></div>
                        <button
                          class="pin-btn"
                          class:pinned={entry.pinned}
                          type="button"
                          aria-label={entry.pinned ? "unpin" : "pin"}
                          onclick={() => void togglePin(entry)}
                        >
                          {entry.pinned ? "★" : "☆"}
                        </button>
                      </div>
                      {#if entry.kind === "answer"}
                        <CitedAnswer
                          body={entry.body}
                          citations={entry.contextSummary?.citations ?? []}
                          {onOpenCitation}
                        />
                      {:else}
                        <p>{entry.body}</p>
                      {/if}
                      {#if entry.kind === "answer" && entry.contextSummary?.citations?.length}
                        <!-- RFC 0078: only passages the answer actually cited.
                             What the model was offered is not evidence. -->
                        <div class="references">
                          <div class="mono-dim ref-title">References</div>
                          {#each entry.contextSummary.citations as citation (citation.handle)}
                            <div class="ref-row">
                              <button
                                class="ref-open"
                                type="button"
                                title="Jump to this passage"
                                onclick={() => onOpenCitation(citation)}
                              >
                                <span class="row ref-top">
                                  <span class="ref-handle">[{citation.handle}]</span>
                                  <span class="ref-where">{citationLabel(citation)}</span>
                                </span>
                                <!-- What the passage says, not just where it is.
                                     Taken from the text rather than summarized:
                                     a model call per reference would cost more
                                     latency than the answer itself. -->
                                <span class="ref-preview">{citation.preview}</span>
                              </button>
                              {#if citation.chunkId && !isVirtual}
                                <!-- Retrieved passages are ephemeral, re-selected
                                     each turn. Keeping one makes it persistent. -->
                                <button
                                  class="keep-chip"
                                  type="button"
                                  title="Keep this passage in the thread's context"
                                  onclick={() => void keepCitation(citation)}
                                >keep</button>
                              {/if}
                            </div>
                          {/each}
                        </div>
                      {/if}
                      {#if entry.kind === "answer" && entry.contextSummary?.passages?.length}
                        <!-- RFC 0078: everything the agent read this turn,
                             including what it chose not to cite. Closed by
                             default — the References above are the evidence,
                             this is the audit trail. -->
                        <button
                          class="drawer-toggle mono-dim"
                          type="button"
                          aria-expanded={Boolean(openPassageDrawers[entry.id])}
                          onclick={() => togglePassageDrawer(entry.id)}
                        >
                          {openPassageDrawers[entry.id] ? "▾" : "▸"}
                          read {entry.contextSummary.passages.length} passage{entry
                            .contextSummary.passages.length === 1
                            ? ""
                            : "s"}
                        </button>
                        {#if openPassageDrawers[entry.id]}
                          {#each entry.contextSummary.passages as passage (passage.handle)}
                            <div class="row drawer-row" class:uncited={!passage.cited}>
                              <span class="ref-handle">[{passage.handle}]</span>
                              <span class="ref-where">{passageLabel(passage)} — {passage.preview}</span>
                              <div class="flex1"></div>
                              {#if !passage.cited}
                                <span class="mono-dim">not cited</span>
                              {/if}
                            </div>
                          {/each}
                        {/if}
                      {/if}
                      {#if entry.kind === "answer" && chatContextLabel(entry)}
                        <div class="entry-context mono-dim">{chatContextLabel(entry)}</div>
                      {/if}
                    </div>
                  {/each}

                  {#if pendingQuestion !== null}
                    <div class="entry question">
                      <div class="label">You</div>
                      <p>{pendingQuestion}</p>
                    </div>
                    <div class="entry answer">
                      <div class="label">AI</div>
                      {#if !streamingAnswer}
                        <p class="working">
                          <span class="spin">{SPINNER[spinnerFrame]}</span>
                          {#if retrievalProgress}
                            <span class="working-query">{retrievalProgress}</span>
                          {/if}
                          {#if retrievalCount}
                            <span class="working-count"
                              >{retrievalCount} passage{retrievalCount === 1 ? "" : "s"}</span
                            >
                          {/if}
                        </p>
                      {:else}
                        <!-- The same renderer as a finished answer, so nothing
                             reflows when the stream ends. Half-written syntax
                             renders literally, which is what the parser does. -->
                        <CitedAnswer body={streamingAnswer} />
                      {/if}
                    </div>
                  {/if}

                  {#if !openThread.entries.length && pendingQuestion === null}
                    <p class="empty-note">Ask a question below to start a conversation about this passage.</p>
                  {/if}
                </div>

                <div class="thread-input">
                  <textarea
                    bind:value={chatInput}
                    aria-label="Ask a question"
                    placeholder="Ask the AI about this passage… (Enter sends, Shift+Enter newline)"
                    rows="3"
                    disabled={isBusy}
                    onkeydown={handleComposerKeydown}
                  ></textarea>
                  <div class="row note-actions">
                    <button class="btn primary" type="button" disabled={!canSubmit} onclick={() => void ask()}>
                      {isBusy ? "…" : "Ask"}
                    </button>
                  </div>
                </div>
              {/if}
  {/if}
{/snippet}

{#snippet chatListBody()}
              <div class="thread-list">
                <button class="ask-paper-row" type="button" onclick={openWholePaper}>
                  <MessageSquare size={13} strokeWidth={1.75} aria-hidden="true" />
                  <span class="ask-paper-title">New chat</span>
                </button>

                {#each paperChats as chat (chat.id)}
                  <button class="thread-row" type="button" onclick={() => void openThreadById(chat.id)}>
                    <MessageSquare size={12} strokeWidth={1.75} aria-hidden="true" />
                    <span class="thread-row-title">{chat.title.trim() || "Whole paper"}</span>
                    <span class="mono-dim">{chat.entryCount}</span>
                  </button>
                {/each}

                {#each passageChats as chat (chat.id)}
                  <button class="thread-row" type="button" onclick={() => void openThreadById(chat.id)}>
                    <MessageSquare size={12} strokeWidth={1.75} aria-hidden="true" />
                    <span class="thread-row-title">{chat.title.trim() || anchorSelectedText(chat.anchor) || "Conversation"}</span>
                    <span class="mono-dim">{chat.entryCount}</span>
                  </button>
                {/each}
              </div>
{/snippet}

<aside class="reader-inspector hair-l">
  <div class="inspector-body">
    <header class="hair-b">
      <div class="paper-name truncate">{document.title}</div>
      <div class="mono-dim">paper / selected / reader</div>
    </header>

    <div class="tab-panel" bind:this={tabPanelElement}>
      {#if activeSection === "info"}
        <section class="metadata">
          <div class="meta-grid">
            <span>id</span><strong>{document.identifier}</strong>
            <span>cite</span><strong>{document.citationKey}</strong>
            <span>marks</span><strong>{document.marks.length}</strong>
          </div>

          <MetadataPanel
            paperId={document.paperId}
            title={document.title}
            authors={document.authors}
            venue={document.venue}
            year={document.year}
            tags={document.tags}
            progress={metadataAutofillProgress}
            isAutofilling={isAutofillingMetadata}
            onAutofill={onAutofillMetadata}
            onApplyCandidate={onApplyMetadataCandidate}
            onSaveMetadata={onUpdatePaperMetadata}
          />
        </section>
      {:else}
        <section>
          {#if !chatEnabled}
            <div class="row section-title"><span class="label hot">{activeSection === "chat" ? "Chat" : "Notes"}</span></div>
            <p class="empty-note">Add this paper to a Vault to {activeSection === "chat" ? "chat about" : "annotate"} it.</p>
          {:else if activeSection === "notes"}
            <!-- RFC 0073 (R2.1): Notes COMPOSES the open passage and the marks
                 list — the detail used to replace the list, so annotating a
                 passage hid every other mark. Chat keeps replace-semantics: a
                 conversation needs the whole panel. -->
            {#if openThread && openPassage}
              {@render passageDetail()}
            {/if}

            <div class="row section-title">
              <span class="label hot">Marks</span>
              <div class="flex1"></div>
              <span class="mono-dim">{markRows.length}</span>
            </div>

            <div class="filter-control">
              <div class="row filter-head">
                <button class="filter-toggle" class:on={filtersOpen || anyFilterActive} type="button" onclick={() => (filtersOpen = !filtersOpen)}>
                  <SlidersHorizontal size={12} strokeWidth={1.75} aria-hidden="true" />
                  Filter{activeFilterCount > 0 ? ` · ${activeFilterCount}` : ""}
                </button>
                <div class="flex1"></div>
                {#if anyFilterActive}
                  <button class="link-btn" type="button" onclick={clearFilters}>Clear</button>
                {/if}
              </div>

              {#if filtersOpen}
                <div class="filter-panel">
                  <div class="filter-group">
                    <span class="filter-label">Author</span>
                    <div class="row filter-options">
                      <button class="chip-btn" class:on={filterAuthor === "all"} type="button" onclick={() => (filterAuthor = "all")}>All</button>
                      <button class="chip-btn" class:on={filterAuthor === "me"} type="button" onclick={() => (filterAuthor = "me")}>Me</button>
                      <button class="chip-btn" class:on={filterAuthor === "ai"} type="button" onclick={() => (filterAuthor = "ai")}>
                        <Sparkles size={11} strokeWidth={1.75} aria-hidden="true" /> AI
                      </button>
                    </div>
                  </div>

                  <div class="filter-group">
                    <span class="filter-label">Color</span>
                    <div class="row filter-options">
                      <button
                        class="color-dot none"
                        class:on={filterColor === null}
                        type="button"
                        aria-label="Any color"
                        title="Any color"
                        onclick={() => (filterColor = null)}
                      ></button>
                      {#each HIGHLIGHT_COLORS as color}
                        <button
                          class="color-dot"
                          class:on={filterColor === color}
                          type="button"
                          style={`background:${highlightFill(color)}`}
                          aria-label={`Filter ${color}`}
                          title={color}
                          onclick={() => (filterColor = filterColor === color ? null : color)}
                        ></button>
                      {/each}
                    </div>
                  </div>

                  <div class="filter-group">
                    <span class="filter-label">Has</span>
                    <div class="row filter-options">
                      <button class="chip-btn" class:on={filterHasNote} type="button" onclick={() => (filterHasNote = !filterHasNote)}>
                        <StickyNote size={11} strokeWidth={1.75} aria-hidden="true" /> Note
                      </button>
                      <button class="chip-btn" class:on={filterHasConversation} type="button" onclick={() => (filterHasConversation = !filterHasConversation)}>
                        <MessageSquare size={11} strokeWidth={1.75} aria-hidden="true" /> Chat
                      </button>
                      <button class="chip-btn" class:on={filterStarred} type="button" onclick={() => (filterStarred = !filterStarred)}>
                        <Star size={11} strokeWidth={1.75} aria-hidden="true" /> Starred
                      </button>
                    </div>
                  </div>
                </div>
              {/if}
            </div>

            <div class="thread-list">
              {#if isLoadingChat}
                <p class="empty-note">Loading annotations…</p>
              {:else if filteredAnnotations.length}
                {#each filteredAnnotations as row (row.highlight.id)}
                  <button class="thread-row" type="button" onclick={() => onOpenHighlight(row.highlight.id)}>
                    <!-- RFC 0074: the list mirrors the page — a sticky note reads
                         as its glyph, a passage mark as its color chip. -->
                    {#if isStickyNote(row.highlight.locator)}
                      <StickyGlyph color={row.highlight.color} size={13} />
                    {:else}
                      <span class="color-chip" style={`background:${markFill(row.highlight.color)}`} aria-hidden="true"></span>
                    {/if}
                    <span class="thread-row-title">{row.highlight.note?.trim() || row.highlight.excerpt}</span>
                    {#if row.isAgent}<span class="badge" title="AI-authored"><Sparkles size={12} strokeWidth={1.75} aria-hidden="true" /></span>{/if}
                    {#if row.hasNote}<span class="badge" title="has a note"><StickyNote size={12} strokeWidth={1.75} aria-hidden="true" /></span>{/if}
                    {#if row.hasConversation}<span class="badge" title="has a conversation"><MessageSquare size={12} strokeWidth={1.75} aria-hidden="true" /></span>{/if}
                    {#if row.starred}<span class="badge" title="starred"><Star size={12} strokeWidth={1.75} aria-hidden="true" /></span>{/if}
                  </button>
                {/each}
              {:else if anyFilterActive}
                <p class="empty-note">No marks match these filters. <button class="link-btn" type="button" onclick={clearFilters}>Clear</button></p>
              {:else}
                <p class="empty-note">Highlight or note a passage in the Reader to see it here.</p>
              {/if}
            </div>

            {#if visibleError}
              <p class="note-error">{visibleError}</p>
            {/if}
          {:else if openThread}
            {@render passageDetail()}

            <!-- RFC 0073 (R2.4): the other conversations stay reachable from the
                 same panel as the composer, but collapsed by default — the thread
                 you are reading needs the height more than the list does. -->
            <div class="row section-title">
              <button class="list-toggle" type="button" aria-expanded={!chatListCollapsed} onclick={toggleChatList}>
                {#if chatListCollapsed}
                  <ChevronRight size={12} strokeWidth={1.75} aria-hidden="true" />
                {:else}
                  <ChevronDown size={12} strokeWidth={1.75} aria-hidden="true" />
                {/if}
                <span class="label hot">Chat</span>
              </button>
              <div class="flex1"></div>
              <span class="mono-dim">{chatCount}</span>
            </div>

            {#if !chatListCollapsed}
              {@render chatListBody()}
            {/if}

            {#if visibleError}
              <p class="note-error">{visibleError}</p>
            {/if}
          {:else}
            <div class="row section-title">
              <span class="label hot">Chat</span>
              <div class="flex1"></div>
              <span class="mono-dim">{chatCount}</span>
            </div>

            {@render chatListBody()}

            {#if visibleError}
              <p class="note-error">{visibleError}</p>
            {/if}
          {/if}
        </section>
      {/if}
    </div>
  </div>

  <nav class="section-rail" aria-label="Reader sections">
    <button class="rail-btn" class:active={activeSection === "info"} type="button" title="Info" aria-label="Info" onclick={() => (activeSection = "info")}>
      <Info size={16} strokeWidth={1.75} aria-hidden="true" />
    </button>
    <button class="rail-btn" class:active={activeSection === "notes"} type="button" title="Notes" aria-label="Notes" onclick={() => (activeSection = "notes")}>
      <StickyNote size={16} strokeWidth={1.75} aria-hidden="true" />
    </button>
    <button class="rail-btn" class:active={activeSection === "chat"} type="button" title="Chat" aria-label="Chat" onclick={() => (activeSection = "chat")}>
      <MessageSquare size={16} strokeWidth={1.75} aria-hidden="true" />
    </button>
  </nav>
</aside>

<style>
  .reader-inspector {
    width: 100%;
    height: 100%;
    flex-shrink: 0;
    display: flex;
    flex-direction: row;
    overflow: hidden;
    background: var(--panel);
  }

  .inspector-body {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }

  /* RFC 0071: the right-edge icon rail switches Info / Notes / Chat. */
  .section-rail {
    flex-shrink: 0;
    width: 40px;
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 6px 0;
    border-left: 1px solid var(--border);
    background: var(--bg-1);
  }

  .rail-btn {
    width: 40px;
    height: 38px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    border: 0;
    border-left: 2px solid transparent;
    background: transparent;
    color: var(--fg-3);
    cursor: pointer;
  }

  .rail-btn:hover {
    color: var(--fg-1);
  }

  .rail-btn.active {
    border-left-color: var(--amber);
    background: rgba(242, 169, 59, 0.06);
    color: var(--amber);
  }

  header {
    flex-shrink: 0;
    padding: 10px 14px;
    background: var(--bg-1);
    font-size: 10px;
  }

  .paper-name {
    margin-bottom: 2px;
    color: var(--amber);
    font-size: 11px;
  }

  .tab-panel {
    min-height: 0;
    flex: 1;
    overflow: auto;
  }

  section {
    padding: 14px 14px 0;
  }

  .section-title {
    gap: 8px;
    align-items: center;
  }

  /* RFC 0073 (R2.4): the whole label is the collapse target, so it reads as one
     disclosure control rather than a chevron sitting next to a heading. */
  .list-toggle {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    padding: 0;
    border: 0;
    background: none;
    color: var(--fg-3);
    cursor: pointer;
  }

  .list-toggle:hover {
    color: var(--amber);
  }

  blockquote {
    margin: 8px 0 0;
    padding: 0 0 0 8px;
    border-left: 2px solid var(--amber-mid);
    color: var(--fg-2);
    font-size: 10.5px;
    line-height: 1.45;
  }

  .swatch-row {
    margin-top: 8px;
    gap: 6px;
    align-items: center;
  }

  .swatch {
    width: 20px;
    height: 20px;
    flex-shrink: 0;
    border: 1px solid var(--border-2);
    border-radius: 50%;
    padding: 0;
    cursor: pointer;
  }

  .swatch:hover {
    border-color: var(--fg-3);
  }

  .swatch.active {
    border-color: var(--fg-1);
    box-shadow: 0 0 0 1px var(--fg-1);
  }

  .note-field {
    margin-top: 10px;
  }

  .note-field-head {
    align-items: center;
    justify-content: space-between;
    margin-bottom: 4px;
  }

  .note-field textarea {
    min-height: 48px;
  }

  textarea {
    width: 100%;
    min-height: 72px;
    resize: vertical;
    padding: 7px;
    border: 1px solid var(--border-2);
    outline: none;
    background: var(--bg);
    color: var(--fg-1);
    font: inherit;
    font-size: 11px;
    line-height: 1.45;
  }

  textarea:focus {
    border-color: var(--cyan);
  }

  .note-actions {
    justify-content: flex-end;
    gap: 6px;
    margin-top: 8px;
  }

  button:disabled {
    cursor: default;
    opacity: 0.45;
  }

  .empty-note,
  .note-error {
    margin: 8px 0 0;
    color: var(--fg-3);
    font-size: 10.5px;
    line-height: 1.45;
  }

  .note-error {
    color: var(--red);
  }

  .note-icon {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 20px;
    height: 20px;
    flex-shrink: 0;
    border: 1px solid transparent;
    background: transparent;
    color: var(--fg-3);
    font: inherit;
    font-size: 14px;
    line-height: 1;
    cursor: pointer;
  }

  .note-icon:hover {
    border-color: var(--border-2);
    background: rgba(242, 169, 59, 0.08);
    color: var(--amber);
  }

  .note-icon.remove:hover {
    border-color: var(--border-2);
    background: rgba(227, 88, 74, 0.08);
    color: var(--red);
  }

  /* Threads + Pins */
  .link-btn {
    border: 0;
    background: transparent;
    color: var(--amber);
    font: inherit;
    font-size: 10px;
    cursor: pointer;
    padding: 0;
  }

  .thread-title {
    margin: 8px 0 0;
    color: var(--fg);
    font-size: 12px;
    line-height: 1.35;
  }

  .rename-input {
    width: 100%;
    margin-top: 8px;
    height: 26px;
    border: 1px solid var(--border-2);
    outline: none;
    background: var(--bg);
    color: var(--fg-1);
    padding: 0 7px;
    font: inherit;
    font-size: 11px;
  }

  .rename-input:focus {
    border-color: var(--cyan);
  }

  .thread-list {
    margin-top: 8px;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .thread-row {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    padding: 7px 8px;
    border: 1px solid var(--border);
    background: rgba(255, 255, 255, 0.015);
    color: var(--fg-1);
    font: inherit;
    font-size: 11px;
    text-align: left;
    cursor: pointer;
  }

  .thread-row:hover {
    border-color: var(--amber-dim);
    background: rgba(242, 169, 59, 0.05);
  }

  .thread-row-title {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .color-chip {
    width: 9px;
    height: 9px;
    flex-shrink: 0;
    border-radius: 50%;
  }

  .badge {
    display: inline-flex;
    align-items: center;
    flex-shrink: 0;
    color: var(--fg-3);
    font-size: 10px;
    line-height: 1;
  }

  .thread-view {
    display: flex;
    flex-direction: column;
    gap: 8px;
    margin-top: 10px;
  }

  .entry {
    padding: 7px 8px;
    border: 1px solid var(--border);
  }

  .entry.note {
    border-color: var(--amber-dim);
    background: rgba(242, 169, 59, 0.05);
  }

  .entry.question {
    border-color: var(--border-2);
    background: rgba(107, 160, 168, 0.05);
  }

  .entry.answer {
    border-color: var(--border-2);
    background: rgba(255, 255, 255, 0.015);
  }

  .entry-head {
    align-items: center;
    gap: 6px;
  }

  .entry p {
    margin: 4px 0 0;
    color: var(--fg-1);
    font-size: 11px;
    line-height: 1.5;
    white-space: pre-wrap;
  }

  .entry-context {
    margin-top: 6px;
    font-size: 9px;
  }

  /* RFC 0077 */
  .context-panel {
    margin-bottom: 8px;
    padding: 6px 8px;
    border: 1px solid var(--border);
    background: var(--bg-1);
  }

  .context-head {
    width: 100%;
    gap: 4px;
    padding: 0;
    border: 0;
    background: transparent;
    color: var(--fg-2);
    font: inherit;
    font-size: 10px;
    text-align: left;
    cursor: pointer;
  }

  .context-warn {
    color: var(--red);
  }

  /* The agent's additions are visible and attributed, never silent. */
  .origin-tag {
    flex-shrink: 0;
    padding: 0 3px;
    border: 1px solid var(--border-2);
    color: var(--fg-3);
    font-size: 9px;
  }

  .context-note {
    margin-bottom: 4px;
    font-size: 9px;
  }

  .context-item {
    gap: 6px;
    font-size: 10px;
  }

  /* Reported, never silently dropped — a shrinking context you cannot see is
     worse than a visible hole. */
  .context-item.unresolved .context-label {
    color: var(--red);
  }

  .context-label {
    overflow: hidden;
    color: var(--fg-2);
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .references {
    margin-top: 6px;
    padding-top: 5px;
    border-top: 1px solid var(--border);
  }

  .ref-title {
    margin-bottom: 3px;
    font-size: 9px;
  }

  .ref-row {
    display: flex;
    gap: 6px;
    align-items: flex-start;
    padding: 2px 0;
    font-size: 10px;
  }

  .ref-open {
    display: block;
    flex: 1;
    min-width: 0;
    padding: 0;
    border: 0;
    background: transparent;
    font: inherit;
    font-size: 10px;
    text-align: left;
    cursor: pointer;
  }

  .ref-top {
    gap: 6px;
  }

  .ref-preview {
    display: -webkit-box;
    overflow: hidden;
    color: var(--fg-3);
    line-height: 1.35;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 2;
    line-clamp: 2;
  }

  .ref-open:hover .ref-preview {
    color: var(--fg-2);
  }

  .ref-handle {
    flex-shrink: 0;
    color: var(--amber);
  }

  .working {
    display: flex;
    gap: 6px;
    align-items: baseline;
    margin: 0;
    color: var(--fg-3);
    font-size: 10px;
  }

  /* A glyph that moves. A static "thinking…" is indistinguishable from a hang,
     which is the thing this line exists to rule out. */
  .spin {
    flex-shrink: 0;
    color: var(--amber);
  }

  .working-query {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .working-count {
    flex-shrink: 0;
    margin-left: auto;
    color: var(--amber-dim);
  }

  .drawer-toggle {
    display: block;
    margin-top: 6px;
    padding: 0;
    border: 0;
    background: transparent;
    font: inherit;
    font-size: 9px;
    text-align: left;
    cursor: pointer;
  }

  .drawer-row {
    gap: 6px;
    padding-left: 10px;
    font-size: 10px;
  }

  /* Dimmed, not hidden: the agent read it and passed on it, and that is worth
     being able to see without it competing with the actual references. */
  .drawer-row.uncited {
    opacity: 0.55;
  }

  .ref-where {
    overflow: hidden;
    color: var(--fg-2);
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .ref-open:hover .ref-where {
    color: var(--fg-1);
  }

  .keep-chip {
    padding: 0 4px;
    border: 1px solid var(--border-2);
    background: transparent;
    color: var(--amber);
    font: inherit;
    font-size: 9px;
    cursor: pointer;
  }

  .keep-chip:hover {
    border-color: var(--border-hot);
    background: var(--bg-2);
  }

  .pin-btn {
    flex-shrink: 0;
    border: 0;
    background: transparent;
    color: var(--fg-3);
    font: inherit;
    font-size: 13px;
    line-height: 1;
    cursor: pointer;
  }

  .pin-btn.pinned {
    color: var(--amber);
  }

  .thread-input {
    margin-top: 10px;
  }

  /* Annotations panel (RFC 0062) */
  .ask-paper-row {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    margin-top: 10px;
    padding: 8px;
    border: 1px solid var(--border-2);
    background: rgba(107, 160, 168, 0.05);
    color: var(--fg-1);
    font: inherit;
    font-size: 11px;
    text-align: left;
    cursor: pointer;
  }

  .ask-paper-row:hover {
    border-color: var(--cyan);
    color: var(--cyan);
  }

  .ask-paper-title {
    flex: 1;
    min-width: 0;
  }

  .filter-control {
    margin-top: 10px;
  }

  .filter-head {
    align-items: center;
    gap: 8px;
  }

  .filter-toggle {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    padding: 3px 9px;
    border: 1px solid var(--border-2);
    border-radius: 4px;
    background: transparent;
    color: var(--fg-2);
    font: inherit;
    font-size: 10px;
    cursor: pointer;
  }

  .filter-toggle:hover,
  .filter-toggle.on {
    border-color: var(--amber-dim);
    color: var(--amber);
  }

  .filter-panel {
    margin-top: 8px;
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 8px;
    border: 1px solid var(--border);
    background: rgba(255, 255, 255, 0.015);
  }

  .filter-group {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .filter-label {
    color: var(--fg-3);
    font-size: 9px;
    letter-spacing: 0.06em;
    text-transform: uppercase;
  }

  .filter-options {
    gap: 5px;
    align-items: center;
    flex-wrap: wrap;
  }

  .chip-btn {
    display: inline-flex;
    align-items: center;
    gap: 3px;
    padding: 3px 8px;
    border: 1px solid var(--border-2);
    border-radius: 999px;
    background: transparent;
    color: var(--fg-3);
    font: inherit;
    font-size: 10px;
    cursor: pointer;
  }

  .chip-btn:hover {
    color: var(--fg-1);
  }

  .chip-btn.on {
    border-color: var(--amber);
    background: rgba(242, 169, 59, 0.08);
    color: var(--amber);
  }

  .color-dot {
    width: 16px;
    height: 16px;
    flex-shrink: 0;
    border: 1px solid var(--border-2);
    border-radius: 50%;
    padding: 0;
    cursor: pointer;
  }

  .color-dot.none {
    background:
      linear-gradient(45deg, transparent 45%, var(--fg-3) 45%, var(--fg-3) 55%, transparent 55%),
      var(--bg);
  }

  .color-dot.on {
    border-color: var(--fg-1);
    box-shadow: 0 0 0 1px var(--fg-1);
  }

  .metadata {
    display: flex;
    flex-direction: column;
    gap: 14px;
    padding-bottom: 14px;
  }

  .meta-grid {
    display: grid;
    grid-template-columns: 48px 1fr;
    gap: 4px 8px;
    margin-top: 6px;
    color: var(--fg-3);
    font-size: 10px;
  }

  .meta-grid strong {
    min-width: 0;
    color: var(--fg-2);
    font-weight: 400;
  }
</style>
