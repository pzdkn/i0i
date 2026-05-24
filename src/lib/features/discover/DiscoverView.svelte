<script lang="ts">
  import type { DiscoverWorkspace } from "$lib/domain/discover";
  import type { VaultWorkspace } from "$lib/mock/vault-workspaces";
  import DiscoverFeed from "$lib/features/discover/DiscoverFeed.svelte";
  import DiscoverInspector from "$lib/features/discover/DiscoverInspector.svelte";
  import DiscoverSeedBar from "$lib/features/discover/DiscoverSeedBar.svelte";

  let {
    workspace,
    vaults,
    onOpenCandidate,
    onAddCandidate,
    getCandidateVaultTargets,
  }: {
    workspace: DiscoverWorkspace;
    vaults: VaultWorkspace[];
    onOpenCandidate: (candidateId: string) => void;
    onAddCandidate: (candidateId: string, vaultIds: string[]) => void;
    getCandidateVaultTargets: (candidateId: string) => VaultWorkspace[];
  } = $props();
</script>

<section class="discover-workspace col">
  <DiscoverSeedBar {workspace} />
  <div class="discover-body row">
    <DiscoverFeed {workspace} {vaults} {onOpenCandidate} {onAddCandidate} {getCandidateVaultTargets} />
    <DiscoverInspector />
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
