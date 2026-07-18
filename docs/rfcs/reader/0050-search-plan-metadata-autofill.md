# RFC 0050: Search-Plan Metadata Autofill (Author-Corroborated)

- Status: Implemented
- Date: 2026-07-18
- Supersedes: extends RFC 0049 (verification-first metadata autofill)

## Problem

RFC 0049 fixed the visibility and verification problems, but the *quality* of
suggestions is still unreliable:

1. **The LLM's extraction is mostly thrown away.** After the LLM extracts
   `{title, authors, venue, year, doi?, arxivId?}` from page 1, only two things
   happen with it: a guessed DOI/arXiv id gets validated against providers, and
   the title becomes **one** OpenAlex title query. The extracted **authors are
   never used anywhere** — not as search constraints, not as corroboration when
   scoring candidates. Year and venue are likewise discarded.

2. **arXiv is never searched by title.** The pipeline only queries arXiv when it
   already has an arXiv id. But the papers that *need* autofill most — recent ML
   preprints and conference submissions (the two real failing PDFs are ICLR
   submissions) — often exist **only** on arXiv/OpenReview and not in Crossref.
   For exactly the papers we care about, the one provider that has them is
   skipped. Meanwhile the arXiv discovery provider already supports
   author-constrained queries (`embed_authors`, `search_query=ti:"..."`), so
   this is pure wiring.

3. **Crossref bibliographic gets the noisy blob, not the clean extraction.**
   `query.bibliographic` is fed the first 50 raw page-1 lines (affiliations,
   emails, abstract fragments) even when the LLM has already produced a clean
   title + author list. Crossref supports `query.title` + `query.author`, which
   is far more precise. The noisy blob returns *related published papers* —
   plausible-looking but wrong — and those become the "pretty bad" suggestions.

4. **Junk candidates are always shown.** Unverified candidates are capped at
   0.25–0.50 but still rendered in the review list with Apply buttons. When the
   right paper isn't found, the user sees three confidently-formatted wrong
   papers instead of an honest "couldn't verify a match".

5. **Single-result title matching.** `lookup_openalex_by_title` takes the first
   result passing `titles_similar` and drops the rest; near-miss normalization
   (subtitle punctuation, ligatures) silently loses the correct hit. And the
   filename-derived `current_paper.title` is still used as a query even when a
   better title exists, producing junk lookups.

6. **The panel stays open after Apply.** `applyMetadataCandidateForPaper` sets
   progress to `applied` but keeps `candidates: [candidate]`, so the review
   card — Apply button included — remains on screen after applying.

## Design

### 1. SearchPlan: the LLM output becomes the query, not just a candidate

Introduce a small struct built right after evidence extraction:

```rust
struct SearchPlan {
    title: Option<String>,        // best of: LLM title, largest-font, embedded
    author_last_names: Vec<String>, // from LLM authors + embedded authors (max 3)
    year: Option<i32>,
}
```

- The LLM stage runs early exactly as in RFC 0049 (same single call, same
  page-1-only 6,000-char cap, 500 max tokens — token cost unchanged), but its
  full output now feeds the plan.
- Query fan-out driven by the plan (skipped entirely when a page-1 identifier
  already resolved):
  - **arXiv (new):** `ti:"<title>"` with `authors` set on the request so the
    provider ANDs `au:` clauses. Fall back to plain title query if the fielded
    query returns nothing.
  - **OpenAlex:** title search as today, but score **all** returned candidates
    (see §3) instead of first-match.
  - **Crossref:** `query.title=<title>&query.author=<last names>&rows=5`.
    The raw-blob `query.bibliographic` becomes a *fallback*, used only when the
    plan has no title or the fielded query returned nothing.
- `title_queries` drops the filename-derived `current_paper.title` whenever any
  extracted title (LLM / largest-font / embedded) exists.

### 2. Author corroboration in the verification gate

New check next to `title_on_page1`:

```rust
fn authors_on_page1(candidate: &MetadataCandidate, page1_text: &str) -> AuthorMatch
// Fraction of the candidate's first 3 authors whose normalized last name
// occurs in normalized page-1 text. Strong = >= 2/3, Weak = >= 1, None = 0.
```

Updated tiers (auto-apply threshold stays 0.90):

| Verification | Provider-backed | Authors on p.1 | Confidence |
| --- | --- | --- | --- |
| Page-1 id match | yes | any | 0.98 |
| Title on page 1 | yes | strong | **0.95** |
| Title on page 1 | yes | none/weak | 0.90 |
| Title on page 1 | no (LLM/single) | strong | **0.85** |
| Title on page 1 | no | none/weak | 0.80 |
| Title similar | any | strong | 0.72 |
| Title similar | any | none | 0.60 |
| Unverified | any | any | **0.35 cap** |

The asymmetric payoff: a *wrong* candidate essentially never has both its title
and its authors on page 1, so author corroboration raises true positives
without raising false ones.

### 3. Score-all, keep-best provider matching

Provider lookups return their top N (3–5) results; every result is scored by
the verification gate rather than pre-filtered by `titles_similar`. The gate is
the single arbiter — a correct hit that fails strict local similarity but whose
title is literally on page 1 now survives.

### 4. Display floor: stop showing junk

- Review list shows only candidates with confidence **>= 0.60**.
- If nothing clears the floor, the outcome is `no_match` with an honest
  message, plus the best low-confidence guess (usually the LLM extraction)
  offered as a **"Use as draft"** action: it pre-fills the edit form instead of
  applying directly. The user fixes two fields and saves — faster than typing
  from scratch, and never silently wrong.
- Cap the review list at 3 candidates after merging.

### 5. Close the panel after Apply

- `applyMetadataCandidateForPaper` clears the paper's progress entry entirely
  (delete the key) after a successful apply; `MetadataPanel` shows a transient
  "Metadata applied." status note (no candidate cards, no Apply buttons) driven
  by a short-lived local flag, then returns to idle.
- Same on `onSaveMetadata` success while a review list is open — saving manual
  edits also dismisses stale candidates.
- The PaperList chip already hides on `applied`; deleting the progress entry
  keeps that behavior.

## What stays the same

- Verification-first principle (RFC 0049): matching beats extraction; nothing
  auto-applies without page-1 evidence.
- Token budget: one LLM call, page-1 text only, 6,000-char input cap, 500
  output tokens, skipped when a page-1 identifier exists.
- Events (`metadata_autofill_progress`, `paper_metadata_updated`), the shared
  `MetadataPanel`, manual editing, and the `#[ignore]`d probe harness.
- Deferred: Obscura `web_lookup` stage, DOI/arXiv edit fields (schema
  migration), OpenReview provider (candidate for a later RFC — it is the
  authority for ICLR submissions).

## Implementation order

1. `SearchPlan` construction + fielded Crossref `query.title`/`query.author`
   lookup; demote raw-blob bibliographic to fallback.
2. arXiv title+author search stage.
3. `authors_on_page1` + new scoring table (unit tests per row).
4. Score-all provider matching; drop filename query when extracted title exists.
5. Display floor + "Use as draft" + candidate cap.
6. Apply/save dismissal in `+page.svelte` and `MetadataPanel`.
7. Probe-harness validation against the two known ICLR PDFs; both should now
   reach a >= 0.85 candidate (arXiv or OpenReview-mirrored-on-arXiv version).

## Risks

- **arXiv fielded queries are brittle** (quoting, Lucene syntax): fall back to
  unfielded title query on zero results; regression test both paths.
- **Last-name normalization** (diacritics, hyphenated names): normalize via the
  same `normalize_title` word machinery; corroboration only *raises* scores, so
  a missed match degrades to today's behavior, never below it.
- **Preprint vs published duplicates** (arXiv + Crossref versions of the same
  paper): `merge_metadata_candidates` already merges by DOI/arXiv id/title;
  extend `same_candidate` to treat title-equal candidates with disjoint ids as
  mergeable so corroboration accrues instead of splitting.

## Implementation Notes (2026-07-18)

- All six stages implemented as specified. Live-probed the arXiv API: fielded
  `ti:"..." AND (au:"..." OR ...)` queries work, including titles containing
  colons; "ConceptScope: Characterizing Dataset Bias via Disentangled Visual
  Concepts" resolves on arXiv via the new title stage.
- The other test PDF (Label-Free Mitigation of Spurious Correlations...) is an
  anonymous ICLR submission not on arXiv/Crossref — it now correctly reaches
  `no_match` with the LLM extraction offered as "Use as draft", instead of
  showing wrong published papers with Apply buttons.
- `same_candidate` already merged across disjoint ids via title similarity;
  the implemented change backfills DOI/arXiv id during merge so the surviving
  candidate can hit the page-1 identifier tier.
- Crossref list parsing is shared (`crossref_list`) between the fielded and
  bibliographic queries; the bibliographic path is reached only when the
  fielded fan-out produced nothing provider-backed.
- Deferred as planned: OpenReview provider, Obscura `web_lookup`, DOI/arXiv
  manual edit fields.
