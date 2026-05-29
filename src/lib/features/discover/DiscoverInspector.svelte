<script lang="ts">
  import type { DiscoverWorkspace } from "$lib/domain/discover";

  let {
    workspace,
  }: {
    workspace: DiscoverWorkspace;
  } = $props();

  const selectedCandidate = $derived(
    workspace.candidates.find((candidate) => candidate.id === workspace.selectedCandidateId),
  );

  function providerLabel(provider?: string) {
    if (!provider) {
      return "OpenAlex";
    }

    return provider === "openalex" ? "OpenAlex" : provider;
  }
</script>

<aside class="discover-inspector col hair-l">
  <header class="panel-header hair-b">
    <div class="label hot">Run</div>
    <h2>{workspace.status}</h2>
  </header>

  <section class="panel-section col">
    <div class="row section-title">
      <span>Current Search</span>
      <span class="mono-dim">{providerLabel(workspace.lastRun?.provider)}</span>
    </div>
    <dl class="kv">
      <div>
        <dt>query</dt>
        <dd>{workspace.lastRun?.query || workspace.query || "none"}</dd>
      </div>
      <div>
        <dt>results</dt>
        <dd>{workspace.lastRun?.resultCount ?? workspace.candidates.length}</dd>
      </div>
      <div>
        <dt>limit</dt>
        <dd>{workspace.resultLimit}</dd>
      </div>
      <div>
        <dt>sort</dt>
        <dd>{workspace.sortBy}</dd>
      </div>
      <div>
        <dt>open access</dt>
        <dd>{workspace.openAccessOnly ? "on" : "off"}</dd>
      </div>
    </dl>
    {#if workspace.lastRun?.filters.length}
      <div class="chips row">
        {#each workspace.lastRun.filters as filter}
          <span class="chip">{filter}</span>
        {/each}
      </div>
    {/if}
    {#if workspace.error}
      <p class="error">{workspace.error}</p>
    {/if}
  </section>

  <section class="panel-section col selected-section">
    <div class="row section-title">
      <span>Selected Candidate</span>
      {#if selectedCandidate}
        <span class="mono-dim">{providerLabel(selectedCandidate.sourceProvider)}</span>
      {/if}
    </div>

    {#if selectedCandidate}
      <h3>{selectedCandidate.title}</h3>
      <p class="authors">{selectedCandidate.authors.slice(0, 4).join(", ") || "unknown authors"}</p>
      <dl class="kv">
        <div>
          <dt>venue</dt>
          <dd>{selectedCandidate.venue}</dd>
        </div>
        <div>
          <dt>year</dt>
          <dd>{selectedCandidate.year || "unknown"}</dd>
        </div>
        <div>
          <dt>citations</dt>
          <dd>{selectedCandidate.citations}</dd>
        </div>
        <div>
          <dt>pdf</dt>
          <dd>{selectedCandidate.pdfUrl ? "available" : "not found"}</dd>
        </div>
      </dl>
      <div class="reason-list col">
        {#each selectedCandidate.match?.reasons ?? [selectedCandidate.why] as reason}
          <span>{reason}</span>
        {/each}
      </div>
      {#if selectedCandidate.abstract}
        <p class="abstract">{selectedCandidate.abstract}</p>
      {/if}
    {:else}
      <p class="empty">Select a candidate to inspect metadata and match reasons.</p>
    {/if}
  </section>

  <div class="flex1"></div>
</aside>

<style>
  .discover-inspector {
    width: 280px;
    flex-shrink: 0;
    min-height: 0;
    background: var(--panel);
  }

  .panel-header {
    padding: 14px;
  }

  h2 {
    margin: 3px 0 0;
    color: var(--amber);
    font-size: 15px;
    font-weight: 600;
    text-transform: capitalize;
  }

  h3 {
    margin: 0;
    color: var(--fg);
    font-size: 13px;
    line-height: 1.35;
  }

  .panel-section {
    gap: 10px;
    padding: 14px;
    border-bottom: 1px solid var(--border);
  }

  .selected-section {
    min-height: 0;
    overflow: auto;
  }

  .section-title {
    justify-content: space-between;
    color: var(--fg-2);
    font-size: 10px;
    text-transform: uppercase;
  }

  .kv {
    display: grid;
    gap: 6px;
    margin: 0;
  }

  .kv div {
    display: grid;
    grid-template-columns: 78px 1fr;
    gap: 8px;
    font-size: 10px;
  }

  dt {
    color: var(--fg-3);
    text-transform: uppercase;
  }

  dd {
    margin: 0;
    min-width: 0;
    color: var(--fg-1);
    overflow-wrap: anywhere;
  }

  .chips {
    gap: 5px;
    flex-wrap: wrap;
  }

  .chip {
    border: 1px solid var(--border-2);
    padding: 1px 5px;
    color: var(--fg-2);
    font-size: 9px;
  }

  .authors,
  .abstract,
  .empty,
  .error {
    margin: 0;
    color: var(--fg-2);
    font-size: 11px;
    line-height: 1.5;
  }

  .abstract {
    color: var(--fg-3);
  }

  .error {
    color: var(--red);
  }

  .reason-list {
    gap: 5px;
  }

  .reason-list span {
    border-left: 2px solid var(--amber-dim);
    padding-left: 7px;
    color: var(--fg-2);
    font-size: 10px;
    line-height: 1.45;
  }
</style>
