<script lang="ts">
  import { listen } from "@tauri-apps/api/event";
  import { onMount } from "svelte";
  import AppShell from "$lib/app/AppShell.svelte";
  import SettingsDialog from "$lib/features/settings/SettingsDialog.svelte";
  import WorkspaceTabs from "$lib/app/WorkspaceTabs.svelte";
  import ResizableSplit from "$lib/components/layout/ResizableSplit.svelte";
  import { searchPapers, expandSearch } from "$lib/bridge/discovery";
  import { getSettings } from "$lib/bridge/settings";
  import {
    createSearch,
    listSearchCandidates,
    listenSearchCandidatesPreview,
    listenSearchUpdated,
    runSearch as runResearchSearch,
  } from "$lib/bridge/research";
  import {
    addPaperToVaults,
    applyPaperMetadataCandidate,
    autofillPaperMetadata,
    createVault,
    getLibrary,
    importLocalPdfs,
    addHtmlUrlToVault,
    probeDiscoveryCandidatePdf,
    removeVault,
    removePaperFromLibrary as removePaperFromLibraryCommand,
    removePaperFromVault,
    renameVault,
    updatePaperMetadata,
  } from "$lib/bridge/library";
  import type {
    LibrarySnapshot,
    MetadataAutofillProgress,
    MetadataCandidate,
    PaperMetadataUpdate,
    Vault,
  } from "$lib/domain/library";
  import { getVaultStatus } from "$lib/bridge/tauri";
  import type { Paper } from "$lib/domain/paper";
  import type { DiscoveryReaderCandidate } from "$lib/domain/reader";
  import type { ChunkHit } from "$lib/domain/search";
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
    applyDiscoverExpansion,
    applyDiscoverDefaults,
    createDiscoverWorkspace,
    createDiscoverWorkspaceFrom,
    discoverTitleFromQuery,
    paperDraftFromDiscoverCandidate,
    paperFromDiscoverCandidate,
    removeDiscoverWorkspace,
    setDiscoverImproveStarted,
    setDiscoverRunStarted,
    setDiscoverStatus,
    setDiscoverCandidateAvailability,
    setDiscoverSelectedCandidate,
  } from "$lib/state/library-cache.svelte";
  import { depthStrategy, isTerminalStatus, type SearchCandidatesPreview, type SearchUpdated } from "$lib/domain/research";

  let vaultStatus = $state<VaultStatus | null>(null);
  let bridgeError = $state("");
  let settingsOpen = $state(false);
  let settingsAttention = $state(false);

  // Load user settings: seed Discover defaults and flag the gear when a required
  // key (OpenRouter) is unresolved (RFC 0055). Re-run when Settings closes.
  async function refreshSettingsState() {
    try {
      const view = await getSettings();
      applyDiscoverDefaults(view.prefs);
      settingsAttention = !view.secrets.find((secret) => secret.name === "openrouter")?.configured;
    } catch {
      // Settings are best-effort; defaults stand if the load fails.
    }
  }
  let activeVaultId = $state("");
  let selectedPaperId = $state("");
  let selectedReaderPaper = $state<Paper | null>(null);
  let autofillingMetadataPaperIds = $state<string[]>([]);
  let metadataAutofillProgressByPaperId = $state<Record<string, MetadataAutofillProgress>>({});
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
      // RFC 0084: `selectedReaderPaper` holds the last opened paper, including
      // the synthetic one a discovery candidate resolves to — which is not in
      // the library, so `getPaperById` misses it. With one reader tab that
      // fallback was always the right paper. With several it has to prove it is
      // this tab's, or two candidate tabs render the same document.
      return (
        getPaperById(activeTab.paperId) ??
        (selectedReaderPaper?.id === activeTab.paperId ? selectedReaderPaper : null)
      );
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
    await refreshSettingsState();
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

    listen<MetadataAutofillProgress>("metadata_autofill_progress", (event) => {
      metadataAutofillProgressByPaperId = {
        ...metadataAutofillProgressByPaperId,
        [event.payload.paperId]: event.payload,
      };
      if (["applied", "failed", "no_match", "needs_review"].includes(event.payload.status)) {
        autofillingMetadataPaperIds = autofillingMetadataPaperIds.filter((paperId) => paperId !== event.payload.paperId);
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

  /**
   * RFC 0084 R1.1/R1.2: place a reader tab without evicting the others. Opening
   * a paper used to close whichever paper was open — the tab id has always been
   * `reader:<paperId>`, so one tab per paper only ever needed this.
   *
   * A paper already open keeps its position and takes the new tab's contents,
   * so reopening it as a discovery candidate refreshes the candidate rather
   * than adding a second tab for the same paper.
   */
  function withReaderTab(readerTab: WorkspaceTab): WorkspaceTab[] {
    const existing = tabs.findIndex((tab) => tab.id === readerTab.id);
    if (existing === -1) {
      return [...tabs, readerTab];
    }
    return tabs.map((tab, index) => (index === existing ? readerTab : tab));
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

    tabs = withReaderTab(readerTab);
    activeTabId = readerTab.id;
  }

  const resolvePaperTitle = (paperId: string) => getPaperTitle(paperId);

  /**
   * Open the paper a title-bar search hit belongs to (RFC 0076).
   *
   * Stops at the paper for now. Jumping to the hit's page needs the reader to
   * accept a target page on open, and scrolling to the exact passage needs the
   * chunk resolved through its blocks to spans and rectangles — both real, both
   * more than wiring up a dead input.
   */
  function openSearchResult(paperId: string, _hit: ChunkHit) {
    openPaper(paperId);
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

    const request = {
      query,
      yearFrom: parseOptionalYear(workspace.yearFrom),
      yearTo: parseOptionalYear(workspace.yearTo),
      resultLimit: Number(workspace.resultLimit),
      sortBy: workspace.sortBy,
      provider: workspace.provider,
      providers: selectedProviders(workspace),
      openAccess: workspace.openAccess,
      onlyViewable: workspace.onlyViewable,
      venues: selectedVenues(workspace),
    };

    try {
      const response = await searchPapers(request);
      applyDiscoverSearchResponse(workspace.id, response);
      const title = discoverTitleFromQuery(query);
      workspace.title = title;
      updateDiscoverTabTitle(workspace.id, title);

      // Progressive expansion (RFC 0054): literal results are on screen; widen
      // recall in the background and merge the reranked superset when it lands.
      // Best-effort — failures leave the literal results untouched.
      if (workspace.expandSearch) {
        void expandSearch(request)
          .then((expanded) => applyDiscoverExpansion(workspace.id, expanded, query))
          .catch(() => {});
      }
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

  function readerCandidateFrom(candidate: NonNullable<ReturnType<typeof getDiscoverCandidate>>): DiscoveryReaderCandidate {
    return {
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
      doi: candidate.doi,
      arxivId: candidate.arxivId,
    };
  }

  function selectDiscoverCandidate(discoverId: string, candidateId: string) {
    setDiscoverSelectedCandidate(discoverId, candidateId);

    // RFC 0051: verify PDF availability in the background on selection so the
    // chip reflects reality before the user commits to opening the Reader.
    const candidate = getDiscoverCandidate(candidateId);
    if (!candidate || candidate.pdfAvailability) {
      return;
    }
    probeDiscoveryCandidatePdf(readerCandidateFrom(candidate))
      .then((availability) => {
        setDiscoverCandidateAvailability(discoverId, candidateId, availability);
      })
      .catch(() => {
        // Probe failures leave the chip in its unverified state.
      });
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
      readerCandidate: readerCandidateFrom(candidate),
    };

    tabs = withReaderTab(readerTab);
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

  // RFC 0065: add a web page by URL. Rethrows so VaultHome can show the fetch
  // error inline next to the URL field (paywalls / JS-only pages fail here).
  async function addHtmlUrlToVaultWorkspace(vaultId: string, url: string) {
    const result = await addHtmlUrlToVault(vaultId, url);
    hydrateLibrary(result.snapshot);
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

  // RFC 0050: applying or manually saving metadata dismisses the review
  // panel — stale candidates must not linger with live Apply buttons.
  function clearMetadataAutofillState(paperId: string) {
    autofillingMetadataPaperIds = autofillingMetadataPaperIds.filter((id) => id !== paperId);
    const { [paperId]: _cleared, ...rest } = metadataAutofillProgressByPaperId;
    metadataAutofillProgressByPaperId = rest;
  }

  async function applyMetadataCandidateForPaper(paperId: string, candidate: MetadataCandidate) {
    try {
      const snapshot = await applyPaperMetadataCandidate(paperId, candidate);
      hydrateLibrary(snapshot);
      syncPaperMetadata(paperId);
      clearMetadataAutofillState(paperId);
    } catch (error) {
      bridgeError = String(error);
    }
  }

  async function updatePaperMetadataForPaper(paperId: string, update: PaperMetadataUpdate) {
    const snapshot = await updatePaperMetadata(paperId, update);
    hydrateLibrary(snapshot);
    syncPaperMetadata(paperId);
    clearMetadataAutofillState(paperId);
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

<AppShell {activeMode} {currentPath} {vaultStatus} {bridgeError} readerFocusMode={isReaderFocusMode} onSelectMode={handleModeSelect} onOpenSettings={() => (settingsOpen = true)} {settingsAttention} searchVaultId={activeVaultId} {resolvePaperTitle} onOpenSearchResult={openSearchResult}>
  {#if isReaderFocusMode && activePaper}
    <section class="workspace focus-workspace col">
      <ReaderView
        paper={activePaper}
        candidate={activeReaderCandidate}
        layoutMode="focus"
        metadataAutofillProgress={metadataAutofillProgressByPaperId[activePaper.id]}
        isAutofillingMetadata={autofillingMetadataPaperIds.includes(activePaper.id)}
        onAutofillMetadata={autofillMetadataForPaper}
        onApplyMetadataCandidate={applyMetadataCandidateForPaper}
        onUpdatePaperMetadata={updatePaperMetadataForPaper}
        onToggleFocus={exitReaderFocus}
      />
    </section>
  {:else}
    <ResizableSplit
      storageKey="i0i.main-split"
      panes={[
        { id: "explorer", min: 160, max: 720, default: 320 },
        { id: "workspace", min: 480, default: 1040 },
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
                metadataAutofillProgress={metadataAutofillProgressByPaperId[activePaper.id]}
                isAutofillingMetadata={autofillingMetadataPaperIds.includes(activePaper.id)}
                onAutofillMetadata={autofillMetadataForPaper}
                onApplyMetadataCandidate={applyMetadataCandidateForPaper}
                onUpdatePaperMetadata={updatePaperMetadataForPaper}
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
                onAddHtmlUrl={addHtmlUrlToVaultWorkspace}
                onAutofillMetadata={autofillMetadataForPaper}
                {autofillingMetadataPaperIds}
                {metadataAutofillProgressByPaperId}
                onApplyMetadataCandidate={applyMetadataCandidateForPaper}
                onUpdatePaperMetadata={updatePaperMetadataForPaper}
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

<SettingsDialog
  open={settingsOpen}
  onClose={() => {
    settingsOpen = false;
    void refreshSettingsState();
  }}
/>

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
