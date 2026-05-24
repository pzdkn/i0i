<script lang="ts">
  import type { Snippet } from "svelte";
  import ActivityRail from "$lib/app/ActivityRail.svelte";
  import StatusBar from "$lib/app/StatusBar.svelte";
  import TitleBar from "$lib/app/TitleBar.svelte";
  import type { VaultStatus } from "$lib/domain/vault";

  let {
    activeMode = "V",
    vaultStatus = null,
    bridgeError = "",
    children,
  }: {
    activeMode?: string;
    vaultStatus?: VaultStatus | null;
    bridgeError?: string;
    children: Snippet;
  } = $props();
</script>

<div class="crt app-shell">
  <TitleBar {vaultStatus} />
  <div class="app-body row">
    <ActivityRail active={activeMode} />
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
