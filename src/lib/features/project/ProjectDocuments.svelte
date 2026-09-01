<script lang="ts">
  import type { LibrarySnapshot, ProjectDocument, ProjectWorkspace } from "$lib/domain/library";
  import {
    createProjectDocument,
    deleteProjectDocument,
    getLibrary,
    getProjectDocument,
    updateProjectDocument,
  } from "$lib/bridge/library";

  let {
    project,
    selectedDocumentId,
    onSelectDocument,
    onLibraryChanged,
    onDirtyChange,
  }: {
    project: ProjectWorkspace;
    selectedDocumentId: string;
    onSelectDocument: (documentId: string) => void;
    onLibraryChanged: (snapshot: LibrarySnapshot) => void;
    onDirtyChange: (dirty: boolean) => void;
  } = $props();

  let document = $state<ProjectDocument | null>(null);
  let draftTitle = $state("");
  let draftContent = $state("");
  let draftHarnessWritable = $state(false);
  let loading = $state(false);
  let saving = $state(false);
  let error = $state("");
  let loadSequence = 0;
  const dirty = $derived(
    Boolean(
      document &&
        (draftTitle !== document.title ||
          draftContent !== document.content ||
          draftHarnessWritable !== document.harnessWritable),
    ),
  );

  $effect(() => {
    onDirtyChange(dirty);
  });

  $effect(() => {
    const nextId = selectedDocumentId || project.documents[0]?.id || "";
    if (!nextId) {
      document = null;
      draftTitle = "";
      draftContent = "";
      draftHarnessWritable = false;
      return;
    }
    if (document?.id !== nextId) {
      void loadDocument(nextId);
    }
  });

  async function loadDocument(documentId: string) {
    const sequence = ++loadSequence;
    loading = true;
    error = "";
    try {
      const loaded = await getProjectDocument(documentId);
      if (sequence !== loadSequence) return;
      document = loaded;
      draftTitle = loaded.title;
      draftContent = loaded.content;
      draftHarnessWritable = loaded.harnessWritable;
      if (selectedDocumentId !== loaded.id) onSelectDocument(loaded.id);
    } catch (caught) {
      if (sequence === loadSequence) error = String(caught);
    } finally {
      if (sequence === loadSequence) loading = false;
    }
  }

  async function createDocument() {
    if (dirty && !window.confirm("Discard unsaved changes and create a document?")) return;
    error = "";
    try {
      const created = await createProjectDocument(project.id, "Untitled.md", "# Untitled\n");
      onLibraryChanged(await getLibrary());
      onDirtyChange(false);
      onSelectDocument(created.id);
    } catch (caught) {
      error = String(caught);
    }
  }

  async function saveDocument() {
    if (!document || !draftTitle.trim()) return;
    saving = true;
    error = "";
    try {
      document = await updateProjectDocument({
        id: document.id,
        title: draftTitle,
        content: draftContent,
        harnessWritable: draftHarnessWritable,
      });
      draftTitle = document.title;
      onLibraryChanged(await getLibrary());
    } catch (caught) {
      error = String(caught);
    } finally {
      saving = false;
    }
  }

  async function removeDocument() {
    if (!document || !window.confirm(`Delete “${document.title}”?`)) return;
    const deletedId = document.id;
    error = "";
    try {
      await deleteProjectDocument(deletedId);
      const snapshot = await getLibrary();
      onLibraryChanged(snapshot);
      const next = snapshot.projectDocuments.find(
        (candidate) => candidate.projectId === project.id && candidate.id !== deletedId,
      );
      document = null;
      onDirtyChange(false);
      onSelectDocument(next?.id ?? "");
    } catch (caught) {
      error = String(caught);
    }
  }
</script>

<section class="documents-workspace">
  <aside class="document-list hair-r">
    <header class="row hair-b">
      <span class="label hot">Documents</span>
      <div class="flex1"></div>
      <button type="button" title="Create Markdown document" onclick={() => void createDocument()}>+</button>
    </header>
    <div class="list">
      {#each project.documents as summary}
        <button
          class:active={document?.id === summary.id}
          type="button"
          onclick={() => onSelectDocument(summary.id)}
        >
          <span class="title truncate">{summary.title}</span>
          <span class="format">MD</span>
        </button>
      {:else}
        <div class="empty">No documents yet.</div>
      {/each}
    </div>
  </aside>

  <main class="editor">
    {#if error}<div class="error hair-b">{error}</div>{/if}
    {#if loading}
      <div class="empty-state">Loading document…</div>
    {:else if document}
      <header class="editor-header row hair-b">
        <input bind:value={draftTitle} aria-label="Document title" />
        <span class:visible={dirty} class="dirty">unsaved</span>
        <button type="button" disabled={!dirty || saving || !draftTitle.trim()} onclick={() => void saveDocument()}>
          {saving ? "Saving…" : "Save"}
        </button>
        <button class="danger" type="button" onclick={() => void removeDocument()}>Delete</button>
      </header>
      <textarea bind:value={draftContent} aria-label="Markdown content" spellcheck="true"></textarea>
      <footer class="editor-footer row hair-t">
        <label>
          <input type="checkbox" bind:checked={draftHarnessWritable} />
          Harness may update this document
        </label>
        <div class="flex1"></div>
        <span>Markdown · working context, not source evidence</span>
      </footer>
    {:else}
      <div class="empty-state">
        <strong>No document selected</strong>
        <span>Create a Markdown document for notes, drafts, or research outputs.</span>
        <button type="button" onclick={() => void createDocument()}>Create document</button>
      </div>
    {/if}
  </main>
</section>

<style>
  .documents-workspace {
    flex: 1;
    min-height: 0;
    display: grid;
    grid-template-columns: 230px minmax(0, 1fr);
    background: var(--bg);
  }

  .document-list,
  .editor {
    min-height: 0;
    display: flex;
    flex-direction: column;
  }

  header {
    min-height: 34px;
    padding: 0 10px;
    gap: 8px;
    background: var(--bg-1);
  }

  button {
    border: 1px solid var(--border-2);
    background: transparent;
    color: var(--fg-2);
    font: inherit;
    cursor: pointer;
  }

  button:hover:not(:disabled) {
    color: var(--amber);
    border-color: var(--amber-mid);
  }

  button:disabled {
    opacity: 0.4;
    cursor: default;
  }

  .list {
    overflow: auto;
    padding: 6px;
  }

  .list button {
    width: 100%;
    min-height: 30px;
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 0 8px;
    border-color: transparent;
    text-align: left;
  }

  .list button.active {
    color: var(--amber);
    border-color: var(--border-2);
    background: rgba(242, 169, 59, 0.06);
  }

  .title {
    flex: 1;
  }

  .format,
  .dirty,
  .editor-footer {
    color: var(--fg-3);
    font-size: 10px;
  }

  .dirty {
    visibility: hidden;
  }

  .dirty.visible {
    visibility: visible;
    color: var(--amber);
  }

  .editor-header input {
    flex: 1;
    min-width: 0;
    border: 0;
    outline: 0;
    background: transparent;
    color: var(--fg-1);
    font: inherit;
    font-size: 14px;
  }

  .editor-header button,
  .empty-state button,
  .document-list header button {
    min-height: 24px;
    padding: 0 9px;
  }

  .danger {
    color: var(--red, #d87868);
  }

  textarea {
    flex: 1;
    min-height: 0;
    resize: none;
    border: 0;
    outline: 0;
    padding: 24px clamp(24px, 6vw, 80px);
    background: var(--bg);
    color: var(--fg-1);
    font: 13px/1.65 var(--font-mono, monospace);
    tab-size: 2;
  }

  .editor-footer {
    min-height: 30px;
    padding: 0 12px;
    gap: 12px;
  }

  .editor-footer label {
    display: flex;
    align-items: center;
    gap: 6px;
    color: var(--fg-2);
  }

  .empty,
  .empty-state {
    color: var(--fg-3);
  }

  .empty {
    padding: 12px 8px;
  }

  .empty-state {
    flex: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 10px;
  }

  .empty-state strong {
    color: var(--fg-1);
  }

  .error {
    padding: 8px 12px;
    color: var(--red, #d87868);
    background: rgba(216, 120, 104, 0.08);
  }
</style>
