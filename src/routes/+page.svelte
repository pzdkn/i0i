<script lang="ts">
  import { listen } from "@tauri-apps/api/event";
  import { onMount } from "svelte";
  import AppShell from "$lib/app/AppShell.svelte";
  import WorkspaceTabs from "$lib/app/WorkspaceTabs.svelte";
  import ResizableSplit from "$lib/components/layout/ResizableSplit.svelte";
  import { searchPapers } from "$lib/bridge/discovery";
  import {
    createSearch,
    listSearchCandidates,
    listenSearchCandidatesPreview,
    listenSearchUpdated,
    runSearch as runResearchSearch,
  } from "$lib/bridge/research";
  import {
    addPaperToVaults,
    autofillPaperMetadata,
    createVault,
    getLibrary,
    importLocalPdfs,
    removeVault,
    removePaperFromLibrary as removePaperFromLibraryCommand,
    removePaperFromVault,
    renameVault,
  } from "$lib/bridge/library";
  import type { LibrarySnapshot, Vault } from "$lib/domain/library";
  import { getVaultStatus } from "$lib/bridge/tauri";
  import type { Paper } from "$lib/domain/paper";
  import type { DiscoveryReaderCandidate } from "$lib/domain/reader";
  import type { VaultStatus } from "$lib/domain/vault";
  import type { WorkspaceTab } from "$lib/domain/workspace";
  import DiscoverView from "$lib/features/discover/DiscoverView.svelte";
  import ReaderView from "$lib/features/reader/ReaderView.svelte";
  import VaultExplorer from "$lib/features/vault/VaultExplorer.svelte";
  import VaultHome from "$lib/features/vault/VaultHome.svelte";
  import {
    getCandidateVaultTargets,
    getDiscoverCandidate,
    getDiscoverWorkspace,
    getDiscoverWorkspaces,
    getPaperById,
    getPaperTitle,
    getVaultWorkspace,
    getVaultWorkspaces,
    hydrateLibrary,
    isPaperInLibrary,
    appendDiscoverRunTrace,
    applyDiscoverResearchPreview,
    applyDiscoverResearchCandidates,
    applyDiscoverSearchResponse,
    createDiscoverWorkspace,
    createDiscoverWorkspaceFrom,
    discoverTitleFromQuery,
    paperDraftFromDiscoverCandidate,
    paperFromDiscoverCandidate,
    removeDiscoverWorkspace,
    setDiscoverImproveStarted,
    setDiscoverRunStarted,
    setDiscoverStatus,
    setDiscoverSelectedCandidate,
  } from "$lib/state/library-cache.svelte";
  import { depthStrategy, isTerminalStatus, type SearchCandidatesPreview, type SearchUpdated } from "$lib/domain/research";

  let vaultStatus = $state<VaultStatus | null>(null);
  let bridgeError = $state("");
  let activeVaultId = $state("");
  let selectedPaperId = $state("");
  let selectedReaderPaper = $state<Paper | null>(null);
  let autofillingMetadataPaperIds = $state<string[]>([]);
  let activeTabId = $state("");
  let tabs = $state<WorkspaceTab[]>([]);
  let readerLayoutMode = $state<"normal" | "focus">("normal");

  const activeTab = $derived(tabs.find((tab) => tab.id === activeTabId));
  const activeVaultWorkspace = $derived(getVaultWorkspace(activeVaultId));
  const vaultWorkspaces = $derived(getVaultWorkspaces());
  const discoverWorkspaces = $derived(getDiscoverWorkspaces());
  const activeDiscoverWorkspace = $derived(getDiscoverWorkspace(activeTab?.discoverId ?? "discover-1"));
  const activePaper = $derived.by<Paper | null>(() => {
    if (activeTab?.kind !== "reader") {
      return null;
    }

    if (activeTab.paperId) {
      return getPaperById(activeTab.paperId) ?? selectedReaderPaper;
    }

    return selectedReaderPaper;
  });
  const activeReaderCandidate = $derived.by<DiscoveryReaderCandidate | undefined>(() => {
    if (activeTab?.kind !== "reader" || !activeTab.readerCandidate) {
      return undefined;
    }

    return isPaperInLibrary(activeTab.paperId ?? activeTab.readerCandidate.id) ? undefined : activeTab.readerCandidate;
  });
  const currentPath = $derived(activeTab?.title ?? "no workspace");
  const activeMode = $derived(activeTab?.kind === "reader" ? "R" : activeTab?.kind === "discover" ? "F" : "V");
  const isReaderFocusMode = $derived(readerLayoutMode === "focus" && activeTab?.kind === "reader" && Boolean(activePaper));

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

  onMount(() => {
    let unlisten: (() => void) | undefined;

    listen("document_source_updated", async () => {
      try {
        hydrateLibrary(await getLibrary());
      } catch (error) {
        bridgeError = String(error);
      }
    })
      .then((nextUnlisten) => {
        unlisten = nextUnlisten;
      })
      .catch((error) => {
        bridgeError = String(error);
      });

    return () => unlisten?.();
  });

  onMount(() => {
    let unlisten: (() => void) | undefined;

    listen<{ paperId: string; status: string; error?: string }>("paper_metadata_updated", async (event) => {
      try {
        const snapshot = await getLibrary();
        hydrateLibrary(snapshot);
        syncPaperMetadata(event.payload.paperId);
        autofillingMetadataPaperIds = autofillingMetadataPaperIds.filter((paperId) => paperId !== event.payload.paperId);
        if (event.payload.status === "failed" && event.payload.error) {
          bridgeError = event.payload.error;
        }
      } catch (error) {
        bridgeError = String(error);
      }
    })
      .then((nextUnlisten) => {
        unlisten = nextUnlisten;
      })
      .catch((error) => {
        bridgeError = String(error);
      });

    return () => unlisten?.();
  });

  onMount(() => {
    let unlisten: (() => void) | undefined;

    listenSearchUpdated(handleResearchUpdate)
      .then((nextUnlisten) => {
        unlisten = nextUnlisten;
      })
      .catch((error) => {
        bridgeError = String(error);
      });

    return () => unlisten?.();
  });

  onMount(() => {
    let unlisten: (() => void) | undefined;

    listenSearchCandidatesPreview(handleResearchPreview)
      .then((nextUnlisten) => {
        unlisten = nextUnlisten;
      })
      .catch((error) => {
        bridgeError = String(error);
      });

    return () => unlisten?.();
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
    if (!paper) {
      return;
    }
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

  function enterReaderFocus() {
    readerLayoutMode = "focus";
  }

  function exitReaderFocus() {
    readerLayoutMode = "normal";
  }

  function handleWindowKeydown(event: KeyboardEvent) {
    const target = event.target as HTMLElement | null;
    const isEditing =
      target?.tagName === "INPUT" || target?.tagName === "TEXTAREA" || target?.isContentEditable;
    if (event.key === "Escape" && isReaderFocusMode && !isEditing) {
      event.preventDefault();
      exitReaderFocus();
    }
  }

  function syncPaperMetadata(paperId: string) {
    const paper = getPaperById(paperId);
    if (!paper) {
      return;
    }

    if (selectedReaderPaper?.id === paperId) {
      selectedReaderPaper = paper;
    }

    tabs = tabs.map((tab) => (tab.paperId === paperId ? { ...tab, title: getPaperTitle(paperId) } : tab));
  }

  function makeDiscoverTab(workspace: ReturnType<typeof getDiscoverWorkspace>): WorkspaceTab {
    return {
      id: `discover:${workspace.id}`,
      kind: "discover",
      title: workspace.title,
      discoverId: workspace.id,
    };
  }

  function openDiscover(discoverId?: string) {
    if (!discoverId) {
      const existingDiscoverTab = tabs.find((tab) => tab.kind === "discover");
      if (existingDiscoverTab) {
        activeTabId = existingDiscoverTab.id;
        return;
      }
    }

    const workspace = getDiscoverWorkspace(discoverId ?? "discover-1");
    const discoverTab = makeDiscoverTab(workspace);

    tabs = [...tabs.filter((tab) => tab.id !== discoverTab.id), discoverTab];
    activeTabId = discoverTab.id;
  }

  function openNewDiscover() {
    const workspace = createDiscoverWorkspace();
    const discoverTab = makeDiscoverTab(workspace);

    tabs = [...tabs, discoverTab];
    activeTabId = discoverTab.id;
  }

  function openDiscoverWorkspace(workspace: ReturnType<typeof getDiscoverWorkspace>) {
    const discoverTab = makeDiscoverTab(workspace);
    tabs = [...tabs.filter((tab) => tab.id !== discoverTab.id), discoverTab];
    activeTabId = discoverTab.id;
  }

  function updateDiscoverTabTitle(discoverId: string, title: string) {
    tabs = tabs.map((tab) => (tab.discoverId === discoverId ? { ...tab, title } : tab));
  }

  function parseOptionalYear(value: string) {
    const trimmed = value.trim();
    if (!trimmed) {
      return undefined;
    }

    const year = Number(trimmed);
    return Number.isInteger(year) ? year : undefined;
  }

  function workspaceForNewRun(discoverId: string) {
    const workspace = getDiscoverWorkspace(discoverId);
    if (workspace.candidates.length === 0 || workspace.status === "running") {
      return workspace;
    }

    const nextWorkspace = createDiscoverWorkspaceFrom(workspace);
    openDiscoverWorkspace(nextWorkspace);
    return nextWorkspace;
  }

  function selectedProviders(workspace: ReturnType<typeof getDiscoverWorkspace>) {
    return workspace.providers.length > 0 ? [...workspace.providers] : [workspace.provider];
  }

  function selectedVenues(workspace: ReturnType<typeof getDiscoverWorkspace>) {
    const venue = workspace.venue.trim();
    return venue ? [venue] : [];
  }

  async function runDiscoverSearch(discoverId: string) {
    const workspace = workspaceForNewRun(discoverId);
    const query = workspace.query.trim();
    if (!query) {
      setDiscoverStatus(workspace.id, "failed", "Enter a search query before running discovery.");
      return;
    }

    if (workspace.deep) {
      await runDeepDiscoverSearch(workspace.id, query);
      return;
    }

    setDiscoverStatus(workspace.id, "running");
    setDiscoverRunStarted(workspace.id, "shallow");

    try {
      const response = await searchPapers({
        query,
        yearFrom: parseOptionalYear(workspace.yearFrom),
        yearTo: parseOptionalYear(workspace.yearTo),
        resultLimit: Number(workspace.resultLimit),
        sortBy: workspace.sortBy,
        provider: workspace.provider,
        providers: selectedProviders(workspace),
        openAccess: workspace.openAccess,
        venues: selectedVenues(workspace),
      });
      applyDiscoverSearchResponse(workspace.id, response);
      const title = discoverTitleFromQuery(query);
      workspace.title = title;
      updateDiscoverTabTitle(workspace.id, title);
    } catch (error) {
      setDiscoverStatus(workspace.id, "failed", String(error));
    }
  }

  async function runDeepDiscoverSearch(discoverId: string, query: string) {
    const workspace = getDiscoverWorkspace(discoverId);
    const title = discoverTitleFromQuery(query);
    workspace.title = title;
    updateDiscoverTabTitle(discoverId, title);
    setDiscoverStatus(discoverId, "running");
    setDiscoverRunStarted(discoverId, "deep");

    try {
      const search = await createSearch({
        title,
        goal: query,
        constraints: {
          yearFrom: parseOptionalYear(workspace.yearFrom),
          yearTo: parseOptionalYear(workspace.yearTo),
          providers: selectedProviders(workspace),
          openAccess: workspace.openAccess,
          targetCount: Number(workspace.resultLimit),
          venues: selectedVenues(workspace),
          authors: [],
          fieldsOfStudy: [],
          seedPaperIds: [],
        },
        strategy: depthStrategy(workspace.deepDepth),
      });
      setDiscoverRunStarted(discoverId, "deep", search.id);
      const runId = await runResearchSearch(search.id, "deep");
      setDiscoverRunStarted(discoverId, "deep", search.id, runId);
    } catch (error) {
      setDiscoverStatus(discoverId, "failed", String(error));
    }
  }

  async function improveDiscoverSearch(discoverId: string) {
    const workspace = getDiscoverWorkspace(discoverId);
    const query = workspace.query.trim();
    if (!query || workspace.status === "running") {
      return;
    }

    const topTitles = workspace.candidates
      .slice(0, 8)
      .map((candidate, index) => `${index + 1}. ${candidate.title}`)
      .join("\n");
    const title = workspace.title || discoverTitleFromQuery(query);
    setDiscoverImproveStarted(discoverId);

    try {
      const search = await createSearch({
        title: `${title} · improve`,
        goal: `Improve this literature search.\nOriginal query: ${query}\nCurrent top results:\n${topTitles}`,
        constraints: {
          yearFrom: parseOptionalYear(workspace.yearFrom),
          yearTo: parseOptionalYear(workspace.yearTo),
          providers: selectedProviders(workspace),
          openAccess: workspace.openAccess,
          targetCount: Number(workspace.resultLimit),
          venues: selectedVenues(workspace),
          authors: [],
          fieldsOfStudy: [],
          seedPaperIds: [],
        },
        strategy: depthStrategy(workspace.deepDepth),
      });
      setDiscoverImproveStarted(discoverId, search.id);
      const runId = await runResearchSearch(search.id, "improve");
      setDiscoverImproveStarted(discoverId, search.id, runId);
    } catch (error) {
      setDiscoverStatus(discoverId, "failed", String(error));
    }
  }

  async function handleResearchUpdate(event: SearchUpdated) {
    const workspace = getDiscoverWorkspaces().find((item) => item.researchSearchId === event.searchId);
    if (!workspace) {
      return;
    }

    appendDiscoverRunTrace(workspace.id, event);

    if (!isTerminalStatus(event.status)) {
      return;
    }

    if (event.status === "failed") {
      setDiscoverStatus(workspace.id, "failed", event.message || "Deep research failed.");
      return;
    }

    if (event.status === "cancelled") {
      setDiscoverStatus(workspace.id, "failed", "Deep research was cancelled.");
      return;
    }

    try {
      const candidates = await listSearchCandidates(event.searchId);
      applyDiscoverResearchCandidates(workspace.id, workspace.query, candidates);
      updateDiscoverTabTitle(workspace.id, workspace.title);
    } catch (error) {
      setDiscoverStatus(workspace.id, "failed", String(error));
    }
  }

  function handleResearchPreview(event: SearchCandidatesPreview) {
    const workspace = getDiscoverWorkspaces().find((item) => item.researchSearchId === event.searchId);
    if (!workspace || workspace.status !== "running") {
      return;
    }

    applyDiscoverResearchPreview(workspace.id, event.candidates);
  }

  function selectDiscoverCandidate(discoverId: string, candidateId: string) {
    setDiscoverSelectedCandidate(discoverId, candidateId);
  }

  function openCandidate(candidateId: string) {
    const candidate = getDiscoverCandidate(candidateId);
    const paper = paperFromDiscoverCandidate(candidateId);
    if (!paper || !candidate) {
      return;
    }
    selectedPaperId = paper.id;
    selectedReaderPaper = paper;

    const readerTab: WorkspaceTab = {
      id: `reader:${paper.id}`,
      kind: "reader",
      title: paper.title,
      paperId: paper.id,
      readerCandidate: {
        id: candidate.id,
        sourceProvider: candidate.sourceProvider,
        sourceId: candidate.sourceId,
        title: candidate.title,
        authors: [...candidate.authors],
        venue: candidate.venue,
        year: candidate.year,
        citations: candidate.citations,
        tags: [...candidate.tags],
        abstract: candidate.abstract,
        externalUrl: candidate.externalUrl,
        pdfUrl: candidate.pdfUrl,
      },
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

  async function importPdfsToVault(vaultId: string, paths: string[]) {
    try {
      const result = await importLocalPdfs(vaultId, paths);
      hydrateLibrary(result.snapshot);
    } catch (error) {
      bridgeError = String(error);
    }
  }

  async function autofillMetadataForPaper(paperId: string) {
    if (autofillingMetadataPaperIds.includes(paperId)) {
      return;
    }

    autofillingMetadataPaperIds = [...autofillingMetadataPaperIds, paperId];
    try {
      await autofillPaperMetadata(paperId);
    } catch (error) {
      autofillingMetadataPaperIds = autofillingMetadataPaperIds.filter((id) => id !== paperId);
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

  async function removeVaultFromExplorer(vaultId: string) {
    try {
      const snapshot = await removeVault(vaultId);
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
      const snapshot = await removePaperFromLibraryCommand(paperId);
      hydrateLibrary(snapshot);
    } catch (error) {
      bridgeError = String(error);
    }
  }

  function closeTab(tabId: string) {
    const closedTab = tabs.find((tab) => tab.id === tabId);
    const nextTabs = tabs.filter((tab) => tab.id !== tabId);
    tabs = nextTabs;
    if (closedTab?.discoverId) {
      removeDiscoverWorkspace(closedTab.discoverId);
    }

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
        const paper =
          getPaperById(tab.paperId) ??
          (tab.readerCandidate ? paperFromDiscoverCandidate(tab.readerCandidate.id) : undefined);
        if (paper) {
          selectedReaderPaper = paper;
        }
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

<svelte:window onkeydown={handleWindowKeydown} />

<AppShell {activeMode} {currentPath} {vaultStatus} {bridgeError} readerFocusMode={isReaderFocusMode} onSelectMode={handleModeSelect}>
  {#if isReaderFocusMode && activePaper}
    <section class="workspace focus-workspace col">
      <ReaderView
        paper={activePaper}
        candidate={activeReaderCandidate}
        layoutMode="focus"
        onToggleFocus={exitReaderFocus}
      />
    </section>
  {:else}
    <ResizableSplit
      storageKey="i0i.main-split"
      panes={[
        { id: "explorer", min: 240, max: 560, default: 320 },
        { id: "workspace", min: 640, default: 1040 },
      ]}
    >
      {#snippet pane(id: string)}
        {#if id === "explorer"}
          <VaultExplorer
            {activeVaultId}
            vaults={vaultWorkspaces}
            onOpenVault={openVault}
            onCreateVault={createVaultFromExplorer}
            onRenameVault={renameVaultFromExplorer}
            onRemoveVault={removeVaultFromExplorer}
          />
        {:else}
          <section class="workspace col">
            <WorkspaceTabs {tabs} {activeTabId} onActivate={activateTab} onClose={closeTab} />

            {#if activeTab?.kind === "reader" && activePaper}
              <ReaderView
                paper={activePaper}
                candidate={activeReaderCandidate}
                layoutMode="normal"
                onToggleFocus={enterReaderFocus}
              />
            {:else if activeTab?.kind === "discover"}
              <DiscoverView
                workspace={activeDiscoverWorkspace}
                discoverWorkspaces={discoverWorkspaces}
                vaults={vaultWorkspaces}
                onNewSearch={openNewDiscover}
                onActivateSearch={openDiscover}
                onRunSearch={runDiscoverSearch}
                onImproveSearch={improveDiscoverSearch}
                onSelectCandidate={selectDiscoverCandidate}
                onOpenCandidate={openCandidate}
                onAddCandidate={addCandidate}
                {getCandidateVaultTargets}
              />
            {:else if activeTab?.kind === "vault" && activeVaultWorkspace}
              <VaultHome
                workspace={activeVaultWorkspace}
                onOpenPaper={openPaper}
                onImportPdfs={importPdfsToVault}
                onAutofillMetadata={autofillMetadataForPaper}
                {autofillingMetadataPaperIds}
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
        {/if}
      {/snippet}
    </ResizableSplit>
  {/if}
</AppShell>

<style>
  .workspace {
    flex: 1;
    min-width: 0;
    min-height: 0;
    background: var(--bg);
  }

  .focus-workspace {
    overflow: hidden;
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
