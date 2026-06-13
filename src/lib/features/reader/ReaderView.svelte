<script lang="ts">
  import { listen } from "@tauri-apps/api/event";
  import { onMount } from "svelte";
  import {
    createPaperNote,
    deletePaperNote,
    getDiscoveryReaderDocument,
    getPaperNotes,
    updatePaperNote,
    getReaderDocument,
  } from "$lib/bridge/library";
  import type { PaperNote } from "$lib/domain/library";
  import type { Paper } from "$lib/domain/paper";
  import type { DiscoveryReaderCandidate, ReaderDocument, ReaderTextSelection } from "$lib/domain/reader";
  import ReaderFooter from "$lib/features/reader/ReaderFooter.svelte";
  import ReaderHeader from "$lib/features/reader/ReaderHeader.svelte";
  import ReaderInspector from "$lib/features/reader/ReaderInspector.svelte";
  import PdfPage from "$lib/features/reader/PdfPage.svelte";
  import { decrementPaperNoteCount, incrementPaperNoteCount, isPaperInLibrary } from "$lib/state/library-cache.svelte";

  let {
    paper,
    candidate,
  }: {
    paper: Paper;
    candidate?: DiscoveryReaderCandidate;
  } = $props();

  let notes = $state<PaperNote[]>([]);
  let noteDraft = $state<ReaderTextSelection | null>(null);
  let noteError = $state("");
  let isLoadingNotes = $state(false);
  let activeNoteId = $state<string | null>(null);
  let readerDocument = $state<ReaderDocument | null>(null);
  let docError = $state("");
  let docErrorDebug = $state("");
  let isLoadingDoc = $state(false);
  let refreshTick = $state(0);
  let documentLoadSequence = 0;

  const document = $derived<ReaderDocument | null>(readerDocument);
  const notesEnabled = $derived(isPaperInLibrary(paper.id));
  const activeCandidate = $derived(candidate && !notesEnabled ? candidate : undefined);
  const hasCachedPdf = $derived(Boolean(readerDocument?.pdfLocalPath));

  onMount(() => {
    let unlisten: (() => void) | undefined;

    listen("document_source_updated", (event) => {
      const payload = event.payload as { paper_id: string };
      if (payload.paper_id === paper.id) {
        refreshTick += 1;
      }
    })
      .then((nextUnlisten) => {
        unlisten = nextUnlisten;
      })
      .catch((error) => {
        console.error("Failed to listen for document_source_updated:", error);
      });

    return () => unlisten?.();
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

  // Notes effect
  $effect(() => {
    const paperId = paper.id;
    noteDraft = null;
    noteError = "";
    activeNoteId = null;

    if (!notesEnabled) {
      notes = [];
      isLoadingNotes = false;
      return;
    }

    isLoadingNotes = true;
    getPaperNotes(paperId)
      .then((nextNotes) => {
        if (paper.id === paperId) {
          notes = nextNotes;
        }
      })
      .catch((error) => {
        if (paper.id === paperId) {
          noteError = String(error);
          notes = [];
        }
      })
      .finally(() => {
        if (paper.id === paperId) {
          isLoadingNotes = false;
        }
      });
  });

  function createNoteDraft(selection: ReaderTextSelection) {
    noteDraft = selection;
    noteError = "";
  }

  function cancelNoteDraft() {
    noteDraft = null;
    noteError = "";
    clearReaderSelection();
  }

  async function saveNote(body: string) {
    if (!noteDraft || !notesEnabled) {
      return;
    }

    try {
      const nextNotes = await createPaperNote({
        paperId: paper.id,
        sourceId: noteDraft.sourceId,
        startOffset: noteDraft.startOffset,
        endOffset: noteDraft.endOffset,
        selectedText: noteDraft.selectedText,
        anchorKind: noteDraft.anchorKind,
        pageIndex: noteDraft.pageIndex,
        rectsJson: noteDraft.rectsJson,
        quoteContext: noteDraft.quoteContext,
        body,
      });
      notes = nextNotes;
      activeNoteId = nextNotes[0]?.id ?? null;
      noteDraft = null;
      noteError = "";
      clearReaderSelection();
      incrementPaperNoteCount(paper.id);
    } catch (error) {
      noteError = String(error);
    }
  }

  function clearReaderSelection() {
    window.getSelection()?.removeAllRanges();
  }

  async function removeNote(noteId: string) {
    if (!notesEnabled) {
      return;
    }

    try {
      const previousNoteCount = notes.length;
      const nextNotes = await deletePaperNote({ paperId: paper.id, noteId });

      notes = nextNotes;
      noteError = "";
      if (!nextNotes.some((note) => note.id === activeNoteId)) {
        activeNoteId = null;
      }
      if (nextNotes.length < previousNoteCount) {
        decrementPaperNoteCount(paper.id);
      }
    } catch (error) {
      noteError = String(error);
    }
  }

  async function updateNote(noteId: string, body: string) {
    if (!notesEnabled) {
      return;
    }

    try {
      notes = await updatePaperNote({ paperId: paper.id, noteId, body });
      activeNoteId = noteId;
      noteError = "";
    } catch (error) {
      noteError = String(error);
      throw error;
    }
  }

  function activateNote(noteId: string) {
    activeNoteId = noteId;
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
              {notes}
              {activeNoteId}
              {noteDraft}
              {notesEnabled}
              onCreateNoteFromSelection={createNoteDraft}
              onActivateNote={activateNote}
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
        {notes}
        {activeNoteId}
        {noteDraft}
        {notesEnabled}
        {noteError}
        {isLoadingNotes}
        onSaveNote={saveNote}
        onCancelNoteDraft={cancelNoteDraft}
        onDeleteNote={removeNote}
        onUpdateNote={updateNote}
        onActivateNote={activateNote}
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
