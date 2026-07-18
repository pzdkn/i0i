<script lang="ts">
  import type {
    MetadataAutofillProgress,
    MetadataCandidate,
    PaperMetadataUpdate,
  } from "$lib/domain/library";

  let {
    paperId,
    title,
    authors,
    venue,
    year,
    abstractText,
    tags = [],
    progress,
    isAutofilling = false,
    onAutofill,
    onApplyCandidate,
    onSaveMetadata,
  }: {
    paperId: string;
    title: string;
    authors: string[];
    venue: string;
    year: number;
    abstractText?: string;
    tags?: string[];
    progress?: MetadataAutofillProgress;
    isAutofilling?: boolean;
    onAutofill?: (paperId: string) => void | Promise<void>;
    onApplyCandidate: (paperId: string, candidate: MetadataCandidate) => void | Promise<void>;
    onSaveMetadata: (paperId: string, update: PaperMetadataUpdate) => void | Promise<void>;
  } = $props();

  let editing = $state(false);
  let draftTitle = $state("");
  let draftAuthors = $state("");
  let draftVenue = $state("");
  let draftYear = $state("");
  let draftAbstract = $state("");
  let saveError = $state("");
  let justApplied = $state(false);
  let appliedTimer: ReturnType<typeof setTimeout> | undefined;

  const candidates = $derived(progress?.candidates ?? []);
  // RFC 0050: no_match candidates are unverified guesses — offered as an
  // editable draft, never as a one-click Apply.
  const draftOnly = $derived(progress?.status === "no_match");
  const isRunning = $derived(progress?.status === "running" || isAutofilling);
  const statusLabel = $derived.by(() => {
    if (isAutofilling && !progress) {
      return "starting";
    }
    return progress?.stage ?? "idle";
  });

  function startEdit() {
    draftTitle = title;
    draftAuthors = authors.join("\n");
    draftVenue = venue;
    draftYear = year ? String(year) : "";
    draftAbstract = abstractText ?? "";
    saveError = "";
    editing = true;
  }

  function useAsDraft(candidate: MetadataCandidate) {
    draftTitle = candidate.title;
    draftAuthors = candidate.authors.join("\n");
    draftVenue = candidate.venue ?? "";
    draftYear = candidate.year ? String(candidate.year) : "";
    draftAbstract = candidate.abstractText ?? "";
    saveError = "";
    editing = true;
  }

  async function applyCandidate(candidate: MetadataCandidate) {
    await onApplyCandidate(paperId, candidate);
    justApplied = true;
    clearTimeout(appliedTimer);
    appliedTimer = setTimeout(() => (justApplied = false), 3000);
  }

  async function saveEdit() {
    const parsedYear = Number.parseInt(draftYear, 10);
    const update: PaperMetadataUpdate = {
      title: draftTitle.trim() || undefined,
      authors: draftAuthors
        .split("\n")
        .map((author) => author.trim())
        .filter((author) => author.length > 0),
      venue: draftVenue.trim() || undefined,
      year: Number.isFinite(parsedYear) ? parsedYear : undefined,
      abstract: draftAbstract.trim() || undefined,
    };

    saveError = "";
    try {
      await onSaveMetadata(paperId, update);
      editing = false;
    } catch (error) {
      saveError = String(error);
    }
  }
</script>

<div class="metadata-panel">
  <div class="row section-title">
    <span class="label hot">Metadata</span>
    <div class="flex1"></div>
    {#if editing}
      <button class="btn" type="button" onclick={() => (editing = false)}>Cancel</button>
      <button class="btn primary" type="button" onclick={() => void saveEdit()}>Save</button>
    {:else}
      <button class="btn" type="button" onclick={startEdit}>Edit</button>
      {#if onAutofill}
        <button
          class="btn"
          type="button"
          disabled={isRunning}
          onclick={() => void onAutofill?.(paperId)}
        >
          {isRunning ? "Autofilling..." : "Autofill"}
        </button>
      {/if}
    {/if}
  </div>

  {#if editing}
    <div class="edit-form">
      <label>
        <span>Title</span>
        <textarea bind:value={draftTitle} rows="2"></textarea>
      </label>
      <label>
        <span>Authors (one per line)</span>
        <textarea bind:value={draftAuthors} rows="3"></textarea>
      </label>
      <div class="edit-row">
        <label>
          <span>Year</span>
          <input bind:value={draftYear} inputmode="numeric" pattern="[0-9]*" />
        </label>
        <label class="grow">
          <span>Venue</span>
          <input bind:value={draftVenue} />
        </label>
      </div>
      <label>
        <span>Abstract</span>
        <textarea bind:value={draftAbstract} rows="4"></textarea>
      </label>
      {#if saveError}
        <p class="error mono-dim">{saveError}</p>
      {/if}
    </div>
  {:else}
    <div class="meta-fields">
      <span>title</span><strong>{title}</strong>
      <span>authors</span><strong>{authors.join(", ") || "Unknown"}</strong>
      <span>venue</span><strong>{venue || "Unknown"} {year || ""}</strong>
    </div>
    {#if tags.length}
      <div class="tags row">
        {#each tags as tag}
          <span class="chip">{tag}</span>
        {/each}
      </div>
    {/if}
  {/if}

  <div class="autofill-block">
    <div class="row section-title">
      <span class="label hot">Autofill</span>
      <div class="flex1"></div>
      <span class="mono-dim">{statusLabel}</span>
    </div>

    {#if progress}
      <p class="status-note">{progress.message}</p>

      {#if candidates.length && (progress?.status === "needs_review" || draftOnly)}
        <div class="candidate-list">
          {#each candidates as candidate}
            <article class="metadata-candidate">
              <div class="row candidate-head">
                <strong>{Math.round(candidate.confidence * 100)}%</strong>
                <span>{candidate.providers.join(" + ")}</span>
                <div class="flex1"></div>
                {#if draftOnly}
                  <button class="btn" type="button" onclick={() => useAsDraft(candidate)}>
                    Use as draft
                  </button>
                {:else}
                  <button
                    class="btn primary"
                    type="button"
                    onclick={() => void applyCandidate(candidate)}
                  >
                    Apply
                  </button>
                {/if}
              </div>

              <div class="metadata-diff">
                <span>title</span>
                <div>
                  <p class="current">Current: {title}</p>
                  <p>Suggested: {candidate.title}</p>
                </div>

                <span>authors</span>
                <div>
                  <p class="current">Current: {authors.join(", ") || "Unknown"}</p>
                  <p>Suggested: {candidate.authors.join(", ") || "Unknown"}</p>
                </div>

                <span>venue</span>
                <div>
                  <p class="current">Current: {venue || "Unknown"} {year || ""}</p>
                  <p>Suggested: {candidate.venue ?? "Unknown"} {candidate.year ?? ""}</p>
                </div>
              </div>

              {#if candidate.evidence.length}
                <div class="evidence mono-dim">{candidate.evidence.join(" · ")}</div>
              {/if}
            </article>
          {/each}
        </div>
      {/if}
    {:else if isAutofilling}
      <p class="status-note">Starting metadata autofill...</p>
    {:else if justApplied}
      <p class="status-note">Metadata applied.</p>
    {:else}
      <p class="status-note">Run Autofill or edit the fields directly.</p>
    {/if}
  </div>
</div>

<style>
  .metadata-panel {
    display: flex;
    flex-direction: column;
    gap: 8px;
    min-width: 0;
  }

  .section-title {
    gap: 6px;
    align-items: center;
  }

  .meta-fields {
    display: grid;
    grid-template-columns: 52px 1fr;
    gap: 4px 8px;
    color: var(--fg-2);
    font-size: 10px;
  }

  .meta-fields span {
    color: var(--fg-3);
  }

  .meta-fields strong {
    min-width: 0;
    color: var(--fg-1);
    font-weight: 500;
    overflow-wrap: anywhere;
  }

  .tags {
    flex-wrap: wrap;
    gap: 4px;
  }

  .edit-form {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .edit-row {
    display: flex;
    gap: 8px;
  }

  .edit-row .grow {
    flex: 1;
  }

  label {
    display: flex;
    flex-direction: column;
    gap: 3px;
    color: var(--fg-3);
    font-size: 9px;
    text-transform: uppercase;
  }

  input,
  textarea {
    border: 1px solid var(--border-2);
    background: var(--bg);
    color: var(--fg-1);
    padding: 4px 6px;
    font: inherit;
    font-size: 11px;
    text-transform: none;
    resize: vertical;
  }

  input:focus,
  textarea:focus {
    border-color: var(--amber-dim);
    outline: none;
  }

  .error {
    margin: 0;
    color: var(--red);
    font-size: 10px;
  }

  .status-note {
    margin: 0;
    color: var(--fg-2);
    font-size: 10px;
    line-height: 1.5;
  }

  .autofill-block {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .candidate-list {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .metadata-candidate {
    border: 1px solid var(--border-2);
    padding: 8px;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .candidate-head {
    gap: 8px;
    align-items: center;
  }

  .candidate-head strong {
    color: var(--green);
    font-size: 11px;
  }

  .candidate-head span {
    color: var(--fg-3);
    font-size: 9px;
    text-transform: uppercase;
  }

  .metadata-diff {
    display: grid;
    grid-template-columns: 52px 1fr;
    gap: 4px 8px;
    font-size: 10px;
  }

  .metadata-diff span {
    color: var(--fg-3);
  }

  .metadata-diff p {
    margin: 0;
    color: var(--fg-1);
    overflow-wrap: anywhere;
  }

  .metadata-diff .current {
    color: var(--fg-3);
  }

  .evidence {
    font-size: 9px;
    line-height: 1.4;
  }
</style>
