<script lang="ts">
  import type { DiscoverWorkspace } from "$lib/domain/discover";
  import type { VaultWorkspace } from "$lib/domain/library";
  import DiscoverTargetEditor from "$lib/features/discover/DiscoverTargetEditor.svelte";

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

  let editingCandidateId = $state("");

  function formatCitations(citations: number) {
    if (citations >= 1000) {
      return `${(citations / 1000).toFixed(1)}k`;
    }

    return String(citations);
  }
</script>

<section class="discover-feed col">
  <div class="feed-header row">
    <span class="rank">Rank</span>
    <span class="score">Score</span>
    <span class="paper">Candidate</span>
    <span class="meta">Meta</span>
    <span class="actions-label">Actions</span>
  </div>

  <div class="feed-body">
    {#each workspace.candidates as candidate, index}
      {@const targets = getCandidateVaultTargets(candidate.id)}
      <article
        class:owned={candidate.owned}
        class="candidate-row"
        ondblclick={() => {
          if (editingCandidateId !== candidate.id) {
            onOpenCandidate(candidate.id);
          }
        }}
      >
        <span class="rank">#{index + 1}</span>
        <span class="score">
          {(candidate.score * 100).toFixed(0)}
          <i style={`width: ${candidate.score * 100}%`}></i>
        </span>
        <span class="paper col">
          <span class="title-line row">
            <strong class="truncate">{candidate.title}</strong>
            {#if candidate.isNew}
              <em>new</em>
            {/if}
            {#if candidate.owned}
              <em>vault</em>
            {/if}
          </span>
          <span class="authors truncate">{candidate.authors.slice(0, 3).join(", ")}</span>
          <span class="why truncate">{candidate.why}</span>
          <span class="tags row">
            {#each candidate.tags as tag}
              <span>{tag}</span>
            {/each}
          </span>
        </span>
        <span class="meta col">
          <span>{candidate.venue} {candidate.year}</span>
          <span>{formatCitations(candidate.citations)} cites</span>
        </span>
        <span class="row-actions row">
          {#if candidate.owned}
            <span class="target-summary row">
              <span class="owned-label">In Vault</span>
              {#each targets.slice(0, 2) as vault}
                <span class="target-chip">{vault.path}</span>
              {/each}
              {#if targets.length > 2}
                <span class="target-chip">+{targets.length - 2}</span>
              {/if}
            </span>
            <button
              class="action"
              type="button"
              onclick={(event) => {
                event.stopPropagation();
                editingCandidateId = candidate.id;
              }}
            >
              Add target
            </button>
          {:else}
            <button
              class="action"
              type="button"
              onclick={(event) => {
                event.stopPropagation();
                editingCandidateId = candidate.id;
              }}
            >
              Add
            </button>
          {/if}
          <button class="action" type="button">Preview</button>
          <button class="action" type="button">Graph</button>
          {#if candidate.owned}
            <button class="action" type="button" onclick={() => onOpenCandidate(candidate.id)}>Open</button>
          {/if}
          {#if editingCandidateId === candidate.id}
            <DiscoverTargetEditor
              {vaults}
              initialVaultIds={targets.map((vault) => vault.id)}
              onCancel={() => (editingCandidateId = "")}
              onConfirm={(vaultIds) => {
                onAddCandidate(candidate.id, vaultIds);
                editingCandidateId = "";
              }}
            />
          {/if}
        </span>
      </article>
    {/each}
  </div>
</section>

<style>
  .discover-feed {
    flex: 1;
    min-width: 0;
    min-height: 0;
    background: var(--bg);
  }

  .feed-header {
    height: 24px;
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

  .feed-body {
    min-height: 0;
    flex: 1;
    overflow: auto;
  }

  .candidate-row {
    width: 100%;
    min-height: 92px;
    display: flex;
    align-items: stretch;
    gap: 12px;
    padding: 12px;
    border: 0;
    border-bottom: 1px solid var(--border);
    border-left: 2px solid transparent;
    background: transparent;
    color: var(--fg);
    text-align: left;
    cursor: pointer;
  }

  .candidate-row:hover {
    border-left-color: var(--amber-dim);
    background: rgba(242, 169, 59, 0.04);
  }

  .candidate-row.owned {
    background: rgba(138, 168, 74, 0.05);
  }

  .rank {
    width: 42px;
    flex-shrink: 0;
    color: var(--amber-mid);
    font-size: 11px;
  }

  .score {
    position: relative;
    width: 52px;
    flex-shrink: 0;
    color: var(--amber);
    font-size: 11px;
  }

  .score i {
    position: absolute;
    left: 0;
    bottom: 16px;
    height: 3px;
    background: var(--amber);
  }

  .paper {
    flex: 1;
    min-width: 0;
    gap: 5px;
  }

  .title-line {
    min-width: 0;
    gap: 6px;
  }

  .title-line strong {
    min-width: 0;
    color: var(--fg);
    font-size: 13px;
    font-weight: 600;
  }

  .title-line em {
    flex-shrink: 0;
    padding: 1px 4px;
    border: 1px solid var(--amber-dim);
    color: var(--amber);
    font-size: 9px;
    font-style: normal;
    text-transform: uppercase;
  }

  .authors,
  .why {
    color: var(--fg-3);
    font-size: 10px;
  }

  .why {
    color: var(--fg-2);
  }

  .tags {
    gap: 5px;
    flex-wrap: wrap;
  }

  .tags span {
    border: 1px solid var(--border-2);
    padding: 1px 5px;
    color: var(--fg-2);
    font-size: 9px;
  }

  .meta {
    width: 86px;
    flex-shrink: 0;
    gap: 5px;
    color: var(--fg-3);
    font-size: 10px;
  }

  .actions-label {
    width: 180px;
    flex-shrink: 0;
  }

  .row-actions {
    width: 180px;
    flex-shrink: 0;
    align-content: flex-start;
    align-items: flex-start;
    gap: 6px;
    flex-wrap: wrap;
  }

  .action {
    height: 18px;
    border: 1px solid var(--border-2);
    background: transparent;
    padding: 1px 6px;
    color: var(--fg-2);
    font-size: 9px;
    cursor: pointer;
  }

  .action:hover {
    border-color: var(--amber-dim);
    color: var(--amber);
  }

  .target-summary {
    width: 100%;
    gap: 4px;
    flex-wrap: wrap;
  }

  .owned-label,
  .target-chip {
    height: 18px;
    border: 1px solid var(--border-2);
    padding: 1px 5px;
    color: var(--fg-2);
    font-size: 9px;
  }

  .owned-label {
    border-color: var(--green);
    color: var(--green);
  }
</style>
