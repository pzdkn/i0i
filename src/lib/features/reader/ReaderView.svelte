<script lang="ts">
  import type { Paper } from "$lib/domain/paper";
  import type { ReaderMode } from "$lib/domain/reader";
  import { createReaderDocument } from "$lib/mock/reader";
  import ReaderFooter from "$lib/features/reader/ReaderFooter.svelte";
  import ReaderHeader from "$lib/features/reader/ReaderHeader.svelte";
  import ReaderInspector from "$lib/features/reader/ReaderInspector.svelte";
  import ReaderMargin from "$lib/features/reader/ReaderMargin.svelte";
  import TextPage from "$lib/features/reader/TextPage.svelte";

  let {
    paper,
  }: {
    paper: Paper;
  } = $props();

  let mode = $state<ReaderMode>("TEXT");
  const document = $derived(createReaderDocument(paper));
</script>

<section class="reader-workspace col">
  <div class="reader-body row">
    <main class="reader-main col">
      <ReaderHeader {document} {mode} onModeChange={(nextMode) => (mode = nextMode)} />

      <div class="reading-surface row">
        <div class="page-wrap">
          <TextPage {document} />
        </div>
        <ReaderMargin {document} />
      </div>

      <ReaderFooter {mode} />
    </main>

    <ReaderInspector {document} />
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
