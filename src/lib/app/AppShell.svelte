<script lang="ts">
  import type { Snippet } from "svelte";
  import ActivityRail from "$lib/app/ActivityRail.svelte";
  import StatusBar from "$lib/app/StatusBar.svelte";
  import TitleBar from "$lib/app/TitleBar.svelte";
  import type { VaultStatus } from "$lib/domain/vault";
  import type { ChunkHit } from "$lib/domain/search";
  import RevealHandle from "$lib/components/RevealHandle.svelte";
  import { RevealZone } from "$lib/components/reveal.svelte";

  let {
    activeMode = "P",
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

  // RFC 0081 R2.1: in focus mode the app's own bars collapse to strips at the
  // window edges, so the space is given back to the page rather than dimmed.
  // RFC 0082: each strip is marked, forgiving of an overshoot, and pinnable.
  const titleZone = new RevealZone();
  const statusZone = new RevealZone();
  // RFC 0082 R4.1: and the activity rail, so switching mode no longer costs you
  // focus mode.
  const railZone = new RevealZone();
  // RFC 0081 R2.2: a bridge error is the app explaining why nothing works. It is
  // never something you should have to go looking for.
  const statusRevealed = $derived(statusZone.revealed || Boolean(bridgeError));
  // The zones unmount when focus mode ends — possibly with the pointer inside
  // one, so no pointerleave arrives, and a pin must not outlive the mode
  // (RFC 0082 R3.3).
  $effect(() => {
    void readerFocusMode;
    titleZone.reset();
    statusZone.reset();
    railZone.reset();
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
      class:revealed={titleZone.revealed}
      role="presentation"
      onpointerenter={titleZone.enter}
      onpointerleave={titleZone.leave}
    >
      <RevealHandle edge="top" pinned={titleZone.pinned} label="the title bar" onToggle={titleZone.togglePin} />
      <div class="edge-panel">
        {@render titleBar()}
      </div>
    </div>
  {:else}
    {@render titleBar()}
  {/if}

  <div class="app-body row">
    {#if readerFocusMode}
      <div
        class="edge-zone left"
        class:revealed={railZone.revealed}
        role="presentation"
        onpointerenter={railZone.enter}
        onpointerleave={railZone.leave}
      >
        <RevealHandle edge="left" pinned={railZone.pinned} label="the activity rail" onToggle={railZone.togglePin} />
        <div class="edge-panel">
          <ActivityRail active={activeMode} {onSelectMode} {onOpenSettings} {settingsAttention} />
        </div>
      </div>
    {:else}
      <ActivityRail active={activeMode} {onSelectMode} {onOpenSettings} {settingsAttention} />
    {/if}
    {@render children()}
  </div>

  {#if readerFocusMode}
    <div
      class="edge-zone bottom"
      class:revealed={statusRevealed}
      role="presentation"
      onpointerenter={statusZone.enter}
      onpointerleave={statusZone.leave}
    >
      <RevealHandle edge="bottom" pinned={statusZone.pinned} label="the status bar" onToggle={statusZone.togglePin} />
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
    z-index: 30;
  }

  /* RFC 0082 R1.1: 12px, up from RFC 0081's 6px. A strip you have to aim at is
     a strip you miss. */
  .edge-zone.top,
  .edge-zone.bottom {
    height: 12px;
  }

  .edge-zone.left {
    width: 12px;
  }

  .edge-panel {
    position: absolute;
    transition: transform 120ms ease-out;
  }

  .edge-zone.top .edge-panel,
  .edge-zone.bottom .edge-panel {
    left: 0;
    right: 0;
  }

  .edge-zone.left .edge-panel {
    top: 0;
    bottom: 0;
    left: 0;
    display: flex;
  }

  .edge-zone.top .edge-panel {
    top: 0;
    transform: translateY(-100%);
  }

  .edge-zone.bottom .edge-panel {
    bottom: 0;
    transform: translateY(100%);
  }

  .edge-zone.left .edge-panel {
    transform: translateX(-100%);
  }

  .edge-zone.top.revealed .edge-panel,
  .edge-zone.bottom.revealed .edge-panel {
    transform: translateY(0);
  }

  .edge-zone.left.revealed .edge-panel {
    transform: translateX(0);
  }
</style>
