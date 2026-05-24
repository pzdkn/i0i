export type WorkspaceKind = "vault" | "reader" | "discover";

export type WorkspaceTab = {
  id: string;
  kind: WorkspaceKind;
  title: string;
  vaultId?: string;
  paperId?: string;
  discoverId?: string;
};
