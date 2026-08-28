# RFC 0104: Hybrid Browser and API Find

- Status: Implemented
- Date: 2026-08-28
- Area: Discovery
- Builds on: RFC 0044, RFC 0054, RFC 0098, RFC 0103

## Summary

Run Quick Find's Obscura browser lane and its configured scholarly API lanes
as one search. Merge every successful lane into the existing candidate model,
deduplicate and rank once, and return useful results when Google Scholar
challenges Obscura or one API is unavailable.

This restores the multi-provider behavior already represented by the Discover
workspace and `DiscoveryOrchestrator`; it does not add a new provider or try to
bypass a search-engine challenge.

## Observed Failure

Quick Find currently sends `providers: []` even though a new Discover workspace
defaults to OpenAlex and arXiv. The backend then calls only
`BrowserDiscoverySource::discover()`. If Google Scholar redirects Obscura to its
`/sorry/` challenge, the browser error aborts the command before any direct API
search can run.

The candidate feed and Open action are already capable of displaying API
candidates. The existing orchestrator already supports concurrent provider
fanout, partial provider failure, merge, deduplication, filtering, and ranking.
Those paths are disconnected from Quick Find rather than absent.

Two required example queries expose an additional recall mismatch:

- `lowrank adaptation` must find the LoRA paper, whose title spells the term
  `Low-Rank`;
- `selfsupervised vision` must find the DINO paper, whose title spells the term
  `Self-Supervised`.

The current API query and lexical ranker treat those compact and hyphenated
spellings differently. The arXiv adapter also sends raw free text such as
`sparse autoencoder` even though arXiv's documented API grammar requires
fielded terms such as `all:sparse AND all:autoencoder`.

## Diagnosis

Ranked hypotheses were checked against the production call path:

1. **Browser-exclusive Quick Find.** Confirmed: `search_papers()` calls only
   `browser_search()`, whose browser error is propagated.
2. **Configured providers are discarded.** Confirmed: the frontend hard-codes
   `providers: []` instead of sending `workspace.providers`.
3. **Compact spellings reduce recall and rank.** Confirmed: API requests use the
   literal query and keyword scoring compares whitespace tokens as substrings.
4. **arXiv receives an invalid/underspecified query form.** Confirmed: the
   adapter omits the documented `all:` field prefixes and Boolean operators.
5. **One API may be unavailable.** Confirmed as an expected operating state:
   arXiv needs no key, while OpenAlex/CORE can be unconfigured or fail. The
   orchestrator already treats provider failures independently.
6. **The feed cannot display API candidates.** Falsified: `PaperCandidate` is
   shared and the existing feed renders candidates with external/PDF URLs.

## Decision

### 1. Search browser and APIs concurrently

For one Quick Find request, start:

- the configured Obscura browser discovery; and
- the selected scholarly API fanout.

Do not wait for one lane before starting the other. Preserve the browser lane's
provisional progress events. The final response is a single merged set.

Quick Find must send its sanitized workspace provider selection. If the
selection is empty after sanitization, use the existing legacy provider field
so older persisted workspaces retain their behavior.

### 2. Treat source failures independently

Represent a successful browser lane as the existing `web`
`ProviderSearchResult` and combine it with successful API provider results.
Record browser and provider failures in response filters/logs without discarding
successful candidates.

Return an error only when no lane produced a usable result. The error must
summarize the browser and API failures so an upstream challenge is still
observable. An empty successful result is a valid response when at least one
source completed normally.

### 3. Share one merge and rank pass

Apply resolved constraints to browser candidates, then merge all source
results through `merge_and_filter()`. Compute semantic scores and rank once
against the original user query. Preserve the requested year, venue, open
access, viewability, sort, and result-limit behavior.

The response provider is `multi`; filters identify contributing sources and
partial failures.

### 4. Normalize orthographic query variants

Canonicalize the API query for the observed compact scholarly spellings:

```text
lowrank        -> low-rank
selfsupervised -> self-supervised
```

Use the canonical spelling for the single API fanout while the browser keeps
the literal user query. This avoids sending duplicate back-to-back requests to
arXiv. It is bounded spelling normalization, not LLM query expansion.

The arXiv adapter must convert natural free-text terms to the documented
all-fields Boolean form. For multi-term text, combine an exact title-phrase
branch with the all-fields term branch so canonical papers whose titles match
the user's wording are present in a small result window. Quoted user phrases
remain phrases, author and date constraints retain their existing field
clauses, and arXiv identifiers retain their identifier lookup behavior.

For lexical ranking, compare lowercase alphanumeric forms so compact,
hyphenated, and space-separated spellings match. Do not special-case paper
titles, authors, arXiv identifiers, LoRA, or DINO in ranking.

## Scope

### In scope

- Quick Find browser/API concurrency.
- Frontend propagation of selected providers.
- Partial browser/API failure tolerance.
- One shared merge, filter, semantic-score, sort, and rank pass.
- Bounded compact-to-hyphen query normalization.
- Valid arXiv all-fields query construction.
- Correct-seam tests and live LoRA/DINO verification.

### Out of scope

- Bypassing or solving Google challenges.
- Replacing the configured browser search engine.
- Adding a scholarly data provider.
- Making API keys mandatory.
- Changing deep-research planning or ingestion.
- Persisting full browser activity outside the existing bounded logs.

## Acceptance Criteria

- [x] Quick Find sends the workspace's selected API providers.
- [x] Browser discovery and API fanout start without serial dependency.
- [x] A Scholar challenge plus a successful API result returns candidates.
- [x] An API failure plus successful Obscura results returns candidates.
- [x] Browser and API candidates are merged, deduplicated, filtered, and ranked
  once in the final response.
- [x] Natural arXiv queries use documented `all:` fields and Boolean joins.
- [x] If every source fails, the returned error identifies both browser and API
  failures.
- [x] `lowrank adaptation` returns `LoRA: Low-Rank Adaptation of Large Language
  Models` as a displayable candidate.
- [x] `selfsupervised vision` returns `Emerging Properties in Self-Supervised
  Vision Transformers` (DINO) as a displayable candidate.
- [x] Focused tests cover partial failure, spelling normalization, merge, and
  candidate URLs.
- [x] The full Rust and frontend verification suites pass.

## Verification Plan

1. Add a correct-seam test with a challenged browser runtime and a successful
   fake API provider; assert the command-level hybrid search succeeds.
2. Add the inverse test with browser candidates and failed APIs.
3. Add unit tests for orthographic variants and punctuation-insensitive lexical
   matching.
4. Add a merge test proving the same paper from browser and API is one
   candidate with combined source evidence.
5. Run the focused Rust tests and frontend type/lint checks.
6. Run live Quick Find-equivalent searches for the two required queries against
   the installed stealth Obscura plus configured APIs, and verify title and
   external/PDF URL evidence.
7. Run the full Rust library suite.

## Implementation Approval

Approved by the user on 2026-08-28. The user explicitly authorized the agent to
write and auto-approve RFCs while they are AFK, so implementation may proceed
without a separate approval round.

## Implementation Notes

Implemented on 2026-08-28. The live hybrid verification used the recorded Brave
result page through the production browser-discovery pipeline together with
live arXiv metadata resolution and provider fanout. Both required arXiv papers
were returned with landing and PDF URLs. The full Rust suite passed with 483
tests and 6 pre-existing/live tests ignored; the dedicated LoRA/DINO live test
also passed explicitly. `pnpm check`, `pnpm build`, and the Obscura setup test
passed.
