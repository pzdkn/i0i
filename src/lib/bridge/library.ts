import { invoke } from "@tauri-apps/api/core";
import type { LibrarySnapshot, PaperDraft, VaultDraft, VaultRenameDraft } from "$lib/domain/library";

export async function getLibrary(): Promise<LibrarySnapshot> {
  return invoke<LibrarySnapshot>("get_library");
}

export async function addPaperToVaults(paper: PaperDraft, vaultIds: string[]): Promise<LibrarySnapshot> {
  return invoke<LibrarySnapshot>("add_paper_to_vaults", {
    paper,
    vaultIds,
  });
}

export async function createVault(path: string): Promise<LibrarySnapshot> {
  const draft: VaultDraft = { path };

  return invoke<LibrarySnapshot>("create_vault", {
    draft,
  });
}

export async function renameVault(id: string, path: string): Promise<LibrarySnapshot> {
  const draft: VaultRenameDraft = { id, path };

  return invoke<LibrarySnapshot>("rename_vault", {
    draft,
  });
}
