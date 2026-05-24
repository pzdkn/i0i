<script lang="ts">
  import PaperList from "$lib/features/vault/PaperList.svelte";
  import VaultInspector from "$lib/features/vault/VaultInspector.svelte";
  import { papers } from "$lib/mock/papers";

  let selectedPaperId = $state(papers[0]?.id ?? "");
  const selectedPaper = $derived(
    papers.find((paper) => paper.id === selectedPaperId) ?? papers[0],
  );
</script>

<section class="workspace col">
  <div class="tab-strip row hair-b">
    <div class="tab active row">
      <span>#</span>
      <span>/transformers/attention</span>
      <span class="dirty">*</span>
    </div>
    <div class="flex1"></div>
    <div class="tab-actions row">
      <span>split</span>
      <span>layout</span>
    </div>
  </div>

  <div class="vault-home row">
    <main class="vault-main col">
      <header class="folder-header hair-b">
        <div class="row heading-line">
          <div>
            <div class="label">Folder</div>
            <h1>attention</h1>
          </div>
          <span class="mono-dim">22 papers / 3 unread / last add 2d</span>
          <div class="flex1"></div>
          <div class="actions row">
            <button class="btn" type="button">+ Add</button>
            <button class="btn" type="button">Import</button>
            <button class="btn" type="button">Export .bib</button>
          </div>
        </div>

        <div class="row chip-row">
          <span class="chip hot">foundational x8</span>
          <span class="chip">transformer x22</span>
          <span class="chip">attention x22</span>
          <span class="chip">NeurIPS x6</span>
          <span class="chip">ICLR x4</span>
          <span class="chip">2017-2024</span>
          <div class="flex1"></div>
          <span class="mono-dim">filter [/]</span>
          <span class="mono-dim">sort recent</span>
        </div>
      </header>

      <nav class="view-tabs row hair-b">
        <span class="active">Papers <strong>22</strong></span>
        <span>Notes <strong>14</strong></span>
        <span>Annotations <strong>87</strong></span>
        <span>Graph</span>
        <span>Q&A</span>
      </nav>

      <PaperList {papers} {selectedPaperId} onSelect={(paperId) => (selectedPaperId = paperId)} />
    </main>

    <VaultInspector {papers} {selectedPaper} />
  </div>
</section>

<style>
  .workspace {
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

  .dirty {
    color: var(--amber);
  }

  .tab-actions {
    gap: 12px;
    padding: 0 10px;
    color: var(--fg-3);
    font-size: 10px;
  }

  .vault-home {
    flex: 1;
    min-height: 0;
    align-items: stretch;
  }

  .vault-main {
    flex: 1;
    min-width: 0;
    min-height: 0;
    background: var(--bg);
  }

  .folder-header {
    flex-shrink: 0;
    padding: 14px 18px 12px;
    background: var(--bg-1);
  }

  .heading-line {
    gap: 12px;
    align-items: baseline;
  }

  h1 {
    margin: 2px 0 0;
    color: var(--amber);
    font-size: 22px;
    font-weight: 600;
  }

  .actions {
    gap: 6px;
  }

  .chip-row {
    gap: 6px;
    margin-top: 12px;
    overflow: hidden;
    color: var(--fg-3);
    font-size: 10px;
    white-space: nowrap;
  }

  .view-tabs {
    height: 28px;
    flex-shrink: 0;
    gap: 18px;
    padding: 0 18px;
    background: var(--bg-1);
    color: var(--fg-2);
    font-size: 11px;
  }

  .view-tabs span {
    height: 100%;
    display: inline-flex;
    align-items: center;
    gap: 4px;
    white-space: nowrap;
  }

  .view-tabs .active {
    border-bottom: 1px solid var(--amber);
    color: var(--amber);
  }

  .view-tabs strong {
    color: var(--amber-mid);
    font-weight: 500;
  }
</style>
