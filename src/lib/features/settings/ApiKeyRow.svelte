<script lang="ts">
  import {
    saveSetting,
    clearSetting,
    testProviderKey,
    type SecretStatus,
  } from "$lib/bridge/settings";

  let {
    secret,
    label,
    unlocks,
    placeholder,
    isEmail = false,
    onChanged,
  }: {
    secret: SecretStatus;
    label: string;
    unlocks: string;
    placeholder: string;
    isEmail?: boolean;
    onChanged: () => Promise<void> | void;
  } = $props();

  let draft = $state("");
  let busy = $state(false);
  let message = $state("");
  let messageKind = $state<"" | "ok" | "err">("");

  // Non-sensitive entries (the contact email) return their value and show it in
  // a plain, prefilled field; API keys are masked and write-only.
  const visible = $derived(secret.value !== undefined && secret.value !== null);

  // Keep a visible field in sync with the stored value across reloads. Tracks
  // secret.value only, so it doesn't clobber the user while they type.
  $effect(() => {
    if (visible) {
      draft = secret.value ?? "";
    }
  });

  const statusText = $derived(
    secret.configured ? `Set${secret.source ? ` · ${secret.source}` : ""}` : "Not set",
  );

  async function save() {
    if (!draft.trim() || busy) return;
    busy = true;
    message = "";
    messageKind = "";
    try {
      await saveSetting(secret.settingKey, draft.trim());
      if (!visible) {
        draft = "";
      }
      await onChanged();
      message = "Saved";
      messageKind = "ok";
    } catch (error) {
      message = String(error);
      messageKind = "err";
    } finally {
      busy = false;
    }
  }

  async function test() {
    if (busy) return;
    busy = true;
    message = "Testing…";
    messageKind = "";
    try {
      await testProviderKey(secret.name);
      message = "Works ✓";
      messageKind = "ok";
    } catch (error) {
      message = String(error);
      messageKind = "err";
    } finally {
      busy = false;
    }
  }

  async function clear() {
    if (busy) return;
    busy = true;
    message = "";
    messageKind = "";
    try {
      await clearSetting(secret.settingKey);
      await onChanged();
      message = "Cleared";
      messageKind = "ok";
    } catch (error) {
      message = String(error);
      messageKind = "err";
    } finally {
      busy = false;
    }
  }
</script>

<div class="row-item col hair-b">
  <div class="head row">
    <div class="col label-col">
      <span class="label">{label}</span>
      <span class="unlocks">{unlocks}</span>
    </div>
    <span class="pill" class:set={secret.configured}>{statusText}</span>
  </div>
  <div class="controls row">
    <input
      type={visible ? (isEmail ? "email" : "text") : "password"}
      autocomplete="off"
      {placeholder}
      bind:value={draft}
      disabled={busy}
      onkeydown={(event) => event.key === "Enter" && save()}
    />
    <button type="button" disabled={busy || !draft.trim()} onclick={save}>Save</button>
    <button type="button" disabled={busy || !secret.configured} onclick={test}>Test</button>
    {#if secret.source === "user"}
      <button type="button" class="ghost" disabled={busy} onclick={clear}>Clear</button>
    {/if}
  </div>
  {#if message}
    <span class="message" class:ok={messageKind === "ok"} class:err={messageKind === "err"}>
      {message}
    </span>
  {/if}
</div>

<style>
  .row-item {
    gap: 8px;
    padding: 14px 16px;
  }

  .head {
    align-items: flex-start;
    justify-content: space-between;
    gap: 12px;
  }

  .label {
    font-size: 12px;
    font-weight: 700;
    color: var(--fg-1, #ddd);
  }

  .unlocks {
    font-size: 11px;
    color: var(--fg-3, #888);
  }

  .pill {
    flex-shrink: 0;
    font-size: 10px;
    letter-spacing: 0.04em;
    padding: 2px 8px;
    border: 1px solid var(--fg-3, #555);
    color: var(--fg-3, #888);
    border-radius: 2px;
  }

  .pill.set {
    color: var(--amber, #f2a93b);
    border-color: var(--amber, #f2a93b);
  }

  .controls {
    gap: 8px;
    align-items: center;
  }

  input {
    flex: 1;
    min-width: 0;
    background: var(--bg-0, #0b0b0b);
    border: 1px solid var(--fg-3, #444);
    color: var(--fg-1, #ddd);
    padding: 6px 8px;
    font-size: 12px;
    font-family: inherit;
  }

  button {
    border: 1px solid var(--fg-3, #444);
    background: transparent;
    color: var(--fg-1, #ccc);
    padding: 6px 12px;
    font-size: 11px;
    cursor: pointer;
  }

  button:hover:not(:disabled) {
    border-color: var(--amber, #f2a93b);
    color: var(--amber, #f2a93b);
  }

  button:disabled {
    opacity: 0.4;
    cursor: default;
  }

  button.ghost {
    color: var(--fg-3, #888);
  }

  .message {
    font-size: 11px;
    color: var(--fg-3, #888);
  }

  .message.ok {
    color: var(--amber, #f2a93b);
  }

  .message.err {
    color: #e06c6c;
  }
</style>
