import type { Paper } from "$lib/domain/paper";
import type { DiscoverWorkspace } from "$lib/domain/discover";
import { discoverWorkspaces as discoverSeeds } from "$lib/mock/discover";
import { vaultWorkspaces as vaultSeeds, type VaultWorkspace } from "$lib/mock/vault-workspaces";

type LibraryState = {
  vaults: VaultWorkspace[];
  discoverWorkspaces: DiscoverWorkspace[];
};

const fallbackVaultId = "self-supervised";

const initialVaults = vaultSeeds.map(cloneVaultWorkspace);
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

function updateVaultPaperCount(workspace: VaultWorkspace) {
  const paperTab = workspace.tabs.find((tab) => tab.label === "Papers");
  if (paperTab?.count !== undefined) {
    paperTab.count += 1;
  }

  workspace.summary = workspace.summary.replace(/^\d+ papers/, `${paperTab?.count ?? workspace.papers.length} papers`);
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

export function getCandidateVaultTargets(candidateId: string) {
  return library.vaults.filter((workspace) => workspace.papers.some((paper) => paper.id === candidateId));
}

export function addCandidateToVault(candidateId: string, vaultId = fallbackVaultId) {
  return addCandidateToVaults(candidateId, [vaultId]);
}

export function addCandidateToVaults(candidateId: string, vaultIds: string[]) {
  const candidate = findCandidate(candidateId);
  const paper = candidateToPaper(candidateId);
  const targetIds = vaultIds.length ? vaultIds : [fallbackVaultId];

  for (const vaultId of targetIds) {
    const targetVault =
      library.vaults.find((workspace) => workspace.id === vaultId) ?? getVaultWorkspace(fallbackVaultId);

    if (!targetVault.papers.some((existingPaper) => existingPaper.id === candidateId)) {
      targetVault.papers = [clonePaper(paper), ...targetVault.papers];
      updateVaultPaperCount(targetVault);
    }
  }

  if (candidate) {
    candidate.owned = true;
  }

  return paper;
}

export function paperFromDiscoverCandidate(candidateId: string) {
  return candidateToPaper(candidateId);
}
