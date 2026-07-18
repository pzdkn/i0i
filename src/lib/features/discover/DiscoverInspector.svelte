<script lang="ts">
  import { providerDisplayName, type DiscoverCandidate, type DiscoveryProviderChoice, type DiscoverWorkspace } from "$lib/domain/discover";

  let {
    workspace,
  }: {
    workspace: DiscoverWorkspace;
  } = $props();

  const selectedCandidate = $derived(
    workspace.candidates.find((candidate) => candidate.id === workspace.selectedCandidateId),
  );
  const runLabel = $derived(workspace.lastRun?.mode === "deep" || workspace.activeRunMode === "deep" ? "Deep" : "Shallow");
  const providerLabel = $derived(
    workspace.lastRun?.mode === "deep"
      ? "Deep"
      : providerDisplayName((workspace.lastRun?.provider ?? workspace.provider) as DiscoveryProviderChoice),
  );


  function statusLabel(status: DiscoverWorkspace["status"]) {
    if (status === "idle") {
      return "Idle";
    }

    return status[0].toUpperCase() + status.slice(1);
  }

  function candidateSignals(candidate: DiscoverCandidate) {
    const signals = [providerDisplayName((candidate.sourceProvider ?? "open_alex") as DiscoveryProviderChoice)];

    if (candidate.citations > 0) {
      signals.push(`${candidate.citations.toLocaleString()} citations`);
    }

    if (candidate.openAccess?.isOpenAccess) {
      signals.push("Open access");
    }

    if (candidate.pdfAvailability === "verified") {
      signals.push("PDF verified");
    } else if (candidate.pdfAvailability === "browser_required") {
      signals.push("PDF needs browser");
    } else if (candidate.pdfAvailability === "unavailable") {
      signals.push("No PDF found");
    } else if (candidate.pdfUrl) {
      signals.push("PDF available");
    }

    if (candidate.doi) {
      signals.push("DOI");
    }

    return signals;
  }
</script>

<aside class="discover-inspector col hair-l">
  <header class="panel-header hair-b">
    <div class="label hot">Run</div>
    <h2>{statusLabel(workspace.status)}</h2>
  </header>

  <section class="panel-section col">
    <div class="row section-title">
      <span>Current Run</span>
      <span class="mono-dim">{runLabel}</span>
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
        <dt>provider</dt>
        <dd>{providerLabel}</dd>
      </div>
      <div>
        <dt>limit</dt>
        <dd>{workspace.resultLimit}</dd>
      </div>
      <div>
        <dt>sort</dt>
        <dd>{workspace.sortBy}</dd>
      </div>
    </dl>
    {#if workspace.runProgress}
      <dl class="kv">
        <div>
          <dt>queries</dt>
          <dd>{workspace.runProgress.iteration}</dd>
        </div>
        <div>
          <dt>found</dt>
          <dd>{workspace.runProgress.found}</dd>
        </div>
        <div>
          <dt>unique</dt>
          <dd>{workspace.runProgress.unique}</dd>
        </div>
        <div>
          <dt>new</dt>
          <dd>{workspace.runProgress.new}</dd>
        </div>
      </dl>
      <p class="current-step">{workspace.runProgress.message}</p>
    {/if}
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
    {#if !selectedCandidate && workspace.runTrace.length > 0}
      <div class="trace col">
        <div class="section-title">Trace</div>
        <ol class="trace-lines col">
          {#each workspace.runTrace.slice(-8) as line}
            <li>{line}</li>
          {/each}
        </ol>
      </div>
    {/if}
  </section>

  <section class="panel-section col selected-section">
    <div class="row section-title">
      <span>Selected Candidate</span>
      {#if selectedCandidate}
        <span class="mono-dim">{providerDisplayName((selectedCandidate.sourceProvider ?? "open_alex") as DiscoveryProviderChoice)}</span>
      {/if}
    </div>

    {#if selectedCandidate}
      {#if workspace.status === "running" && workspace.runProgress}
        <div class="active-run row">
          <span>{runLabel} running</span>
          <span>{workspace.runProgress.unique} candidates</span>
        </div>
      {/if}
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
          <dd>
            {selectedCandidate.pdfAvailability === "verified"
              ? "verified"
              : selectedCandidate.pdfAvailability === "browser_required"
                ? "needs browser"
                : selectedCandidate.pdfAvailability === "unavailable"
                  ? "not found"
                  : selectedCandidate.pdfUrl
                    ? "checking…"
                    : "not found"}
          </dd>
        </div>
      </dl>
      <div class="signals col">
        <div class="section-title">Signals</div>
        <div class="chips row">
          {#each candidateSignals(selectedCandidate) as signal}
            <span class="chip">{signal}</span>
          {/each}
        </div>
      </div>
      {#if selectedCandidate.match?.matchedKeywords.length}
        <div class="signals col">
          <div class="section-title">Matched Keywords</div>
          <div class="chips row">
            {#each selectedCandidate.match.matchedKeywords as keyword}
              <span class="chip">{keyword}</span>
            {/each}
          </div>
        </div>
      {/if}
      {#if selectedCandidate.abstract}
        <p class="abstract">{selectedCandidate.abstract}</p>
      {/if}
    {:else}
      <p class="empty">Select a candidate to inspect metadata and signals.</p>
    {/if}
  </section>

  <div class="flex1"></div>
</aside>

<style>
  .discover-inspector {
    width: 100%;
    height: 100%;
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
  .current-step,
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

  .signals {
    gap: 6px;
  }

  .trace {
    gap: 6px;
  }

  .trace-lines {
    gap: 4px;
    margin: 0;
    padding-left: 16px;
    color: var(--fg-3);
    font-size: 10px;
    line-height: 1.4;
  }

  .active-run {
    justify-content: space-between;
    border: 1px solid var(--border-2);
    padding: 4px 6px;
    color: var(--green);
    font-size: 9px;
    text-transform: uppercase;
  }
</style>
