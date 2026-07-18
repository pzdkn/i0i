<script lang="ts">
  import { open } from "@tauri-apps/plugin-dialog";
  import ResizableSplit from "$lib/components/layout/ResizableSplit.svelte";
  import PaperList from "$lib/features/vault/PaperList.svelte";
  import VaultInspector from "$lib/features/vault/VaultInspector.svelte";
  import type {
    MetadataAutofillProgress,
    MetadataCandidate,
    PaperMetadataUpdate,
    VaultWorkspace,
  } from "$lib/domain/library";

  let {
    workspace,
    onOpenPaper,
    onImportPdfs,
    onAutofillMetadata,
    autofillingMetadataPaperIds,
    metadataAutofillProgressByPaperId = {},
    onApplyMetadataCandidate,
    onUpdatePaperMetadata,
    onRemovePaperFromVault,
    onRemovePaperFromLibrary,
  }: {
    workspace: VaultWorkspace;
    onOpenPaper: (paperId: string) => void;
    onImportPdfs: (vaultId: string, paths: string[]) => void | Promise<void>;
    onAutofillMetadata: (paperId: string) => void | Promise<void>;
    autofillingMetadataPaperIds: string[];
    metadataAutofillProgressByPaperId?: Record<string, MetadataAutofillProgress>;
    onApplyMetadataCandidate: (paperId: string, candidate: MetadataCandidate) => void | Promise<void>;
    onUpdatePaperMetadata: (paperId: string, update: PaperMetadataUpdate) => void | Promise<void>;
    onRemovePaperFromVault: (vaultId: string, paperId: string) => void;
    onRemovePaperFromLibrary: (paperId: string) => void;
  } = $props();

  let selectedPaperId = $state("");
  let localFilter = $state("");
  let isImporting = $state(false);
  const selectedPaper = $derived(
    workspace.papers.find((paper) => paper.id === selectedPaperId) ?? workspace.papers[0],
  );

  $effect(() => {
    if (!workspace.papers.some((paper) => paper.id === selectedPaperId)) {
      selectedPaperId = workspace.papers[0]?.id ?? "";
    }
  });

  async function chooseLocalPdfs() {
    if (isImporting) {
      return;
    }

    const selected = await open({
      multiple: true,
      filters: [{ name: "PDF", extensions: ["pdf"] }],
    });
    const paths = Array.isArray(selected) ? selected : selected ? [selected] : [];
    if (paths.length === 0) {
      return;
    }

    isImporting = true;
    try {
      await onImportPdfs(workspace.id, paths);
    } finally {
      isImporting = false;
    }
  }
</script>

<section class="workspace col">
  <div class="vault-home row">
    <ResizableSplit
      storageKey="i0i.vault-split"
      panes={[
        { id: "papers", min: 360, default: 900 },
        { id: "inspector", min: 220, default: 320 },
      ]}
    >
      {#snippet pane(id: string)}
        {#if id === "papers"}
          <main class="vault-main col">
            <header class="folder-header hair-b">
              <div class="row heading-line">
                <div>
                  <div class="label">Folder</div>
                  <h1>{workspace.title}</h1>
                </div>
                <span class="mono-dim">{workspace.summary}</span>
                <div class="flex1"></div>
                <div class="actions row">
                  <button class="btn" type="button" disabled={isImporting} onclick={chooseLocalPdfs}>
                    {isImporting ? "Importing" : "Import PDF"}
                  </button>
                  <button class="btn" type="button">Export .bib</button>
                </div>
              </div>

              <div class="row chip-row">
                {#each workspace.chips as chip, index}
                  <span class:hot={index === 0} class="chip">{chip}</span>
                {/each}
                <div class="flex1"></div>
                <label class="inline-filter row">
                  <span>filter</span>
                  <input bind:value={localFilter} aria-label="Paper filter" placeholder="type..." />
                </label>
                <span class="mono-dim">sort recent</span>
              </div>
            </header>

            <nav class="view-tabs row hair-b">
              {#each workspace.tabs as tab, index}
                <span class:active={index === 0}>
                  {tab.label}
                  {#if tab.count !== undefined}
                    <strong>{tab.count}</strong>
                  {/if}
                </span>
              {/each}
            </nav>

            <PaperList
              papers={workspace.papers}
              {selectedPaperId}
              {autofillingMetadataPaperIds}
              {metadataAutofillProgressByPaperId}
              onSelect={(paperId) => (selectedPaperId = paperId)}
              onOpen={(paperId) => {
                selectedPaperId = paperId;
                onOpenPaper(paperId);
              }}
              {onAutofillMetadata}
              onRemoveFromVault={(paperId) => onRemovePaperFromVault(workspace.id, paperId)}
              onRemoveFromLibrary={onRemovePaperFromLibrary}
            />
          </main>
        {:else}
          <VaultInspector
            papers={workspace.papers}
            {selectedPaper}
            {metadataAutofillProgressByPaperId}
            {autofillingMetadataPaperIds}
            {onAutofillMetadata}
            {onApplyMetadataCandidate}
            {onUpdatePaperMetadata}
          />
        {/if}
      {/snippet}
    </ResizableSplit>
  </div>
</section>

<style>
  .workspace {
    flex: 1;
    min-width: 0;
    min-height: 0;
    background: var(--bg);
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

  .inline-filter {
    gap: 6px;
    height: 20px;
    color: var(--fg-3);
  }

  .inline-filter input {
    width: 80px;
    border: 1px solid var(--border-2);
    outline: none;
    background: var(--bg);
    color: var(--fg-2);
    font: inherit;
    font-size: 10px;
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
