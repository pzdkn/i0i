<script lang="ts">
  import { saveSetting } from "$lib/bridge/settings";

  let {
    settingKey,
    label,
    hint = "",
    checked = false,
    disabled = false,
    onSaved,
  }: {
    settingKey: string;
    label: string;
    hint?: string;
    checked?: boolean;
    disabled?: boolean;
    onSaved: () => Promise<void> | void;
  } = $props();

  let busy = $state(false);

  async function toggle(event: Event) {
    const next = (event.currentTarget as HTMLInputElement).checked;
    busy = true;
    try {
      await saveSetting(settingKey, next ? "true" : "false");
      await onSaved();
    } finally {
      busy = false;
    }
  }
</script>

<label class="pref" class:disabled>
  <input type="checkbox" {checked} disabled={disabled || busy} onchange={toggle} />
  <span class="col text">
    <span class="label">{label}</span>
    {#if hint}<span class="hint">{hint}</span>{/if}
  </span>
</label>

<style>
  .pref {
    display: flex;
    align-items: flex-start;
    gap: 10px;
    padding: 12px 16px;
    cursor: pointer;
  }

  .pref.disabled {
    cursor: default;
    opacity: 0.5;
  }

  input {
    margin-top: 2px;
  }

  .text {
    gap: 3px;
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
</style>
