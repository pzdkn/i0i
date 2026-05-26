import type { Paper } from "$lib/domain/paper";
import type { DiscoverWorkspace } from "$lib/domain/discover";
import type { LibrarySnapshot, PaperDraft, VaultWorkspace } from "$lib/domain/library";
import { discoverWorkspaces as discoverSeeds } from "$lib/mock/discover";
import { librarySeedWorkspaces } from "$lib/mock/library-seed";

type LibraryState = {
  vaults: VaultWorkspace[];
  discoverWorkspaces: DiscoverWorkspace[];
};

// Saved library data comes from Rust/SQLite. This module caches the latest
// LibrarySnapshot for Svelte views and uses seed data only before hydration.
const initialVaults = librarySeedWorkspaces.map(cloneVaultWorkspace);
const initialPaperIds = new Set(initialVaults.flatMap((workspace) => workspace.papers.map((paper) => paper.id)));
const initialDiscoverWorkspaces = discoverSeeds.map(cloneDiscoverWorkspace);

for (const workspace of initialDiscoverWorkspaces) {
  for (const candidate of workspace.candidates) {
    candidate.owned ||= initialPaperIds.has(candidate.id);
  }
}

const library = $state<LibraryState>({
  vaults: initialVaults,
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
    candidates: workspace.candidates.map((candidate) => ({
      ...candidate,
      authors: [...candidate.authors],
      tags: [...candidate.tags],
    })),
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
      candidate.owned = paperIds.has(candidate.id);
    }
  }
}

function candidateToPaper(candidateId: string): Paper {
  const candidate = findCandidate(candidateId);
  if (!candidate) {
    return library.vaults[0].papers[0];
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
    abstract: candidate.why,
  };
}

export function getVaultWorkspaces() {
  return library.vaults;
}

export function getVaultWorkspace(vaultId: string) {
  return library.vaults.find((workspace) => workspace.id === vaultId) ?? library.vaults[0];
}

export function getDiscoverWorkspace(discoverId: string) {
  return library.discoverWorkspaces.find((workspace) => workspace.id === discoverId) ?? library.discoverWorkspaces[0];
}

export function getPaperById(paperId: string) {
  for (const workspace of library.vaults) {
    const paper = workspace.papers.find((item) => item.id === paperId);
    if (paper) {
      return paper;
    }
  }

  return candidateToPaper(paperId);
}

export function getPaperTitle(paperId: string) {
  return getPaperById(paperId).title;
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

export function paperFromDiscoverCandidate(candidateId: string) {
  return candidateToPaper(candidateId);
}

export function paperDraftFromDiscoverCandidate(candidateId: string): PaperDraft {
  const paper = candidateToPaper(candidateId);

  return {
    id: paper.id,
    title: paper.title,
    authors: [...paper.authors],
    venue: paper.venue,
    year: paper.year,
    citations: paper.citations,
    tags: [...paper.tags],
    status: paper.status,
    abstract: paper.abstract,
  };
}

export function hydrateLibrary(snapshot: LibrarySnapshot) {
  library.vaults = snapshot.vaults.map((vault) => makeVaultWorkspace(snapshot, vault.id));
  markDiscoverOwnership();
}
