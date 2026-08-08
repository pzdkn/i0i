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

/// Add a web page to a vault by URL (RFC 0065): fetch + sanitize + save it as a
/// permanent, annotatable paper. Returns the refreshed library snapshot.
export async function addHtmlUrlToVault(vaultId: string, url: string): Promise<LocalPdfImportResult> {
  return invoke<LocalPdfImportResult>("import_html_url", {
    vaultId,
    url,
  });
}

/// Export a `.bib` for every paper in a vault to `path` (RFC 0070). Returns the
/// number of entries written. The caller chooses `path` via a Save dialog.
export async function exportVaultBibtex(vaultId: string, path: string): Promise<number> {
  return invoke<number>("export_vault_bibtex", {
    vaultId,
    destPath: path,
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

export async function getDiscoveryReaderDocument(
  candidate: DiscoveryReaderCandidate,
  force = false,
): Promise<ReaderDocument> {
  return invoke<ReaderDocument>("get_discovery_reader_document", {
    candidate,
    force,
  });
}

export async function cancelDiscoveryPdfAcquisition(sourceId: string, paperId: string): Promise<void> {
  return invoke<void>("cancel_discovery_pdf_acquisition", {
    sourceId,
    paperId,
  });
}

export type PdfAvailability = "verified" | "browser_required" | "unavailable";

export async function probeDiscoveryCandidatePdf(
  candidate: DiscoveryReaderCandidate,
): Promise<PdfAvailability> {
  return invoke<PdfAvailability>("probe_discovery_candidate_pdf", {
    candidate,
  });
}

/// Raw PDF bytes as an `ArrayBuffer`. The command returns `tauri::ipc::Response`
/// so the bytes cross the IPC as `application/octet-stream` rather than a JSON
/// array of integers — see the comment on `get_reader_pdf_bytes` in
/// `commands/reader.rs` for the measurements behind that.
export async function getReaderPdfBytes(sourceId: string): Promise<ArrayBuffer> {
  return invoke<ArrayBuffer>("get_reader_pdf_bytes", {
    sourceId,
  });
}

/// Open an arbitrary URL as an HTML reader document (RFC 0056).
export async function openHtmlDocument(url: string): Promise<ReaderDocument> {
  return invoke<ReaderDocument>("open_html_document", { url });
}

/// Serve the sanitized HTML for a cached HTML source (RFC 0056).
export async function getReaderHtml(sourceId: string): Promise<string> {
  return invoke<string>("get_reader_html", { sourceId });
}

export async function extractPaperDocument(paperId: string, sourceId?: string, force = false): Promise<void> {
  return invoke<void>("extract_paper_document", {
    paperId,
    sourceId,
    force,
  });
}
