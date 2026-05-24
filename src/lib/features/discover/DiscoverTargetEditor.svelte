<script lang="ts">
  import type { VaultWorkspace } from "$lib/domain/library";

  let {
    vaults,
    initialVaultIds = [],
    onConfirm,
    onCancel,
  }: {
    vaults: VaultWorkspace[];
    initialVaultIds?: string[];
    onConfirm: (vaultIds: string[]) => void;
    onCancel: () => void;
  } = $props();

  let query = $state("");
  let addedVaultIds = $state<string[]>([]);
  let removedVaultIds = $state<string[]>([]);
  const selectedVaultIds = $derived(
    [...new Set([...initialVaultIds, ...addedVaultIds])].filter((vaultId) => !removedVaultIds.includes(vaultId)),
  );
  const selectedVaults = $derived(
    selectedVaultIds
      .map((vaultId) => vaults.find((vault) => vault.id === vaultId))
      .filter((vault): vault is VaultWorkspace => Boolean(vault)),
  );
  const suggestions = $derived(
    vaults
      .filter((vault) => !selectedVaultIds.includes(vault.id))
      .filter((vault) => {
        const normalizedQuery = query.trim().replace(/,$/, "").toLowerCase();
        if (!normalizedQuery) {
          return true;
        }

        return (
          vault.id.toLowerCase().includes(normalizedQuery) ||
          vault.title.toLowerCase().includes(normalizedQuery) ||
          vault.path.toLowerCase().includes(normalizedQuery)
        );
      })
      .slice(0, 5),
  );

  function selectVault(vaultId: string) {
    if (!selectedVaultIds.includes(vaultId)) {
      addedVaultIds = [...addedVaultIds, vaultId];
    }
    removedVaultIds = removedVaultIds.filter((removedVaultId) => removedVaultId !== vaultId);
    query = "";
  }

  function removeVault(vaultId: string) {
    addedVaultIds = addedVaultIds.filter((selectedVaultId) => selectedVaultId !== vaultId);
    if (initialVaultIds.includes(vaultId) && !removedVaultIds.includes(vaultId)) {
      removedVaultIds = [...removedVaultIds, vaultId];
    }
  }

  function submit() {
    if (selectedVaultIds.length > 0) {
      onConfirm(selectedVaultIds);
    }
  }

  function handleKeydown(event: KeyboardEvent) {
    if (event.key === "Escape") {
      event.stopPropagation();
      onCancel();
      return;
    }

    if (event.key === "Enter") {
      event.preventDefault();
      event.stopPropagation();
      submit();
      return;
    }

    if (event.key === "," && suggestions[0]) {
      event.preventDefault();
      event.stopPropagation();
      selectVault(suggestions[0].id);
    }
  }
</script>

<div
  class="target-editor col"
  role="group"
  aria-label="Select Vault targets"
>
  <div class="target-input row">
    {#each selectedVaults as vault}
      <button class="selected-chip" type="button" onclick={() => removeVault(vault.id)}>
        {vault.path} x
      </button>
    {/each}
    <input
      bind:value={query}
      aria-label="Filter Vault targets"
      onkeydown={handleKeydown}
      placeholder={selectedVaults.length ? "add another vault..." : "type vault..."}
    />
  </div>

  <div class="suggestions col">
    {#if suggestions.length}
      {#each suggestions as vault}
        <button class="suggestion row" type="button" onclick={() => selectVault(vault.id)}>
          <span class="path truncate">{vault.path}</span>
          <span class="count">{vault.papers.length} papers</span>
        </button>
      {/each}
    {:else}
      <div class="no-match">no matching Vault</div>
    {/if}
  </div>

  <div class="target-actions row">
    <button class="btn" type="button" onclick={onCancel}>Cancel</button>
    <button class="btn primary" disabled={selectedVaultIds.length === 0} type="button" onclick={submit}>
      Add to {selectedVaultIds.length || 0} Vault{selectedVaultIds.length === 1 ? "" : "s"}
    </button>
  </div>
</div>

<style>
  .target-editor {
    flex: 1 1 100%;
    min-width: 280px;
    gap: 8px;
    margin-top: 2px;
    padding: 8px;
    border: 1px solid var(--border-2);
    background: var(--bg-1);
  }

  .target-input {
    min-height: 26px;
    gap: 5px;
    flex-wrap: wrap;
    padding: 3px;
    border: 1px solid var(--border-2);
    background: var(--bg);
  }

  .selected-chip {
    height: 18px;
    border: 1px solid var(--amber-dim);
    background: rgba(242, 169, 59, 0.06);
    color: var(--amber);
    font-size: 9px;
    cursor: pointer;
  }

  input {
    min-width: 150px;
    flex: 1;
    border: 0;
    outline: none;
    background: transparent;
    color: var(--fg-1);
    font: inherit;
    font-size: 10px;
  }

  input::placeholder {
    color: var(--fg-3);
  }

  .suggestions {
    max-height: 112px;
    overflow: auto;
    border: 1px solid var(--border);
    background: var(--panel);
  }

  .suggestion {
    height: 22px;
    gap: 8px;
    padding: 0 8px;
    border: 0;
    border-bottom: 1px solid var(--border);
    background: transparent;
    color: var(--fg-1);
    font-size: 10px;
    text-align: left;
    cursor: pointer;
  }

  .suggestion:hover {
    background: rgba(242, 169, 59, 0.08);
    color: var(--amber);
  }

  .path {
    flex: 1;
    min-width: 0;
  }

  .count,
  .no-match {
    color: var(--fg-3);
    font-size: 9px;
  }

  .no-match {
    padding: 6px 8px;
  }

  .target-actions {
    justify-content: flex-end;
    gap: 6px;
  }

  .btn:disabled,
  .btn:disabled:hover {
    border-color: var(--border);
    color: var(--fg-4);
    cursor: default;
  }
</style>
