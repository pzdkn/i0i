# RFC 0053: Discovery Provider Expansion — Europe PMC + CORE

Status: Implemented
Date: 2026-07-23
Product: i0i
Target: Tauri v2 + Svelte, macOS first
Builds on: RFC 0043 (Multi-Provider Scout Search Quality), RFC 0044 (Remove
Semantic Scholar And Parallel Provider Fan-Out), RFC 0051 (Reliable Discovery
PDF Open)

## Summary

Widen discovery coverage beyond OpenAlex + arXiv by adding two new providers,
and make "can I actually open this?" a first-class ranking signal rather than a
surprise at read time.

The decision is:

- Add **Europe PMC** as a discovery provider (biomedical + life-sciences +
  preprints, keyless API, exposes full-text PDF/HTML links).
- Add **CORE** as a discovery provider (domain-general open-access aggregator,
  free API key, exposes direct download URLs).
- Do **not** attempt to bring back Semantic Scholar (RFC 0044 removed it for
  real API-reliability reasons that have not changed).
- Treat **viewability** — whether we hold an obtainable PDF/HTML location for a
  candidate — as an explicit ranking signal, and offer an optional
  "only results I can open" filter.
- Feed each provider's obtainable location into the RFC 0051 location plan so a
  new result's "PDF ready" chip can light up immediately.

Plain English version: search more places, prefer papers we can actually show,
and stop surfacing results that turn out to be unopenable.

## Problem

Two problems, one theme.

1. **Coverage.** After RFC 0044 the supported provider set is OpenAlex + arXiv.
   arXiv is CS/physics/math-heavy; OpenAlex is broad metadata but its open-access
   links are frequently stale or point at landing pages (documented in RFC
   0051). Whole domains — biomedicine, chemistry, social sciences, and the long
   tail of institutional-repository open access — are under-covered, and for
   many results we have metadata but no reliable way to read the paper.

2. **Viewability is invisible until too late.** Today a candidate is ranked
   with an `availability` sub-score (0.10 weight) that is 1.0 if a `pdf_url`
   string exists, 0.7 if the open-access flag is set but no URL, else 0.0. That
   is a weak proxy, it is buried in the blended score, and the user cannot ask
   for "only papers I can open." The user's stated requirement is explicit: a
   discovery result should come with a PDF or HTML view of the article.

Both new providers are chosen precisely because they expose obtainable
full-text locations directly, which turns problem 2 from a guess into a fact
for those results.

## Goals

- Add Europe PMC and CORE behind the existing `DiscoveryProvider` trait, with
  no changes to the orchestrator's merge/dedupe/rank contract.
- Both providers participate in the existing concurrent fan-out (RFC 0044).
- Normalize each provider's obtainable full-text location onto the existing
  `PaperCandidate` (`pdf_url` / `external_url` / `open_access`).
- Make viewability an explicit, surfaced signal and an optional hard filter.
- Hand obtainable locations to the RFC 0051 acquisition path so the Discover
  "PDF ready" chip reflects reality for new-provider results.
- Degrade gracefully: a missing CORE key, or any provider failure, drops that
  provider from the batch without failing the search (RFC 0044 partial-failure
  behavior).

## Non-Goals

- No embedding reranker and no query expansion — those are RFC 0054.
- No return of Semantic Scholar.
- No new full-text *hosting*; we link to / acquire from the provider's OA
  location, we do not mirror content.
- No change to the RFC 0051 acquisition internals (timeouts, hedging, negative
  cache). We only feed it better starting hints.
- No PubMed-metadata-only path. If a PubMed record has no PMC/OA full text, it
  reaches us through Europe PMC already; we do not add a separate NCBI
  E-utilities integration whose results we could not open.

## Decisions

1. **Europe PMC over raw PubMed.** Raw PubMed (NCBI E-utilities) returns
   biomedical *metadata only*; full text lives in PMC, and only the PMC
   open-access subset is downloadable. Europe PMC is a single REST API that
   already unifies PubMed + PMC + preprints and returns `fullTextUrlList`
   entries plus an `isOpenAccess` flag — i.e. it tells us directly whether a
   view exists. Keyless for normal use.
2. **CORE for domain-general OA breadth.** CORE aggregates open-access works
   from repositories worldwide, is open-access by construction, and its v3 API
   returns `downloadUrl` / full-text availability. This is the main lever for
   coverage outside arXiv's fields. Requires a free API key.
3. **Viewability is a classification, not just a URL string.** Each candidate
   gets a derived viewability tier (see Backend Design). It strengthens the
   existing `availability` sub-score and drives an optional filter.
4. **Obtainable location flows into RFC 0051.** When a provider gives a direct
   PDF/HTML URL, it populates `pdf_url` / `external_url` so the Discover
   availability probe (RFC 0051 B3) and background acquisition start from a
   known-good location.
5. **Graceful provider absence.** No CORE key ⇒ CORE is silently excluded from
   the provider set (log once, no user-facing error). This mirrors how a failed
   provider is tolerated today.

## Execution Model

Unchanged orchestration; two more providers in the fan-out:

```text
literal query
  -> selected providers: OpenAlex + arXiv + Europe PMC + CORE
     (whichever are enabled and configured)
  -> start all provider calls concurrently  (RFC 0044 join_all)
  -> await all
  -> merge successful results, record provider errors
  -> dedupe (RFC 0043 key: DOI -> arXiv -> OpenAlex -> normalized title)
  -> derive viewability per candidate
  -> rank (availability sub-score now viewability-aware)
  -> optional: drop non-viewable candidates if "only openable" is on
  -> return results
```

Deep Research (RFC 0037) inherits the new providers for free, because it
searches through the same orchestrator via `RealCandidateSource`.

## Backend Design

### Provider registration

Extend the small, deliberately-minimal enum (its own doc comment says: add a
variant only when a real adapter exists):

```rust
// src-tauri/src/commands/discovery/provider.rs
pub enum DiscoveryProviderId {
    OpenAlex,
    Arxiv,
    EuropePmc,   // new
    Core,        // new
}
```

New adapter modules, mirroring the arXiv module layout
(`config.rs` / `remote.rs` / `search.rs` / `normalize.rs` / `mod.rs`):

```text
src-tauri/src/commands/discovery/providers/europe_pmc/
src-tauri/src/commands/discovery/providers/core/
```

Each implements `DiscoveryProvider` (`id`, `api_key`, `build_url`, `search`).
Europe PMC's `api_key()` returns an empty/"not required" sentinel; CORE's reads
the configured secret and errors if absent so the orchestrator can skip it.

### Europe PMC adapter

- Endpoint: `GET https://www.ebi.ac.uk/europepmc/webservices/rest/search`
  with `query`, `format=json`, `pageSize`, `resultType=core`.
- Query mapping (like arXiv/OpenAlex filter translation):
  - free-text goes into `query`;
  - year → `PUB_YEAR:[from TO to]`;
  - open-access preference → `OPEN_ACCESS:y`;
  - author → `AUTH:"Name"` clauses.
- `normalize.rs` maps each result to `PaperCandidate`:
  - identifiers: `doi`, plus PMID/PMCID captured as `source_id` /
    `external_url` (see Dedup for identifier handling);
  - `open_access` from `isOpenAccess`;
  - `pdf_url` / `external_url` from `fullTextUrlList` entries, preferring a
    `documentStyle=pdf` / `availability=Open access` link, else the HTML
    full-text link, else the landing URL.

### CORE adapter

- Endpoint: `GET https://api.core.ac.uk/v3/search/works?q=<query>` with
  `Authorization: Bearer <CORE_API_KEY>`, `limit`.
- Query mapping: free text into `q`; year via `yearPublished>=`/`<=` clauses
  CORE supports in `q`; there is no reliable venue/field mapping (like arXiv,
  those constraints are dropped for CORE and honored on OpenAlex/Europe PMC).
- `normalize.rs` maps to `PaperCandidate`:
  - `doi`, `title`, `authors`, `year` from `yearPublished`, `abstract`;
  - `pdf_url` from `downloadUrl` when present;
  - `open_access = true` (CORE is OA by construction), `status = "core"`.

### Viewability

Add a small derived classifier used by both scoring and the optional filter.
It is computed after dedupe/merge from data already on the candidate — no extra
network calls:

```text
Viewable      -> has a direct pdf_url, OR a constructible arXiv PDF, OR a
                 fullTextUrl marked open access
MaybeViewable -> open_access flag true but only a landing URL
NotViewable   -> no OA signal and no obtainable URL
```

This maps onto — and replaces the ad-hoc logic inside — the existing
`availability` sub-score in `orchestrator.rs::rank_score`:

```text
availability = 1.0  if Viewable
               0.6  if MaybeViewable
               0.0  if NotViewable
```

Weights are otherwise unchanged (provider 0.35, keyword 0.25, citations 0.15,
recency 0.10, availability 0.10, multi-provider 0.05). Rebalancing the whole
formula is deferred to RFC 0054, which introduces the semantic sub-score.

Optional hard filter: when the request carries `only_viewable = true`, drop
`NotViewable` candidates *after* ranking (so provenance/telemetry still sees
them). Default off.

### Dedup and new identifier spaces

Europe PMC and CORE results usually carry a DOI, so the existing key
(DOI → arXiv → OpenAlex → normalized title) already merges most cross-provider
duplicates. Two additions:

- Capture PMID/PMCID on the candidate (reuse `source_id`, and optionally add
  `pmid` / `pmcid` fields to `PaperCandidate`) so a DOI-less biomedical record
  still merges by a stable id before falling back to title.
- `merge_candidate` keeps the *most viewable* location when fusing duplicates
  (e.g. an OpenAlex landing-page duplicate should not overwrite a CORE
  `downloadUrl`).

### Feeding RFC 0051

`discovery_location_hints` (reader_service, RFC 0051) already builds
`PdfLocationHints` from `pdf_url` / `external_url` / `doi` / `arxiv_id`. Because
the new providers populate those fields with obtainable OA URLs, no change is
required there — a Europe PMC / CORE candidate arrives pre-loaded with a strong
rank-4 (or better) location, and the RFC 0051 availability probe verifies it
before the user clicks.

### Configuration / secrets

- `CORE_API_KEY` resolved from the existing secret source (env / `.env`), same
  pattern as other keyed providers. Absent ⇒ provider skipped.
- Europe PMC needs no key; include a contact string in the User-Agent for
  politeness, consistent with the Unpaywall email convention.

## Frontend Design

Discover settings provider list gains two checkboxes:

```text
Providers
[x] OpenAlex
[x] arXiv
[ ] Europe PMC
[ ] CORE            (disabled with hint if no API key configured)
```

Defaults stay OpenAlex + arXiv so existing behavior is unchanged until the user
opts in. Sanitize stale/unknown provider values exactly as RFC 0044 does.

New optional toggle near the sort control:

```text
[ ] Only results I can open
```

Wired to `only_viewable`. The existing RFC 0051 availability chip
("PDF ready" / "PDF needs browser" / "no PDF found") already communicates
per-result viewability, so no new chip is needed — the new providers simply make
"PDF ready" far more common.

## Validation

Backend tests:

```text
europe_pmc: query with year+author maps to PUB_YEAR/AUTH clauses
europe_pmc: normalize picks the OA pdf fullTextUrl over the landing URL
core: missing api key -> provider excluded, search still succeeds
core: normalize maps downloadUrl -> pdf_url and marks open_access
orchestrator: four-provider fan-out runs concurrently
orchestrator: one provider failing still returns the others (partial failure)
dedup: same DOI from CORE + OpenAlex merges, keeps the more viewable location
viewability: Viewable/MaybeViewable/NotViewable map to 1.0/0.6/0.0 availability
only_viewable: drops NotViewable candidates after ranking, default off
```

Frontend checks:

```text
Europe PMC + CORE appear in provider settings
CORE checkbox disabled/hinted when no key configured
defaults remain OpenAlex + arXiv
"Only results I can open" toggles only_viewable
stale/unknown provider values are sanitized (RFC 0044)
```

Commands:

```bash
cargo fmt --check
cargo test discovery
cargo test research
cargo test
cargo check
pnpm check
git diff --check
```

Manual smoke:

```text
1. Enable Europe PMC, search a biomedical topic; confirm results with
   "PDF ready" chips that open without the browser fallback.
2. Enable CORE (with key), search a non-CS topic; confirm OA results appear.
3. Toggle "Only results I can open"; confirm metadata-only results disappear.
4. Confirm a paper found by both OpenAlex and CORE appears once, openable.
5. Remove the CORE key; confirm search still runs on the other providers.
```

## Implementation Order

1. Add RFC 0053.
2. Add `EuropePmc` / `Core` enum variants + empty adapter modules wired into
   the provider registry (compiles, returns no results).
3. Implement Europe PMC adapter (config/remote/search/normalize) + tests.
4. Implement CORE adapter + key handling + skip-when-absent + tests.
5. Add the viewability classifier; route it through `rank_score`'s
   availability sub-score; add `only_viewable` post-rank filter.
6. Extend dedup/merge for PMID/PMCID and most-viewable-location retention.
7. Frontend: provider checkboxes, key-aware disabling, "only openable" toggle,
   stale-value sanitization.
8. Confirm RFC 0051 hints/probe light up for new-provider candidates.
9. Run validation.

## Risks

- **CORE rate limits / key friction.** Free tier is modest. Mitigation:
  provider is opt-in and skipped without a key; failures degrade to the other
  providers. Add per-provider timeout (already implied by the shared client).
- **Europe PMC full-text link quality varies.** Some `fullTextUrlList` entries
  are HTML-only or publisher landing pages. Mitigation: the viewability tiers
  and the RFC 0051 probe verify before the user commits; MaybeViewable is
  ranked below Viewable, not hidden.
- **Identifier-space growth (PMID/PMCID).** Adding fields touches
  `PaperCandidate` constructors. Mitigation: additive `Option` fields with
  `#[serde(default)]`, same pattern used for `doi`/`arxiv_id` in RFC 0051.
- **Coverage overlap inflates result counts before dedup.** Mitigation: dedup
  runs before ranking/truncation as today; multi-provider agreement is a small
  positive ranking signal, not double-counting.

## Implementation Notes (2026-07-23)

- **Europe PMC** implemented and live-verified: a `CRISPR AND OPEN_ACCESS:y`
  search returned 25 candidates, all 25 carrying a direct `pdf_url` (e.g.
  `https://europepmc.org/articles/PMC…?pdf=render`) with title/year/DOI parsed.
  Author names come from the structured `authorList` when present, else the
  flat `authorString`. `isOpenAccess` is a `"Y"/"N"` string; `pubYear` is a
  string — both handled.
- **CORE** implemented from the v3 `search/works` schema and, once a
  `CORE_API_KEY` was added, **live-verified** (2026-07-23): a "deep learning"
  search returned 25 candidates, all 25 with a direct `downloadUrl` → `pdf_url`
  (e.g. `https://core.ac.uk/download/611946517.pdf`), titles/years parsed;
  numeric `id`s and DOI-less repository records handled. Note CORE's API sits
  behind Cloudflare and rejects some clients by User-Agent (a default Python
  client got a 1010 block); the adapter's `User-Agent` header passes. The
  adapter resolves its key lazily and the orchestrator skips CORE when the key
  is absent (`CoreProvider::is_configured()`), so it stays inert without a key.
  Wire types are `#[serde(default)]`-tolerant against schema drift.
- **Viewability** is folded into the existing `availability` sub-score rather
  than a separate enum: Viewable (has `pdf_url` or arXiv id) = 1.0,
  MaybeViewable (open-access flag, landing only) = 0.6, NotViewable = 0.0. The
  `only_viewable` request flag drops NotViewable candidates *before* ranking so
  truncation never wastes slots. Weights are otherwise unchanged; the full
  rebalance is deferred to RFC 0054.
- **Orchestrator wiring**: `DiscoveryOrchestrator` holds Europe PMC / CORE as
  `Option`s; `new()` stays 2-arg (deep research keeps OpenAlex + arXiv) and
  quick search calls `.with_expansion_providers(...)`. Unavailable providers
  are filtered out before fan-out, so no spurious `provider_error:core`
  appears when a user has no key.
- **Deferred as planned**: dedicated PMID/PMCID dedup fields on
  `PaperCandidate` (DOI + normalized-title dedup covers the common case;
  DOI-less PMC records currently dedup by title), and richer
  most-viewable-location retention in `merge_candidate` (current `.or()` keeps
  an existing PDF URL and adopts an incoming one when absent).
- **Deep research does not yet query the new providers** — the LLM planner only
  emits `open_alex` / `arxiv`, so the new `search_one` arms stay dormant in a
  deep run. Out of scope here; noted so it is not mistaken for a bug.
- **Frontend**: opt-in provider checkboxes for Europe PMC + CORE, an "Only
  results I can open" toggle (both in the quick-search `DiscoverSeedBar`
  settings), "All providers" now compares against `SUPPORTED_PROVIDERS.length`,
  and stale/empty provider lists fall back to `DEFAULT_PROVIDERS`
  (OpenAlex + arXiv) so the new providers stay off by default.
- Validation: `cargo test` 199 passed / 0 failed / 1 ignored; `pnpm check` 0
  errors / 0 warnings; Europe PMC live round-trip confirmed then its throwaway
  test removed.

## Future Work

- Provider-specific timeout + rate-limit settings and timing metrics in search
  traces (carried over from RFC 0044 future work).
- OpenReview provider for ICLR / anonymous-submission coverage (also noted as
  deferred in RFC 0050).
- Reconsider a metadata-only PubMed path only if a strong use case appears for
  surfacing non-openable biomedical records.
- Persist per-provider viewability hit-rates to inform default provider
  selection per query domain.
