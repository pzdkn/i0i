# RFC 0098: Browser-First Scholarly Discovery

Status: Open
Date: 2026-08-24
Product: i0i
Target: Tauri v2 + SvelteKit (Svelte 5), macOS first
Builds on: RFC 0052 (reliable Obscura sessions), RFC 0054 (semantic reranking),
RFC 0057 (deep-search ranking), RFC 0088 (Deep Research harness)
Supersedes for candidate generation: RFC 0043 and RFC 0053 provider fan-out

## Summary

OpenAlex, arXiv, Europe PMC, and CORE are useful structured catalogs, but broad
provider queries repeatedly produce blunt candidate sets. Make Obscura-backed
web discovery the default candidate source for both Quick and Deep Search.

Obscura is a browser runtime, not a search index. i0i must still choose a web
search entry point, parse result pages, and establish paper identity. Provider
APIs therefore remain, but only as metadata resolvers for DOI, arXiv id, other
recognized identifiers, or a verified exact title.

```mermaid
flowchart LR
  Q[Query] --> B[Obscura web search]
  B --> C[Provisional candidates]
  C --> R[Exact metadata resolution]
  R --> D[Deduplicate and filter]
  D --> S[Local semantic ranking]
  S --> U[Discover results]
```

## Decision

### 1. Separate discovery from resolution

Use two explicit responsibilities:

```text
CandidateSource.discover(query, constraints) -> candidate stream
MetadataResolver.resolve(identifier | exact title) -> normalized paper
```

The browser implements candidate discovery. OpenAlex and arXiv initially
implement exact metadata resolution. Existing normalized `PaperCandidate`,
deduplication, filtering, and local reranking remain shared.

No production path uses broad provider search as a fallback. That would retain
two candidate-generation systems and make relevance impossible to reason about.

### 2. Quick Search uses one bounded browser lane

Quick Search:

1. Sends the literal user query to one configured scholarly web-search entry
   point through the persistent Obscura session.
2. Reads one or two result pages within a strict time budget.
3. Emits provisional title, URL, and snippet candidates immediately.
4. Resolves identity and metadata concurrently.
5. Deduplicates, filters, and semantically reranks as metadata arrives.

The list may reorder when verified metadata improves ranking, but stable paper
identity prevents duplicate rows and gratuitous movement.

### 3. Deep Search fans out browser queries

The existing Rust-controlled Deep Research loop remains. Its planner creates a
bounded set of distinct queries; browser lanes execute concurrently; every
candidate enters one shared pool. Reflection decides whether another bounded
round is worthwhile.

One failed lane does not fail the run. Budgets cap browser pages, elapsed time,
and metadata resolutions as well as LLM/provider calls.

### 4. Resolve identity in evidence order

Resolution order:

1. DOI found in the URL or page metadata.
2. arXiv identifier.
3. Another supported scholarly identifier.
4. Quoted exact-title lookup.
5. Honest unresolved web candidate with partial metadata.

Exact-title results are accepted only when normalized titles match within a
documented threshold. Provider result order is not identity verification.

Resolved candidates retain available DOI, provider ids, authors, year, venue,
abstract, URLs, and provenance. Filters such as year, venue, author, citations,
and open access are applied after resolution because a search-result page
cannot supply them reliably.

### 5. Use the persistent Obscura session

RFC 0052 is a prerequisite. Discovery connects to the managed Obscura server
and reuses a browser context. It must not launch several independent
`obscura fetch` processes per result page.

The search entry point and parser are configuration-backed and replaceable.
Browser transport details stay behind `CandidateSource`.

### 6. Stream useful, transport-neutral progress

The inspector reports:

```text
Planning queries
Searching the web
Found 18 links
Resolving paper metadata
Merged 12 papers
Ranking results
Checking coverage
```

These stages work for Quick and Deep Search and do not expose provider fan-out
as the user's mental model.

### 7. Gate the default with the RFC 0088 corpus

Browser-first becomes the default only after a fixed-corpus comparison against
the current API-first baseline. Record:

- recall of hand-marked papers;
- precision of the first result page;
- time to first provisional result;
- time to stable ranked results;
- proportion resolved to a scholarly identity;
- browser failures and challenges;
- network/model calls spent.

The architectural decision is browser-first. The benchmark is the migration
gate and the basis for tuning budgets, not an invitation to retain broad API
search indefinitely.

## RFC Consolidation

- RFC 0052 owns one reliable, session-backed browser and readable-page
  acquisition.
- RFC 0088 keeps its bounded loop, reflection, evaluation corpus, and ranking;
  its unfinished generic browsing task is replaced by this candidate source.
- RFC 0054's local semantic reranker remains. Query expansion moves to browser
  query planning.
- RFCs 0043 and 0053 remain implemented history. Their broad provider fan-out
  is superseded once this RFC passes the migration gate.

## Non-Goals

- Using Obscura as though it were a search index.
- Scraping publisher pages before a candidate is selected for reading.
- Maintaining API-first and browser-first ranking pipelines indefinitely.
- Replacing local semantic ranking with browser result order.
- Pretending unresolved web results have verified scholarly metadata.
- Bypassing authentication or paywalls.

## Risks

- Search-result DOMs change and require maintained parsers.
- Browser search is slower and more failure-prone than structured APIs.
- Search engines may challenge or throttle automation.
- Web results include SEO pages, mirrors, citations, and non-paper content.
- Post-resolution filters may remove many provisional candidates and reorder
  the list.

## Acceptance Criteria

- [ ] Quick and Deep Search obtain initial candidates through Obscura.
- [ ] Broad provider API searches are absent from production discovery paths.
- [ ] APIs are used only for identifier or verified exact-title resolution.
- [ ] Provisional candidates appear before metadata resolution completes.
- [ ] Verified metadata and provenance update candidate rows in place.
- [ ] Existing deduplication and local semantic ranking remain shared.
- [ ] Browser, resolver, and ranking progress appears in the inspector.
- [ ] One failed browser lane does not fail a Deep Search run.
- [ ] Discovery uses the managed persistent Obscura session.
- [ ] The fixed corpus records acceptable recall, precision, resolution rate,
  and latency before browser-first becomes the default.

