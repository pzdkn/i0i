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
</script>

<div class="crt app-shell">
  <TitleBar
    {vaultStatus}
    {currentPath}
    vaultId={searchVaultId}
    {resolvePaperTitle}
    onOpenResult={onOpenSearchResult}
  />
  <div class="app-body row">
    {#if !readerFocusMode}
      <ActivityRail active={activeMode} {onSelectMode} {onOpenSettings} {settingsAttention} />
    {/if}
    {@render children()}
  </div>
  <StatusBar {vaultStatus} {bridgeError} />
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
</style>
