# RFC 0049: Verification-First Metadata Autofill

Status: Implemented (except Obscura web_lookup stage)
Date: 2026-07-18
Product: i0i
Target: Tauri v2 + Svelte, macOS first
Replaces: RFC 0046 (Local PDF Import With Metadata Autofill), RFC 0047 (Manual Local PDF Metadata Autofill)
Builds on: RFC 0024 (PDF Ingestion And Cache), RFC 0035 (Background Text Extraction), RFC 0040 (Bundle Pdfium Resource)

## Why RFC 0046/0047 Are Replaced

Two implementation attempts of the previous RFCs produced a pipeline that
compiles, passes tests, and still does not work for the user. Observed run:

```text
[metadata-enrichment ...] start paper_id=local:b02bcc50d021 ...
[metadata-enrichment ...] review paper_id=local:b02bcc50d021 candidates=1
```

The worker "succeeded" (one review candidate), yet from the user's seat
nothing happened. Root causes, confirmed by code review:

1. **Extraction-first strategy.** The pipeline's load-bearing step was guessing
   the paper title from pdfium's text dump with line heuristics ("first 40
   lines, 12-180 chars, not 'abstract'"). PDF text extraction order is not
   visual order, so the title queries were mostly noise and matching failed.
   More heuristics of this kind will not fix it.
2. **Invisible outcome.** Progress and candidate review rendered only in the
   Reader right panel. The user triggers autofill from the Vault paper list
   and stays in the Vault, so a run ending in `candidate_review` was
   indistinguishable from "nothing happened".
3. **No first paint.** The command queued a background job and returned;
   the first progress event arrived only after the worker started PDF
   extraction. Multi-second silence after the click.
4. **No iteration loop.** There was no way to see what extraction produced for
   a specific failing PDF without running the whole app. Every agent attempt
   shipped blind.
5. **Cited-DOI bug.** The first DOI regex match across five pages of text was
   treated as the paper's own identity at 0.98 confidence. Papers cite DOIs;
   this could auto-apply the wrong paper's metadata.

What is carried forward unchanged from RFC 0046/0047 (already implemented and
stable):

- `Import PDF` from the Vault copies files into app-managed storage, creates
  paper records with fallback metadata and the `needs-review` tag, and stays
  in the Vault (no auto-open, no auto-enrichment).
- Autofill is an explicit command on the paper row context menu.
- `metadata_autofill_progress` events, `autofill_paper_metadata` and
  `apply_paper_metadata_candidate` commands, the `MetadataCandidate` shape,
  and the existing candidate/merge plumbing.

## Summary

Metadata autofill becomes verification-first: instead of trying to *extract*
the title locally (hard, fragile), the worker gathers cheap evidence, asks
providers and an LLM for *candidates*, and then *verifies* each candidate
against the PDF's own first-page text (easy, reliable). Progress is immediate
and lives where the user is. Metadata is directly editable by hand in the
right panel, so autofill is an accelerator, not a gate.

## Core Principle

```text
Matching is easier than extraction.
```

You never need to find the title in the PDF. You only need to confirm that a
candidate's title appears in the PDF's first-page text. Normalized
containment ("does this candidate title occur in the page-1 text?") is nearly
trivially reliable, works regardless of text-dump ordering, and applies the
same way to candidates from Crossref, OpenAlex, arXiv, the LLM, or the web.

## Pipeline

```text
stage 0  queued           emit progress synchronously in the command
stage 1  extracting       pdfium: page-1 text, pages 2-5 text, embedded
                          metadata, largest-font run on page 1
stage 2  identifiers      DOI / arXiv id regex, page 1 preferred
stage 3  llm_extracting   LLM turns page-1 text into {title, authors, year,
                          venue, doi?, arxivId?} search fields  (EARLY, not
                          a fallback)
stage 4  providers        arXiv id -> arXiv;  DOI -> Crossref + OpenAlex;
                          Crossref query.bibliographic with raw page-1 text;
                          OpenAlex title search with LLM title
stage 5  web_lookup       Obscura browser fallback (optional, see below)
stage 6  verifying        every candidate verified against page-1 text
stage 7  applied | candidate_review | no_match
```

### Stage 1: Evidence

- Page-1 text and pages 2-5 text kept separate (identifiers on page 1
  outrank identifiers on later pages).
- Embedded PDF metadata title/author, filtered for junk as today.
- **Largest-font run on page 1**: pdfium exposes per-character font size; the
  contiguous run of largest-size text on page 1 is almost always the title.
  This replaces the line-heuristic title guesser as the deterministic title
  candidate.

### Stage 2: Identifiers

- DOI/arXiv regex as today, but scoped: a page-1 match is `own-id` evidence;
  a later-pages match is only a hint and never auto-applies without stage-6
  verification.

### Stage 3: LLM Search-Field Extraction (early)

The configured chat model is a first-class extractor, not a last resort.
Turning messy first-page text into structured search fields is exactly what
an LLM is good at and what heuristics are bad at.

- Input: capped page-1 text + embedded metadata + filename.
- Output JSON: `{ title, authors[], year?, venue?, doi?, arxivId? }`.
- Runs whenever stage 2 found no page-1 identifier (and may run in parallel
  with stage 4's Crossref bibliographic query).
- Hallucination control is stage 6 verification, not ordering: an LLM title
  that does not appear in the PDF text is discarded; an LLM DOI/arXiv id is
  only used after a provider lookup returns a record that itself verifies.
- If no API key / provider unavailable: log, skip, continue. The pipeline
  must still work without an LLM (identifiers + bibliographic query).

### Stage 4: Providers

- `arXiv id -> arXiv API`, `DOI -> Crossref work lookup + OpenAlex` as today.
- **New: Crossref `query.bibliographic`** — send the first ~50 lines of raw
  page-1 text and let Crossref's server-side fuzzy citation matching return
  candidates. This is the standard tool for "I have a messy reference blob";
  it needs no local title guessing at all. Keep the polite `mailto` config.
- OpenAlex title search uses the largest-font title and the LLM title, not
  line heuristics.

### Stage 5: Obscura Web Lookup (fallback)

The app already ships the Obscura browser runtime for source acquisition.
When stages 2-4 produce nothing verifiable, the worker may:

- fetch `https://doi.org/<doi>` for an unverified DOI and read the landing
  page's citation meta tags (`citation_title`, `citation_author`,
  `citation_doi`), and/or
- fetch a scholarly search page for the best title candidate and read result
  titles/links.

Rules: bounded to at most 2 fetches per run, reuses the existing Obscura
manager and stealth config, emits a `web_lookup` progress stage, and its
candidates are review-only unless stage-6 verification plus a provider
record confirms them. This stage is feature-flagged
(`I0I_METADATA_WEB_LOOKUP=1` initially) so the core pipeline never depends
on it.

### Stage 6: Verification (the gate)

Every candidate from every source passes the same check before it may
auto-apply or even rank highly in review:

```text
verified(candidate) :=
  normalize(candidate.title) is contained in normalize(page1_text)
  OR candidate.doi/arxivId matches a page-1 identifier
```

Confidence:

```text
page-1 id match + provider record        -> 0.98  auto-apply
verified title + provider record         -> 0.90  auto-apply
verified title, single source            -> 0.80  suggested (1-click apply)
unverified anything                      -> <=0.5 review list only
```

This single rule fixes the cited-DOI bug (a reference-list DOI fails the
page-1 id check and its title is not on page 1) and makes LLM and web
candidates safe to use early.

## UX

### Immediate progress, where the user is

- `autofill_paper_metadata` emits `queued` progress synchronously before
  returning, so the UI updates in the same tick as the click.
- The **paper row** shows a compact status chip: `autofilling...`,
  `review (2)`, `no match`, cleared on `applied`.
- The **Vault right panel** (`VaultInspector`) becomes the primary progress
  and review surface for the selected paper — the user starts autofill in
  the Vault, so results appear in the Vault. The Reader Meta tab renders the
  same component for the open paper.
- Stages map to visible one-liners exactly as emitted: `Reading PDF...`,
  `Found DOI 10.x on page 1. Checking Crossref...`, `Asking LLM for search
  fields...`, `Verifying 3 candidates...`.

### Candidate review

- Candidates render with confidence, sources, evidence line, and field diff
  against current metadata; `Apply` per candidate (existing
  `apply_paper_metadata_candidate`).
- Terminal states always say something actionable: `Applied`,
  `2 candidates need review`, `No reliable match — edit manually below`.

### Manual metadata editing (new)

The right panel's Meta section shows editable fields for the selected paper:

```text
title, authors (one per line), year, venue, doi, arxiv id, abstract
```

- `Edit` toggles the fields; `Save` persists via a new command
  `update_paper_metadata(paper_id, fields)`; `Cancel` reverts.
- Saving manually clears `needs-review`.
- Autofill never overwrites fields while an edit is in progress, and a
  successful apply merges: user-edited fields win unless the candidate is an
  auto-apply identifier match.
- Available in both VaultInspector and Reader Meta tab (shared component).

## Backend Shape

- Keep: `autofill_paper_metadata`, `apply_paper_metadata_candidate`,
  `metadata_autofill_progress`, `MetadataCandidate`, candidate merge code.
- Add: `update_paper_metadata(paper_id, fields)` command +
  `paper_metadata_updated` emission.
- Add stages: `identifiers`, `llm_extracting` (moved earlier), `web_lookup`,
  `verifying`. Remove reliance on `title_candidates_from_text` line
  heuristics (delete the function once the font-size extractor lands).
- Progress cache: keep the session-local per-paper progress map; the command
  seeds it with `queued` synchronously.

## Dev Harness (build first)

A probe target that makes the pipeline observable offline:

```text
cargo run --bin metadata_probe -- <path/to.pdf>
```

Prints, stage by stage: extracted page-1 text (first 40 lines), largest-font
title run, identifiers found and where, LLM fields (if key configured),
each provider query + top results, and each candidate with its verification
verdict. Network stages can be skipped with `--offline`.

Fixtures: `src-tauri/tests/fixtures/pdfs/` with 3-5 real-world PDFs that
previously failed (including the two papers from the observed log). Ignored
integration tests assert that the deterministic stages (evidence, largest
font, identifiers, verification) produce expected values for each fixture.

This harness is the first implementation step. Every subsequent stage lands
with a probe run against the fixtures, so failures are visible per stage
instead of as a silent `no candidates`.

## Implementation Order

1. `metadata_probe` binary + fixtures + ignored fixture tests (evidence
   stages only).
2. Largest-font title extraction; page-1-scoped identifiers; delete line
   heuristics.
3. Verification gate + new confidence rules (fixes cited-DOI bug).
4. Crossref `query.bibliographic` stage.
5. LLM search-field extraction moved early.
6. Synchronous `queued` emission + paper-row status chip + VaultInspector
   progress/review surface (shared component with Reader Meta tab).
7. Manual metadata editing (`update_paper_metadata` + editable fields).
8. Obscura `web_lookup` stage behind `I0I_METADATA_WEB_LOOKUP`.
9. Docs: remove references to RFC 0046/0047 elsewhere if any.

Each step is a small, independently verifiable diff; steps 1-3 alone should
already fix the observed "review candidates=1, nothing visible" runs.

## Risks

- Largest-font heuristic fails on scanned PDFs (no text) and exotic layouts.
  Mitigation: LLM fields + Crossref bibliographic query are independent
  paths; scanned PDFs stay out of scope (no OCR, as before).
- Containment verification fails when providers return translated or
  subtitle-expanded titles. Mitigation: token-overlap fallback (existing
  `titles_similar`) at the `suggested` tier, never auto-apply.
- Obscura lookups can be slow or blocked. Mitigation: feature flag, 2-fetch
  budget, review-only candidates.
- LLM cost per autofill click. Mitigation: single small completion, capped
  input, only when no page-1 identifier.

## Validation

- `metadata_probe` on each fixture PDF shows a verified candidate for at
  least the DOI-bearing and arXiv-bearing fixtures.
- Clicking `Autofill metadata` shows a status chip on the row and progress
  in the Vault right panel within one frame.
- A paper whose page 1 contains its DOI auto-applies correct metadata.
- A paper citing other DOIs does not adopt a cited paper's metadata.
- LLM-only candidates appear as review candidates and never auto-apply
  unverified.
- Editing fields manually in the right panel persists and clears
  `needs-review`.
- With no LLM key configured, identifier and bibliographic paths still work.
- `cargo test`, `pnpm check`, `cargo fmt --check`, `git diff --check` pass.

## Implementation Notes

Implemented (2026-07-18):

- Verification gate (`score_candidate` / `verification_for` /
  `title_on_page1`) with the four confidence tiers; auto-apply threshold is
  0.90. Providers no longer assign their own confidence.
- Page-1 vs later-pages identifier split; cited-DOI candidates demote to the
  review list (regression test: `verification_demotes_cited_doi_candidates`).
- Largest-font title extraction via pdfium per-char font sizes. Two findings
  from probing the real failing PDFs: small-caps titles mix two font sizes
  (threshold is 0.75 x max, not 0.9), and whitespace/control glyphs report
  size 0 so they bridge runs instead of breaking them.
- Crossref `query.bibliographic` reverse lookup over the leading page-1
  lines (capped at 1,500 chars).
- LLM promoted to early search-field extraction; runs only when page 1 has
  no identifier. Token cost: input is capped page-1 text only (6,000 chars,
  max 500 output tokens) — never the whole PDF.
- Immediate progress: `queued` emits synchronously; paper rows show a status
  chip; `VaultInspector` and the Reader Meta tab share `MetadataPanel.svelte`
  (progress, candidate review with Apply, and an Autofill button).
- Manual editing: `update_paper_metadata` command + editable
  title/authors/year/venue/abstract in both panels; saving clears
  `needs-review`.
- Cross-surface refresh: apply/update commands emit `paper_metadata_updated`;
  the ReaderView now listens and reloads its document, and `+page.svelte`
  rehydrates the library, so Vault rows, the reader header, and both
  inspectors update together. User-approved Apply bypasses the needs-review
  gate (`apply_paper_metadata_enrichment_with_policy`).

Deviations from the spec above:

- The probe harness is an `#[ignore]`d test, not a separate binary (the
  crate's modules are private). Run it with:
  `I0I_PROBE_PDF=/path/to.pdf cargo test probe_pdf_evidence -- --ignored --nocapture`
  Fixture PDFs are not committed to the repo; the probe points at any path,
  including PDFs already in app storage.
- DOI / arXiv id are not manually editable yet — the `papers` table has no
  columns for them; adding them needs a schema migration and is deferred.
- The Obscura `web_lookup` stage (stage 5) is not implemented yet; the flag
  `I0I_METADATA_WEB_LOOKUP` is reserved for it.
