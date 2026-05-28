<script lang="ts">
  import { createPaperNote, deletePaperNote, getPaperNotes, updatePaperNote } from "$lib/bridge/library";
  import type { PaperNote } from "$lib/domain/library";
  import type { Paper } from "$lib/domain/paper";
  import type { ReaderMode, ReaderTextSelection } from "$lib/domain/reader";
  import { createReaderDocument } from "$lib/mock/reader";
  import ReaderFooter from "$lib/features/reader/ReaderFooter.svelte";
  import ReaderHeader from "$lib/features/reader/ReaderHeader.svelte";
  import ReaderInspector from "$lib/features/reader/ReaderInspector.svelte";
  import TextPage from "$lib/features/reader/TextPage.svelte";
  import { decrementPaperNoteCount, incrementPaperNoteCount, isPaperInLibrary } from "$lib/state/library-cache.svelte";

  let {
    paper,
  }: {
    paper: Paper;
  } = $props();

  let mode = $state<ReaderMode>("TEXT");
  let notes = $state<PaperNote[]>([]);
  let noteDraft = $state<ReaderTextSelection | null>(null);
  let noteError = $state("");
  let isLoadingNotes = $state(false);
  let activeNoteId = $state<string | null>(null);
  const document = $derived(createReaderDocument(paper));
  const notesEnabled = $derived(isPaperInLibrary(paper.id));

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
        body,
      });
      notes = nextNotes;
      activeNoteId = nextNotes[0]?.id ?? null;
      noteDraft = null;
      noteError = "";
      incrementPaperNoteCount(paper.id);
    } catch (error) {
      noteError = String(error);
    }
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
</script>

<section class="reader-workspace col">
  <div class="reader-body row">
    <main class="reader-main col">
      <ReaderHeader {document} {mode} onModeChange={(nextMode) => (mode = nextMode)} />

      <div class="reading-surface row">
        <div class="page-wrap">
          <TextPage
            {document}
            {notes}
            {activeNoteId}
            {notesEnabled}
            onCreateNoteFromSelection={createNoteDraft}
          />
        </div>
      </div>

      <ReaderFooter {mode} />
    </main>

    <ReaderInspector
      {document}
      {notes}
      {activeNoteId}
      {noteDraft}
      {notesEnabled}
      {noteError}
      {isLoadingNotes}
      onSaveNote={saveNote}
      onDeleteNote={removeNote}
      onUpdateNote={updateNote}
      onActivateNote={activateNote}
    />
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

  .page-wrap {
    flex: 1;
    min-width: 0;
    overflow: auto;
    display: flex;
    justify-content: center;
    padding: 20px 24px;
  }

</style>
