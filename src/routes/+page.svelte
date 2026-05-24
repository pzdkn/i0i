<script lang="ts">
  import { onMount } from "svelte";
  import AppShell from "$lib/app/AppShell.svelte";
  import WorkspaceTabs from "$lib/app/WorkspaceTabs.svelte";
  import {
    addPaperToVaults,
    createVault,
    deleteVault,
    deletePaperGlobally,
    getLibrary,
    removePaperFromVault,
    renameVault,
  } from "$lib/bridge/library";
  import type { LibrarySnapshot, Vault } from "$lib/domain/library";
  import { getVaultStatus } from "$lib/bridge/tauri";
  import type { Paper } from "$lib/domain/paper";
  import type { VaultStatus } from "$lib/domain/vault";
  import type { WorkspaceTab } from "$lib/domain/workspace";
  import DiscoverView from "$lib/features/discover/DiscoverView.svelte";
  import ReaderView from "$lib/features/reader/ReaderView.svelte";
  import VaultExplorer from "$lib/features/vault/VaultExplorer.svelte";
  import VaultHome from "$lib/features/vault/VaultHome.svelte";
  import {
    getCandidateVaultTargets,
    getDiscoverWorkspace,
    getPaperById,
    getPaperTitle,
    getVaultWorkspace,
    getVaultWorkspaces,
    hydrateLibrary,
    paperDraftFromDiscoverCandidate,
    paperFromDiscoverCandidate,
  } from "$lib/state/library-cache.svelte";

  let vaultStatus = $state<VaultStatus | null>(null);
  let bridgeError = $state("");
  let activeVaultId = $state("attention");
  let selectedPaperId = $state("vaswani2017");
  let selectedReaderPaper = $state<Paper>(getPaperById("vaswani2017"));
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
  const vaultWorkspaces = $derived(getVaultWorkspaces());
  const activeDiscoverWorkspace = $derived(getDiscoverWorkspace(activeTab?.discoverId ?? "ssl-dino"));
  const activePaper = $derived(selectedReaderPaper);
  const currentPath = $derived(activeTab?.title ?? "no workspace");
  const activeMode = $derived(activeTab?.kind === "reader" ? "R" : activeTab?.kind === "discover" ? "F" : "V");

  function makeVaultTab(vault: Pick<Vault, "id" | "path">): WorkspaceTab {
    return {
      id: `vault:${vault.id}`,
      kind: "vault",
      title: vault.path,
      vaultId: vault.id,
    };
  }

  onMount(async () => {
    try {
      const [nextVaultStatus, librarySnapshot] = await Promise.all([getVaultStatus(), getLibrary()]);
      vaultStatus = nextVaultStatus;
      hydrateLibrary(librarySnapshot);
    } catch (error) {
      bridgeError = String(error);
    }
  });

  function openVault(vaultId = activeVaultId) {
    const workspace = getVaultWorkspace(vaultId);
    if (!workspace) {
      activeVaultId = "";
      activeTabId = "";
      return;
    }

    const vaultTab = makeVaultTab(workspace);

    activeVaultId = workspace.id;
    tabs = [vaultTab, ...tabs.filter((tab) => tab.kind !== "vault")];
    activeTabId = vaultTab.id;
  }

  function openPaper(paperId: string) {
    const paper = getPaperById(paperId);
    selectedPaperId = paper.id;
    selectedReaderPaper = paper;

    const readerTab: WorkspaceTab = {
      id: `reader:${paper.id}`,
      kind: "reader",
      title: getPaperTitle(paper.id),
      paperId: paper.id,
    };

    tabs = [...tabs.filter((tab) => tab.kind !== "reader"), readerTab];
    activeTabId = readerTab.id;
  }

  function openDiscover(discoverId = "ssl-dino") {
    const workspace = getDiscoverWorkspace(discoverId);
    const discoverTab: WorkspaceTab = {
      id: `discover:${workspace.id}`,
      kind: "discover",
      title: workspace.title,
      discoverId: workspace.id,
    };

    tabs = [...tabs.filter((tab) => tab.id !== discoverTab.id), discoverTab];
    activeTabId = discoverTab.id;
  }

  function openCandidate(candidateId: string) {
    const paper = paperFromDiscoverCandidate(candidateId);
    selectedPaperId = paper.id;
    selectedReaderPaper = paper;

    const readerTab: WorkspaceTab = {
      id: `reader:${paper.id}`,
      kind: "reader",
      title: paper.title,
      paperId: paper.id,
    };

    tabs = [...tabs.filter((tab) => tab.kind !== "reader"), readerTab];
    activeTabId = readerTab.id;
  }

  async function addCandidate(candidateId: string, vaultIds: string[]) {
    try {
      const snapshot = await addPaperToVaults(paperDraftFromDiscoverCandidate(candidateId), vaultIds);
      hydrateLibrary(snapshot);
    } catch (error) {
      bridgeError = String(error);
    }
  }

  async function createVaultFromExplorer(path: string) {
    try {
      const snapshot = await createVault(path);
      hydrateLibrary(snapshot);
    } catch (error) {
      bridgeError = String(error);
    }
  }

  async function renameVaultFromExplorer(vaultId: string, path: string) {
    try {
      const snapshot = await renameVault(vaultId, path);
      hydrateLibrary(snapshot);

      const renamedVault = snapshot.vaults.find((vault) => vault.id === vaultId);
      if (renamedVault) {
        tabs = tabs.map((tab) => (tab.vaultId === vaultId ? { ...tab, title: renamedVault.path } : tab));
      }
    } catch (error) {
      bridgeError = String(error);
    }
  }

  async function deleteVaultFromExplorer(vaultId: string) {
    try {
      const snapshot = await deleteVault(vaultId);
      hydrateLibrary(snapshot);
      reconcileDeletedVault(snapshot, vaultId);
    } catch (error) {
      bridgeError = String(error);
    }
  }

  function reconcileDeletedVault(snapshot: LibrarySnapshot, deletedVaultId: string) {
    const wasActiveTabDeleted = tabs.some((tab) => tab.id === activeTabId && tab.vaultId === deletedVaultId);
    const nextTabs = tabs.filter((tab) => tab.vaultId !== deletedVaultId);
    tabs = nextTabs;

    if (activeVaultId === deletedVaultId) {
      activeVaultId = snapshot.vaults[0]?.id ?? "";
    }

    if (!wasActiveTabDeleted && nextTabs.some((tab) => tab.id === activeTabId)) {
      return;
    }

    const nextTab = nextTabs[0];
    if (nextTab) {
      activeTabId = nextTab.id;
      if (nextTab.vaultId) {
        activeVaultId = nextTab.vaultId;
      }
      return;
    }

    const nextVault = snapshot.vaults[0];
    if (nextVault) {
      const vaultTab = makeVaultTab(nextVault);
      tabs = [vaultTab];
      activeVaultId = nextVault.id;
      activeTabId = vaultTab.id;
      return;
    }

    activeVaultId = "";
    activeTabId = "";
  }

  async function removePaperFromActiveVault(vaultId: string, paperId: string) {
    try {
      const snapshot = await removePaperFromVault(vaultId, paperId);
      hydrateLibrary(snapshot);
    } catch (error) {
      bridgeError = String(error);
    }
  }

  async function removePaperFromLibrary(paperId: string) {
    try {
      const snapshot = await deletePaperGlobally(paperId);
      hydrateLibrary(snapshot);
    } catch (error) {
      bridgeError = String(error);
    }
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
      if (!selectedReaderPaper || selectedReaderPaper.id !== tab.paperId) {
        selectedReaderPaper = getPaperById(tab.paperId);
      }
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
      return;
    }

    if (mode === "F") {
      openDiscover();
    }
  }
</script>

<AppShell {activeMode} {currentPath} {vaultStatus} {bridgeError} onSelectMode={handleModeSelect}>
  <VaultExplorer
    {activeVaultId}
    vaults={vaultWorkspaces}
    onOpenVault={openVault}
    onCreateVault={createVaultFromExplorer}
    onRenameVault={renameVaultFromExplorer}
    onDeleteVault={deleteVaultFromExplorer}
  />
  <section class="workspace col">
    <WorkspaceTabs {tabs} {activeTabId} onActivate={activateTab} onClose={closeTab} />

    {#if activeTab?.kind === "reader"}
      <ReaderView paper={activePaper} />
    {:else if activeTab?.kind === "discover"}
      <DiscoverView
        workspace={activeDiscoverWorkspace}
        vaults={vaultWorkspaces}
        onOpenCandidate={openCandidate}
        onAddCandidate={addCandidate}
        {getCandidateVaultTargets}
      />
    {:else if activeTab?.kind === "vault"}
      <VaultHome
        workspace={activeVaultWorkspace}
        onOpenPaper={openPaper}
        onRemovePaperFromVault={removePaperFromActiveVault}
        onRemovePaperFromLibrary={removePaperFromLibrary}
      />
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
