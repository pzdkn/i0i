<script lang="ts">
  import type { Paper } from "$lib/domain/paper";

  let {
    papers,
    selectedPaperId,
    onSelect,
    onOpen,
  }: {
    papers: Paper[];
    selectedPaperId: string;
    onSelect: (paperId: string) => void;
    onOpen: (paperId: string) => void;
  } = $props();

  function formatCitations(citations: number) {
    if (citations >= 1000) {
      return `${(citations / 1000).toFixed(1)}k`;
    }

    return String(citations);
  }
</script>

<div class="paper-list">
  <div class="paper-header row">
    <span class="year">Year</span>
    <span class="venue">Venue</span>
    <span class="title">Title</span>
    <span class="authors">Authors</span>
    <span class="metric">Cites</span>
    <span class="metric">Note</span>
    <span class="metric">Ann</span>
    <span class="status">Status</span>
  </div>

  <div class="paper-body">
    {#each papers as paper}
      <button
        class:selected={paper.id === selectedPaperId}
        class="paper-row"
        type="button"
        onclick={() => onSelect(paper.id)}
        ondblclick={() => onOpen(paper.id)}
      >
        <span class="year">{paper.year}</span>
        <span class="venue">{paper.venue}</span>
        <span class="title truncate">{paper.title}</span>
        <span class="authors truncate">
          {paper.authors.slice(0, 2).join(", ")}{paper.authors.length > 2 ? ` +${paper.authors.length - 2}` : ""}
        </span>
        <span class="metric">{formatCitations(paper.citations)}</span>
        <span class="metric">{paper.noteCount ? `#${paper.noteCount}` : "-"}</span>
        <span class="metric">{paper.annotationCount || "-"}</span>
        <span class="status">{paper.status}</span>
        {#if paper.id === selectedPaperId}
          <span class="open-hint"><span class="key">dbl</span> read</span>
        {/if}
      </button>
    {/each}
  </div>
</div>

<style>
  .paper-list {
    min-height: 0;
    flex: 1;
    display: flex;
    flex-direction: column;
  }

  .paper-header {
    height: 22px;
    flex-shrink: 0;
    gap: 12px;
    padding: 0 12px;
    border-bottom: 1px solid var(--border);
    background: var(--bg-1);
    color: var(--fg-3);
    font-size: 9px;
    letter-spacing: 0.12em;
    text-transform: uppercase;
  }

  .paper-body {
    min-height: 0;
    flex: 1;
    overflow: auto;
  }

  .paper-row {
    position: relative;
    width: 100%;
    height: 30px;
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 0 12px;
    border: 0;
    border-bottom: 1px solid var(--border);
    border-left: 2px solid transparent;
    background: transparent;
    color: var(--fg);
    font-size: 12px;
    text-align: left;
    cursor: pointer;
  }

  .paper-row:hover {
    background: rgba(242, 169, 59, 0.04);
  }

  .paper-row.selected {
    border-left-color: var(--amber);
    background: rgba(242, 169, 59, 0.1);
    color: var(--amber);
  }

  .year {
    width: 36px;
    flex-shrink: 0;
    color: var(--fg-3);
    font-size: 10px;
  }

  .venue {
    width: 62px;
    flex-shrink: 0;
    color: var(--fg-2);
    font-size: 10px;
  }

  .title {
    flex: 1;
    min-width: 120px;
    font-weight: 500;
  }

  .authors {
    width: 140px;
    flex-shrink: 0;
    color: var(--fg-3);
    font-size: 10px;
  }

  .metric {
    width: 38px;
    flex-shrink: 0;
    color: var(--fg-3);
    font-size: 10px;
    text-align: right;
  }

  .status {
    width: 58px;
    flex-shrink: 0;
    color: var(--fg-3);
    font-size: 9px;
    letter-spacing: 0.08em;
    text-align: right;
  }

  .open-hint {
    position: absolute;
    right: 8px;
    height: 18px;
    display: inline-flex;
    align-items: center;
    gap: 5px;
    padding: 0 6px;
    border: 1px solid var(--amber-dim);
    background: var(--bg);
    color: var(--amber);
    font-size: 9px;
    letter-spacing: 0.08em;
    text-transform: uppercase;
  }
</style>
