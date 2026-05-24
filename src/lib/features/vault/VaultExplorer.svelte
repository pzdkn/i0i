<script lang="ts">
  import { tick } from "svelte";
  import type { VaultWorkspace } from "$lib/domain/library";

  let {
    activeVaultId,
    vaults,
    onOpenVault,
    onCreateVault,
  }: {
    activeVaultId: string;
    vaults: VaultWorkspace[];
    onOpenVault: (vaultId: string) => void;
    onCreateVault: (path: string) => Promise<void>;
  } = $props();

  let filterText = $state("");
  let createText = $state("");
  let isCreating = $state(false);
  let createInput = $state<HTMLInputElement | null>(null);
  const visibleVaults = $derived(
    vaults.filter((vault) => {
      const query = filterText.trim().toLowerCase();
      if (!query) {
        return true;
      }

      return (
        vault.id.toLowerCase().includes(query) ||
        vault.title.toLowerCase().includes(query) ||
        vault.path.toLowerCase().includes(query)
      );
    }),
  );

  async function submitCreate() {
    const path = createText.trim();
    if (!path) {
      return;
    }

    await onCreateVault(path);
    createText = "";
    isCreating = false;
  }

  async function startCreate() {
    isCreating = true;
    await tick();
    createInput?.focus();
  }

  function handleCreateKeydown(event: KeyboardEvent) {
    if (event.key === "Escape") {
      createText = "";
      isCreating = false;
      return;
    }

    if (event.key === "Enter") {
      event.preventDefault();
      void submitCreate();
    }
  }
</script>

<aside class="explorer hair-r">
  <header class="row hair-b">
    <span class="label hot">Explorer</span>
    <div class="flex1"></div>
    <span class="mono-dim">import</span>
  </header>

  <label class="filter row">
    <span>/</span>
    <input bind:value={filterText} aria-label="Filter vault" placeholder="filter vault..." />
    <span class="key">/</span>
  </label>

  <div class="tree">
    <div class="section section-row row">
      <span class="label">Vaults</span>
      <div class="flex1"></div>
      <button class="section-action" type="button" title="Create Vault" onclick={() => void startCreate()}>+</button>
    </div>

    {#if isCreating}
      <div class="tree-row create-item">
        <span class="glyph">=</span>
        <input
          bind:this={createInput}
          bind:value={createText}
          aria-label="New Vault path"
          onkeydown={handleCreateKeydown}
          placeholder="new vault..."
        />
      </div>
    {/if}

    {#if visibleVaults.length}
      {#each visibleVaults as vault}
        <button
          class:active={activeVaultId === vault.id}
          class="tree-row folder"
          type="button"
          onclick={() => onOpenVault(vault.id)}
          title={vault.path}
        >
          <span class="glyph">#</span>
          <span class="folder-dot"></span>
          <span class="truncate">{vault.path}</span>
          <span class="count">{vault.papers.length}</span>
        </button>
      {/each}
    {:else}
      <div class="empty-row">No Vaults</div>
    {/if}
  </div>
</aside>

<style>
  .explorer {
    width: 240px;
    flex-shrink: 0;
    display: flex;
    flex-direction: column;
    overflow: hidden;
    background: var(--panel);
  }

  header {
    height: 26px;
    flex-shrink: 0;
    gap: 8px;
    padding: 0 10px;
    background: var(--bg-1);
  }

  .filter {
    height: 34px;
    flex-shrink: 0;
    gap: 6px;
    margin: 6px 8px;
    padding: 0 6px;
    border: 1px solid var(--border-2);
    color: var(--fg-3);
    background: var(--bg);
  }

  .filter span:first-child {
    color: var(--amber);
  }

  input {
    flex: 1;
    min-width: 0;
    border: 0;
    outline: none;
    background: transparent;
    color: var(--fg-2);
    font: inherit;
    font-size: 11px;
  }

  input::placeholder {
    color: var(--fg-3);
  }

  .tree {
    flex: 1;
    min-height: 0;
    overflow: auto;
    padding: 2px 0 12px;
  }

  .section {
    height: 30px;
    display: flex;
    align-items: flex-end;
    padding: 0 12px 5px;
    border-bottom: 1px solid var(--border);
  }

  .section-row {
    align-items: center;
    padding-bottom: 0;
  }

  .section-action {
    width: 22px;
    height: 22px;
    border: 1px solid var(--border-2);
    background: transparent;
    color: var(--amber-mid);
    font: inherit;
    font-size: 15px;
    line-height: 1;
    cursor: pointer;
  }

  .section-action:hover {
    border-color: var(--amber);
    color: var(--amber);
  }

  .tree-row {
    width: 100%;
    height: 20px;
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 0 12px;
    color: var(--fg-1);
    border: 0;
    border-left: 2px solid transparent;
    background: transparent;
    font-size: 11px;
    font-family: inherit;
    text-align: left;
    cursor: pointer;
  }

  .tree-row.active {
    border-left: 2px solid var(--amber);
    background: rgba(242, 169, 59, 0.1);
    color: var(--amber);
  }

  .create-item {
    height: 24px;
    cursor: text;
  }

  .create-item input {
    height: 18px;
    padding: 0 4px;
    border: 1px solid var(--cyan);
    background: var(--bg);
    color: var(--fg);
  }

  .glyph {
    width: 10px;
    color: var(--fg-3);
  }

  .folder-dot {
    width: 6px;
    height: 6px;
    flex-shrink: 0;
    background: var(--amber-mid);
  }

  .count {
    margin-left: auto;
    color: var(--fg-3);
    font-size: 10px;
  }

  .empty-row {
    height: 24px;
    display: flex;
    align-items: center;
    padding: 0 12px 0 24px;
    color: var(--fg-3);
    font-size: 11px;
  }
</style>
