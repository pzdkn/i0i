# RFC 0152: Empty Ready Research Run Diagnosis

- Status: Diagnostic complete
- Date: 2026-09-09
- Scope: Research Run completed at 2026-09-09 14:51:17

## Question

Explain why the Run found no additional papers and why its attempt to read the
existing relevant paper failed. Do not infer the cause from the final summary;
trace the persisted activity and document state that produced it.

## Diagnostic Method

1. Identify the exact Run and list its ordered activity records.
2. Inspect every child search query, provider outcome, candidate count, and
   terminal error.
3. Identify the unread paper selected by the agent and trace its active source,
   known alternatives, local snapshot, extraction, chunks, and Reader result.
4. Separate expected empty results from infrastructure or contract failures.
5. Report the smallest corrective actions, but do not implement them without
   separate approval.

For browser discovery, replay one recorded query and distinguish three
boundaries: Brave's returned page, Obscura's rendered snapshot, and i0i's result
selectors. A replay must not create a Search or mutate library state.

The diagnostic uses the existing SQLite state and logs only. It must not launch
a model-backed Research Run or mutate the Vault.

## Output

- A causal timeline for discovery and reading.
- The concrete failed queries and source identifiers.
- A ranked root-cause assessment supported by persisted evidence.
- A recommendation for whether the Run should remain `ready`, become
  `completed_without_evidence`, or be marked failed.

## Findings

### Discovery

The parent Run was `harness_run_1788965424172594000`. It started one child
Search, `run_1788965443703675000`, with a single quick iteration and three
concurrent queries:

1. `sparse autoencoder attention head comparison intervention`
2. `SAE features attention mechanisms circuit analysis transformers`
3. `dictionary learning attention query key intervention causal`

All three used the `web` transport and failed within one second with `Browser
search returned no scholarly links`. The child Run persisted `provider_set =
[]`, zero candidates, and `max_iterations` as its stop reason.

This is not a negative scholarly result. Production agent Search intentionally
uses `BrowserCandidateSource`: Obscura finds provisional links first, and
OpenAlex/arXiv are used only to resolve an identifier or exact title. Because
the browser parser produced no provisional links and these were broad queries
rather than exact titles, no provider API fallback ran. The same browser error
appears in several earlier child Searches, so it is not unique to these three
query formulations.

### Browser Replay

A non-mutating replay used the first recorded query and the configured default
URL:

```text
https://search.brave.com/search?q=sparse%20autoencoder%20attention%20head%20comparison%20intervention&source=web
```

The configured Obscura process was healthy. Obscura successfully navigated to
Brave and returned a rendered document, so browser startup, CDP connection, DNS,
and page navigation were not the failing boundary. The rendered page contained:

```text
title: Brave Search
text: Verifying you're not a bot ... Quick check before you continue searching.
links: 3
Brave web-result rows: 0
```

The raw Brave response likewise contained a `challengeSet` and no web-result
rows. Brave therefore withheld the result page behind its bot-verification
challenge. Obscura's stealth mode did not solve that challenge.

i0i then reported the challenge as `Browser search returned no scholarly links`
because `is_search_challenge()` currently recognizes only Google's `/sorry/`
URL and unusual-traffic wording. It has no Brave challenge classifier. The
failure chain is therefore:

```text
query -> Obscura navigation succeeds -> Brave bot challenge -> zero parsed rows
      -> challenge goes unrecognized -> misleading empty-results error
```

The process inspection also found many orphaned Obscura servers whose parent is
PID 1. The active i0i instance had its own healthy child, so those stale
processes did not cause this replay failure, but process cleanup is independently
broken and should be addressed.

### Challenge Handling Research

Obscura cannot reliably pass the Brave page shown in the replay. Its documented
stealth mode addresses basic TLS, User-Agent, and browser-fingerprint checks,
but explicitly does not handle interactive challenges or CAPTCHAs. Obscura can
also use HTTP/SOCKS proxies, a custom User-Agent, and persistent browser storage.
Those controls may reduce challenge frequency by preserving cookies and avoiding
a rate-limited IP, but they are mitigations rather than a supported challenge
solver.

Automating clicks on Brave's `Verify` control would remain brittle: the control
may lead to proof-of-work or an interactive CAPTCHA, and success can depend on
IP reputation and browser state. i0i should detect this state and change search
transport instead of repeatedly attempting to defeat it.

### Search Transport Recommendation

Search-result discovery should use an interface intended for machine access.
Obscura should remain the acquisition browser that opens and inspects result
URLs after discovery.

Preferred order:

1. Use OpenAlex and arXiv APIs directly for broad scholarly candidate discovery.
2. Use the official Brave Search API for broad web discovery when configured.
   It returns structured web results from Brave's index, requires an API key,
   and avoids scraping Brave's interactive result page.
3. Optionally support Tavily as an agent-oriented web-search provider. It
   returns ranked URLs and extracted content through a keyed API.
4. Treat a self-hosted SearXNG JSON endpoint as an advanced, user-operated
   option. Its API is easy to consume, but its upstream engines can still be
   challenged, so it is not equivalent to owning a search index.
5. Keep headless search-page scraping only as a best-effort fallback. A
   challenge must be reported as `challenged`, never as zero scholarly results.

There is no dependable, keyless public search-engine webpage specifically
designed for unattended headless scraping. A self-hosted SearXNG instance is
the closest operational option, but reliability still depends on its configured
upstream engines. For i0i's default path, direct scholarly APIs plus one official
web-search API offer the strongest result quality for the least runtime
complexity.

### Reading

The agent called `reader_read` for *Tracing Attention Computation Through
Feature Interactions* (`web:0c515b610751`). The exact MCP failure was:

```text
Could not read document evidence:
UNIQUE constraint failed: document_pages.id
```

The paper has a valid saved abstract and an existing abstract extraction. It was
materialized under an older chunk-ID convention. The current materialization
function checks only for the newer expected chunk ID; it therefore mistakes the
legacy extraction for missing data and tries to insert its already-existing
page again.

Nine papers currently have this legacy shape and can trigger the same collision.
This is an idempotency/migration bug in metadata-abstract evidence creation, not
a publisher-access or HTML-rendering failure.

### Terminal Status

The Run is `ready` because the managed agent returned a structurally valid final
answer and finalization completed. Today, `ready` describes orchestration
completion, not evidence success.

For this case, `ready` is misleading: every child provider query failed, the
only Reader call failed, and no candidate or passage was obtained. Until a more
specific `completed_with_errors` state exists, a Run with this evidence profile
should be marked failed while preserving its useful diagnostic summary.

## Recommended Follow-Ups

1. Make metadata-abstract materialization idempotent by recognizing existing
   extraction content independently of historical row-ID conventions, with a
   regression fixture for the legacy shape.
2. Recognize Brave's bot-verification page and surface it as a challenge rather
   than an empty scholarly result.
3. Let browser-first Search use a bounded OpenAlex/arXiv fallback when every
   browser query is challenged before producing a provisional link.
4. Ensure managed Obscura processes terminate with their owning i0i process.
5. Distinguish a legitimate evidence-free conclusion from infrastructure-empty
   completion when assigning the parent Run's terminal status.
6. Persist the concrete MCP error in Research Activity so this diagnosis does
   not require mining Codex's local diagnostic database.

## Sources

- [Obscura: configure stealth and proxies](https://github.com/h4ckf0r0day/obscura/wiki/Configure-stealth-and-proxies)
- [Obscura README](https://github.com/h4ckf0r0day/obscura)
- [Brave Search API reference](https://api-dashboard.search.brave.com/api-reference/web/search/post)
- [Brave Search API pricing](https://api-dashboard.search.brave.com/app/plans)
- [Tavily Search API](https://docs.tavily.com/documentation/api-reference/endpoint/search)
- [SearXNG Search API](https://github.com/searxng/searxng/blob/master/docs/dev/search_api.rst)
