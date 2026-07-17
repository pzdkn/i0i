# RFC 0046: Local PDF Import With Metadata Autofill

Status: Partially Implemented; Metadata Autofill Deferred
Date: 2026-07-17
Product: i0i
Target: Tauri v2 + Svelte, macOS first
Builds on: RFC 0024 (PDF Ingestion And Cache), RFC 0026 (PDF-First Reader Notes), RFC 0035 (Background Text Extraction)

## Summary

Add a Vault-level flow for importing local PDFs from the user's computer.

The decision is:

- The user imports PDFs from the Vault view.
- i0i copies each selected PDF into app-managed document storage immediately.
- i0i creates a paper draft linked to that local PDF.
- Metadata autofill is deferred; imported PDFs currently keep fallback metadata.
- The user can confirm/edit important metadata before or after saving.
- The reader should be able to open the imported PDF immediately.

Plain English version: "Import PDF" should get the document into the vault now.
Metadata repair needs a stronger follow-up design before it can be trusted.

## Problem

Today, papers mostly enter i0i through Discover/provider results. But users
already have PDFs on their machine:

- downloaded papers,
- PDFs from email/slack/browser,
- old reading folders,
- conference proceedings,
- scanned or renamed files.

The app needs a direct local import path. Without it, i0i is not a real vault.

## Goals

- Add an `Import PDF` action in the Vault view.
- Use a native Tauri file picker for local PDF selection.
- Support selecting one or multiple PDFs.
- Copy imported PDFs into the same app-controlled document storage model used
  by downloaded PDFs.
- Create paper records and attach them to the active vault.
- Let the user open imported PDFs in the reader immediately after import.
- Defer metadata autofill until structured evidence extraction is reliable.
- Let the user edit/confirm metadata instead of trusting guesses blindly.

## Non-Goals

- No bulk folder watcher in this RFC.
- No OCR for scanned PDFs.
- No perfect citation parser.
- No manual BibTeX/RIS import yet.
- No cloud sync.
- No requirement that metadata autofill finishes before the PDF is saved.

## Product Flow

```text
Vault view
  -> Import PDF
  -> native file picker
  -> user selects one or more PDFs
  -> backend copies PDFs into app storage
  -> backend creates paper drafts / records
  -> paper appears in vault
  -> user can review fallback metadata
  -> user opens PDF when ready
```

## UX Placement

The action belongs in the Vault view header near existing actions:

```text
[ + Add ] [ Import PDF ] [ Export .bib ]
```

For v1, `Import PDF` is enough. The existing `Import` label should become more
specific if it means PDF import.

## Metadata Autofill Strategy

Autofill should be evidence-first:

```text
1. PDF embedded metadata
2. DOI / arXiv id detected from first pages
3. OpenAlex / arXiv lookup
4. LLM cleanup or best-effort guess
5. user confirm/edit
```

The LLM is not the source of truth. It proposes cleaned metadata from evidence.

Suggested fields:

```text
title
authors
year
venue
doi
arxiv_id
abstract
```

## Import States

Imported papers should be usable before metadata is perfect.

```text
copied
  -> PDF is safely stored locally

created
  -> paper exists in the vault with fallback metadata

enriching
  -> metadata lookup / extraction is running

ready
  -> metadata autofill finished

needs_review
  -> metadata is uncertain or incomplete

failed_enrichment
  -> PDF is saved, metadata enrichment failed
```

The important invariant:

```text
PDF import success is independent from metadata enrichment success.
```

## Fallback Metadata

If autofill finds nothing reliable:

```text
title: filename without extension
authors: []
year: none
venue: none
abstract: none
status: needs_review
```

This keeps the document usable.

## Backend Design

Add a Tauri command:

```rust
import_local_pdfs(
    vault_id: String,
    files: Vec<LocalPdfImport>
) -> Result<LocalPdfImportResult, String>
```

The command should:

1. validate files exist and are PDFs,
2. copy each PDF into app data document storage,
3. create a `Paper` record,
4. create a local `DocumentSource`,
5. set the local source as active,
6. attach the paper to the target vault,
7. enqueue metadata enrichment,
8. return the updated `LibrarySnapshot`.

Suggested storage path:

```text
~/Library/Application Support/com.i0i.app/documents/{paper_id}/sources/local_pdf_{hash}/source.pdf
```

The source metadata should distinguish local imports:

```text
source_provider: local
source_kind: pdf
acquisition_method: local_import
original_path: optional, if we decide to store it
```

Do not rely on the original path after import. The app-managed copy is the
canonical source.

## Metadata Enrichment Design

Add a background enrichment worker or reuse an existing background-job shape.

RFC 0047 explored changing the trigger from automatic import-time enrichment to
an explicit paper context-menu action, but it is deferred because the enrichment
worker often has no usable metadata evidence.

Potential extraction inputs:

- PDF metadata fields,
- first-page text from existing Pdfium text extraction,
- filename,
- DOI/arXiv regex matches.

Lookup order:

```text
doi -> OpenAlex
arxiv_id -> arXiv
title -> OpenAlex search
LLM cleanup -> only after structured attempts
```

The enrichment result should update the paper record, but it should not replace
user-edited metadata without a later conflict policy.

## Frontend Design

Vault view:

```text
[ Import PDF ]
```

Click behavior:

1. Open native file picker.
2. Allow multiple `.pdf` files.
3. Call `importLocalPdfs(activeVaultId, paths)`.
4. Hydrate library from returned snapshot.
5. If one PDF was imported, open it in the reader.
6. If multiple PDFs were imported, keep the vault view and show imported rows.

Metadata review v1 can be lightweight:

```text
row badge: needs review / enriching / ready
inspector shows editable metadata later
```

Do not block v1 on a polished modal.

## Tauri / Rust Notes

The native file picker should happen on the frontend with Tauri's dialog API,
then selected paths are sent to Rust.

Why:

- Svelte owns user interaction.
- Rust owns filesystem validation, copying, DB writes, and background work.
- The bridge stays explicit and teachable:

```text
Svelte dialog -> invoke(import_local_pdfs) -> Rust copies/saves -> Svelte hydrates snapshot
```

## Risks

- PDFs can be huge.
- Duplicate imports can create duplicate papers.
- PDF metadata is often bad.
- LLM guesses can be wrong.
- Imported filenames may contain private information.
- Background enrichment may finish after the user edits metadata.

Mitigations:

- hash files and detect duplicate local sources,
- save first, enrich later,
- mark uncertain metadata as `needs_review`,
- use provider evidence before LLM guesses,
- do not overwrite user edits in v1.

## Validation

Backend tests:

```text
import rejects non-PDF paths
import copies PDF into app document storage
import creates paper and vault membership
import creates active local document source
duplicate PDF import is handled predictably
metadata fallback uses filename
enrichment failure does not remove imported paper
```

Frontend checks:

```text
Import PDF button opens native file picker
selected PDFs call backend command
returned snapshot updates vault
single imported PDF opens in reader
multi-import leaves rows visible in vault
```

Commands:

```bash
cargo test
cargo check
pnpm check
git diff --check
```

Manual smoke:

```text
1. Open a vault.
2. Click Import PDF.
3. Select one local PDF.
4. Confirm it appears in the vault.
5. Confirm reader opens the local PDF.
6. Confirm metadata is at least filename fallback.
7. Import multiple PDFs and confirm all are added.
```

## Implementation Order

1. Add frontend file picker and bridge function.
2. Add Rust command for local PDF import without enrichment.
3. Copy PDFs into app document storage and create active document source.
4. Create paper and vault membership with filename fallback metadata.
5. Open single imported PDF in the reader.
6. Add duplicate/hash handling.
7. Add metadata enrichment worker.
8. Add metadata review UI.

## Implementation Notes

Implemented in the first slice:

- native file picker via `@tauri-apps/plugin-dialog`,
- Vault header `Import PDF` action,
- multi-select PDF import,
- Rust-side file validation and content hashing,
- app-managed PDF copy under `documents/{paper_id}/sources/{source_id}/source.pdf`,
- local cached `document_sources` row with `acquisition_method = local_import`,
- active source assignment so the reader opens the imported PDF immediately,
- fallback metadata from filename with `local` and `needs-review` tags.
- experimental manual metadata enrichment trigger from the paper row context menu,
- partial PDF embedded metadata / first-page text evidence extraction,
- partial DOI, arXiv id, and title-based provider lookup,
- `paper_metadata_updated` event that refreshes the frontend snapshot.
- import no longer opens a Reader tab automatically,
- removal of the inert Vault `+ Add` button; local import is now the only
  implemented Vault-level ingest action in this slice.

Not implemented yet:

- reliable metadata autofill,
- LLM metadata cleanup,
- metadata review/editor UI.

## Future Work

- Folder import.
- Drag-and-drop PDF import.
- BibTeX/RIS import and matching.
- OCR for scanned PDFs.
- Better metadata review modal.
- Conflict handling when enrichment differs from user edits.
