import type { Paper } from "$lib/domain/paper";
import type { DiscoverCandidate, DiscoverWorkspace, DiscoverySearchResponse } from "$lib/domain/discover";
import type { LibrarySnapshot, PaperDraft, VaultWorkspace } from "$lib/domain/library";

type LibraryState = {
  vaults: VaultWorkspace[];
  discoverWorkspaces: DiscoverWorkspace[];
};

// Saved library data comes from Rust/SQLite. This module caches the latest
// LibrarySnapshot for Svelte views.
let discoverSequence = 1;
const initialDiscoverWorkspaces = [makeDiscoverWorkspace()];

const library = $state<LibraryState>({
  vaults: [],
  discoverWorkspaces: initialDiscoverWorkspaces,
});

function clonePaper(paper: Paper): Paper {
  return {
    ...paper,
    authors: [...paper.authors],
    tags: [...paper.tags],
  };
}

function cloneVaultWorkspace(workspace: VaultWorkspace): VaultWorkspace {
  return {
    ...workspace,
    tabs: workspace.tabs.map((tab) => ({ ...tab })),
    chips: [...workspace.chips],
    papers: workspace.papers.map(clonePaper),
  };
}

function cloneDiscoverWorkspace(workspace: DiscoverWorkspace): DiscoverWorkspace {
  return {
    ...workspace,
    seeds: [...workspace.seeds],
    lastRun: workspace.lastRun
      ? {
          ...workspace.lastRun,
          filters: [...workspace.lastRun.filters],
        }
      : undefined,
    candidates: workspace.candidates.map((candidate) => ({
      ...candidate,
      authors: [...candidate.authors],
      tags: [...candidate.tags],
      match: candidate.match
        ? {
            ...candidate.match,
            reasons: [...candidate.match.reasons],
            matchedKeywords: [...candidate.match.matchedKeywords],
            fromSeedPaperIds: [...candidate.match.fromSeedPaperIds],
          }
        : undefined,
    })),
  };
}

function nextDiscoverId() {
  const id = `discover-${discoverSequence}`;
  discoverSequence += 1;
  return id;
}

function makeDiscoverWorkspace(id = nextDiscoverId()): DiscoverWorkspace {
  return {
    id,
    title: "Discover: Untitled",
    seeds: [],
    query: "",
    yearFrom: "",
    yearTo: "",
    resultLimit: 25,
    sortBy: "relevance",
    openAccessOnly: true,
    status: "idle",
    error: "",
    candidates: [],
  };
}

function findCandidate(candidateId: string) {
  for (const workspace of library.discoverWorkspaces) {
    const candidate = workspace.candidates.find((item) => item.id === candidateId);
    if (candidate) {
      return candidate;
    }
  }

  return undefined;
}

function makeVaultWorkspace(snapshot: LibrarySnapshot, vaultId: string): VaultWorkspace {
  const vault = snapshot.vaults.find((item) => item.id === vaultId) ?? snapshot.vaults[0];
  const papers = snapshot.vaultPapers
    .filter((vaultPaper) => vaultPaper.vaultId === vault.id)
    .map((vaultPaper) => snapshot.papers.find((paper) => paper.id === vaultPaper.paperId))
    .filter((paper): paper is Paper => Boolean(paper));
  const tagCounts = new Map<string, number>();

  for (const paper of papers) {
    for (const tag of paper.tags) {
      tagCounts.set(tag, (tagCounts.get(tag) ?? 0) + 1);
    }
  }

  const chips = [...tagCounts.entries()]
    .sort((left, right) => right[1] - left[1])
    .slice(0, 6)
    .map(([tag, count]) => `${tag} x${count}`);
  const noteCount = papers.reduce((sum, paper) => sum + paper.noteCount, 0);
  const annotationCount = papers.reduce((sum, paper) => sum + paper.annotationCount, 0);
  const unreadCount = papers.filter((paper) => paper.status === "UNREAD").length;

  return {
    id: vault.id,
    title: vault.title,
    path: vault.path,
    summary: `${papers.length} papers / ${unreadCount} unread / local`,
    tabs: [
      { label: "Papers", count: papers.length },
      { label: "Notes", count: noteCount },
      { label: "Annotations", count: annotationCount },
      { label: "Graph" },
      { label: "Q&A" },
    ],
    chips,
    papers,
  };
}

function markDiscoverOwnership() {
  const paperIds = new Set(library.vaults.flatMap((workspace) => workspace.papers.map((paper) => paper.id)));

  for (const workspace of library.discoverWorkspaces) {
    for (const candidate of workspace.candidates) {
      candidate.owned = paperIds.has(candidate.id) || candidate.alreadyInLibrary === true;
    }
  }
}

function candidateToPaper(candidateId: string): Paper | undefined {
  const candidate = findCandidate(candidateId);
  if (!candidate) {
    return undefined;
  }

  return {
    id: candidate.id,
    title: candidate.title,
    authors: [...candidate.authors],
    venue: candidate.venue,
    year: candidate.year,
    citations: candidate.citations,
    tags: [...candidate.tags],
    noteCount: 0,
    annotationCount: 0,
    status: candidate.owned ? "READ" : "UNREAD",
    abstract: candidate.abstract ?? candidate.why,
  };
}

export function getVaultWorkspaces() {
  return library.vaults;
}

export function getVaultWorkspace(vaultId: string) {
  return library.vaults.find((workspace) => workspace.id === vaultId);
}

export function getDiscoverWorkspace(discoverId: string) {
  return library.discoverWorkspaces.find((workspace) => workspace.id === discoverId) ?? library.discoverWorkspaces[0];
}

export function createDiscoverWorkspace() {
  const workspace = makeDiscoverWorkspace();
  library.discoverWorkspaces = [...library.discoverWorkspaces, workspace];
  return workspace;
}

export function setDiscoverStatus(discoverId: string, status: DiscoverWorkspace["status"], error = "") {
  const workspace = getDiscoverWorkspace(discoverId);
  workspace.status = status;
  workspace.error = error;

  if (status === "running") {
    workspace.candidates = [];
    workspace.selectedCandidateId = undefined;
    workspace.lastRun = undefined;
  }
}

export function setDiscoverSelectedCandidate(discoverId: string, candidateId: string) {
  const workspace = getDiscoverWorkspace(discoverId);
  workspace.selectedCandidateId = candidateId;
}

export function applyDiscoverSearchResponse(discoverId: string, response: DiscoverySearchResponse) {
  const workspace = getDiscoverWorkspace(discoverId);
  workspace.status = "completed";
  workspace.error = "";
  workspace.lastRun = {
    provider: response.provider,
    query: response.query,
    filters: [...response.filters],
    resultCount: response.resultCount,
  };
  workspace.candidates = response.candidates.map(toDiscoverCandidate);
  workspace.selectedCandidateId = workspace.candidates[0]?.id;
  markDiscoverOwnership();
}

export function discoverTitleFromQuery(query: string) {
  const normalized = query.trim().replace(/\s+/g, " ");
  if (!normalized) {
    return "Discover: Untitled";
  }

  return `Discover: ${normalized.length > 28 ? `${normalized.slice(0, 28)}...` : normalized}`;
}

function candidateSignalSummary(candidate: DiscoverySearchResponse["candidates"][number]) {
  const signals = ["OpenAlex"];
  const citationCount = candidate.citationCount ?? 0;

  if (citationCount > 0) {
    signals.push(`${citationCount.toLocaleString()} citations`);
  }

  if (candidate.openAccess?.isOpenAccess) {
    signals.push("Open access");
  }

  if (candidate.pdfUrl) {
    signals.push("PDF available");
  }

  return signals.join(" · ");
}

function toDiscoverCandidate(candidate: DiscoverySearchResponse["candidates"][number]): DiscoverCandidate {
  const score = candidate.matchSummary.score ?? 0;
  const openAccessTags = candidate.openAccess?.isOpenAccess ? ["open-access"] : [];

  return {
    id: candidate.id,
    sourceProvider: candidate.sourceProvider,
    sourceId: candidate.sourceId,
    title: candidate.title,
    authors: [...candidate.authors],
    venue: candidate.venue ?? "OpenAlex",
    year: candidate.year ?? 0,
    citations: candidate.citationCount ?? 0,
    score,
    why: candidateSignalSummary(candidate),
    tags: ["openalex", ...openAccessTags],
    abstract: candidate.abstract,
    publicationDate: candidate.publicationDate,
    doi: candidate.doi,
    openalexId: candidate.openalexId,
    arxivId: candidate.arxivId,
    externalUrl: candidate.externalUrl,
    pdfUrl: candidate.pdfUrl,
    openAccess: candidate.openAccess,
    match: {
      score: candidate.matchSummary.score,
      reasons: [...candidate.matchSummary.reasons],
      matchedKeywords: [...candidate.matchSummary.matchedKeywords],
      fromSeedPaperIds: [...candidate.matchSummary.fromSeedPaperIds],
    },
    owned: candidate.alreadyInLibrary,
    alreadyInLibrary: candidate.alreadyInLibrary,
  };
}

export function getPaperById(paperId: string): Paper | undefined {
  for (const workspace of library.vaults) {
    const paper = workspace.papers.find((item) => item.id === paperId);
    if (paper) {
      return paper;
    }
  }

  return candidateToPaper(paperId);
}

export function getPaperTitle(paperId: string): string {
  return getPaperById(paperId)?.title ?? "Unknown paper";
}

export function isCandidateInVault(candidateId: string) {
  return library.vaults.some((workspace) => workspace.papers.some((paper) => paper.id === candidateId));
}

export function isPaperInLibrary(paperId: string) {
  return library.vaults.some((workspace) => workspace.papers.some((paper) => paper.id === paperId));
}

export function getCandidateVaultTargets(candidateId: string) {
  return library.vaults.filter((workspace) => workspace.papers.some((paper) => paper.id === candidateId));
}

export function incrementPaperNoteCount(paperId: string) {
  for (const workspace of library.vaults) {
    const paper = workspace.papers.find((item) => item.id === paperId);
    if (paper) {
      paper.noteCount += 1;
    }
  }
}

export function decrementPaperNoteCount(paperId: string) {
  for (const workspace of library.vaults) {
    const paper = workspace.papers.find((item) => item.id === paperId);
    if (paper) {
      paper.noteCount = Math.max(paper.noteCount - 1, 0);
    }
  }
}

export function paperFromDiscoverCandidate(candidateId: string) {
  return candidateToPaper(candidateId);
}

export function getDiscoverCandidate(candidateId: string) {
  return findCandidate(candidateId);
}

export function paperDraftFromDiscoverCandidate(candidateId: string): PaperDraft {
  const candidate = findCandidate(candidateId);
  const paper = candidateToPaper(candidateId);

  return {
    id: paper?.id ?? candidateId,
    title: paper?.title ?? "Unknown",
    authors: [...(paper?.authors ?? [])],
    venue: paper?.venue ?? "",
    year: paper?.year ?? 0,
    citations: paper?.citations ?? 0,
    tags: [...(paper?.tags ?? [])],
    status: paper?.status ?? "UNREAD",
    abstract: paper?.abstract,
    sources: candidate?.pdfUrl
      ? [
          {
            sourceKind: "pdf",
            sourceUrl: candidate.pdfUrl,
          },
        ]
      : [],
  };
}

export function hydrateLibrary(snapshot: LibrarySnapshot) {
  library.vaults = snapshot.vaults.map((vault) => makeVaultWorkspace(snapshot, vault.id));
  markDiscoverOwnership();
}
