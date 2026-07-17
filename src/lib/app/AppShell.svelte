<script lang="ts">
  import type { Snippet } from "svelte";
  import ActivityRail from "$lib/app/ActivityRail.svelte";
  import StatusBar from "$lib/app/StatusBar.svelte";
  import TitleBar from "$lib/app/TitleBar.svelte";
  import type { VaultStatus } from "$lib/domain/vault";

  let {
    activeMode = "V",
    currentPath = "/transformers/attention",
    vaultStatus = null,
    bridgeError = "",
    readerFocusMode = false,
    onSelectMode = () => {},
    children,
  }: {
    activeMode?: string;
    currentPath?: string;
    vaultStatus?: VaultStatus | null;
    bridgeError?: string;
    readerFocusMode?: boolean;
    onSelectMode?: (mode: string) => void;
    children: Snippet;
  } = $props();
</script>

<div class="crt app-shell">
  <TitleBar {vaultStatus} {currentPath} />
  <div class="app-body row">
    {#if !readerFocusMode}
      <ActivityRail active={activeMode} {onSelectMode} />
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
