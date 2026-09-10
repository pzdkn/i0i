<script lang="ts">
  import { CircleAlert, CircleCheck, ExternalLink, LoaderCircle, RefreshCw } from "@lucide/svelte";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import {
    getRuntimeCapabilities,
    retryRuntimeCapability,
    type RuntimeCapability,
  } from "$lib/bridge/settings";

  let {
    onNavigate,
  }: {
    onNavigate: (tab: "keys" | "models") => void;
  } = $props();

  let capabilities = $state<RuntimeCapability[]>([]);
  let loading = $state(true);
  let retrying = $state("");
  let error = $state("");

  async function load(): Promise<void> {
    loading = true;
    error = "";
    try {
      capabilities = await getRuntimeCapabilities();
    } catch (caught) {
      error = String(caught);
    } finally {
      loading = false;
    }
  }

  async function runAction(capability: RuntimeCapability): Promise<void> {
    if (capability.action === "open_codex_install" && capability.setupUrl) {
      await openUrl(capability.setupUrl);
      return;
    }
    if (capability.action === "open_api_keys") {
      onNavigate("keys");
      return;
    }
    retrying = capability.id;
    error = "";
    try {
      capabilities = await retryRuntimeCapability(capability.id);
    } catch (caught) {
      error = String(caught);
      await load();
    } finally {
      retrying = "";
    }
  }

  function actionLabel(capability: RuntimeCapability): string {
    if (capability.action === "open_codex_install") return "Install";
    if (capability.action === "open_api_keys") return "Configure";
    if (capability.action === "test") return "Test";
    return "Retry";
  }

  $effect(() => {
    void load();
  });
</script>

<div class="tab col">
  {#if loading && capabilities.length === 0}
    <div class="loading row"><LoaderCircle class="spin" size={14} aria-hidden="true" /> Checking system</div>
  {:else}
    {#each capabilities as capability (capability.id)}
      <div class="capability row hair-b">
        <span class="status" class:ready={capability.state === "ready"}>
          {#if capability.state === "ready"}
            <CircleCheck size={15} aria-hidden="true" />
          {:else if capability.state === "starting"}
            <LoaderCircle class="spin" size={15} aria-hidden="true" />
          {:else}
            <CircleAlert size={15} aria-hidden="true" />
          {/if}
        </span>
        <span class="copy col">
          <span class="label">{capability.label}{#if capability.version}<small>{capability.version}</small>{/if}</span>
          <span class="message">{capability.message}</span>
        </span>
        {#if capability.action}
          <button type="button" disabled={retrying === capability.id} onclick={() => void runAction(capability)}>
            {#if retrying === capability.id}
              <LoaderCircle class="spin" size={13} aria-hidden="true" />
            {:else if capability.action === "open_codex_install"}
              <ExternalLink size={13} aria-hidden="true" />
            {:else}
              <RefreshCw size={13} aria-hidden="true" />
            {/if}
            {actionLabel(capability)}
          </button>
        {/if}
      </div>
    {/each}
  {/if}
  {#if error}<p class="error">{error}</p>{/if}
</div>

<style>
  .loading,
  .error {
    align-items: center;
    gap: 7px;
    margin: 0;
    padding: 16px;
    font-size: 11px;
    color: var(--fg-3, #888);
  }

  .error {
    color: #e06c6c;
  }

  .capability {
    min-height: 58px;
    align-items: center;
    gap: 10px;
    padding: 9px 14px;
  }

  .status {
    display: inline-flex;
    color: var(--amber, #f2a93b);
  }

  .status.ready {
    color: #79b99a;
  }

  .copy {
    min-width: 0;
    flex: 1;
    gap: 3px;
  }

  .label {
    display: flex;
    align-items: baseline;
    gap: 7px;
    font-size: 12px;
    font-weight: 700;
    color: var(--fg-1, #ddd);
  }

  small,
  .message {
    font-size: 10px;
    font-weight: 400;
    color: var(--fg-3, #888);
  }

  button {
    min-width: 70px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 5px;
    border: 1px solid var(--fg-3, #444);
    background: transparent;
    color: var(--fg-2, #aaa);
    padding: 5px 8px;
    font: inherit;
    font-size: 10px;
    cursor: pointer;
  }

  button:hover:not(:disabled) {
    border-color: var(--amber, #f2a93b);
    color: var(--amber, #f2a93b);
  }

  button:disabled {
    opacity: 0.55;
    cursor: default;
  }

  .tab :global(.spin) {
    animation: spin 0.9s linear infinite;
  }

  @keyframes spin {
    to { transform: rotate(360deg); }
  }
</style>
