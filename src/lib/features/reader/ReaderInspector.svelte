<script lang="ts">
  import {
    addChatNote,
    askChatThreadStreamed,
    createChatThread,
    deleteChatThread,
    getChatThread,
    listChatThreads,
    listPinnedChatEntries,
    openChatDocumentThread,
    renameChatThread,
    setChatEntryPinned,
  } from "$lib/bridge/chat";
  import type {
    ChatEntry,
    ChatScope,
    ChatThreadSummary,
    ChatThreadView,
    PinnedHighlight,
    ThreadAnchor,
  } from "$lib/domain/chat";
  import type { NoteAnchorKind, PaperNote } from "$lib/domain/library";
  import type { ReaderDocument, ReaderTextSelection } from "$lib/domain/reader";

  type InspectorTab = "notes" | "threads" | "pins" | "meta";

  let {
    document,
    notes,
    activeNoteId,
    noteDraft,
    notesEnabled,
    noteError,
    isLoadingNotes,
    onSaveNote,
    onCancelNoteDraft,
    onDeleteNote,
    onUpdateNote,
    onActivateNote,
  }: {
    document: ReaderDocument;
    notes: PaperNote[];
    activeNoteId: string | null;
    noteDraft: ReaderTextSelection | null;
    notesEnabled: boolean;
    noteError: string;
    isLoadingNotes: boolean;
    onSaveNote: (body: string) => Promise<void>;
    onCancelNoteDraft: () => void;
    onDeleteNote: (noteId: string) => Promise<void>;
    onUpdateNote: (noteId: string, body: string) => Promise<void>;
    onActivateNote: (noteId: string) => void;
  } = $props();

  let noteBody = $state("");
  let isSaving = $state(false);
  let deletingNoteId = $state<string | null>(null);
  let editingNoteId = $state<string | null>(null);
  let editBody = $state("");
  let isUpdating = $state(false);
  let activeTab = $state<InspectorTab>("notes");
  let inspectorElement = $state<HTMLElement | null>(null);
  const canSave = $derived(Boolean(noteDraft && noteBody.trim() && !isSaving));
  const canUpdate = $derived(Boolean(editingNoteId && editBody.trim() && !isUpdating));

  $effect(() => {
    if (noteDraft) {
      activeTab = "notes";
    }

    noteBody = "";
  });

  $effect(() => {
    if (!noteDraft) {
      return;
    }

    const handlePointerDown = (event: PointerEvent) => {
      if (!inspectorElement || inspectorElement.contains(event.target as Node)) {
        return;
      }

      cancelEmptyDraft();
    };

    window.document.addEventListener("pointerdown", handlePointerDown);

    return () => {
      window.document.removeEventListener("pointerdown", handlePointerDown);
    };
  });

  async function saveNote() {
    if (!canSave) {
      return;
    }

    isSaving = true;
    try {
      await onSaveNote(noteBody);
    } finally {
      isSaving = false;
    }
  }

  function cancelDraft() {
    noteBody = "";
    onCancelNoteDraft();
  }

  function cancelEmptyDraft() {
    if (!noteBody.trim()) {
      cancelDraft();
    }
  }

  function handleNoteKeydown(event: KeyboardEvent) {
    if (event.key === "Escape") {
      event.preventDefault();
      cancelEmptyDraft();
      return;
    }

    if (event.key !== "Enter" || event.shiftKey) {
      return;
    }

    event.preventDefault();
    void saveNote();
  }

  async function deleteNote(noteId: string) {
    if (deletingNoteId) {
      return;
    }

    deletingNoteId = noteId;
    try {
      await onDeleteNote(noteId);
    } finally {
      deletingNoteId = null;
    }
  }

  function startEdit(note: PaperNote) {
    editingNoteId = note.id;
    editBody = note.body;
  }

  function cancelEdit() {
    editingNoteId = null;
    editBody = "";
  }

  async function updateNote() {
    if (!editingNoteId || !canUpdate) {
      return;
    }

    isUpdating = true;
    try {
      await onUpdateNote(editingNoteId, editBody);
      cancelEdit();
    } catch {
      // ReaderView owns the visible error message; keep the draft open.
    } finally {
      isUpdating = false;
    }
  }

  function handleEditKeydown(event: KeyboardEvent) {
    if (event.key === "Escape") {
      event.preventDefault();
      cancelEdit();
      return;
    }

    if (event.key !== "Enter" || event.shiftKey) {
      return;
    }

    event.preventDefault();
    void updateNote();
  }

  function noteAnchorLabel(note: { selectedText: string; anchorKind?: NoteAnchorKind; pageIndex?: number }) {
    if (note.selectedText.trim()) {
      return note.selectedText;
    }

    if (note.anchorKind === "pdf_rect" && note.pageIndex !== undefined) {
      return `PDF region, page ${note.pageIndex + 1}`;
    }

    return "Untitled note anchor";
  }

  // --- Threads + Pins: anchored chat (RFC 0033) ---
  let threads = $state<ChatThreadSummary[]>([]);
  let pins = $state<PinnedHighlight[]>([]);
  let openThread = $state<ChatThreadView | null>(null);
  let threadsLoadedPaperId = $state<string | null>(null);
  let pinsLoadedPaperId = $state<string | null>(null);
  let isLoadingThreads = $state(false);
  let isLoadingPins = $state(false);
  let chatError = $state("");
  let chatInput = $state("");
  let isBusy = $state(false);
  let pendingQuestion = $state<string | null>(null);
  let streamingAnswer = $state<string | null>(null);
  let renaming = $state(false);
  let renameTitle = $state("");

  // Chat is durable, so it is only available once the paper is in a Vault —
  // the same boundary that gates Notes.
  const chatEnabled = $derived(notesEnabled);
  const scope = $derived<ChatScope>({ kind: "paper", paperId: document.paperId });
  const documentThread = $derived(threads.find((thread) => thread.anchor.kind === "document"));
  const anchoredThreads = $derived(threads.filter((thread) => thread.anchor.kind !== "document"));
  const canSubmit = $derived(Boolean(openThread && chatInput.trim() && !isBusy));

  // Reset chat state whenever the open paper changes.
  $effect(() => {
    void document.paperId;
    openThread = null;
    threadsLoadedPaperId = null;
    pinsLoadedPaperId = null;
    chatError = "";
    chatInput = "";
    renaming = false;
  });

  $effect(() => {
    const paperId = document.paperId;
    if (activeTab !== "threads" || !chatEnabled || openThread) {
      return;
    }
    if (threadsLoadedPaperId === paperId || isLoadingThreads) {
      return;
    }
    void loadThreads(paperId);
  });

  $effect(() => {
    const paperId = document.paperId;
    if (activeTab !== "pins" || !chatEnabled) {
      return;
    }
    if (pinsLoadedPaperId === paperId || isLoadingPins) {
      return;
    }
    void loadPins(paperId);
  });

  async function loadThreads(paperId: string) {
    isLoadingThreads = true;
    chatError = "";
    try {
      const list = await listChatThreads({ kind: "paper", paperId });
      if (document.paperId === paperId) {
        threads = list;
      }
    } catch (error) {
      if (document.paperId === paperId) {
        chatError = String(error);
      }
    } finally {
      if (document.paperId === paperId) {
        isLoadingThreads = false;
        threadsLoadedPaperId = paperId;
      }
    }
  }

  async function loadPins(paperId: string) {
    isLoadingPins = true;
    try {
      const list = await listPinnedChatEntries({ kind: "paper", paperId });
      if (document.paperId === paperId) {
        pins = list;
      }
    } catch (error) {
      if (document.paperId === paperId) {
        chatError = String(error);
      }
    } finally {
      if (document.paperId === paperId) {
        isLoadingPins = false;
        pinsLoadedPaperId = paperId;
      }
    }
  }

  async function openDocumentThread() {
    chatError = "";
    try {
      openThread = await openChatDocumentThread(scope);
      threadsLoadedPaperId = null;
    } catch (error) {
      chatError = String(error);
    }
  }

  async function openThreadById(threadId: string) {
    chatError = "";
    try {
      openThread = await getChatThread(threadId);
    } catch (error) {
      chatError = String(error);
    }
  }

  function backToThreadList() {
    openThread = null;
    renaming = false;
    threadsLoadedPaperId = null;
  }

  function anchorFromDraft(draft: ReaderTextSelection): ThreadAnchor {
    if (draft.anchorKind === "pdf_rect") {
      return {
        kind: "pdfRect",
        sourceId: draft.sourceId,
        pageIndex: draft.pageIndex ?? 0,
        rectsJson: draft.rectsJson ?? "[]",
        selectedText: draft.selectedText,
      };
    }
    return {
      kind: "textOffset",
      sourceId: draft.sourceId,
      startOffset: draft.startOffset,
      endOffset: draft.endOffset,
      selectedText: draft.selectedText,
    };
  }

  async function askFromSelection() {
    if (!noteDraft || !chatEnabled || isBusy) {
      return;
    }

    isBusy = true;
    chatError = "";
    try {
      const view = await createChatThread(scope, anchorFromDraft(noteDraft));
      onCancelNoteDraft();
      openThread = view;
      threadsLoadedPaperId = null;
      activeTab = "threads";
    } catch (error) {
      chatError = String(error);
    } finally {
      isBusy = false;
    }
  }

  async function refreshOpenThread() {
    if (!openThread) {
      return;
    }
    const threadId = openThread.thread.id;
    const view = await getChatThread(threadId);
    if (openThread?.thread.id === threadId) {
      openThread = view;
    }
  }

  function handleThreadKeydown(event: KeyboardEvent) {
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

    const threadId = openThread.thread.id;
    isBusy = true;
    chatError = "";
    pendingQuestion = body;
    streamingAnswer = "";
    chatInput = "";
    try {
      const view = await askChatThreadStreamed(threadId, body, (text) => {
        if (openThread?.thread.id === threadId) {
          streamingAnswer = (streamingAnswer ?? "") + text;
        }
      });
      if (openThread?.thread.id === threadId) {
        openThread = view;
      }
    } catch (error) {
      chatError = String(error);
      chatInput = body;
    } finally {
      isBusy = false;
      pendingQuestion = null;
      streamingAnswer = null;
    }
  }

  async function addNote() {
    const body = chatInput.trim();
    if (!body || isBusy || !openThread) {
      return;
    }

    const threadId = openThread.thread.id;
    isBusy = true;
    chatError = "";
    try {
      const view = await addChatNote(threadId, body);
      if (openThread?.thread.id === threadId) {
        openThread = view;
        chatInput = "";
      }
    } catch (error) {
      chatError = String(error);
    } finally {
      isBusy = false;
    }
  }

  async function togglePin(entry: ChatEntry) {
    chatError = "";
    try {
      await setChatEntryPinned(entry.id, !entry.pinned);
      pinsLoadedPaperId = null;
      await refreshOpenThread();
    } catch (error) {
      chatError = String(error);
    }
  }

  async function deleteOpenThread() {
    if (!openThread) {
      return;
    }
    const threadId = openThread.thread.id;
    try {
      await deleteChatThread(threadId);
      openThread = null;
      threadsLoadedPaperId = null;
      pinsLoadedPaperId = null;
    } catch (error) {
      chatError = String(error);
    }
  }

  function startRename() {
    if (openThread) {
      renameTitle = openThread.thread.title;
      renaming = true;
    }
  }

  async function commitRename() {
    if (!openThread || !renameTitle.trim()) {
      renaming = false;
      return;
    }
    const threadId = openThread.thread.id;
    try {
      await renameChatThread(threadId, renameTitle);
      await refreshOpenThread();
      threadsLoadedPaperId = null;
    } catch (error) {
      chatError = String(error);
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

  function chatContextLabel(entry: ChatEntry) {
    const summary = entry.contextSummary;
    if (!summary) {
      return "";
    }
    if (summary.includedChars === 0) {
      return "Context: no paper text available";
    }
    const chars = summary.includedChars.toLocaleString();
    return `Context: ${chars} chars${summary.truncated ? " · truncated" : ""}`;
  }

  const tabs: Array<{ id: InspectorTab; label: string }> = [
    { id: "notes", label: "Notes" },
    { id: "threads", label: "Threads" },
    { id: "pins", label: "Pins" },
    { id: "meta", label: "Meta" },
  ];
</script>

<aside bind:this={inspectorElement} class="reader-inspector hair-l">
  <header class="hair-b">
    <div class="paper-name truncate">{document.title}</div>
    <div class="mono-dim">paper / selected / reader</div>
  </header>

  <nav class="inspector-tabs row hair-b" aria-label="Reader Inspector">
    {#each tabs as tab}
      <button class:active={activeTab === tab.id} type="button" onclick={() => (activeTab = tab.id)}>
        {tab.label}
      </button>
    {/each}
  </nav>

  <div class="tab-panel">
    {#if activeTab === "notes"}
      <section>
        <div class="row section-title">
          <span class="label hot">Notes</span>
          <div class="flex1"></div>
          <span class="mono-dim">{notes.length}</span>
        </div>

        {#if notesEnabled}
          {#if noteDraft}
            <div class="note-draft">
              <div class="label">{noteDraft.anchorKind === "pdf_rect" ? "PDF anchor" : "Selected quote"}</div>
              <blockquote>{noteAnchorLabel(noteDraft)}</blockquote>
              <textarea
                bind:value={noteBody}
                aria-label="Note body"
                placeholder="Write a note..."
                rows="5"
                onkeydown={handleNoteKeydown}
              ></textarea>
              <div class="row note-actions">
                <button class="btn primary" type="button" disabled={!canSave} onclick={() => void saveNote()}>
                  {isSaving ? "Saving" : "Save"}
                </button>
                <button class="btn ghost" type="button" disabled={isBusy} onclick={() => void askFromSelection()}>
                  Ask
                </button>
                <button class="btn ghost" type="button" disabled={isSaving} onclick={cancelDraft}>
                  Cancel
                </button>
              </div>
            </div>
          {:else}
            <p class="empty-note">Select text in the Reader to add a note or ask about it.</p>
          {/if}

          {#if noteError}
            <p class="note-error">{noteError}</p>
          {/if}

          <div class="saved-notes">
            <div class="label">Saved notes</div>
            {#if isLoadingNotes}
              <p class="empty-note">Loading notes...</p>
            {:else if notes.length}
              {#each notes as note}
                <article
                  class="saved-note"
                  class:active={activeNoteId === note.id}
                >
                  {#if editingNoteId !== note.id}
                    <button
                      class="note-recall"
                      type="button"
                      aria-label="show note anchor"
                      onclick={() => onActivateNote(note.id)}
                    ></button>
                  {/if}
                  <div class="row saved-note-head">
                    <blockquote>{noteAnchorLabel(note)}</blockquote>
                    <div class="row note-buttons">
                      <button
                        class="note-icon"
                        type="button"
                        aria-label="edit note"
                        disabled={Boolean(editingNoteId && editingNoteId !== note.id) || isUpdating}
                        onclick={(event) => {
                          event.stopPropagation();
                          startEdit(note);
                        }}
                      >
                        ✎
                      </button>
                      <button
                        class="note-icon remove"
                        type="button"
                        aria-label="remove note"
                        disabled={deletingNoteId === note.id || isUpdating}
                        onclick={(event) => {
                          event.stopPropagation();
                          void deleteNote(note.id);
                        }}
                      >
                        -
                      </button>
                    </div>
                  </div>
                  {#if editingNoteId === note.id}
                    <textarea
                      bind:value={editBody}
                      aria-label="Edit note body"
                      rows="4"
                      onclick={(event) => event.stopPropagation()}
                      onkeydown={handleEditKeydown}
                    ></textarea>
                    <div class="row note-actions">
                      <button class="btn primary" type="button" disabled={!canUpdate} onclick={() => void updateNote()}>
                        {isUpdating ? "Saving" : "Save"}
                      </button>
                    </div>
                  {:else}
                    <p>{note.body}</p>
                  {/if}
                  <div class="mono-dim">{note.updatedAt}</div>
                </article>
              {/each}
            {:else}
              <p class="empty-note">No saved notes yet.</p>
            {/if}
          </div>
        {:else}
          <p class="empty-note">Add this paper to a Vault before saving notes.</p>
        {/if}
      </section>
    {:else if activeTab === "threads"}
      <section>
        {#if !chatEnabled}
          <div class="row section-title"><span class="label hot">Threads</span></div>
          <p class="empty-note">Add this paper to a Vault to chat with it.</p>
        {:else if openThread}
          <div class="row section-title">
            <button class="link-btn" type="button" onclick={backToThreadList}>‹ Threads</button>
            <div class="flex1"></div>
            <button class="note-icon" type="button" aria-label="rename thread" onclick={startRename}>✎</button>
            <button class="note-icon remove" type="button" aria-label="delete thread" onclick={() => void deleteOpenThread()}>-</button>
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
          {:else}
            <h3 class="thread-title">{openThread.thread.title}</h3>
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
                <p>{entry.body}</p>
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
                <p>{streamingAnswer ? streamingAnswer : "…"}</p>
              </div>
            {/if}

            {#if !openThread.entries.length && pendingQuestion === null}
              <p class="empty-note">Write a note or ask a question to start this thread.</p>
            {/if}

            {#if chatError}
              <p class="note-error">{chatError}</p>
            {/if}
          </div>

          <div class="thread-input">
            <textarea
              bind:value={chatInput}
              aria-label="Note or question"
              placeholder="Note this passage, or ask… (Enter asks, Shift+Enter newline)"
              rows="3"
              disabled={isBusy}
              onkeydown={handleThreadKeydown}
            ></textarea>
            <div class="row note-actions">
              <button class="btn ghost" type="button" disabled={!canSubmit} onclick={() => void addNote()}>Note</button>
              <button class="btn primary" type="button" disabled={!canSubmit} onclick={() => void ask()}>
                {isBusy ? "…" : "Ask"}
              </button>
            </div>
          </div>
        {:else}
          <div class="row section-title">
            <span class="label hot">Threads</span>
            <div class="flex1"></div>
            <span class="mono-dim">{threads.length}</span>
          </div>

          <div class="thread-list">
            <button class="thread-row" type="button" onclick={() => void openDocumentThread()}>
              <span class="thread-row-title">Whole paper</span>
              {#if documentThread}
                <span class="mono-dim">{documentThread.pinnedCount > 0 ? "★ " : ""}{documentThread.entryCount}</span>
              {/if}
            </button>

            {#if isLoadingThreads}
              <p class="empty-note">Loading threads…</p>
            {:else}
              {#each anchoredThreads as thread}
                <button class="thread-row" type="button" onclick={() => void openThreadById(thread.id)}>
                  <span class="thread-row-title">{thread.title}</span>
                  <span class="mono-dim">{thread.pinnedCount > 0 ? "★ " : ""}{thread.entryCount}</span>
                </button>
              {/each}
            {/if}
          </div>

          <p class="empty-note">Select text in the Reader and choose “Ask” to start a thread about a passage.</p>

          {#if chatError}
            <p class="note-error">{chatError}</p>
          {/if}
        {/if}
      </section>
    {:else if activeTab === "pins"}
      <section>
        <div class="row section-title">
          <span class="label hot">Pins</span>
          <div class="flex1"></div>
          <span class="mono-dim">{pins.length}</span>
        </div>

        {#if !chatEnabled}
          <p class="empty-note">Add this paper to a Vault to collect highlights.</p>
        {:else if isLoadingPins}
          <p class="empty-note">Loading highlights…</p>
        {:else if pins.length}
          <div class="pins-list">
            {#each pins as pin}
              <button
                class="pin-item"
                type="button"
                onclick={() => {
                  activeTab = "threads";
                  void openThreadById(pin.entry.threadId);
                }}
              >
                <p>{pin.entry.body}</p>
                <div class="pin-source mono-dim">{pin.entry.kind === "answer" ? "AI" : "Note"} · {pin.threadTitle}</div>
              </button>
            {/each}
          </div>
        {:else}
          <p class="empty-note">Pin a note or answer in a thread and it shows up here.</p>
        {/if}
      </section>
    {:else}
      <section class="metadata">
        <div class="label hot">Metadata</div>
        <div class="meta-grid">
          <span>id</span><strong>{document.identifier}</strong>
          <span>venue</span><strong>{document.venue} {document.year}</strong>
          <span>cite</span><strong>{document.citationKey}</strong>
          <span>marks</span><strong>{document.marks.length}</strong>
        </div>
      </section>
    {/if}
  </div>
</aside>

<style>
  .reader-inspector {
    width: 320px;
    flex-shrink: 0;
    display: flex;
    flex-direction: column;
    overflow: hidden;
    background: var(--panel);
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

  .inspector-tabs {
    height: 30px;
    flex-shrink: 0;
    background: var(--bg);
  }

  .inspector-tabs button {
    flex: 1;
    border: 0;
    border-right: 1px solid var(--border);
    border-bottom: 2px solid transparent;
    background: transparent;
    color: var(--fg-3);
    font: inherit;
    font-size: 10px;
    cursor: pointer;
  }

  .inspector-tabs button:last-child {
    border-right: 0;
  }

  .inspector-tabs button:hover {
    color: var(--fg-1);
  }

  .inspector-tabs button.active {
    border-bottom-color: var(--amber);
    background: rgba(242, 169, 59, 0.06);
    color: var(--amber);
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

  .note-draft {
    margin-top: 8px;
    padding: 8px;
    border: 1px solid var(--border-2);
    background: rgba(107, 160, 168, 0.04);
  }

  blockquote {
    margin: 6px 0;
    padding: 0 0 0 8px;
    border-left: 2px solid var(--amber-mid);
    color: var(--fg-2);
    font-size: 10.5px;
    line-height: 1.45;
  }

  textarea {
    width: 100%;
    min-height: 86px;
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

  .saved-notes {
    margin-top: 10px;
  }

  .saved-note {
    position: relative;
    margin-top: 8px;
    padding: 8px;
    border: 1px solid var(--border);
    background: rgba(255, 255, 255, 0.015);
    cursor: pointer;
  }

  .saved-note.active {
    border-color: var(--amber-dim);
    background: rgba(242, 169, 59, 0.055);
  }

  .note-recall {
    position: absolute;
    inset: 0;
    z-index: 0;
    border: 0;
    background: transparent;
    cursor: pointer;
  }

  .saved-note-head {
    position: relative;
    z-index: 1;
    align-items: flex-start;
    gap: 8px;
    pointer-events: none;
  }

  .saved-note-head blockquote {
    flex: 1;
    min-width: 0;
  }

  .note-buttons {
    gap: 4px;
    align-items: flex-start;
    pointer-events: auto;
  }

  .note-icon {
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

  .saved-note p {
    position: relative;
    z-index: 1;
    margin: 6px 0;
    color: var(--fg-1);
    font-size: 11px;
    line-height: 1.45;
    pointer-events: none;
  }

  .saved-note textarea,
  .saved-note .note-actions {
    position: relative;
    z-index: 1;
  }

  .saved-note .mono-dim {
    position: relative;
    z-index: 1;
    pointer-events: none;
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

  .pins-list {
    margin-top: 8px;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .pin-item {
    width: 100%;
    padding: 8px;
    border: 1px solid var(--border);
    background: rgba(255, 255, 255, 0.015);
    text-align: left;
    cursor: pointer;
  }

  .pin-item:hover {
    border-color: var(--amber-dim);
    background: rgba(242, 169, 59, 0.05);
  }

  .pin-item p {
    margin: 0;
    color: var(--fg-1);
    font-size: 11px;
    line-height: 1.45;
  }

  .pin-source {
    margin-top: 5px;
    font-size: 9px;
  }

  .metadata {
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
