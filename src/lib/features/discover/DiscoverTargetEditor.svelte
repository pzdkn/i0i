<script lang="ts">
  import { tick } from "svelte";
  import type { VaultWorkspace } from "$lib/domain/library";

  let {
    vaults,
    excludedVaultIds = [],
    onSelect,
    onCancel,
  }: {
    vaults: VaultWorkspace[];
    excludedVaultIds?: string[];
    onSelect: (vaultId: string) => void;
    onCancel: () => void;
  } = $props();

  let query = $state("");
  let input: HTMLInputElement;
  const suggestions = $derived(
    vaults
      .filter((vault) => !excludedVaultIds.includes(vault.id))
      .filter((vault) => {
        const normalizedQuery = query.trim().toLowerCase();
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

  $effect(() => {
    tick().then(() => input?.focus());
  });

  function choose(vaultId: string) {
    onSelect(vaultId);
  }

  function handleKeydown(event: KeyboardEvent) {
    if (event.key === "Escape") {
      event.stopPropagation();
      onCancel();
      return;
    }

    if (event.key === "Enter" && suggestions[0]) {
      event.preventDefault();
      event.stopPropagation();
      choose(suggestions[0].id);
    }
  }
</script>

<div class="target-editor col" role="group" aria-label="Add candidate to Vault">
  <input
    bind:this={input}
    bind:value={query}
    aria-label="Filter Vault targets"
    onkeydown={handleKeydown}
    placeholder="type vault..."
  />

  <div class="suggestions col">
    {#if suggestions.length}
      {#each suggestions as vault}
        <button class="suggestion row" type="button" onclick={() => choose(vault.id)}>
          <span class="path truncate">{vault.path}</span>
          <span class="count">{vault.papers.length}</span>
        </button>
      {/each}
    {:else}
      <div class="no-match">no matching Vault</div>
    {/if}
  </div>
</div>

<style>
  .target-editor {
    width: 205px;
    gap: 0;
    padding: 6px;
    border: 1px solid var(--border-2);
    background: var(--bg-1);
  }

  input {
    height: 24px;
    border: 1px solid var(--border-2);
    outline: none;
    background: var(--bg);
    color: var(--fg-1);
    padding: 0 8px;
    font: inherit;
    font-size: 10px;
  }

  input:focus {
    border-color: var(--amber-dim);
  }

  input::placeholder {
    color: var(--fg-3);
  }

  .suggestions {
    max-height: 116px;
    overflow: auto;
    border: 1px solid var(--border);
    border-top: 0;
  }

  .suggestion {
    height: 23px;
    gap: 6px;
    border: 0;
    border-bottom: 1px solid var(--border);
    background: var(--panel);
    color: var(--fg-1);
    padding: 0 7px;
    font-size: 10px;
    cursor: pointer;
  }

  .suggestion:hover,
  .suggestion:focus {
    outline: none;
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

  .count {
    flex-shrink: 0;
  }

  .no-match {
    padding: 6px 7px;
  }
</style>
