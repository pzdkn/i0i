<script lang="ts">
  import type { Snippet } from "svelte";
  import ActivityRail from "$lib/app/ActivityRail.svelte";
  import StatusBar from "$lib/app/StatusBar.svelte";
  import TitleBar from "$lib/app/TitleBar.svelte";
  import type { VaultStatus } from "$lib/domain/vault";
  import type { ChunkHit } from "$lib/domain/search";

  let {
    activeMode = "V",
    currentPath = "/transformers/attention",
    vaultStatus = null,
    bridgeError = "",
    readerFocusMode = false,
    onSelectMode = () => {},
    onOpenSettings = () => {},
    settingsAttention = false,
    searchVaultId = "",
    resolvePaperTitle = (paperId: string) => paperId,
    onOpenSearchResult = () => {},
    children,
  }: {
    activeMode?: string;
    currentPath?: string;
    vaultStatus?: VaultStatus | null;
    bridgeError?: string;
    readerFocusMode?: boolean;
    onSelectMode?: (mode: string) => void;
    onOpenSettings?: () => void;
    settingsAttention?: boolean;
    /** Scope for the title-bar search. Empty searches the whole library. */
    searchVaultId?: string;
    resolvePaperTitle?: (paperId: string) => string;
    onOpenSearchResult?: (paperId: string, hit: ChunkHit) => void;
    children: Snippet;
  } = $props();

  // RFC 0081 R2.1: in focus mode the app's own bars collapse to hover strips at
  // the window edges — the same reveal the reader toolbar uses (RFC 0079 R4.2),
  // so the space is given back to the page rather than merely dimmed.
  let titleBarHovered = $state(false);
  let statusBarHovered = $state(false);
  // R2.2: a bridge error is the app explaining why nothing works. It is never
  // something you should have to go looking for.
  const statusBarRevealed = $derived(statusBarHovered || Boolean(bridgeError));
  // The zones unmount when focus mode ends — possibly with the pointer inside
  // one, so no pointerleave arrives. Reset on the mode change, or the next entry
  // into focus mode starts with a bar already showing.
  $effect(() => {
    void readerFocusMode;
    titleBarHovered = false;
    statusBarHovered = false;
  });
</script>

<div class="crt app-shell">
  {#snippet titleBar()}
    <TitleBar
      {vaultStatus}
      {currentPath}
      vaultId={searchVaultId}
      {resolvePaperTitle}
      onOpenResult={onOpenSearchResult}
    />
  {/snippet}

  {#if readerFocusMode}
    <div
      class="edge-zone top"
      class:revealed={titleBarHovered}
      role="presentation"
      onpointerenter={() => (titleBarHovered = true)}
      onpointerleave={() => (titleBarHovered = false)}
    >
      <div class="edge-panel">
        {@render titleBar()}
      </div>
    </div>
  {:else}
    {@render titleBar()}
  {/if}

  <div class="app-body row">
    {#if !readerFocusMode}
      <ActivityRail active={activeMode} {onSelectMode} {onOpenSettings} {settingsAttention} />
    {/if}
    {@render children()}
  </div>

  {#if readerFocusMode}
    <div
      class="edge-zone bottom"
      class:revealed={statusBarRevealed}
      role="presentation"
      onpointerenter={() => (statusBarHovered = true)}
      onpointerleave={() => (statusBarHovered = false)}
    >
      <div class="edge-panel">
        <StatusBar {vaultStatus} {bridgeError} />
      </div>
    </div>
  {:else}
    <StatusBar {vaultStatus} {bridgeError} />
  {/if}
</div>

<style>
  .app-shell {
    width: 100vw;
    height: 100vh;
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }

  .app-body {
    flex: 1;
    min-height: 0;
    align-items: stretch;
  }

  /* RFC 0081 R2.1: 6px of always-live hover strip; the bar itself slides off the
     window edge until the pointer arrives, and slides back over the page rather
     than pushing it. The zone keeps its 6px whether revealed or not, so the
     reading surface below never reflows.

     The bar rides in an absolutely positioned child rather than a clipped
     max-height box because the title bar's search results are an absolutely
     positioned dropdown taller than the bar — `overflow: hidden` here would cut
     them off, which is the one thing this mode may not do (RFC 0081 §4). */
  .edge-zone {
    position: relative;
    flex-shrink: 0;
    height: 6px;
    z-index: 30;
  }

  .edge-panel {
    position: absolute;
    left: 0;
    right: 0;
    transition: transform 120ms ease-out;
  }

  .edge-zone.top .edge-panel {
    top: 0;
    transform: translateY(-100%);
  }

  .edge-zone.bottom .edge-panel {
    bottom: 0;
    transform: translateY(100%);
  }

  .edge-zone.revealed .edge-panel {
    transform: translateY(0);
  }
</style>
