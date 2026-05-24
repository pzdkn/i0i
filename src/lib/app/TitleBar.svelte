<script lang="ts">
  import type { VaultStatus } from "$lib/domain/vault";

  let { vaultStatus = null }: { vaultStatus?: VaultStatus | null } = $props();
</script>

<header class="titlebar row hair-b">
  <div class="window-controls row">
    <span class="close"></span>
    <span class="minimize"></span>
    <span class="zoom"></span>
  </div>

  <div class="path row">
    <span class="brand">i0i</span>
    <span class="sep">/</span>
    <span>vault</span>
    <span class="sep">/</span>
    <span>transformers</span>
    <span class="sep">/</span>
    <strong>attention</strong>
  </div>

  <div class="command row">
    <span class="prompt">:</span>
    <span class="placeholder">search vault, run command, ask...</span>
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

  .window-controls {
    gap: 6px;
    flex-shrink: 0;
  }

  .window-controls span {
    width: 8px;
    height: 8px;
    display: block;
  }

  .close {
    background: var(--red);
  }

  .minimize {
    background: var(--amber-mid);
  }

  .zoom {
    background: var(--green);
  }

  .path {
    min-width: 280px;
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

  .placeholder {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    color: var(--fg-3);
    font-size: 11px;
    text-overflow: ellipsis;
    white-space: nowrap;
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
