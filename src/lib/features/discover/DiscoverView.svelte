<script lang="ts">
  import type { DiscoverWorkspace } from "$lib/domain/discover";
  import type { VaultWorkspace } from "$lib/domain/library";
  import ResizableSplit from "$lib/components/layout/ResizableSplit.svelte";
  import DiscoverFeed from "$lib/features/discover/DiscoverFeed.svelte";
  import DiscoverInspector from "$lib/features/discover/DiscoverInspector.svelte";
  import DiscoverSeedBar from "$lib/features/discover/DiscoverSeedBar.svelte";

  let {
    workspace,
    discoverWorkspaces,
    vaults,
    onNewSearch,
    onActivateSearch,
    onRunSearch,
    onImproveSearch,
    onSelectCandidate,
    onOpenCandidate,
    onAddCandidate,
    getCandidateVaultTargets,
  }: {
    workspace: DiscoverWorkspace;
    discoverWorkspaces: DiscoverWorkspace[];
    vaults: VaultWorkspace[];
    onNewSearch: () => void;
    onActivateSearch: (discoverId: string) => void;
    onRunSearch: (discoverId: string) => void;
    onImproveSearch: (discoverId: string) => void;
    onSelectCandidate: (discoverId: string, candidateId: string) => void;
    onOpenCandidate: (candidateId: string) => void;
    onAddCandidate: (candidateId: string, vaultIds: string[]) => void;
    getCandidateVaultTargets: (candidateId: string) => VaultWorkspace[];
  } = $props();
</script>

<section class="discover-workspace col">
  <DiscoverSeedBar
    {workspace}
    {discoverWorkspaces}
    {onNewSearch}
    {onActivateSearch}
    {onRunSearch}
    {onImproveSearch}
  />
  <div class="discover-body row">
    <ResizableSplit
      storageKey="i0i.discover-split"
      panes={[
        { id: "feed", min: 320, default: 900 },
        { id: "inspector", min: 220, default: 300 },
      ]}
    >
      {#snippet pane(id: string)}
        {#if id === "feed"}
          <DiscoverFeed
            {workspace}
            {vaults}
            {onSelectCandidate}
            {onOpenCandidate}
            {onAddCandidate}
            {getCandidateVaultTargets}
          />
        {:else}
          <DiscoverInspector {workspace} />
        {/if}
      {/snippet}
    </ResizableSplit>
  </div>
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
</style>
