<script lang="ts">
  import {
    addChatNote,
    askAtAnchorStreamed,
    askChatThreadStreamed,
    deleteChatThread,
    getChatThread,
    noteAtAnchor,
    renameChatThread,
    setChatEntryPinned,
  } from "$lib/bridge/chat";
  import {
    anchorSelectedText,
    type ChatEntry,
    type ChatScope,
    type ChatThreadSummary,
    type ChatThreadView,
    type PinnedHighlight,
    type ThreadAnchor,
  } from "$lib/domain/chat";
  import type { ReaderDocument, ReaderTextSelection } from "$lib/domain/reader";

  type InspectorTab = "threads" | "pins" | "meta";

  let {
    document,
    chatEnabled,
    threads,
    pins,
    selection,
    requestedThreadId,
    isLoadingChat,
    chatError,
    onReloadChat,
    onClearSelection,
    onConsumeRequestedThread,
  }: {
    document: ReaderDocument;
    chatEnabled: boolean;
    threads: ChatThreadSummary[];
    pins: PinnedHighlight[];
    selection: ReaderTextSelection | null;
    requestedThreadId: string | null;
    isLoadingChat: boolean;
    chatError: string;
    onReloadChat: () => void;
    onClearSelection: () => void;
    onConsumeRequestedThread: () => void;
  } = $props();

  let activeTab = $state<InspectorTab>("threads");
  // A thread with an empty id is *virtual*: it shows a passage's composer before
  // the first Note/Ask creates the row (RFC 0034 lazy threads).
  let openThread = $state<ChatThreadView | null>(null);
  let chatInput = $state("");
  let isBusy = $state(false);
  let pendingQuestion = $state<string | null>(null);
  let streamingAnswer = $state<string | null>(null);
  let error = $state("");
  let renaming = $state(false);
  let renameTitle = $state("");
  // Guards streamed deltas against paper switches / superseded asks.
  let askSequence = 0;
  let activeAskId = $state(0);

  const scope = $derived<ChatScope>({ kind: "paper", paperId: document.paperId });
  const documentThread = $derived(threads.find((thread) => thread.anchor.kind === "document"));
  const anchoredThreads = $derived(threads.filter((thread) => thread.anchor.kind !== "document"));
  const isVirtual = $derived(Boolean(openThread) && openThread!.thread.id === "");
  const openPassage = $derived(openThread ? anchorSelectedText(openThread.thread.anchor) : null);
  // For a selection thread the title defaults to the passage, so the quote block
  // alone says it — only show the heading when it adds something (a renamed
  // thread, or the whole-paper thread that has no passage).
  const showTitle = $derived(
    !openPassage || (openThread?.thread.title.trim() ?? "") !== openPassage.trim(),
  );
  const canSubmit = $derived(Boolean(openThread && chatInput.trim() && !isBusy));
  const visibleError = $derived(error || chatError);

  // Reset the open conversation when the paper changes.
  $effect(() => {
    void document.paperId;
    openThread = null;
    chatInput = "";
    error = "";
    renaming = false;
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
    activeTab = "threads";
  });

  // A clicked margin mark (or pin) requests a specific thread to open.
  $effect(() => {
    const threadId = requestedThreadId;
    if (!threadId) {
      return;
    }
    activeTab = "threads";
    void openThreadById(threadId);
    onConsumeRequestedThread();
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

  function openWholePaper() {
    activeTab = "threads";
    error = "";
    if (documentThread) {
      void openThreadById(documentThread.id);
    } else {
      openThread = virtualThread({ kind: "document" }, "Whole paper");
    }
  }

  async function openThreadById(threadId: string) {
    error = "";
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
    pendingQuestion = body;
    streamingAnswer = "";
    chatInput = "";
    try {
      const onDelta = (text: string) => {
        if (activeAskId === askId) {
          streamingAnswer = (streamingAnswer ?? "") + text;
        }
      };
      const view = virtual
        ? await askAtAnchorStreamed(scope, anchor, body, onDelta)
        : await askChatThreadStreamed(threadId, body, onDelta);
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
      }
    }
  }

  async function addNote() {
    const body = chatInput.trim();
    if (!body || isBusy || !openThread) {
      return;
    }

    const virtual = openThread.thread.id === "";
    const anchor = openThread.thread.anchor;
    const threadId = openThread.thread.id;
    isBusy = true;
    error = "";
    try {
      const view = virtual ? await noteAtAnchor(scope, anchor, body) : await addChatNote(threadId, body);
      openThread = view;
      chatInput = "";
      onReloadChat();
      if (virtual) {
        onClearSelection();
      }
    } catch (caught) {
      error = String(caught);
    } finally {
      isBusy = false;
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

  function chatContextLabel(entry: ChatEntry) {
    const summary = entry.contextSummary;
    if (!summary) {
      return "";
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

  const tabs: Array<{ id: InspectorTab; label: string }> = [
    { id: "threads", label: "Threads" },
    { id: "pins", label: "Pins" },
    { id: "meta", label: "Meta" },
  ];
</script>

<aside class="reader-inspector hair-l">
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
    {#if activeTab === "threads"}
      <section>
        {#if !chatEnabled}
          <div class="row section-title"><span class="label hot">Threads</span></div>
          <p class="empty-note">Add this paper to a Vault to chat with it.</p>
        {:else if openThread}
          <div class="row section-title">
            <button class="link-btn" type="button" onclick={backToThreadList}>‹ Threads</button>
            <div class="flex1"></div>
            {#if !isVirtual}
              <button class="note-icon" type="button" aria-label="rename thread" onclick={startRename}>✎</button>
            {/if}
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
          {:else if showTitle}
            <h3 class="thread-title">{openThread.thread.title}</h3>
          {/if}

          {#if openPassage}
            <blockquote>{openPassage}</blockquote>
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

            {#if visibleError}
              <p class="note-error">{visibleError}</p>
            {/if}
          </div>

          <div class="thread-input">
            <textarea
              bind:value={chatInput}
              aria-label="Note or question"
              placeholder="Note this passage, or ask… (Enter asks, Shift+Enter newline)"
              rows="3"
              disabled={isBusy}
              onkeydown={handleComposerKeydown}
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
            <button class="thread-row" type="button" onclick={openWholePaper}>
              <span class="thread-row-title">Whole paper</span>
              {#if documentThread}
                <span class="mono-dim">{documentThread.pinnedCount > 0 ? "★ " : ""}{documentThread.entryCount}</span>
              {/if}
            </button>

            {#if isLoadingChat}
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

          <p class="empty-note">Select text in the Reader and choose “Note” or “Ask” to start a thread about a passage.</p>

          {#if visibleError}
            <p class="note-error">{visibleError}</p>
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
        {:else if isLoadingChat}
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

  blockquote {
    margin: 8px 0 0;
    padding: 0 0 0 8px;
    border-left: 2px solid var(--amber-mid);
    color: var(--fg-2);
    font-size: 10.5px;
    line-height: 1.45;
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
