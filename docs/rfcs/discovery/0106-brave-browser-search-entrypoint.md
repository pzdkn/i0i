# RFC 0106: Brave Browser Search Entry Point

- Status: Implemented
- Date: 2026-08-28
- Area: Discovery / Source acquisition
- Builds on: RFC 0098, RFC 0103, RFC 0104, RFC 0105

## Summary

Change Obscura browser discovery's default search entry point from challenged
Google Scholar to Brave Search and parse Brave's server-rendered web-result
rows explicitly. Keep the URL configurable and retain arXiv/OpenAlex metadata
resolution plus the hybrid API lane.

## Observed Failure and Comparison

Google Scholar consistently redirects i0i's working Obscura CDP session to a
Google `/sorry/` unusual-traffic challenge. DuckDuckGo's static HTML endpoint
returns its image CAPTCHA from the same network. Neither response contains
scholarly result links, so switching between them does not restore Find.

Brave Search returned a normal server-rendered result page for both required
queries without a challenge:

- `lowrank adaptation`: the first web result was arXiv 2106.09685, `LoRA:
  Low-Rank Adaptation of Large Language Models`;
- `selfsupervised vision`: the web results included arXiv 2104.14294,
  `Emerging Properties in Self-Supervised Vision Transformers` (DINO), plus an
  open-access CVF PDF.

Each result is represented by a `div.snippet[data-type="web"]` row containing
one primary result link, title, and snippet. The page does not require a client-
side API call before those rows exist, which fits Obscura's snapshot contract.

## Decision

### 1. Make Brave Search the default browser URL

Use:

```text
https://search.brave.com/search?q={query}&source=web
```

The existing `[discovery].browser_search_url` override remains authoritative.
This is a default recovery, not a hard-coded dependency in the transport.

Continue to start the installed rendering-enabled Obscura build with
`--stealth`. Stealth is an ordinary browser transport mode; it does not justify
solving or bypassing a challenge when one is returned.

### 2. Parse only primary Brave web results

Before the generic anchor fallback, parse
`div.snippet[data-type="web"]` rows. Within each row:

- take the primary result anchor as the external URL;
- take its `.title` text as the candidate title; and
- take `.generic-snippet .content` as the optional abstract snippet.

Do not ingest Brave navigation tabs, images, videos, discussions, internal
feature callbacks, or serialized hydration data as candidates. Keep Google
Scholar's explicit parser for user-configured Scholar URLs and recorded
regression fixtures.

### 3. Preserve scholarly resolution and hybrid merge

An arXiv result URL supplies an arXiv identifier immediately. Resolve it through
the existing arXiv provider, then merge it with direct API candidates in the
RFC 0104 source batch. Browser and API evidence for the same arXiv identifier
must deduplicate to one richer candidate with both `found_by:web` and
`found_by:arxiv` reasons.

### 4. Keep diagnostics bounded and source-neutral

RFC 0103 page summaries continue to log requested/final host/path, page sizes,
raw links, candidate counts, timing, and bounded debug evidence. Do not log
Brave's full HTML or serialized result payload.

## Scope

### In scope

- Brave Search as the default browser search URL.
- Explicit parsing of primary Brave web-result rows.
- Reduced fixtures for required LoRA and DINO results.
- Existing metadata resolution, logging, merge, deduplication, and ranking.
- Live Obscura stealth verification when the host permits browser networking.

### Out of scope

- Brave Search API keys or paid API use.
- CAPTCHA solving, challenge bypass, proxies, or IP rotation.
- Removing the configurable URL or Google Scholar regression parser.
- Parsing non-web Brave verticals.
- Treating arbitrary Brave hydration JSON as a stable API.

## Acceptance Criteria

- [x] Default browser discovery requests Brave Search instead of Google
  Scholar.
- [x] A Brave result fixture yields only primary external web results.
- [x] The LoRA fixture yields arXiv 2106.09685 with a displayable landing URL.
- [x] The DINO fixture yields arXiv 2104.14294 and its displayable landing URL.
- [x] Navigation, media, discussion, and internal Brave links are not emitted.
- [x] A browser/API duplicate is merged by arXiv identifier with both source
  reasons.
- [x] The existing Scholar parser corpus and challenge diagnostics still pass.
- [x] Focused and full Rust suites pass.

## Verification Plan

1. Reduce the observed Brave rows to a fixture containing canonical LoRA and
   DINO results plus representative navigation noise.
2. Add failing parser tests at `parse_search_results()`.
3. Add the explicit Brave row parser before the existing generic fallback.
4. Change the default URL only after parser tests pass.
5. Run the browser parser corpus, hybrid merge tests, and full Rust suite.
6. Run the required queries through the installed stealth Obscura process and
   verify final candidates and URLs. If execution sandbox DNS prevents a direct
   subprocess probe, retain the live managed-app log as the required evidence
   rather than weakening stealth or changing transports.

## Implementation Approval

Approved by the user on 2026-08-28. The user authorized the agent to write and
auto-approve RFCs while they are AFK, so implementation may proceed without a
separate approval round.

## Implementation Notes

Implemented on 2026-08-28. Live Brave HTML for both required queries was used
to reduce the regression fixture. The browser parser, Scholar corpus, challenge
diagnostics, arXiv-identifier merge, and hybrid live test passed. Direct
Obscura subprocess navigation was DNS-restricted by the execution sandbox, so
transport verification used the installed stealth build's managed CDP test and
the end-to-end discovery verification used the recorded Brave page with live
arXiv resolution, as allowed by the verification plan.
