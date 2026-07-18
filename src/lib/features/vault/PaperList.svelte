<script lang="ts">
  import type { MetadataAutofillProgress } from "$lib/domain/library";
  import type { Paper } from "$lib/domain/paper";

  let {
    papers,
    selectedPaperId,
    autofillingMetadataPaperIds,
    metadataAutofillProgressByPaperId = {},
    onSelect,
    onOpen,
    onAutofillMetadata,
    onRemoveFromVault,
    onRemoveFromLibrary,
  }: {
    papers: Paper[];
    selectedPaperId: string;
    autofillingMetadataPaperIds: string[];
    metadataAutofillProgressByPaperId?: Record<string, MetadataAutofillProgress>;
    onSelect: (paperId: string) => void;
    onOpen: (paperId: string) => void;
    onAutofillMetadata: (paperId: string) => void | Promise<void>;
    onRemoveFromVault: (paperId: string) => void;
    onRemoveFromLibrary: (paperId: string) => void;
  } = $props();

  let contextMenu = $state<{ x: number; y: number; paperId: string } | null>(null);

  function formatCitations(citations: number) {
    if (citations >= 1000) {
      return `${(citations / 1000).toFixed(1)}k`;
    }

    return String(citations);
  }

  function showContextMenu(event: MouseEvent, paperId: string) {
    event.preventDefault();
    event.stopPropagation();
    contextMenu = {
      x: event.clientX,
      y: event.clientY,
      paperId,
    };
  }

  function closeContextMenu() {
    contextMenu = null;
  }

  function handleWindowKeydown(event: KeyboardEvent) {
    if (event.key === "Escape") {
      closeContextMenu();
    }
  }

  function removeFromVault(paperId: string) {
    closeContextMenu();
    onRemoveFromVault(paperId);
  }

  function removeFromLibrary(paperId: string) {
    closeContextMenu();
    onRemoveFromLibrary(paperId);
  }

  function autofillMetadata(paperId: string) {
    closeContextMenu();
    onAutofillMetadata(paperId);
  }

  // Compact per-row autofill status chip (RFC 0049); hidden once applied.
  function autofillChip(paperId: string): string | undefined {
    if (autofillingMetadataPaperIds.includes(paperId)) {
      return "autofilling";
    }

    const progress = metadataAutofillProgressByPaperId[paperId];
    if (!progress) {
      return undefined;
    }

    switch (progress.status) {
      case "running":
        return "autofilling";
      case "needs_review":
        return `review (${progress.candidates.length})`;
      case "no_match":
        return "no match";
      case "failed":
        return "failed";
      default:
        return undefined;
    }
  }
</script>

<svelte:window onclick={closeContextMenu} onkeydown={handleWindowKeydown} />

<div class="paper-list">
  <div class="paper-header row">
    <span class="year">Year</span>
    <span class="venue">Venue</span>
    <span class="title">Title</span>
    <span class="authors">Authors</span>
    <span class="metric">Cites</span>
    <span class="metric">Pins</span>
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
        oncontextmenu={(event) => showContextMenu(event, paper.id)}
      >
        <span class="year">{paper.year}</span>
        <span class="venue truncate" title={paper.venue}>{paper.venue}</span>
        <span class="title truncate">
          {paper.title}
          {#if autofillChip(paper.id)}
            <em class="autofill-chip" class:review={autofillChip(paper.id)?.startsWith("review")}>
              {autofillChip(paper.id)}
            </em>
          {/if}
        </span>
        <span class="authors truncate">
          {paper.authors.slice(0, 2).join(", ")}{paper.authors.length > 2 ? ` +${paper.authors.length - 2}` : ""}
        </span>
        <span class="metric">{formatCitations(paper.citations)}</span>
        <span class="metric">{paper.highlightCount ? `#${paper.highlightCount}` : "-"}</span>
        <span class="metric">{paper.annotationCount || "-"}</span>
        <span class="status">{paper.status}</span>
      </button>
    {/each}
  </div>

  {#if contextMenu}
    {@const menuPaperId = contextMenu.paperId}
    {@const menuPaper = papers.find((paper) => paper.id === menuPaperId)}
    {@const canAutofillMetadata = menuPaper?.tags.includes("needs-review") ?? false}
    {@const isAutofillingMetadata = autofillingMetadataPaperIds.includes(menuPaperId)}
    <div
      class="context-menu col"
      role="menu"
      tabindex="-1"
      style={`left: ${contextMenu.x}px; top: ${contextMenu.y}px;`}
      onclick={(event) => event.stopPropagation()}
      onkeydown={(event) => event.stopPropagation()}
    >
      {#if canAutofillMetadata}
        <button
          role="menuitem"
          type="button"
          disabled={isAutofillingMetadata}
          onclick={() => autofillMetadata(menuPaperId)}
        >
          {isAutofillingMetadata ? "Autofilling..." : "Autofill metadata"}
        </button>
      {/if}
      <button role="menuitem" type="button" onclick={() => removeFromVault(menuPaperId)}>
        Remove from vault
      </button>
      <button role="menuitem" class="danger" type="button" onclick={() => removeFromLibrary(menuPaperId)}>
        Remove from library
      </button>
    </div>
  {/if}
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

  .autofill-chip {
    margin-left: 6px;
    border: 1px solid var(--border-2);
    padding: 0 5px;
    color: var(--fg-3);
    font-size: 9px;
    font-style: normal;
    text-transform: uppercase;
  }

  .autofill-chip.review {
    border-color: var(--amber-dim);
    color: var(--amber);
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

  .context-menu {
    position: fixed;
    z-index: 20;
    min-width: 168px;
    padding: 5px;
    border: 1px solid var(--border-2);
    border-radius: 3px;
    background: var(--bg-1);
    box-shadow: 0 12px 32px rgba(0, 0, 0, 0.38);
  }

  .context-menu button {
    height: 26px;
    width: 100%;
    padding: 0 9px;
    border: 0;
    border-radius: 2px;
    background: transparent;
    color: var(--fg-1);
    font: inherit;
    font-size: 11px;
    text-align: left;
    cursor: pointer;
  }

  .context-menu button:hover {
    background: rgba(242, 169, 59, 0.08);
    color: var(--amber);
  }

  .context-menu button:disabled {
    cursor: default;
    color: var(--fg-3);
    opacity: 0.72;
  }

  .context-menu button:disabled:hover {
    background: transparent;
    color: var(--fg-3);
  }

  .context-menu .danger {
    margin-top: 4px;
    border-top: 1px solid var(--border);
    border-radius: 0 0 2px 2px;
    color: var(--red);
  }
</style>
