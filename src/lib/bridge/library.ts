import { invoke } from "@tauri-apps/api/core";
import type {
  LibrarySnapshot,
  DocumentSource,
  PaperDraft,
  PaperNote,
  PaperNoteDraft,
  VaultDraft,
  VaultRenameDraft,
} from "$lib/domain/library";
import type { ReaderDocument } from "$lib/domain/reader";

export async function getLibrary(): Promise<LibrarySnapshot> {
  return invoke<LibrarySnapshot>("get_library");
}

export async function addPaperToVaults(paper: PaperDraft, vaultIds: string[]): Promise<LibrarySnapshot> {
  return invoke<LibrarySnapshot>("add_paper_to_vaults", {
    paper,
    vaultIds,
  });
}

export async function getDocumentSources(paperId: string): Promise<DocumentSource[]> {
  return invoke<DocumentSource[]>("get_document_sources", {
    paperId,
  });
}

export async function downloadPaperPdf(paperId: string, sourceId?: string): Promise<DocumentSource> {
  return invoke<DocumentSource>("download_paper_pdf", {
    paperId,
    sourceId,
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

export async function updatePaperNote(input: { paperId: string; noteId: string; body: string }): Promise<PaperNote[]> {
  return invoke<PaperNote[]>("update_paper_note", input);
}

export async function getReaderDocument(paperId: string, extractionId?: string): Promise<ReaderDocument> {
  return invoke<ReaderDocument>("get_reader_document", {
    paperId,
    extractionId,
  });
}
