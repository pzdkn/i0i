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
  <!-- "connected" used to be printed unconditionally, so a broken bridge
       reported itself as connected and unavailable in the same line. -->
  {#if bridgeError}
    <span class="warn">rust bridge unavailable</span>
    <span class="truncate">{bridgeError}</span>
  {:else if vaultStatus}
    <span class="ok">connected</span>
    <span>{vaultStatus.paperCount} {vaultStatus.paperCount === 1 ? "paper" : "papers"}</span>
    <span>{vaultStatus.unreadCount} unread</span>
    <span class="mono-dim">sync: {vaultStatus.syncState}</span>
  {:else}
    <span class="mono-dim">loading vault…</span>
  {/if}

  <div class="flex1"></div>

  <!-- RFC 0086 R2: [j k] and [o] are gone rather than left advertising keys
       nothing handles, and cmd+k opens a search, not a palette. -->
  <span>[cmd+k] search</span>
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
