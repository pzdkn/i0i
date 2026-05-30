# RFC 0024: PDF Ingestion and Cache

Status: Draft
Date: 2026-05-30
Product: i0i
Target: Tauri v2 + Svelte, macOS first

## Summary

Define how saved papers get a local PDF attached.

Discover may provide metadata, an abstract, an external URL, and a PDF URL. The Reader becomes useful only once a saved paper has an actual document source. This RFC covers persisting document source metadata and downloading/caching PDFs for saved papers.

This RFC builds on RFC 0023:

```text
Paper 1 -> many DocumentSources
DocumentSource 1 -> many DocumentExtractions
```

RFC 0024 only covers the `DocumentSource` PDF lifecycle. Extraction and structured rendering remain RFC 0025.

## Product Behavior

When a user saves a Discover result to a Vault:

```text
save paper metadata
save remote PDF source, if present
start PDF download after save, if present
cache PDF under app data
mark document status
```

The app must not download PDFs for every transient Discover result. Downloading should be tied to an ingestion decision: save/import/add to Vault.

Saving should not block on the PDF download:

```text
Add to Vault
  -> save paper metadata
  -> save DocumentSource(status = remote)
  -> return UI quickly
  -> start background PDF download
  -> update DocumentSource(status = downloading)
  -> update to cached or failed
```

If download fails, the saved paper remains in the Vault and the Reader can expose retry/open-source actions.

If no PDF URL exists, the paper can still be saved as metadata, but the Reader should show `missing_document` with actions. It should not render abstract text as if it were the paper.

## Status Model

These are internal `DocumentSource.status` values, not UI labels.

```text
remote
  -> known PDF URL, no local file yet

downloading
  -> active download

cached
  -> local PDF exists

failed
  -> download failed; error stored
```

Extraction has a separate `DocumentExtraction.status` lifecycle in RFC 0025. Do not store extraction states on the PDF source row.

## Ingestion Payload

`PaperDraft` currently only carries paper metadata. RFC 0024 should extend the add-to-vault payload so Discover can pass document source metadata without turning transient candidates into saved rows.

Conceptual shape:

```ts
type PaperSourceDraft = {
  sourceKind: "pdf";
  sourceUrl: string;
};

type PaperDraft = {
  ...
  sources?: PaperSourceDraft[];
};
```

When saving a Discover candidate with `pdfUrl`, the frontend should submit:

```text
paper metadata
sources: [{ sourceKind: "pdf", sourceUrl: candidate.pdfUrl }]
```

If the candidate has no `pdfUrl`, submit no document source.

## Storage

Suggested local path:

```text
~/Library/Application Support/i0i/documents/{paper_id}/sources/{source_id}/source.pdf
```

Source-scoped paths prevent replacement PDFs or alternate sources from overwriting each other.

Future extraction output can live beside the source tree:

```text
~/Library/Application Support/i0i/documents/{paper_id}/extractions/{extraction_id}/raw/{extractor}.json
~/Library/Application Support/i0i/documents/{paper_id}/extractions/{extraction_id}/assets/{asset_id}.png
```

Document records:

```text
pdf:{paper_id}:{source_hash}
  id
  paper_id
  source_kind: pdf
  source_url: candidate.pdfUrl
  local_path: .../documents/{paper_id}/sources/{source_id}/source.pdf
  status: cached | failed | remote | downloading
  error
```

Generate stable source ids:

```text
remote PDF source: pdf:{paper_id}:{sha256(source_url)[0..12]}
future local import: pdf:{paper_id}:{sha256(file_bytes)[0..12]}
```

This lets repeated saves of the same candidate reuse the same source row while allowing alternate PDF URLs or replacement files for the same paper.

Remote source metadata and paper metadata should be saved in one SQLite transaction. PDF download should happen after that transaction because it is network work and can fail independently.

On first successful cache for a paper, set:

```text
papers.active_source_id = document_sources.id
```

Do not set `papers.active_extraction_id` in this RFC. Extraction belongs to RFC 0025.

## Commands

```rust
add_paper_to_vaults(paper, vault_ids) -> LibrarySnapshot
download_paper_pdf(paper_id, source_id?) -> DocumentSource
get_document_sources(paper_id) -> Vec<DocumentSource>
```

`add_paper_to_vaults` should persist any known PDF source URL from Discover as a `DocumentSource` row and kick off a background PDF download. The durable DB save must not depend on download success.

If `source_id` is omitted from `download_paper_pdf`, download the active or first remote PDF source for the paper.

## Download Validation

Do not trust file extensions or provider metadata alone. A PDF download only becomes `cached` after byte validation.

Minimum validation:

- HTTP status is 2xx.
- Follow redirects.
- Enforce a maximum file size, initially `100 MB`.
- Prefer `Content-Type: application/pdf`, but do not rely on it alone.
- Verify the downloaded bytes start with `%PDF-`.
- Write to a temporary file first.
- Atomically rename the temporary file to `source.pdf` after validation.

Flow:

```text
download to source.pdf.part
validate status/content/bytes/size
rename source.pdf.part -> source.pdf
status = cached
```

If validation fails:

```text
status = failed
error = "Downloaded file was not a PDF"
```

## Events

PDF downloads are asynchronous, so the backend should emit Tauri events whenever a source changes:

```text
document_source_updated
```

Payload:

```ts
type DocumentSourceUpdated = {
  paperId: string;
  sourceId: string;
  status: "remote" | "downloading" | "cached" | "failed";
  bytesDownloaded?: number;
  contentLength?: number;
  localPath?: string;
  error?: string;
};
```

SQLite should store only the coarse durable status. Live progress belongs in events, not in the database.

```text
DB: remote | downloading | cached | failed
event: bytesDownloaded / contentLength while downloading
```

Backend flow:

```text
download starts
  -> update DB: downloading
  -> emit document_source_updated

download finishes
  -> update DB: cached or failed
  -> emit document_source_updated
```

Frontend behavior:

- Listen for `document_source_updated`.
- Update the matching `DocumentSource` in local state or refresh the library snapshot.
- Re-render Vault/Discover/Reader document status without polling.

RFC 0025 should reuse this event style for extraction updates.

## Deletion

Deleting a paper globally should remove or schedule cleanup for:

- cached PDF
- extracted assets
- raw extractor output
- normalized source/extraction rows

Removing a paper from one Vault should not delete the PDF if the paper remains in another Vault.

## Validation Plan

- Save a Discover candidate with a PDF URL.
- Verify a `document_sources` PDF row exists.
- Verify the source row starts as `remote` or `downloading`.
- Verify the PDF downloads to app data.
- Verify the source row moves to `cached` and gets `local_path`.
- Verify `papers.active_source_id` is set on first successful cache.
- Verify the cached PDF can be opened by the Reader PDF mode or a minimal local-file preview.
- Verify failed downloads store errors without rolling back the saved paper.
- Verify global paper deletion cleans document files or leaves a cleanup task.
