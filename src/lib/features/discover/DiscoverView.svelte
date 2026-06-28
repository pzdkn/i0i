<script lang="ts">
  import type { DiscoverWorkspace } from "$lib/domain/discover";
  import type { VaultWorkspace } from "$lib/domain/library";
  import DiscoverFeed from "$lib/features/discover/DiscoverFeed.svelte";
  import DiscoverInspector from "$lib/features/discover/DiscoverInspector.svelte";
  import DiscoverSeedBar from "$lib/features/discover/DiscoverSeedBar.svelte";
  import ResearchView from "$lib/features/discover/ResearchView.svelte";

  let mode = $state<"quick" | "deep">("quick");

  let {
    workspace,
    vaults,
    onNewSearch,
    onRunSearch,
    onSelectCandidate,
    onOpenCandidate,
    onAddCandidate,
    getCandidateVaultTargets,
  }: {
    workspace: DiscoverWorkspace;
    vaults: VaultWorkspace[];
    onNewSearch: () => void;
    onRunSearch: (discoverId: string) => void;
    onSelectCandidate: (discoverId: string, candidateId: string) => void;
    onOpenCandidate: (candidateId: string) => void;
    onAddCandidate: (candidateId: string, vaultIds: string[]) => void;
    getCandidateVaultTargets: (candidateId: string) => VaultWorkspace[];
  } = $props();
</script>

<section class="discover-workspace col">
  <div class="mode-toggle row">
    <button class="mode" class:active={mode === "quick"} type="button" onclick={() => (mode = "quick")}>
      Quick
    </button>
    <button class="mode" class:active={mode === "deep"} type="button" onclick={() => (mode = "deep")}>
      Deep
    </button>
  </div>
  {#if mode === "quick"}
    <DiscoverSeedBar {workspace} {onNewSearch} {onRunSearch} />
    <div class="discover-body row">
      <DiscoverFeed
        {workspace}
        {vaults}
        {onSelectCandidate}
        {onOpenCandidate}
        {onAddCandidate}
        {getCandidateVaultTargets}
      />
      <DiscoverInspector {workspace} />
    </div>
  {:else}
    <ResearchView />
  {/if}
</section>

<style>
  .discover-workspace {
    flex: 1;
    min-width: 0;
    min-height: 0;
    background: var(--bg);
  }

  .discover-body {
    flex: 1;
    min-width: 0;
    min-height: 0;
    align-items: stretch;
  }

  .mode-toggle {
    flex-shrink: 0;
    gap: 0;
    padding: 8px 18px 0;
    background: var(--bg-1);
  }

  .mode {
    border: 1px solid var(--border-2);
    background: var(--bg);
    color: var(--fg-2);
    padding: 4px 14px;
    font: inherit;
    font-size: 11px;
    text-transform: uppercase;
    cursor: pointer;
  }

  .mode.active {
    color: var(--amber);
    border-color: var(--amber-dim);
    background: rgba(242, 169, 59, 0.05);
  }
</style>
