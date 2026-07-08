<script lang="ts">
  import type { DiscoverWorkspace } from "$lib/domain/discover";
  import type { VaultWorkspace } from "$lib/domain/library";
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
