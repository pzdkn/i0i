<script lang="ts">
  import type { VaultStatus } from "$lib/domain/vault";

  let {
    vaultStatus = null,
    currentPath = "/transformers/attention",
  }: {
    vaultStatus?: VaultStatus | null;
    currentPath?: string;
  } = $props();

  let commandText = $state("");
</script>

<header class="titlebar row hair-b">
  <div class="path row">
    <span class="brand">i0i</span>
    <span class="sep">/</span>
    <strong>{currentPath}</strong>
  </div>

  <div class="command row">
    <span class="prompt">:</span>
    <input bind:value={commandText} aria-label="Command palette" placeholder="search vault, run command, ask..." />
    <span class="key">cmd+k</span>
  </div>

  <div class="readout row">
    <span class="sync">online</span>
    {#if vaultStatus}
      <span>{vaultStatus.paperCount} papers</span>
      <span>{vaultStatus.unreadCount} unread</span>
    {:else}
      <span>loading vault</span>
    {/if}
  </div>
</header>

<style>
  .titlebar {
    height: 28px;
    flex-shrink: 0;
    gap: 12px;
    padding: 0 10px;
    background: var(--bg-1);
  }

  .path {
    min-width: 240px;
    gap: 6px;
    color: var(--fg-2);
    font-size: 11px;
    white-space: nowrap;
  }

  .brand,
  .path strong {
    color: var(--amber);
    font-weight: 600;
  }

  .sep {
    color: var(--fg-4);
  }

  .command {
    flex: 1;
    min-width: 260px;
    max-width: 460px;
    height: 20px;
    gap: 8px;
    padding: 0 8px;
    border: 1px solid var(--border-2);
    background: var(--bg);
  }

  .prompt {
    color: var(--amber);
  }

  input {
    flex: 1;
    min-width: 0;
    border: 0;
    outline: none;
    background: transparent;
    color: var(--fg-3);
    font-size: 11px;
  }

  input::placeholder {
    color: var(--fg-3);
  }

  .readout {
    gap: 12px;
    color: var(--fg-2);
    font-size: 10px;
    white-space: nowrap;
  }

  .sync {
    color: var(--green);
  }
</style>
