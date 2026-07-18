import { invoke } from "@tauri-apps/api/core";
import type {
  LibrarySnapshot,
  DocumentSource,
  LocalPdfImport,
  LocalPdfImportResult,
  MetadataCandidate,
  PaperMetadataUpdate,
  PaperDraft,
  VaultDraft,
  VaultRenameDraft,
} from "$lib/domain/library";
import type { DiscoveryReaderCandidate, ReaderDocument } from "$lib/domain/reader";

export async function getLibrary(): Promise<LibrarySnapshot> {
  return invoke<LibrarySnapshot>("get_library");
}

export async function addPaperToVaults(paper: PaperDraft, vaultIds: string[]): Promise<LibrarySnapshot> {
  return invoke<LibrarySnapshot>("add_paper_to_vaults", {
    paper,
    vaultIds,
  });
}

export async function importLocalPdfs(vaultId: string, paths: string[]): Promise<LocalPdfImportResult> {
  const files: LocalPdfImport[] = paths.map((path) => ({ path }));

  return invoke<LocalPdfImportResult>("import_local_pdfs", {
    vaultId,
    files,
  });
}

export async function autofillPaperMetadata(paperId: string): Promise<void> {
  return invoke<void>("autofill_paper_metadata", {
    paperId,
  });
}

export async function applyPaperMetadataCandidate(
  paperId: string,
  candidate: MetadataCandidate,
): Promise<LibrarySnapshot> {
  return invoke<LibrarySnapshot>("apply_paper_metadata_candidate", {
    paperId,
    candidate,
  });
}

export async function updatePaperMetadata(
  paperId: string,
  update: PaperMetadataUpdate,
): Promise<LibrarySnapshot> {
  return invoke<LibrarySnapshot>("update_paper_metadata", {
    paperId,
    update,
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

export async function getReaderDocument(paperId: string, extractionId?: string): Promise<ReaderDocument> {
  return invoke<ReaderDocument>("get_reader_document", {
    paperId,
    extractionId,
  });
}

export async function getDiscoveryReaderDocument(candidate: DiscoveryReaderCandidate): Promise<ReaderDocument> {
  return invoke<ReaderDocument>("get_discovery_reader_document", {
    candidate,
  });
}

export async function getReaderPdfBytes(sourceId: string): Promise<number[]> {
  return invoke<number[]>("get_reader_pdf_bytes", {
    sourceId,
  });
}

export async function extractPaperDocument(paperId: string, sourceId?: string, force = false): Promise<void> {
  return invoke<void>("extract_paper_document", {
    paperId,
    sourceId,
    force,
  });
}
