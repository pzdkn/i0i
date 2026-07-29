# RFC 0065: Add a web page to a vault by URL

Status: Implemented
Date: 2026-07-29
Product: i0i
Target: Tauri v2 + SvelteKit (Svelte 5), macOS first
Builds on: RFC 0056 (in-app HTML reader — sanitize + render); RFC 0052 / 0042
(HTML page acquisition + web-reader fallback); the local-PDF import flow
(`import_local_pdfs`).

## Summary

Let a user **paste a URL and save that web page into a vault as a permanent
paper** they can read, highlight, note, ask, and search — the same lifecycle a
local PDF already has. The page is captured as a **frozen sanitized snapshot** at
save time, so annotations anchored to its text stay valid.

Today HTML is only ever *transient*: `open_html_document(url)` fetches, sanitizes
(RFC 0056), and caches the result under a `temp-html` **discovery** source id. It
is never a real vault paper — reopening depends on the discovery cache, the page
isn't in the library, and durable annotation/chat isn't wired. This RFC promotes
the URL → HTML path into a first-class vault import.

## Problem

The library supports two acquisition paths that both end in a persistent vault
paper: adding a discovery candidate, and **importing a local PDF**
(`import_local_pdfs`: copy the file into vault-owned storage, mint a
`local:<hash>` paper + `pdf:` source, add to the vault). There is **no equivalent
for a web page**. A user who just wants to read-and-annotate an article they have
the link to has to either find a PDF or open it in the transient reader, where
nothing persists.

## Model

Reuse the existing document model — a `Paper` with one or more `DocumentSource`s
— and introduce an `html:`-kind source backed by a persisted snapshot:

```
paper:  id = web:<sha of normalized url>        (idempotent re-adds; distinct
        title = <IngestedHtml.title | url host>  from the local:<hash> PDF id
                                                  namespace to avoid collisions)
source: kind html:, source_url = <the URL>
        snapshot on disk (vault-owned, permanent — NOT the temp-html cache):
          source.html   ← sanitized clean_html (RFC 0056), rendered verbatim
          meta.json     ← { title, url, source_text }
```

`source_text` (readable plain text) is persisted for chat context and search,
mirroring what the transient reader already produces.

**Why a frozen snapshot (not re-fetch on open):** HTML annotations are anchored
to character offsets in the *rendered* article text (RFC 0056). A re-fetch can
change the markup and shift every offset, silently breaking existing highlights.
The snapshot is captured once, at save, and never re-fetched.

## Interaction

In `VaultHome.svelte`, beside the existing **Add local PDF** button, add an
**Add web page** action:

1. Prompt for a URL (small inline input / dialog).
2. Call `addHtmlUrlToVault(vaultId, url)`; show a loading state (fetch is
   network-bound), reusing the import busy/error affordances already there.
3. On success the paper appears in the vault and can be opened in the reader
   immediately (the reader already renders `contentKind: "html"` — no reader UI
   changes). On failure (paywall, login-required, JS-only page, network) show a
   clear error and add nothing.

## Backend

- **New command** `import_html_url(vault_id, url) -> LocalHtmlImportResult`
  (snapshot + imported paper id — same shape as the PDF import result), modeled
  on `import_local_pdfs`. Steps:
  1. Normalize the URL (trim whitespace + strip the `#fragment` only — no
     query/trailing-slash canonicalization); compute
     `paper_id = web:<sha12 of normalized url>`. Idempotency is handled at the
     snapshot layer: `acquire_and_store_html` reuses an existing on-disk snapshot
     rather than re-fetching (so a re-add never shifts annotation offsets), and
     the store upserts are conflict-safe.
  2. Acquire + sanitize via the existing pipeline (`acquire_html_page` +
     `ingest_html`) — the same code the transient reader uses.
  3. Write `source.html` + `meta.json` to a **permanent** app-data path keyed by
     paper/source id (a new helper analogous to `cached_local_pdf_path`, in a
     non-cache directory).
  4. Build the paper draft (title, `source_url`) and register the `html:` source
     via a store method analogous to `add_local_pdf_to_vault`.
- **Reader read path:** `reader_service.get_reader_html(source_id)` must resolve
  the snapshot for a persisted `html:` vault source, not only discovery
  (`temp-html`) ids. This is the one substantive piece of new wiring; today it
  only knows the discovery path.
- **Metadata:** best-effort autofill reuses the existing enrichment queue after
  import; a failure never blocks the import.

## Frontend

- `addHtmlUrlToVault(vaultId, url)` in `src/lib/bridge/library.ts`, wrapping
  `import_html_url` and returning the snapshot + id (mirrors `importLocalPdfs`).
- `VaultHome.svelte`: an **Add web page** button + URL entry, its own
  `isImporting`-style busy flag, wired through `+page.svelte` the same way
  `onImportPdfs` is. On success, `hydrateLibrary(result.snapshot)`.

## Testing

- **Round-trip:** import a fixture URL (stubbed acquire) → `get_reader_html`
  returns the sanitized `clean_html` byte-for-byte from the snapshot.
- **Dedup:** importing the same URL twice yields one paper and does **not**
  re-fetch (the second call reuses the on-disk snapshot); a `#fragment`
  difference collapses to the same id.
- **Read-path resolution (the key new wiring):** a persisted `html:` source id
  resolves to its snapshot via the stored `local_path` (unit-tested at the store
  layer with a real row + temp file), while a `temp-html:` id still resolves via
  the discovery cache.
- **Failure:** an acquire failure adds no paper and no partial snapshot.
- **Annotation stability:** a highlight created on a saved page still resolves
  after closing and reopening it (offsets stable because the snapshot is frozen).

## Risks

- **Acquisition reliability** — some pages need login/JS and can't be fetched
  cleanly. Mitigation: surface the same clear failure the transient reader
  already produces; the local-`.html`-file path (a non-goal here) is the escape
  hatch we can add later.
- **Snapshot vs. live page drift** — a saved page won't reflect later edits to
  the source. Accepted and intended (annotation stability requires it); a manual
  "re-snapshot" is a possible future add.
- **Read-path branching** — `get_reader_html` now serves two source families
  (persisted vault `html:` and transient `temp-html`). Keep the path resolution
  in one place to avoid divergence.
- **Two annotation namespaces** — a page *saved to the vault* anchors highlights
  on the durable source id, while the same page opened *transiently* anchors on
  the discovery source id. Highlights made in one do not appear in the other.
  Accepted: the saved paper is the durable, annotatable one; the transient reader
  remains a throwaway preview.

## Rollout (slices)

1. ✅ **Storage:** `add_local_html_to_vault` (shares `add_local_source_to_vault`
   with the PDF path; source kind `html`, method `html_import`, `local_path` =
   the snapshot) + `read_html_snapshot` (resolve source id → `local_path` → read).
   Tested: `add_local_html_to_vault_persists_html_source_and_serves_snapshot`.
2. ✅ **Backend (reader):** `durable_html_path` + cache-aware
   `acquire_and_store_html`; `get_reader_html` serves persisted `html:` via
   `read_html_snapshot` (exact `temp-html:` guard); `get_saved_reader_document`
   early-returns `saved_html_reader_document` for `html`-kind sources
   (`pdf_local_path: None`).
3. ✅ **Command:** async `import_html_url` (`web:<sha12(url)>` paper id,
   `html:<sha12>` source id, `#fragment`-only normalization); registered in
   `lib.rs`.
4. ✅ **Bridge/UI:** `addHtmlUrlToVault`; an **Add web page** button + inline URL
   bar in `VaultHome` (Enter submits, inline fetch error), wired via `+page`.
5. Metadata autofill: not enqueued yet (best-effort, deferred — saved pages carry
   `needs-review`, so the existing manual autofill affordance still applies).

**Test coverage note:** the store round-trip + read-path resolution + idempotent
re-add are unit-tested. The network acquire path and the reader-service wiring
(which need a Tauri `AppHandle`) are covered by `cargo build` + the frontend
check + manual verification, matching the codebase's existing test distribution
(no reader-service test harness exists).

## Non-goals

- Local `.html`/`.htm` **file** import (parallel to PDF file import) — deferred;
  the URL path is the wedge.
- **Pasted raw HTML** — deferred (`ingest_html` already supports a raw string, so
  this is a small later add).
- A **"Save to vault"** button promoting the transient HTML reader into the vault
  — a nice fast-follow, not this RFC's primary path.
- **Re-fetch / refresh** of a saved page (would break annotation anchors).
- Renaming `Highlight` → `Annotation` (tracked in RFC 0061).
