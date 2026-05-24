import { invoke } from "@tauri-apps/api/core";
import type { LibrarySnapshot, PaperDraft } from "$lib/domain/library";

export async function getLibrary(): Promise<LibrarySnapshot> {
  return invoke<LibrarySnapshot>("get_library");
}

export async function addPaperToVaults(paper: PaperDraft, vaultIds: string[]): Promise<LibrarySnapshot> {
  return invoke<LibrarySnapshot>("add_paper_to_vaults", {
    paper,
    vaultIds,
  });
}
