import type { DiscoveryReaderCandidate } from "$lib/domain/reader";

export type WorkspaceKind = "vault" | "reader" | "discover";

export type WorkspaceTab = {
  id: string;
  kind: WorkspaceKind;
  title: string;
  projectId?: string;
  vaultId?: string;
  paperId?: string;
  readerCandidate?: DiscoveryReaderCandidate;
  discoverId?: string;
};
