<script lang="ts">
  import { Plus, X } from "@lucide/svelte";
  import type { VaultSuggestion } from "$lib/domain/vault-suggestion";

  let {
    suggestions,
    selectedId,
    busyIds = [],
    onSelect,
    onOpen,
    onAdd,
    onDismiss,
  }: {
    suggestions: VaultSuggestion[];
    selectedId: string;
    busyIds?: string[];
    onSelect: (suggestion: VaultSuggestion) => void;
    onOpen: (suggestion: VaultSuggestion) => void;
    onAdd: (suggestion: VaultSuggestion) => void | Promise<void>;
    onDismiss: (suggestion: VaultSuggestion, keyboard: boolean) => void | Promise<void>;
  } = $props();
</script>

<div class="suggestion-list" role="list" aria-label="Suggested papers">
  {#each suggestions as suggestion (suggestion.id)}
    <div class:selected={selectedId === suggestion.id} class="suggestion-row" role="listitem">
      <button
        class="suggestion-primary"
        type="button"
        aria-label={`Select ${suggestion.candidate.title}`}
        onclick={() => onSelect(suggestion)}
        ondblclick={() => onOpen(suggestion)}
        onkeydown={(event) => {
          if (event.key === "Enter") {
            event.preventDefault();
            onOpen(suggestion);
          }
        }}
      >
        <span class="paper-meta">
          {suggestion.candidate.year ?? "—"}
          <i>/</i>
          {suggestion.candidate.venue ?? suggestion.candidate.sourceProvider}
        </span>
        <span class="paper-title">{suggestion.candidate.title}</span>
        <span class="paper-detail">
          {suggestion.candidate.authors.slice(0, 2).join(", ") || "Unknown authors"}
          <i>·</i>
          <em>{suggestion.reason}</em>
        </span>
      </button>
      <div class="row-actions">
        <button
          class="icon-action add"
          type="button"
          title="Add to vault"
          aria-label={`Add ${suggestion.candidate.title} to vault`}
          disabled={busyIds.includes(suggestion.id)}
          onclick={() => onAdd(suggestion)}
        >
          <Plus size={14} strokeWidth={1.8} />
        </button>
        <button
          class="icon-action dismiss"
          type="button"
          title="Dismiss suggestion"
          aria-label={`Dismiss ${suggestion.candidate.title}`}
          disabled={busyIds.includes(suggestion.id)}
          onclick={(event) => onDismiss(suggestion, event.detail === 0)}
        >
          <X size={14} strokeWidth={1.8} />
        </button>
      </div>
    </div>
  {/each}
</div>

<style>
  .suggestion-list {
    min-height: 0;
    overflow: auto;
  }

  .suggestion-row {
    min-height: 52px;
    display: grid;
    grid-template-columns: minmax(0, 1fr) 58px;
    align-items: stretch;
    border-bottom: 1px solid var(--border);
    background: var(--bg);
  }

  .suggestion-row:hover,
  .suggestion-row.selected {
    background: var(--bg-1);
  }

  .suggestion-row.selected {
    box-shadow: inset 2px 0 0 var(--cyan);
  }

  .suggestion-primary {
    min-width: 0;
    display: grid;
    grid-template-columns: 112px minmax(0, 1fr);
    grid-template-rows: 22px 22px;
    gap: 0 10px;
    padding: 4px 10px 4px 14px;
    border: 0;
    outline: none;
    background: transparent;
    color: inherit;
    text-align: left;
    cursor: default;
  }

  .suggestion-primary:focus-visible {
    box-shadow: inset 0 0 0 1px var(--cyan);
  }

  .paper-meta,
  .paper-title,
  .paper-detail {
    min-width: 0;
    align-self: center;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .paper-meta {
    grid-row: 1 / span 2;
    color: var(--fg-3);
    font-size: 10px;
  }

  .paper-meta i,
  .paper-detail i {
    margin: 0 4px;
    color: var(--border-2);
    font-style: normal;
  }

  .paper-title {
    color: var(--fg-1);
    font-size: 12px;
  }

  .paper-detail {
    color: var(--fg-3);
    font-size: 10px;
  }

  .paper-detail em {
    color: var(--cyan-dim, var(--cyan));
    font-style: normal;
  }

  .row-actions {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 3px;
  }

  .icon-action {
    width: 24px;
    height: 24px;
    display: grid;
    place-items: center;
    padding: 0;
    border: 1px solid transparent;
    background: transparent;
    color: var(--fg-3);
    cursor: pointer;
  }

  .icon-action:hover,
  .icon-action:focus-visible {
    border-color: var(--border-2);
    color: var(--fg-1);
  }

  .icon-action.add:hover,
  .icon-action.add:focus-visible {
    color: var(--cyan);
  }

  .icon-action.dismiss:hover,
  .icon-action.dismiss:focus-visible {
    color: var(--red);
  }
</style>
