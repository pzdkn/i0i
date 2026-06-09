<script lang="ts">
  import type { ReaderDocument } from "$lib/domain/reader";

  let {
    document,
  }: {
    document: ReaderDocument;
  } = $props();
</script>

<header class="reader-header hair-b">
  <div class="row meta-line">
    <span class="label">Paper</span>
    <span class="mono-dim">{document.identifier} / {document.venue} {document.year}</span>
    {#each document.tags as tag}
      <span class="chip">{tag}</span>
    {/each}
    <div class="flex1"></div>
  </div>

  <div class="row title-line">
    <h1>{document.title}</h1>
    <span class="authors truncate">{document.authors.join(" / ")}</span>
  </div>

  <div class="row action-line">
    <button class="btn" type="button">Graph</button>
    <button class="btn" type="button">Ask</button>

    <div class="flex1"></div>
  </div>
</header>

<style>
  .reader-header {
    flex-shrink: 0;
    padding: 12px 18px 10px;
    background: var(--bg-1);
  }

  .meta-line {
    gap: 8px;
    margin-bottom: 5px;
    font-size: 10px;
  }

  .title-line {
    gap: 12px;
    align-items: baseline;
  }

  h1 {
    margin: 0;
    color: var(--amber);
    font-size: 18px;
    font-weight: 600;
  }

  .authors {
    min-width: 0;
    color: var(--fg-2);
    font-size: 11px;
  }

  .action-line {
    gap: 10px;
    margin-top: 10px;
    color: var(--fg-3);
    font-size: 10px;
  }
</style>
