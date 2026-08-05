<script lang="ts">
  import { open, save } from "@tauri-apps/plugin-dialog";
  import { exportVaultBibtex } from "$lib/bridge/library";
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
    onAddHtmlUrl,
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
    onAddHtmlUrl: (vaultId: string, url: string) => void | Promise<void>;
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
  // RFC 0070: export the vault's papers as a BibTeX file.
  let isExporting = $state(false);
  // RFC 0065: add a web page to this vault by URL.
  let showUrlInput = $state(false);
  let urlDraft = $state("");
  let isAddingUrl = $state(false);
  let urlError = $state("");
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

  async function exportCitations() {
    if (isExporting || workspace.papers.length === 0) {
      return;
    }

    const path = await save({
      defaultPath: `${workspace.path}/${workspace.title}.bib`,
      filters: [{ name: "BibTeX", extensions: ["bib"] }],
    });
    if (!path) {
      return;
    }

    isExporting = true;
    try {
      await exportVaultBibtex(workspace.id, path);
    } finally {
      isExporting = false;
    }
  }

  function toggleUrlInput() {
    showUrlInput = !showUrlInput;
    urlError = "";
    if (!showUrlInput) {
      urlDraft = "";
    }
  }

  async function submitUrl() {
    const url = urlDraft.trim();
    if (!url || isAddingUrl) {
      return;
    }
    isAddingUrl = true;
    urlError = "";
    try {
      await onAddHtmlUrl(workspace.id, url);
      urlDraft = "";
      showUrlInput = false;
    } catch (error) {
      urlError = String(error);
    } finally {
      isAddingUrl = false;
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
                  <button class="btn" type="button" class:active={showUrlInput} onclick={toggleUrlInput}>
                    Add web page
                  </button>
                  <button
                    class="btn"
                    type="button"
                    disabled={isExporting || workspace.papers.length === 0}
                    onclick={exportCitations}
                  >
                    {isExporting ? "Exporting" : "Export .bib"}
                  </button>
                </div>
              </div>

              {#if showUrlInput}
                <div class="url-bar row">
                  <input
                    class="url-input"
                    type="url"
                    bind:value={urlDraft}
                    disabled={isAddingUrl}
                    placeholder="https://example.com/article"
                    aria-label="Web page URL"
                    onkeydown={(event) => {
                      if (event.key === "Enter") void submitUrl();
                      if (event.key === "Escape") toggleUrlInput();
                    }}
                  />
                  <button class="btn primary" type="button" disabled={isAddingUrl || !urlDraft.trim()} onclick={() => void submitUrl()}>
                    {isAddingUrl ? "Adding…" : "Add"}
                  </button>
                  <button class="btn" type="button" disabled={isAddingUrl} onclick={toggleUrlInput}>Cancel</button>
                </div>
                {#if urlError}
                  <p class="url-error">{urlError}</p>
                {/if}
              {/if}

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

  .btn.active {
    border-color: var(--amber);
    color: var(--amber);
  }

  .url-bar {
    gap: 6px;
    align-items: center;
    margin-top: 10px;
  }

  .url-input {
    flex: 1;
    min-width: 0;
    height: 28px;
    border: 1px solid var(--border-2);
    outline: none;
    background: var(--bg);
    color: var(--fg-1);
    padding: 0 9px;
    font: inherit;
    font-size: 12px;
  }

  .url-input:focus {
    border-color: var(--cyan);
  }

  .url-error {
    margin: 6px 0 0;
    color: var(--red);
    font-size: 11px;
    line-height: 1.45;
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
