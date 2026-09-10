<script lang="ts">
  import Modal from "$lib/components/Modal.svelte";
  import { getSettings, type SettingsView } from "$lib/bridge/settings";
  import ApiKeysTab from "./ApiKeysTab.svelte";
  import ModelsTab from "./ModelsTab.svelte";
  import SearchDefaultsTab from "./SearchDefaultsTab.svelte";
  import SystemTab from "./SystemTab.svelte";

  let {
    open = false,
    onClose = () => {},
  }: {
    open?: boolean;
    onClose?: () => void;
  } = $props();

  const tabs = [
    { id: "keys", label: "API Keys" },
    { id: "models", label: "Models & cost" },
    { id: "search", label: "Search" },
    { id: "system", label: "System" },
  ];
  let activeTab = $state("keys");

  let settings = $state<SettingsView | null>(null);
  let loadError = $state("");

  async function load() {
    loadError = "";
    try {
      settings = await getSettings();
    } catch (error) {
      loadError = String(error);
    }
  }

  // Load fresh each time the dialog opens.
  $effect(() => {
    if (open) {
      void load();
    }
  });
</script>

<Modal {open} title="Settings" {onClose}>
  <div class="settings row">
    <nav class="tabs col hair-r">
      {#each tabs as tab (tab.id)}
        <button
          class:active={tab.id === activeTab}
          type="button"
          onclick={() => (activeTab = tab.id)}
        >
          {tab.label}
        </button>
      {/each}
    </nav>

    <div class="content col">
      {#if loadError}
        <p class="error">{loadError}</p>
      {:else if !settings}
        <p class="loading">Loading…</p>
      {:else if activeTab === "keys"}
        <ApiKeysTab secrets={settings.secrets} onChanged={load} />
      {:else if activeTab === "models"}
        <ModelsTab prefs={settings.prefs} onChanged={load} />
      {:else if activeTab === "search"}
        <SearchDefaultsTab prefs={settings.prefs} onChanged={load} />
      {:else if activeTab === "system"}
        <SystemTab onNavigate={(tab) => (activeTab = tab)} />
      {/if}
    </div>
  </div>
</Modal>

<style>
  .settings {
    align-items: stretch;
    min-height: 360px;
  }

  .tabs {
    width: 150px;
    flex-shrink: 0;
    padding: 8px 0;
  }

  .tabs button {
    text-align: left;
    border: 0;
    border-left: 2px solid transparent;
    background: transparent;
    color: var(--fg-3, #888);
    padding: 8px 14px;
    font-size: 11px;
    letter-spacing: 0.04em;
    cursor: pointer;
  }

  .tabs button.active {
    border-left-color: var(--amber, #f2a93b);
    color: var(--amber, #f2a93b);
  }

  .content {
    flex: 1;
    min-width: 0;
    overflow: auto;
  }

  .loading,
  .error {
    padding: 16px;
    font-size: 12px;
    color: var(--fg-3, #888);
  }

  .error {
    color: #e06c6c;
  }
</style>
