# RFC 0029: Open Discovery Candidates In Reader

Status: Draft
Date: 2026-06-09
Product: i0i
Target: Tauri v2 + Svelte, macOS first

## Summary

Let users open a discovery candidate in Reader immediately, without first adding it to a Vault.

Today the Reader only opens durable library papers:

```text
Reader open
  -> backend looks up paper in LibraryStore
  -> paper must already exist in a Vault/library snapshot
```

That makes saved-paper reading work, but it makes discovery browsing feel heavier than it should. In discovery, opening a paper is an inspection action, not a commitment to save it.

This RFC introduces a temporary reader path for unsaved discovery candidates:

```text
Discover candidate
  -> Open in Reader
  -> create temporary reader session
  -> optionally cache PDF in app cache/temp storage
  -> render Reader

Later:
Save to Vault
  -> promote metadata into durable library state
  -> reuse any already cached PDF/source when possible
```

## Context

Discovery candidates are transient frontend objects. They are not durable `Paper` rows in the backend library store until the user explicitly adds them to a Vault.

Reader currently assumes the opposite. Its backend path starts from a durable paper id and fails when the id is only a transient discovery candidate.

Observed behavior:

```text
get_reader_document(paper_id = openalex:W...)
  -> ReaderService searches LibraryStore snapshot
  -> no durable paper row exists
  -> "Paper not found"
```

This means discovery currently has an awkward product rule:

```text
discover candidate
  -> add to vault
  -> then open in reader
```

That is functional but wrong-feeling. Users should be able to inspect first and decide later.

## Product Decision

Reader should support two entry modes:

- durable library papers
- temporary discovery reader sessions

The user-facing rule should be:

```text
Open from Discover should always work.
Save to Vault is a separate decision.
```

Do not silently save a paper to the library just because the user opened it in Reader.

## Goals

- Let any discovery candidate open in Reader immediately.
- Keep unsaved discovery viewing separate from durable library persistence.
- Allow temporary PDF/document caching for unsaved reader sessions.
- Reuse already downloaded temporary files when the user later saves the paper.
- Preserve the current Reader behavior for saved library papers.
- Keep the first implementation small and understandable.

## Non-Goals

- No full redesign of the Reader document model.
- No automatic persistence of every discovery candidate.
- No temporary notes or annotations in the first increment.
- No long-term scout/session history persistence in v0.1.
- No sophisticated cache eviction system in the first increment.
- No multi-provider abstraction changes beyond what is needed for discovery-open.

## Proposed User Flow

```text
User runs Discover
  -> sees candidate rows
  -> clicks or double-clicks Open
  -> Reader opens that candidate immediately

If candidate has a PDF URL:
  -> app may download/cache PDF into temporary app cache storage
  -> Reader can use the cached file when available

If user later clicks Add to Vault:
  -> save durable paper metadata
  -> save durable document source row
  -> reuse the already cached PDF if one exists for the same candidate/source
```

## Product Rules

### Opening from Discover

Opening a candidate from Discover should not require:

- a Vault membership
- a durable `papers` row
- a durable `document_sources` row

It should require only enough candidate metadata to build a Reader document shell:

- stable candidate id such as `openalex:W...`
- title
- authors
- venue/year when present
- external URL
- PDF URL when present
- abstract and other metadata already returned by discovery

### Notes and annotations

For the first increment, notes remain a saved-library feature.

Rule:

```text
unsaved discovery reader session
  -> readable
  -> note creation disabled
```

This keeps the temporary path small and avoids inventing a second note lifecycle before the reading path itself is solid.

### Save to Vault after opening

Yes: if the paper was already downloaded while open in the temporary reader session, saving to Vault should reuse that downloaded file whenever possible.

That is the preferred behavior because it:

- avoids a redundant second download
- makes save/import feel fast
- preserves the user’s work if they already opened the paper successfully

The durable save path should promote or relink the cached file into the durable document-source model rather than pretending the temporary session never happened.

## Storage Decision

Use a temporary app-owned cache layer for unsaved discovery reader assets.

Prefer app cache storage over an arbitrary raw OS temp path:

- app cache is still disposable and non-durable
- app cache is easier to namespace by paper/source id
- app cache is easier to inspect, clean, and migrate later

Conceptual shape:

```text
~/Library/Caches/i0i/discovery-reader/{candidate_id}/
  session.json
  source.pdf
  extraction/...
```

The exact final path can use Tauri’s app-cache APIs, but the product rule is:

```text
temporary reader assets live in disposable app cache storage
```

## Proposed Architecture

Keep the current durable Reader path intact and add a second explicit path for discovery candidates.

### Durable path

Current path remains:

```text
saved Paper id
  -> ReaderService::get_reader_document(paper_id)
  -> LibraryStore-backed ReaderDocument
```

### Temporary discovery path

Add a separate backend command/service path:

```text
DiscoverCandidate
  -> open_discovery_candidate_in_reader(candidate)
  -> build temporary ReaderDocument
  -> resolve/download temporary document source as needed
  -> return ReaderDocument
```

This keeps the semantics clean:

- library reader path = durable
- discovery reader path = temporary

It also avoids forcing `ReaderService` to pretend that every readable thing is already a saved `Paper`.

## Data Shapes

Conceptual frontend payload:

```ts
type DiscoveryReaderOpenInput = {
  candidate: DiscoverCandidate;
};
```

Conceptual backend temporary session shape:

```ts
type TemporaryReaderSession = {
  candidateId: string;
  sourceProvider: string;
  pdfUrl?: string;
  externalUrl?: string;
  cachedPdfPath?: string;
  createdAt: string;
  updatedAt: string;
};
```

This does not need to become a durable database table in the first increment. The initial implementation may keep the session index in memory and the downloaded file on disk in cache storage.

## Promotion Rule

When the user saves a discovery-opened paper to a Vault:

1. Create the durable `Paper`.
2. Create or attach the durable `DocumentSource`.
3. If a temporary cached PDF already exists for the same candidate/source URL, reuse it.
4. Set durable `active_source_id` as usual.
5. Clean up or dereference the temporary session once the durable source owns the file.

Matching should prefer stable source identity:

```text
paper id + source url
```

If the temporary cached file cannot be safely promoted, fallback to the existing durable download path.

## Implementation Strategy

First increment:

1. Add a backend command for opening a discovery candidate in Reader.
2. Build a temporary ReaderDocument from discovery metadata.
3. If a PDF URL exists, allow temporary PDF caching in app cache storage.
4. Reuse cached temporary PDF on later save when source identity matches.
5. Keep notes disabled for unsaved sessions.

This is intentionally smaller than a unified reader abstraction for every possible source.

## Alternatives Considered

### 1. Require save before open

Rejected because it turns inspection into commitment and already feels wrong in use.

### 2. Insert every opened candidate into the durable library automatically

Rejected because it pollutes the durable library with papers the user has not actually chosen to keep.

### 3. Reuse durable library tables with a transient flag

Possible later, but not preferred for the first increment.

It blurs semantics:

- is this in the user’s library or not?
- should notes work?
- should it appear in Vault counts/search?

A separate temporary path is easier to reason about.

### 4. Full unified ReaderSource abstraction now

Attractive long-term, but a larger design move than needed for the immediate product fix.

## Risks

- Temporary cache cleanup policy may be underspecified at first.
- Promotion from temporary cache to durable source must avoid duplicate files and broken links.
- Unsaved reader sessions should not accidentally appear as saved library papers anywhere in the UI.
- We should avoid growing a second shadow persistence system before we need it.

## Open Questions

- Should temporary discovery reader sessions survive app restart, or only the current session?
- Should temporary cached PDFs be retained for a short TTL, or cleared on next startup?
- Should “Open” from Discover immediately start PDF download, or only when the user switches to PDF mode?
- When a temporary cached PDF is promoted on save, should we move the file or copy it into the durable location?

## Recommendation

Implement the first increment with:

- a separate discovery-reader backend command
- temporary app-cache-backed PDF storage
- no temporary notes
- reuse of already downloaded temporary PDFs on save when identities match

This gives the right product behavior with the smallest honest change.
