export type WorkspaceKind = "vault" | "reader";

export type WorkspaceTab = {
  id: string;
  kind: WorkspaceKind;
  title: string;
  vaultId?: string;
  paperId?: string;
};
