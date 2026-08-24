<script lang="ts">
  import type { BrowserRuntimeStatus, DiscoverWorkspace } from "$lib/domain/discover";
  import type { VaultWorkspace } from "$lib/domain/library";
  import DiscoverTargetEditor from "$lib/features/discover/DiscoverTargetEditor.svelte";
  import { LoaderCircle, RefreshCw } from "@lucide/svelte";

  let {
    workspace,
    vaults,
    onSelectCandidate,
    onOpenCandidate,
    onAddCandidate,
    getCandidateVaultTargets,
    browserStatus,
    onRetryBrowser,
  }: {
    workspace: DiscoverWorkspace;
    vaults: VaultWorkspace[];
    onSelectCandidate: (discoverId: string, candidateId: string) => void;
    onOpenCandidate: (candidateId: string) => void;
    onAddCandidate: (candidateId: string, vaultIds: string[]) => void;
    getCandidateVaultTargets: (candidateId: string) => VaultWorkspace[];
    browserStatus: BrowserRuntimeStatus;
    onRetryBrowser: () => void;
  } = $props();

  let editingCandidateId = $state("");

  function formatCitations(citations: number) {
    if (citations >= 1000) {
      return `${(citations / 1000).toFixed(1)}k`;
    }

    return String(citations);
  }

  function handleCandidateKeydown(event: KeyboardEvent, candidateId: string) {
    if (event.key === "Enter" || event.key === " ") {
      event.preventDefault();
      onSelectCandidate(workspace.id, candidateId);
    }
  }

  function openTargetEditor(event: MouseEvent, candidateId: string) {
    event.stopPropagation();
    editingCandidateId = editingCandidateId === candidateId ? "" : candidateId;
  }

  function closeTargetEditor() {
    editingCandidateId = "";
  }

  function targetVaultIds(candidateId: string) {
    return getCandidateVaultTargets(candidateId).map((vault) => vault.id);
  }

  function addCandidateToVault(candidateId: string, vaultId: string) {
    onAddCandidate(candidateId, [vaultId]);
    closeTargetEditor();
  }
</script>

<section class="discover-feed col">
  <div class="feed-header row">
    <span class="rank">Rank</span>
    <span class="paper">Candidate</span>
    <span class="meta">Meta</span>
    <span class="actions-label">Actions</span>
  </div>

  <div class="feed-body">
    {#if browserStatus.state !== "ready" && workspace.candidates.length === 0}
      <div class="empty-state terminal-empty browser-state col">
        {#if browserStatus.state === "failed"}
          <div class="label hot">Browser unavailable</div>
          <p>{browserStatus.message ?? "Browser could not start."}</p>
          <button class="btn retry" type="button" onclick={onRetryBrowser}>
            <RefreshCw size={14} aria-hidden="true" /> Retry
          </button>
        {:else}
          <LoaderCircle class="browser-spinner" size={20} aria-hidden="true" />
          <div class="prompt-line row"><span>&gt;</span><em>starting browser...</em></div>
        {/if}
      </div>
    {:else if workspace.status === "idle" && workspace.candidates.length === 0}
      <div class="empty-state terminal-empty col">
        <div class="prompt-line row"><span>&gt;</span><em>find papers about...</em></div>
      </div>
    {:else if workspace.status === "running" && workspace.candidates.length === 0}
      <div class="empty-state terminal-empty col">
        <div class="prompt-line row"><span>&gt;</span><em>{workspace.deep ? "deep research running" : "search running"}</em></div>
      </div>
    {:else if workspace.status === "failed" && workspace.candidates.length === 0}
      <div class="empty-state col">
        <div class="label hot">Search failed</div>
        <p>{workspace.error}</p>
      </div>
    {:else if workspace.status === "completed" && workspace.candidates.length === 0}
      <div class="empty-state col">
        <div class="label hot">No matching papers</div>
        <p>Try a broader query, wider year range, or disabling open access.</p>
      </div>
    {/if}

    {#each workspace.candidates as candidate, index}
      {@const targets = getCandidateVaultTargets(candidate.id)}
      <div
        class:owned={candidate.owned}
        class:selected={workspace.selectedCandidateId === candidate.id}
        class="candidate-row"
        role="button"
        tabindex="0"
        onclick={() => onSelectCandidate(workspace.id, candidate.id)}
        onkeydown={(event) => handleCandidateKeydown(event, candidate.id)}
        ondblclick={() => {
          if (editingCandidateId !== candidate.id) {
            onOpenCandidate(candidate.id);
          }
        }}
      >
        <span class="rank">#{index + 1}</span>
        <span class="paper col">
          <span class="title-line row">
            <strong class="truncate">{candidate.title}</strong>
            {#if candidate.isNew}
              <em>new</em>
            {/if}
            {#if candidate.owned}
              <em>vault</em>
            {/if}
            {#if candidate.reviewing}
              <em>reviewing</em>
            {/if}
          </span>
          <span class="authors truncate">{candidate.authors.slice(0, 3).join(", ")}</span>
          <span class="why truncate">{candidate.why}</span>
          <span class="tags row">
            {#each candidate.tags as tag}
              <span>{tag}</span>
            {/each}
            {#if candidate.pdfAvailability === "verified"}
              <span class="pdf-tag">PDF ready</span>
            {:else if candidate.pdfAvailability === "browser_required"}
              <span class="pdf-tag">PDF needs browser</span>
            {:else if candidate.pdfAvailability === "unavailable"}
              <span class="pdf-tag">no PDF found</span>
            {:else if candidate.pdfUrl}
              <span class="pdf-tag">PDF</span>
            {/if}
          </span>
        </span>
        <span class="meta col">
          <span>{candidate.venue} {candidate.year || ""}</span>
          <span>{formatCitations(candidate.citations)} cites</span>
          {#if candidate.openAccess?.isOpenAccess}
            <span>open access</span>
          {/if}
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
          {/if}
          <button
            class="action icon-action"
            type="button"
            title="Add to vault"
            aria-label="Add to vault"
            onclick={(event) => openTargetEditor(event, candidate.id)}
          >
            +
          </button>
          <button class="action" type="button" onclick={() => onOpenCandidate(candidate.id)}>Open</button>
          {#if editingCandidateId === candidate.id}
            <DiscoverTargetEditor
              {vaults}
              excludedVaultIds={targetVaultIds(candidate.id)}
              onCancel={closeTargetEditor}
              onSelect={(vaultId) => addCandidateToVault(candidate.id, vaultId)}
            />
          {/if}
        </span>
      </div>
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

  .browser-state {
    gap: 8px;
  }

  :global(.browser-spinner) {
    color: var(--amber);
    animation: browser-spin 0.9s linear infinite;
  }

  .retry {
    gap: 6px;
    align-self: center;
  }

  @keyframes browser-spin {
    to { transform: rotate(360deg); }
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

  .candidate-row.selected {
    border-left-color: var(--amber);
    background: rgba(242, 169, 59, 0.07);
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

  .pdf-tag {
    border-color: var(--cyan) !important;
    color: var(--cyan) !important;
    font-weight: 600;
  }

  .meta {
    width: 86px;
    flex-shrink: 0;
    gap: 5px;
    color: var(--fg-3);
    font-size: 10px;
  }

  .actions-label {
    width: 118px;
    flex-shrink: 0;
  }

  .row-actions {
    position: relative;
    width: 150px;
    flex-shrink: 0;
    align-content: flex-start;
    align-items: flex-start;
    justify-content: flex-end;
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

  .icon-action {
    width: 22px;
    padding: 0;
    color: var(--amber);
    font-size: 12px;
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

  .empty-state {
    height: 100%;
    align-items: center;
    justify-content: center;
    gap: 8px;
    padding: 24px;
    color: var(--fg-3);
    text-align: center;
  }

  .empty-state p {
    max-width: 360px;
    margin: 0;
    color: var(--fg-2);
    font-size: 12px;
  }

  .terminal-empty {
    text-align: left;
  }

  .prompt-line {
    gap: 8px;
    color: var(--fg-3);
    font-size: 12px;
  }

  .prompt-line span {
    color: var(--green);
  }

  .prompt-line em {
    font-style: normal;
  }

</style>
