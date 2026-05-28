import { invoke } from "@tauri-apps/api/core";
import type {
  LibrarySnapshot,
  PaperDraft,
  PaperNote,
  PaperNoteDraft,
  VaultDraft,
  VaultRenameDraft,
} from "$lib/domain/library";

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

export async function removeVault(vaultId: string): Promise<LibrarySnapshot> {
  return invoke<LibrarySnapshot>("delete_vault", {
    vaultId,
  });
}

export async function removePaperFromVault(vaultId: string, paperId: string): Promise<LibrarySnapshot> {
  return invoke<LibrarySnapshot>("remove_paper_from_vault", {
    vaultId,
    paperId,
  });
}

export async function removePaperFromLibrary(paperId: string): Promise<LibrarySnapshot> {
  return invoke<LibrarySnapshot>("delete_paper_globally", {
    paperId,
  });
}

export async function getPaperNotes(paperId: string): Promise<PaperNote[]> {
  return invoke<PaperNote[]>("get_paper_notes", {
    paperId,
  });
}

export async function createPaperNote(draft: PaperNoteDraft): Promise<PaperNote[]> {
  return invoke<PaperNote[]>("create_paper_note", {
    draft,
  });
}

export async function deletePaperNote(input: { paperId: string; noteId: string }): Promise<PaperNote[]> {
  return invoke<PaperNote[]>("delete_paper_note", input);
}
