<script lang="ts">
  import type { ReaderMode } from "$lib/domain/reader";
  import { readerDocuments } from "$lib/mock/reader";
  import ReaderFooter from "$lib/features/reader/ReaderFooter.svelte";
  import ReaderHeader from "$lib/features/reader/ReaderHeader.svelte";
  import ReaderInspector from "$lib/features/reader/ReaderInspector.svelte";
  import ReaderMargin from "$lib/features/reader/ReaderMargin.svelte";
  import TextPage from "$lib/features/reader/TextPage.svelte";

  let {
    paperId,
    onBack,
  }: {
    paperId: string;
    onBack: () => void;
  } = $props();

  let mode = $state<ReaderMode>("TEXT");
  const document = $derived(readerDocuments[paperId]);
</script>

<section class="reader-workspace col">
  <div class="tab-strip row hair-b">
    <div class="tab row">
      <span>#</span>
      <span>/transformers/attention</span>
    </div>
    <div class="tab active row">
      <span>read</span>
      <span>{document?.title ?? "Reader unavailable"}</span>
      <span class="dirty">*</span>
    </div>
    <div class="flex1"></div>
    <div class="tab-actions row">
      <span>text</span>
      <span>layout</span>
    </div>
  </div>

  {#if document}
    <div class="reader-body row">
      <main class="reader-main col">
        <ReaderHeader {document} {mode} {onBack} />

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
  {:else}
    <div class="placeholder col">
      <div class="label hot">Reader mock unavailable</div>
      <h1>No reader document for this paper yet.</h1>
      <p>RFC 0002 only mocks the Reader for "Attention Is All You Need".</p>
      <button class="btn primary" type="button" onclick={onBack}>Back to vault</button>
    </div>
  {/if}
</section>

<style>
  .reader-workspace {
    flex: 1;
    min-width: 0;
    min-height: 0;
    background: var(--bg);
  }

  .tab-strip {
    height: 26px;
    flex-shrink: 0;
    background: var(--bg-1);
  }

  .tab {
    height: 100%;
    max-width: 300px;
    gap: 8px;
    padding: 0 12px;
    border-right: 1px solid var(--border);
    color: var(--fg-2);
    font-size: 11px;
  }

  .tab.active {
    border-top: 1px solid var(--amber);
    background: var(--bg);
    color: var(--amber);
  }

  .tab span:nth-child(2) {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .dirty {
    color: var(--amber);
  }

  .tab-actions {
    gap: 12px;
    padding: 0 10px;
    color: var(--fg-3);
    font-size: 10px;
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

  .placeholder {
    flex: 1;
    align-items: center;
    justify-content: center;
    gap: 10px;
    color: var(--fg-2);
    text-align: center;
  }

  .placeholder h1 {
    margin: 0;
    color: var(--amber);
    font-size: 18px;
  }

  .placeholder p {
    margin: 0 0 8px;
  }
</style>
