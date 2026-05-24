export type VaultStatus = {
  paperCount: number;
  unreadCount: number;
  syncState: string;
};

export type VaultTreeSection = {
  kind: "section";
  label: string;
};

export type VaultTreeItem = {
  kind: "item";
  id: string;
  label: string;
  count?: number;
  key?: string;
  muted?: boolean;
};

export type VaultTreeFolder = {
  kind: "folder";
  id: string;
  label: string;
  count: number;
  open?: boolean;
  active?: boolean;
  muted?: boolean;
  children?: Array<Omit<VaultTreeItem, "kind"> & { active?: boolean }>;
};

export type VaultTreeNode = VaultTreeSection | VaultTreeItem | VaultTreeFolder;
