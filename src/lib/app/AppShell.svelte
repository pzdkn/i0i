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
      class="edge-zone"
      class:revealed={titleBarHovered}
      role="presentation"
      onpointerenter={() => (titleBarHovered = true)}
      onpointerleave={() => (titleBarHovered = false)}
    >
      {@render titleBar()}
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
      class="edge-zone"
      class:revealed={statusBarRevealed}
      role="presentation"
      onpointerenter={() => (statusBarHovered = true)}
      onpointerleave={() => (statusBarHovered = false)}
    >
      <StatusBar {vaultStatus} {bridgeError} />
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

  /* RFC 0081 R2.1: 6px of always-live hover strip; the bar itself is collapsed
     above (or below) the fold until the pointer arrives. Height rather than
     visibility, so the reading surface actually gains the space. */
  .edge-zone {
    flex-shrink: 0;
    max-height: 6px;
    overflow: hidden;
    transition: max-height 120ms ease-out;
  }

  .edge-zone.revealed {
    max-height: 120px;
  }
</style>
