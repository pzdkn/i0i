<script lang="ts">
  import { onMount } from "svelte";
  import AppShell from "$lib/app/AppShell.svelte";
  import WorkspaceTabs from "$lib/app/WorkspaceTabs.svelte";
  import { getVaultStatus } from "$lib/bridge/tauri";
  import type { VaultStatus } from "$lib/domain/vault";
  import type { WorkspaceTab } from "$lib/domain/workspace";
  import ReaderView from "$lib/features/reader/ReaderView.svelte";
  import VaultExplorer from "$lib/features/vault/VaultExplorer.svelte";
  import VaultHome from "$lib/features/vault/VaultHome.svelte";
  import { getPaperById, getPaperTitle, getVaultWorkspace } from "$lib/mock/vault-workspaces";

  let vaultStatus = $state<VaultStatus | null>(null);
  let bridgeError = $state("");
  let activeVaultId = $state("attention");
  let selectedPaperId = $state("vaswani2017");
  let activeTabId = $state("vault:attention");
  let tabs = $state<WorkspaceTab[]>([
    {
      id: "vault:attention",
      kind: "vault",
      title: "/transformers/attention",
      vaultId: "attention",
    },
  ]);

  const activeTab = $derived(tabs.find((tab) => tab.id === activeTabId));
  const activeVaultWorkspace = $derived(getVaultWorkspace(activeVaultId));
  const activePaper = $derived(getPaperById(activeTab?.paperId ?? selectedPaperId));
  const currentPath = $derived(activeTab?.title ?? "no workspace");
  const activeMode = $derived(activeTab?.kind === "reader" ? "R" : "V");

  onMount(async () => {
    try {
      vaultStatus = await getVaultStatus();
    } catch (error) {
      bridgeError = String(error);
    }
  });

  function openVault(vaultId = activeVaultId) {
    const workspace = getVaultWorkspace(vaultId);
    const vaultTab: WorkspaceTab = {
      id: `vault:${workspace.id}`,
      kind: "vault",
      title: workspace.path,
      vaultId: workspace.id,
    };

    activeVaultId = workspace.id;
    tabs = [vaultTab, ...tabs.filter((tab) => tab.kind !== "vault")];
    activeTabId = vaultTab.id;
  }

  function openPaper(paperId: string) {
    selectedPaperId = paperId;

    const readerTab: WorkspaceTab = {
      id: `reader:${paperId}`,
      kind: "reader",
      title: getPaperTitle(paperId),
      paperId,
    };

    tabs = [...tabs.filter((tab) => tab.kind !== "reader"), readerTab];
    activeTabId = readerTab.id;
  }

  function closeTab(tabId: string) {
    const nextTabs = tabs.filter((tab) => tab.id !== tabId);
    tabs = nextTabs;

    if (activeTabId === tabId) {
      activeTabId = nextTabs[0]?.id ?? "";
      const nextActiveTab = nextTabs[0];
      if (nextActiveTab?.vaultId) {
        activeVaultId = nextActiveTab.vaultId;
      }
    }
  }

  function activateTab(tabId: string) {
    const tab = tabs.find((candidate) => candidate.id === tabId);
    if (!tab) {
      return;
    }

    activeTabId = tab.id;

    if (tab.vaultId) {
      activeVaultId = tab.vaultId;
    }

    if (tab.paperId) {
      selectedPaperId = tab.paperId;
    }
  }

  function handleModeSelect(mode: string) {
    if (mode === "V") {
      openVault(activeVaultId);
      return;
    }

    if (mode === "R") {
      const readerTab = tabs.find((tab) => tab.kind === "reader");
      if (readerTab) {
        activateTab(readerTab.id);
      }
    }
  }
</script>

<AppShell {activeMode} {currentPath} {vaultStatus} {bridgeError} onSelectMode={handleModeSelect}>
  <VaultExplorer {activeVaultId} onOpenVault={openVault} />
  <section class="workspace col">
    <WorkspaceTabs {tabs} {activeTabId} onActivate={activateTab} onClose={closeTab} />

    {#if activeTab?.kind === "reader"}
      <ReaderView paper={activePaper} />
    {:else if activeTab?.kind === "vault"}
      <VaultHome workspace={activeVaultWorkspace} onOpenPaper={openPaper} />
    {:else}
      <div class="empty-workspace col">
        <div class="label hot">No workspace open</div>
        <h1>Open a vault folder from the Explorer.</h1>
        <p>The shell is still active; the center workspace is empty.</p>
      </div>
    {/if}
  </section>
</AppShell>

<style>
  .workspace {
    flex: 1;
    min-width: 0;
    min-height: 0;
    background: var(--bg);
  }

  .empty-workspace {
    flex: 1;
    align-items: center;
    justify-content: center;
    gap: 8px;
    color: var(--fg-2);
    text-align: center;
  }

  .empty-workspace h1 {
    margin: 0;
    color: var(--amber);
    font-size: 18px;
    font-weight: 600;
  }

  .empty-workspace p {
    margin: 0;
  }
</style>
