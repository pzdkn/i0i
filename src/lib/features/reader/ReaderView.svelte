<script lang="ts">
  import { createPaperNote, getPaperNotes } from "$lib/bridge/library";
  import type { PaperNote } from "$lib/domain/library";
  import type { Paper } from "$lib/domain/paper";
  import type { ReaderMode, ReaderTextSelection } from "$lib/domain/reader";
  import { createReaderDocument } from "$lib/mock/reader";
  import ReaderFooter from "$lib/features/reader/ReaderFooter.svelte";
  import ReaderHeader from "$lib/features/reader/ReaderHeader.svelte";
  import ReaderInspector from "$lib/features/reader/ReaderInspector.svelte";
  import TextPage from "$lib/features/reader/TextPage.svelte";
  import { incrementPaperNoteCount, isPaperInLibrary } from "$lib/state/library-cache.svelte";

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
  const document = $derived(createReaderDocument(paper));
  const notesEnabled = $derived(isPaperInLibrary(paper.id));

  $effect(() => {
    const paperId = paper.id;
    noteDraft = null;
    noteError = "";

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
      notes = await createPaperNote({
        paperId: paper.id,
        sourceId: noteDraft.sourceId,
        startOffset: noteDraft.startOffset,
        endOffset: noteDraft.endOffset,
        selectedText: noteDraft.selectedText,
        body,
      });
      noteDraft = null;
      noteError = "";
      incrementPaperNoteCount(paper.id);
    } catch (error) {
      noteError = String(error);
    }
  }
</script>

<section class="reader-workspace col">
  <div class="reader-body row">
    <main class="reader-main col">
      <ReaderHeader {document} {mode} onModeChange={(nextMode) => (mode = nextMode)} />

      <div class="reading-surface row">
        <div class="page-wrap">
          <TextPage {document} {notesEnabled} onCreateNoteFromSelection={createNoteDraft} />
        </div>
      </div>

      <ReaderFooter {mode} />
    </main>

    <ReaderInspector {document} {notes} {noteDraft} {notesEnabled} {noteError} {isLoadingNotes} onSaveNote={saveNote} />
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
