# RFC 0103: Honest Browser Discovery Diagnostics

- Status: Implemented
- Date: 2026-08-28
- Area: Discovery / Source acquisition
- Builds on: RFC 0052, RFC 0098, RFC 0100

## Summary

Log a bounded summary of every Obscura-backed discovery page and distinguish a
search-engine challenge from a valid result page that happens to contain no
scholarly links. The logs must show what Obscura returned without dumping raw
HTML, cookies, challenge tokens, IP addresses, or an unbounded page body.

This RFC makes the current failure observable and honest. It does not select a
new search engine or attempt to bypass an anti-automation challenge.

## Observed Failure

Discovery currently reports:

```text
Browser search returned no scholarly links
```

That message is produced after `inspect_browser_page()` succeeds but
`parse_search_results()` returns no candidates. The successful inspection is
not logged, so the message cannot distinguish an empty result set from a
redirect, challenge page, partial DOM, or parser drift.

On 2026-08-28, the failure was reproduced twice with the query
`attention is all you need`:

1. a direct Obscura fetch using the repository resource; and
2. a CDP probe through the exact persistent Obscura process owned by the
   running i0i backend.

The persistent CDP path returned:

```text
requested_host=scholar.google.com
final_host=www.google.com
final_path=/sorry/index
title=https://scholar.google.com/scholar?q=attention+is+all+you+need&start=0
html_bytes=1963
raw_links=3
accepted_candidates=0
page_text=Our systems have detected unusual traffic from your computer network...
```

The three links were Google's challenge explanation, terms, and help pages.
They were correctly rejected by `looks_like_external_result()`. Obscura and
CDP therefore worked; Google Scholar returned an anti-automation challenge
instead of a scholarly result page.

The current error is misleading because it describes only the parser output,
not the cause of that output.

## Diagnosis

Ranked hypotheses were evaluated against the live page:

1. **Google Scholar challenge.** Predicted a `/sorry/` final URL, unusual-
   traffic text, and only Google-owned links. Confirmed.
2. **Scholar selector drift.** Predicted a Scholar result page containing
   external links but no `.gs_ri/.gs_rt` matches. Falsified because no result
   page was returned.
3. **Snapshot captured too early.** Predicted that a bounded wait would reveal
   result links. Falsified by the direct Obscura run, which waited and still
   returned the complete challenge page.
4. **URL normalization or host filtering defect.** Predicted raw scholarly
   links that the parser rejected. Falsified because all raw links were Google
   challenge links.
5. **CDP transport failure.** Predicted an inspection error or empty snapshot.
   Falsified by the complete HTML, text, title, final URL, and links returned
   through i0i's managed CDP session.

The root cause is the configured default search entry point returning an
anti-automation challenge. Logging and classification will explain the
failure, but restoring results requires a separately discussed search-entry-
point or fallback decision.

## Decision

### 1. Log one bounded page summary

After each `inspect_browser_page()` call made by browser discovery, emit one
`browser-discovery` log entry containing:

- page number and total configured pages;
- requested host and final host/path;
- page title, bounded to 160 characters;
- HTML and visible-text byte counts;
- raw link, asset, and observed network-URL counts;
- parsed candidate count; and
- elapsed inspection time in milliseconds.

The summary is an info-level lifecycle record so a normal development run
shows whether Obscura returned a result page. Use the existing
`crate::shared::log` facility rather than a second logging system.

If inspection fails before returning a page, log the page number, requested
host, elapsed time, and typed acquisition error at warning level. Preserve the
existing caller-visible failure behavior.

### 2. Add bounded debug evidence

When `I0I_LOG=debug`, add one evidence entry containing:

- a normalized page classification;
- a whitespace-normalized visible-text preview, bounded to 500 characters;
- up to five returned link hosts.

Do not log full HTML, cookies, headers, response bodies, query parameters,
fragments, challenge tokens, IP-address lines, or all network requests. The
debug entry exists to separate parser behavior from upstream page behavior,
not to reproduce the entire browsing session.

### 3. Classify known challenge pages

Classify a page as `challenge` when either:

- the final host is Google-owned and the final path begins with `/sorry/`; or
- bounded visible text contains the known unusual-traffic challenge marker.

Use both signals because redirect details may change, while requiring a
Google-owned host or a specific page marker avoids classifying an ordinary
empty result as a challenge.

Other classifications are:

```text
result_page       one or more candidates were parsed
empty_result_page inspection succeeded, no challenge was detected, no candidate parsed
inspection_failed Obscura or CDP returned an error
```

The classification belongs at the browser-discovery boundary, where requested
URL, returned page evidence, and parser output are all available together.
`ObscuraClient` should remain a transport adapter and should not know Google
Scholar page semantics.

### 4. Return an honest challenge error

When every inspected page is classified as a challenge and no candidate was
found, return:

```text
Browser search was challenged by Google Scholar; retry later or use another configured search entry point.
```

When a valid page contains no accepted candidates, retain a distinct empty-
result error. When inspection itself fails, retain the typed browser failure.

Do not present a challenge as a process-startup failure, and do not retry the
same challenged URL automatically within one search. Immediate retry adds
traffic and is unlikely to change the response.

### 5. Keep search-backend recovery separate

RFC 0098 deliberately makes the search entry point configurable. Changing its
default, adding fallback engines, introducing backoff, or evaluating another
scholarly index changes discovery behavior and requires a focused follow-up
RFC and live comparison. This RFC supplies the evidence needed for that
decision without silently expanding scope.

## Scope

### In scope

- Per-page Obscura discovery summaries using the existing logger.
- Bounded debug evidence for returned text and links.
- Google Scholar challenge classification.
- Honest challenge, empty-result, and inspection-failure messages.
- Focused fixtures and tests for log evidence, classification, and error
  selection.

### Out of scope

- Changing the default search engine or URL.
- Adding automatic search-engine fallback or retries.
- Solving CAPTCHAs or bypassing anti-automation controls.
- Logging raw HTML, cookies, headers, full URLs, or full page text.
- Changing the generic Obscura process lifecycle.
- Adding a persistent browser-activity database or frontend log viewer.

## Acceptance Criteria

- [x] A successful Obscura inspection logs requested/final host information,
  page sizes, raw link count, parsed candidate count, and elapsed time.
- [x] `I0I_LOG=debug` logs a bounded text preview and up to five sanitized link
  hosts.
- [x] Logs do not contain query parameters, challenge tokens, cookies, raw
  HTML, full page text, or IP-address lines.
- [x] The recorded Google challenge fixture is classified as `challenge`.
- [x] A challenged search returns an explicit challenge error instead of
  `Browser search returned no scholarly links`.
- [x] A valid empty page remains distinguishable from a challenge and an
  inspection failure.
- [x] A valid Google Scholar result fixture still produces provisional
  candidates.
- [x] The existing ten-query parser corpus retains its current recall.
- [x] Focused discovery tests and the full Rust library suite pass.

## Verification Plan

1. Reduce the captured challenge page to a fixture that keeps only the final
   URL, title, challenge marker, and representative Google links.
2. Add failing tests for challenge classification, sanitized bounded evidence,
   and explicit error selection.
3. Add the page summary and debug evidence at the discovery boundary.
4. Run the focused browser-discovery tests and recorded parser corpus.
5. Run the full Rust library suite.
6. Launch with `I0I_LOG=debug`, perform one live search, and confirm the log
   reports `classification=challenge` without query parameters, challenge
   token, raw HTML, or IP address.

## Implementation Approval

Approved by the user on 2026-08-28. The user also explicitly authorized RFCs
written by the agent under the repository's RFC-first workflow to proceed to
implementation without a separate approval round.

## Implementation Notes

Implemented on 2026-08-28. `BrowserDiscoverySource` now records one bounded
info summary for every successful page inspection and a sanitized warning for
inspection failure. Debug logging adds a 500-character visible-text preview
and up to five link hosts.

The recorded Google challenge is classified at the discovery boundary using
the final `/sorry/` URL or its unusual-traffic marker. A challenged search now
returns an explicit Google Scholar challenge error; valid empty pages and CDP
failures remain separate outcomes. Obscura transport and process lifecycle are
unchanged.

Verification covered the correct `BrowserDiscoverySource` call seam with a
fake browser returning the reduced live challenge, the existing valid Scholar
fixture, the ten-query parser corpus, log sanitization bounds, and error
selection. A debug run emitted the expected summary and evidence without the
fixture's query parameters, challenge token, or documentation IP addresses.
The full Rust library suite passed with 475 tests and 5 existing ignored tests.
