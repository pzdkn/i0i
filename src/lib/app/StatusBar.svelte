<script lang="ts">
  import type { VaultStatus } from "$lib/domain/vault";

  let {
    vaultStatus = null,
    bridgeError = "",
  }: {
    vaultStatus?: VaultStatus | null;
    bridgeError?: string;
  } = $props();
</script>

<footer class="statusbar row hair-t">
  <span class="ok">connected</span>
  {#if vaultStatus}
    <span>vault / {vaultStatus.paperCount} papers / {vaultStatus.unreadCount} unread</span>
    <span>sync: {vaultStatus.syncState}</span>
  {:else if bridgeError}
    <span class="warn">rust bridge unavailable</span>
    <span>{bridgeError}</span>
  {:else}
    <span>vault status loading</span>
  {/if}

  <div class="flex1"></div>

  <span>[j k] navigate</span>
  <span>[o] open</span>
  <span>[cmd+k] palette</span>
  <span>i0i v0.1.0</span>
</footer>

<style>
  .statusbar {
    height: 22px;
    flex-shrink: 0;
    gap: 14px;
    padding: 0 10px;
    background: var(--bg-1);
    color: var(--fg-2);
    font-size: 10px;
    white-space: nowrap;
  }

  .ok {
    color: var(--green);
  }

  .warn {
    color: var(--amber);
  }
</style>
