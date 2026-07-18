# RFC 0051: Reliable Discovery PDF Open (Location Fan-Out + Bounded Acquisition)

- Status: Implemented
- Date: 2026-07-18
- Related: RFC 0036 (source acquisition / Obscura), RFC 0050 (metadata autofill)

## Problem

Opening a PDF from Discover is a lottery: sometimes it opens fast, sometimes it
"loads endlessly", sometimes it fails with no PDF. Root causes, traced through
`openCandidate` → `get_discovery_reader_document` → `ReaderService::cache_discovery_pdf`
→ `SourceAcquisitionService::acquire_pdf`:

1. **The direct HTTP fetch has no timeout at all.** The `reqwest::Client` built
   in `lib.rs` sets only a user agent — reqwest's default is *no* total request
   timeout. A publisher server that accepts the connection and stalls hangs the
   whole open forever. This is the literal "loads endlessly" case.

2. **The fallback chain is sequential and unbounded.** On failure the chain is:
   direct HTTP (unbounded) → Obscura browser startup (10 s) → browser fetch
   (45 s) → landing-page inspection (45 s) → then for **each** discovered
   candidate URL: another unbounded HTTP fetch plus another 45 s browser fetch.
   `pdf_candidates` matches any URL containing `.pdf`, `/pdf`, or `download`,
   so a publisher landing page can yield dozens of candidates. Worst case is
   many minutes; with a stalled direct fetch, forever.

3. **The whole chain runs inside one opaque `invoke`.** The Reader shows a
   single indeterminate "Loading document…" bar until the command returns. No
   stage progress, no cancel, and the "Open Source" fallback panel only
   renders *after* the entire chain has failed — the user stares at a spinner
   exactly when they most need the external link.

4. **We bet everything on a single provider-supplied URL.** arXiv candidates
   get a constructed `https://arxiv.org/pdf/<id>` (fast, reliable). OpenAlex
   candidates get only `best_oa_location.pdf_url` — frequently stale, an HTML
   landing page, Cloudflare-guarded, or paywalled. A paper with a DOI usually
   has *several* copies (arXiv, PubMed Central, institutional repositories,
   publisher OA); we never look for them. Which provider a candidate came from
   decides the experience — that is the lottery ticket.

5. **Nothing is verified before the user commits.** The "PDF available" chip
   in Discover means only "a URL string exists". The first time anyone checks
   whether it actually serves a PDF is after the user has clicked Read and is
   watching the spinner.

6. **Failures are never remembered.** Success is cached on disk (sha-keyed),
   but a failed acquisition reruns the full slow chain on every re-open.

Note: a paywall fallback UI (missing-pdf panel with "Open Source") already
exists in `ReaderView.svelte`; it is just unreachable until the unbounded
chain finishes.

## Design

Two halves. Part A makes acquisition *bounded and honest* (no more endless
spinner). Part B makes it *reliable* (much higher hit rate, verified before
click). A is the safety net; B is the well-oiled machine.

### Part A — Bound the machine

#### A1. Time budgets everywhere

- Build the acquisition `reqwest::Client` with `connect_timeout(10 s)` and
  `timeout(30 s)`. (Provider/API clients are unaffected.)
- Wrap each full acquisition in `tokio::time::timeout` — **60 s overall**.
  On expiry, a clear error ("Timed out fetching PDF from <host>") flows into
  the existing `pdf_error` field.
- Cap landing-page candidates at **5**, ordered network URLs → links →
  assets; drop the loose `download` substring match.

#### A2. Open the Reader immediately; acquire in the background

`get_discovery_reader_document` returns without waiting for the PDF:

- Immediate response: metadata + abstract document plus
  `pdf_status: "acquiring"` when any PDF location exists. A cache hit still
  returns the PDF path synchronously.
- Acquisition runs as a spawned task keyed by discovery source id (deduped),
  emitting `reader_pdf_acquisition_progress` events:
  `{ sourceId, paperId, stage, status: "running" | "ready" | "failed",
     message, error? }`. On `ready` the Reader refreshes via the existing
  `refreshTick` mechanism; on `failed` the missing-pdf panel shows.

#### A3. The source link is always one click away

- While acquiring, the Reader shows the metadata view with a live status line
  ("Fetching PDF — trying arXiv…") **and** an "Open in browser" button for
  `externalUrl ?? pdfUrl` from the first render. Paywalled papers no longer
  require waiting out the chain to get the link.
- A **Cancel** button aborts the spawned task and lands in the missing-pdf
  panel. (The `.part`-file write pattern already keeps the cache consistent.)

### Part B — Make it reliable

#### B1. PdfLocationPlan: fan out to every known copy

Mirror of RFC 0050's SearchPlan, built from the candidate's identifiers
before any download starts:

```rust
struct PdfLocation { url: String, source: LocationSource, rank: u8 }
enum LocationSource { ArxivId, Unpaywall, OpenAlexLocation, ProviderPdfUrl }
```

Gathered in parallel (each a single cheap JSON call, 5 s timeout, failures
ignored):

- **arXiv id → constructed URL** (`arxiv.org/pdf/<id>`), rank 0. If the
  candidate lacks an arXiv id but has a DOI/title, the id may already be
  known from metadata enrichment; do not run new searches here.
- **DOI → Unpaywall** (`api.unpaywall.org/v2/<doi>?email=…`): purpose-built
  free OA resolver returning *all* OA locations (repository copies, PMC,
  publisher OA) with version labels. Best `pdf_url`s rank 1–2. This is the
  single biggest hit-rate win for non-arXiv papers.
- **OpenAlex `locations[]`** (already in the work response — today we discard
  everything but `best_oa_location`): every location `pdf_url`, rank 3.
- **Provider `pdf_url` / landing `external_url`** as today, rank 4.

Deduplicate by URL; order by rank, with per-host reliability adjustments (B4).

#### B2. Hedged, budgeted attempts

Replace the single-URL brute-force chain with a small racing loop over the
plan:

- Per-attempt timeout **10 s**; **2 attempts in flight** at a time (hedged
  requests): start rank 0, start rank 1 after 3 s or on first failure; first
  response whose bytes start with `%PDF-` wins, losers are aborted.
- Direct HTTP only in this loop. The **Obscura browser becomes the last
  resort**, invoked once — against the best landing page — only after all
  direct locations are exhausted and only within the remaining overall
  budget.
- Expected effect: papers with any OA copy resolve in ~1–5 s; hopeless ones
  fail honestly inside the 60 s deadline instead of never.

#### B3. Verify at discovery time, not at click time

- When a candidate is **selected** in Discover (and, capped at the top ~10
  results, after a run completes), a low-priority background task builds the
  location plan and issues **HEAD/Range probes** (first bytes → `%PDF-`
  sniff) — no full downloads, concurrency 2, same negative cache.
- The candidate gains `pdf_availability: "unknown" | "verified" | "browser_required" | "unavailable"`,
  pushed via a `discover_pdf_availability` event.
- The Discover chip changes from "PDF available" (a URL exists) to an honest
  signal: **"PDF ready" / "PDF needs browser" / "no PDF found"**. The user
  stops buying lottery tickets because the ticket is scratched before they
  click.
- Optional escalation: for a *selected* candidate, continue past the probe to
  a full prefetch into the discovery cache, so clicking Read opens instantly.

#### B4. Remember what works

- **Session negative cache**: `{ url → failure, 10-min TTL }` in
  `SourceAcquisitionService`; re-opens skip known-dead URLs. The Reader's
  Retry button passes `force` to bypass it.
- **Per-host reliability stats** (in-memory, session-scoped to start):
  success/failure counts per host adjust rank within the plan — e.g. a host
  that returned HTML three times this session sinks below repository copies.
  Persisting stats to SQLite is deferred until the session version proves
  useful.

## What stays the same

- `ReaderDocument` shape, `get_reader_pdf_bytes`, promotion of the discovery
  cache into durable sources on save, the Obscura runtime itself.
- Saved-paper opens (`get_reader_document` / `PdfDownloadManager`) keep their
  pipeline, but B1/B2 slot in beneath `acquire_pdf`, so saved-paper downloads
  inherit the same reliability for free.
- Token cost: zero — no LLM involvement anywhere in this RFC.

## Implementation order

1. **A1** — client timeouts + overall deadline + landing-candidate cap.
   Smallest diff, kills "endless" outright, shippable alone.
2. **B1 + B2** — location plan (arXiv-id + Unpaywall + OpenAlex locations) and
   the hedged attempt loop inside `acquire_pdf`; Obscura demoted to last
   resort. This is the hit-rate win.
3. **A2 + A3** — background acquisition + progress events + acquiring-state UI
   with Open-in-browser and Cancel.
4. **B4** — negative cache + per-host stats + Retry bypass.
5. **B3** — availability probes on selection / top-N, honest Discover chips,
   selected-candidate prefetch.

## Risks

- **Unpaywall dependency**: free, but rate-limited by politeness (email
  param); we call it once per candidate open/probe, cached with the plan.
  Failures degrade to today's behavior — the plan just has fewer entries.
- **HEAD probes lie on some hosts** (200 for HTML interstitials): probes read
  the first bytes via a Range request and sniff `%PDF-`, the same check the
  full download uses; a wrong "verified" still fails fast at open time and
  updates the chip.
- **Hedged requests double some traffic**: bounded (2 in flight, 10 s cap),
  and only for the one paper the user is opening; probe concurrency for
  top-N is 2 with an overall cap.
- **Background task outliving the tab**: tasks are keyed and deduped; results
  land in the on-disk cache, so a missed event is corrected on the next
  refresh. Cancel is explicit, not tied to unmount.
- **Timeout too aggressive on slow links**: constants, overridable later via
  `i0i.config.toml [source_acquisition]`; the Open-in-browser path is always
  available meanwhile.

## Implementation Notes (2026-07-18)

- All stages implemented. Deviations from the design, all narrowing scope
  rather than behavior:
  - **OpenAlex `locations[]` deferred**: threading extra URLs through
    `PaperCandidate` touches nine constructor sites, and Unpaywall (keyed by
    the same DOI, and the source of most OpenAlex OA data) provides
    equivalent coverage. The plan is arXiv-id → Unpaywall → provider URL.
  - **Hedging** is `buffer_unordered(2)` (two in flight from the start)
    rather than a 3-second staggered second attempt.
  - **Progress events** carry `status` (`running` / `ready` / `failed`) and a
    message, without a per-stage breakdown.
  - **Probes run on candidate selection only**; the top-N post-run sweep and
    full selected-candidate prefetch are deferred.
  - **Per-host stats** are in-memory and session-scoped, as planned.
- Unpaywall needs a contact email, resolved from environment or `.env`:
  `I0I_UNPAYWALL_EMAIL` → `IOI_EMAIL` → `I0I_CROSSREF_MAILTO`; without one
  the resolver stage is skipped. Live-verified against the API: repository
  copies (arXiv, PMC) carry `url_for_pdf`; publisher-OA-only entries often
  expose just a landing URL, which the browser last-resort stage covers.
- `acquire_pdf` keeps its old signature as a wrapper over
  `acquire_pdf_with_hints`, so the Vault `PdfDownloadManager` inherits
  timeouts, hedging, and the negative cache without changes.
- New commands: `get_discovery_reader_document` gained `force`;
  `cancel_discovery_pdf_acquisition`; `probe_discovery_candidate_pdf`.
- Known small gap: if a very fast acquisition finishes before the Reader's
  event listener mounts, the `ready` event can be missed; the on-disk cache
  means any refresh or Retry corrects it.
