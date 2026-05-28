<script lang="ts">
  import type { PaperNote } from "$lib/domain/library";
  import type { ReaderDocument, ReaderTextSelection } from "$lib/domain/reader";

  type InspectorTab = "notes" | "lineage" | "ask" | "meta";

  let {
    document,
    notes,
    noteDraft,
    notesEnabled,
    noteError,
    isLoadingNotes,
    onSaveNote,
    onDeleteNote,
    onUpdateNote,
  }: {
    document: ReaderDocument;
    notes: PaperNote[];
    noteDraft: ReaderTextSelection | null;
    notesEnabled: boolean;
    noteError: string;
    isLoadingNotes: boolean;
    onSaveNote: (body: string) => Promise<void>;
    onDeleteNote: (noteId: string) => Promise<void>;
    onUpdateNote: (noteId: string, body: string) => Promise<void>;
  } = $props();

  let noteBody = $state("");
  let isSaving = $state(false);
  let deletingNoteId = $state<string | null>(null);
  let editingNoteId = $state<string | null>(null);
  let editBody = $state("");
  let isUpdating = $state(false);
  let activeTab = $state<InspectorTab>("notes");
  const canSave = $derived(Boolean(noteDraft && noteBody.trim() && !isSaving));
  const canUpdate = $derived(Boolean(editingNoteId && editBody.trim() && !isUpdating));

  $effect(() => {
    if (noteDraft) {
      activeTab = "notes";
    }

    noteBody = "";
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

  function handleNoteKeydown(event: KeyboardEvent) {
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

  const tabs: Array<{ id: InspectorTab; label: string }> = [
    { id: "notes", label: "Notes" },
    { id: "lineage", label: "Lineage" },
    { id: "ask", label: "Ask" },
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
              <div class="label">Selected quote</div>
              <blockquote>{noteDraft.selectedText}</blockquote>
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
              </div>
            </div>
          {:else}
            <p class="empty-note">Select text in the Reader to add a note.</p>
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
                <article class="saved-note">
                  <div class="row saved-note-head">
                    <blockquote>{note.selectedText}</blockquote>
                    <div class="row note-buttons">
                      <button
                        class="note-icon"
                        type="button"
                        aria-label="edit note"
                        disabled={Boolean(editingNoteId && editingNoteId !== note.id) || isUpdating}
                        onclick={() => startEdit(note)}
                      >
                        ✎
                      </button>
                      <button
                        class="note-icon remove"
                        type="button"
                        aria-label="remove note"
                        disabled={deletingNoteId === note.id || isUpdating}
                        onclick={() => void deleteNote(note.id)}
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
    {:else if activeTab === "lineage"}
      <section>
        <div class="row section-title">
          <span class="label hot">Lineage</span>
          <div class="flex1"></div>
          <span class="mono-dim">3 &lt;- / -&gt; 5</span>
        </div>

        <div class="lineage">
          <div class="label">Cites</div>
          <p><span>●</span> Bahdanau 2014 <em>soft alignment seed</em></p>
          <p><span>○</span> Luong 2015 <em>dot-product variant</em></p>
          <p><span>○</span> Cheng 2016 <em>intra-attention</em></p>

          <div class="label cited">Cited by</div>
          <p><span>●</span> Devlin 2019 <em>BERT</em></p>
          <p><span>●</span> Dosovitskiy 2021 <em>ViT</em></p>
          <p><span>○</span> Touvron 2023 <em>LLaMA</em></p>
        </div>

        <button class="btn ghost wide" type="button">Open graph</button>
      </section>
    {:else if activeTab === "ask"}
      <section>
        <div class="label hot">Ask this paper</div>
        <div class="ask-box">
          <div class="label">You</div>
          <p>Explain scaled dot-product attention vs additive attention.</p>
          <div class="answer">
            i0i will cite paragraph anchors here. For now this is a static Reader mock.
          </div>
          <div class="row ask-actions">
            <button class="btn primary" type="button">Run</button>
            <button class="btn ghost" type="button">Follow-up</button>
          </div>
        </div>
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
  }

  .lineage {
    margin-top: 8px;
    color: var(--fg-2);
    font-size: 10px;
    line-height: 1.6;
  }

  .lineage p {
    margin: 3px 0;
  }

  .lineage span {
    color: var(--amber);
  }

  .lineage em {
    margin-left: 4px;
    color: var(--fg-3);
    font-style: normal;
  }

  .cited {
    margin-top: 10px;
  }

  .wide {
    width: 100%;
    margin-top: 8px;
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
    margin-top: 8px;
    padding: 8px;
    border: 1px solid var(--border);
    background: rgba(255, 255, 255, 0.015);
  }

  .saved-note-head {
    align-items: flex-start;
    gap: 8px;
  }

  .saved-note-head blockquote {
    flex: 1;
    min-width: 0;
  }

  .note-buttons {
    gap: 4px;
    align-items: flex-start;
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
    margin: 6px 0;
    color: var(--fg-1);
    font-size: 11px;
    line-height: 1.45;
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

  .ask-box {
    margin-top: 8px;
    padding: 8px;
    border: 1px solid var(--amber-dim);
    background: rgba(242, 169, 59, 0.04);
  }

  .ask-box p {
    margin: 5px 0;
    color: var(--fg-1);
    font-size: 11px;
    line-height: 1.45;
  }

  .answer {
    margin-top: 6px;
    color: var(--fg-2);
    font-size: 10.5px;
    line-height: 1.5;
  }

  .ask-actions {
    gap: 6px;
    margin-top: 8px;
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
