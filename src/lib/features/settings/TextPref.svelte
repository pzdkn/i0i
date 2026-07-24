<script lang="ts">
  import { saveSetting } from "$lib/bridge/settings";

  let {
    settingKey,
    label,
    hint = "",
    value = "",
    placeholder = "",
    options = [],
    restartNote = false,
    onSaved,
  }: {
    settingKey: string;
    label: string;
    hint?: string;
    value?: string;
    placeholder?: string;
    options?: string[];
    restartNote?: boolean;
    onSaved: () => Promise<void> | void;
  } = $props();

  // Editable copy, resynced from the stored value below (intentionally seeded
  // from the prop's initial value).
  // svelte-ignore state_referenced_locally
  let draft = $state(value);
  let busy = $state(false);
  let saved = $state(false);
  const listId = $derived(`dl-${settingKey.replace(/[^a-z0-9]/gi, "-")}`);

  // Resync when the stored value changes underneath us (e.g. after a reload).
  $effect(() => {
    draft = value;
  });

  async function commit() {
    if (busy || draft === value) return;
    busy = true;
    saved = false;
    try {
      await saveSetting(settingKey, draft.trim());
      await onSaved();
      saved = true;
    } finally {
      busy = false;
    }
  }
</script>

<div class="pref col">
  <div class="row head">
    <span class="label">{label}</span>
    {#if saved}<span class="saved">saved{restartNote ? " · restart to apply" : ""}</span>{/if}
  </div>
  {#if hint}<span class="hint">{hint}</span>{/if}
  <input
    list={options.length ? listId : undefined}
    {placeholder}
    bind:value={draft}
    disabled={busy}
    onblur={commit}
    onkeydown={(event) => event.key === "Enter" && commit()}
  />
  {#if options.length}
    <datalist id={listId}>
      {#each options as option}<option value={option}></option>{/each}
    </datalist>
  {/if}
</div>

<style>
  .pref {
    gap: 5px;
    padding: 12px 16px;
  }

  .head {
    align-items: baseline;
    justify-content: space-between;
    gap: 10px;
  }

  .label {
    font-size: 12px;
    font-weight: 700;
    color: var(--fg-1, #ddd);
  }

  .hint {
    font-size: 11px;
    color: var(--fg-3, #888);
  }

  .saved {
    font-size: 10px;
    color: var(--amber, #f2a93b);
  }

  input {
    background: var(--bg-0, #0b0b0b);
    border: 1px solid var(--fg-3, #444);
    color: var(--fg-1, #ddd);
    padding: 6px 8px;
    font-size: 12px;
    font-family: inherit;
  }
</style>
