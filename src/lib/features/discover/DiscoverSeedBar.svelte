<script lang="ts">
  import type { DiscoverWorkspace } from "$lib/domain/discover";

  let {
    workspace,
  }: {
    workspace: DiscoverWorkspace;
  } = $props();

  let query = $state("");
</script>

<header class="discover-seed-bar hair-b col">
  <div class="heading row">
    <div>
      <div class="label">Discover</div>
      <h1>{workspace.title}</h1>
    </div>
    <span class="mono-dim">mock ranking / 7 candidates / no live search</span>
    <div class="flex1"></div>
    <div class="actions row">
      <button class="btn" type="button">Auto-run</button>
      <button class="btn" type="button">Saved</button>
      <button class="btn" type="button">Export .bib</button>
      <button class="btn primary" type="button">Add starred 5</button>
    </div>
  </div>

  <div class="seed-row row">
    <span class="label hot">Seeds</span>
    {#each workspace.seeds as seed, index}
      <span class:hot={index === 0} class="chip">{seed}</span>
    {/each}
    <label class="seed-input row">
      <span>+</span>
      <input bind:value={query} aria-label="Discover seed query" placeholder="paper, author, tag, folder..." />
    </label>
    <div class="flex1"></div>
    <span class="mono-dim">rerank semantic 0.45 / graph 0.35 / novelty 0.20</span>
  </div>
</header>

<style>
  .discover-seed-bar {
    flex-shrink: 0;
    gap: 12px;
    padding: 14px 18px 12px;
    background: var(--bg-1);
  }

  .heading {
    gap: 12px;
    align-items: baseline;
  }

  h1 {
    margin: 2px 0 0;
    color: var(--amber);
    font-size: 22px;
    font-weight: 600;
  }

  .actions {
    gap: 6px;
  }

  .seed-row {
    gap: 6px;
    min-width: 0;
    overflow: hidden;
    white-space: nowrap;
  }

  .seed-input {
    height: 20px;
    min-width: 220px;
    gap: 6px;
    padding: 0 7px;
    border: 1px solid var(--border-2);
    background: var(--bg);
    color: var(--amber-mid);
    font-size: 10px;
  }

  .seed-input input {
    width: 190px;
    min-width: 0;
    border: 0;
    outline: none;
    background: transparent;
    color: var(--fg-1);
    font: inherit;
  }
</style>
