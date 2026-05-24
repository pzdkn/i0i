<script lang="ts">
  import type { DiscoverWorkspace } from "$lib/domain/discover";
  import DiscoverFeed from "$lib/features/discover/DiscoverFeed.svelte";
  import DiscoverInspector from "$lib/features/discover/DiscoverInspector.svelte";
  import DiscoverSeedBar from "$lib/features/discover/DiscoverSeedBar.svelte";

  let {
    workspace,
    onOpenCandidate,
  }: {
    workspace: DiscoverWorkspace;
    onOpenCandidate: (candidateId: string) => void;
  } = $props();
</script>

<section class="discover-workspace col">
  <DiscoverSeedBar {workspace} />
  <div class="discover-body row">
    <DiscoverFeed {workspace} {onOpenCandidate} />
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
