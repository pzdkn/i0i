# RFC 0020: Scout Discovery Model

Status: Draft  
Date: 2026-05-29  
Product: i0i  
Target: Tauri v2 + Svelte, macOS first

## Summary

Introduce **Scout** as i0i's durable discovery object.

A Scout is not just a search query. It is a reusable research intent with constraints, seed papers, provider choices, optional schedule, and an execution strategy. A Scout can be run manually as a classical filtered search, or later run in an agentic mode where an agent repeatedly queries paper APIs until it reaches explicit stopping criteria.

The central model is:

```text
Scout
  -> Scout Run
  -> Provider Queries
  -> Paper Candidates
  -> Run Report
```

This keeps user intent separate from provider-specific API calls.

## Context

i0i needs discovery because a knowledge-curation IDE cannot begin only from papers the user already has. Users need to find candidate papers, evaluate them, and decide what should become part of their durable local library.

Earlier frontend RFCs mocked a Discover view and candidate feed. This RFC defines the longer-lived product and architecture model for real discovery.

## Product Decision

Call the saved discovery object a **Scout**.

Scout is the user-facing term for:

```text
research intent + search configuration + seeds + strategy + schedule
```

Scout should feel like a research scout the user can send into literature databases, inspect when it returns, and rerun later.

## Current Consensus

The first real discovery implementation should use **OpenAlex** as the initial provider.

The initial Scout implementation should be transient:

```text
Open Discover
  -> enter search query and filters
  -> click Run
  -> show OpenAlex candidates in memory
  -> only write to SQLite when the user explicitly adds/imports a candidate
```

Do not persist Scout definitions, Scout Runs, or Scout Candidates in v0.1.

The Discover candidate list should start empty. Mock Discover candidates should not be mixed into the live Scout flow.

The initial Scout UI should expose a small set of controls:

- `+` button for a new empty Discover search tab
- one freeform search query
- year range
- result limit
- sort: relevance, newest, or most cited
- open-access only
- explicit Run button

The query string should be treated as a classical provider search query. v0.1 should not interpret natural-language instructions such as "after 2023" or "prefer ML venues" from the freeform text. The user should set filters explicitly.

Default controls:

```text
query: empty
yearFrom: empty
yearTo: empty
resultLimit: 25
resultLimit options: 10 / 25 / 50
sortBy: relevance
sortBy options: relevance / newest / most cited
openAccessOnly: true
provider: OpenAlex
```

The larger Scout model can still include venue, author, citations, seed papers, similarity, references, citations, schedule, and agentic strategy. Those should not all appear in the first UI.

OpenAlex should use an API key in development and normal app use:

```text
OPENALEX_API_KEY
```

The key should be read by the Rust backend, not exposed to Svelte components. During local development the key can live in `.env`; later it can move into app settings or a secure settings store.

Discovery should store normalized search candidates and run metadata. It should not automatically download or cache PDFs for every search result. PDF files should only be downloaded when the user explicitly previews, saves, or imports a candidate.

Existing Vault/library seed data may stay for now. No database wipe is needed for this feature because Discover candidates are transient until explicitly added to a Vault.

The left Vault Explorer should remain visible and global in v0.1. Scout/search controls belong inside the Discover workspace, not in the left sidebar.

Run behavior:

```text
Run in current Discover tab
  -> replace that tab's current candidate results
```

New search behavior:

```text
Click + in Discover search header
  -> open a new empty Discover tab
  -> focus the query input
```

Each Discover tab owns its own transient query, filters, run status, and candidate list. Multiple Discover tabs can exist at the same time, but none of them are persisted across app reloads.

## Goals

- Define the Scout object model.
- Support both classical filtered search and future agentic search.
- Keep provider-specific query syntax out of the UI.
- Preserve an audit trail for every Scout run.
- Make search configurable without turning the first UI into an advanced-search cockpit.
- Keep discovery distinct from ingestion.
- Prepare for multiple providers such as OpenAlex, arXiv, Semantic Scholar, Crossref, and PubMed.
- Make the v0.1 implementation small while keeping the model extensible.

## Non-Goals

- No full agent implementation in the first discovery slice.
- No scheduled background execution in v0.1.
- No guarantee that every provider supports every filter.
- No perfect cross-provider deduplication in v0.1.
- No automatic import into the local library.
- No local embedding index in the first version.
- No Google Scholar scraping.
- No visual workflow builder.

## Terminology

### Scout

A saved discovery brief.

Example:

```text
Find recent papers about sparse autoencoders for mechanistic interpretability.
Prefer 2023 onward, ML venues, papers similar to these seed papers, and include
highly cited related work even if older.
```

### Scout Run

One execution of a Scout at a point in time.

Runs should be durable because the user may want to know:

- what was searched
- which providers were used
- which candidates were found
- which candidates were saved or dismissed
- why the run stopped

### Provider Query

A concrete query sent to one external provider.

Examples:

```text
OpenAlex works search with query + year filter
arXiv Atom query with category + submitted date sort
Semantic Scholar bulk search with fields + year + min citation count
```

### Paper Candidate

A search result that is not yet a local paper.

Discovery produces candidates. Ingestion turns selected candidates into durable local papers.

## Scout Shape

Conceptual shape:

```ts
type Scout = {
  id: string;
  title: string;
  description: string;

  keywords: {
    include: string[];
    exclude: string[];
    phrases: string[];
  };

  filters: {
    yearFrom?: number;
    yearTo?: number;
    venues?: string[];
    authors?: string[];
    minCitations?: number;
    openAccessOnly?: boolean;
    fieldsOfStudy?: string[];
  };

  seeds: {
    paperIds: string[];
    useSimilarPapers: boolean;
    useReferences: boolean;
    useCitations: boolean;
  };

  providers: SearchProviderId[];

  strategy: {
    mode: "manual" | "hybrid" | "agentic";
    sortBy: "relevance" | "newest" | "most_cited";
    maxProviderQueries: number;
    maxIterations: number;
    targetCandidateCount: number;
    noveltyThreshold?: number;
  };

  schedule?: {
    enabled: boolean;
    frequency: "daily" | "weekly" | "monthly";
    limitPerRun: number;
  };

  createdAt: string;
  updatedAt: string;
};
```

This is not a final TypeScript or Rust contract. It is the product-level shape the implementation should preserve.

## Example Scout

```json
{
  "title": "Sparse autoencoders for mechanistic interpretability",
  "description": "Find recent papers about sparse autoencoders and related representation analysis methods in mechanistic interpretability. Prefer papers that connect to transformer circuits or feature geometry.",
  "keywords": {
    "include": ["sparse autoencoder", "mechanistic interpretability"],
    "exclude": ["survey only"],
    "phrases": ["sparse autoencoders", "dictionary learning"]
  },
  "filters": {
    "yearFrom": 2023,
    "venues": ["NeurIPS", "ICLR", "ICML", "arXiv"],
    "minCitations": 0,
    "openAccessOnly": false,
    "fieldsOfStudy": ["Computer Science"]
  },
  "seeds": {
    "paperIds": ["paper:local:abc123", "paper:local:def456"],
    "useSimilarPapers": true,
    "useReferences": true,
    "useCitations": true
  },
  "providers": ["openalex", "arxiv", "semantic_scholar"],
  "strategy": {
    "mode": "hybrid",
    "sortBy": "relevance",
    "maxProviderQueries": 12,
    "maxIterations": 4,
    "targetCandidateCount": 25
  },
  "schedule": {
    "enabled": true,
    "frequency": "weekly",
    "limitPerRun": 25
  }
}
```

## Search Modes

### Manual

Manual mode runs one set of deterministic provider queries.

Useful for:

- exact keyword search
- year filters
- author filters
- venue filters
- fast reproducible search

Manual mode should be the first implementation target.

### Hybrid

Hybrid mode lets the user provide normal search constraints while an agent helps expand or refine the provider queries.

The agent may:

- generate synonyms
- split broad descriptions into provider-specific queries
- search similar papers from seed papers
- follow references and citations from seeds
- run a second query based on weak first-pass results
- rank and explain candidates

The user should still see what happened.

### Agentic

Agentic mode gives the agent more control over iteration.

The agent may repeatedly:

```text
plan query
  -> call provider
  -> normalize candidates
  -> inspect results
  -> decide whether coverage is good enough
  -> refine query or stop
```

Agentic mode should always have explicit budgets and stopping criteria.

## Agent Stopping Criteria

The agent should never run until it is vaguely "satisfied."

It should stop when one or more of these is true:

- max iterations reached
- max provider queries reached
- target candidate count reached
- new queries stop producing novel candidates
- enough high-confidence candidates were found
- provider rate limits prevent useful continuation
- user cancels the run

Every Scout Run should record why it stopped.

## Classical Filters Vs Agentic Expansion

Some configuration is a hard constraint:

```text
yearFrom
yearTo
providers
openAccessOnly
minCitations
```

Some configuration is guidance:

```text
description
preferred venues
seed similarity
topics
general quality bar
```

Important rule:

```text
The agent may expand guidance, but it should not silently violate hard constraints.
```

If the agent wants to include an older highly cited paper outside the year range, it should mark that candidate as an exception with a reason.

## Provider Capability Model

Not every provider supports the same knobs.

The app should model provider capabilities instead of pretending all filters are universal.

Conceptual shape:

```ts
type SearchProviderCapabilities = {
  id: SearchProviderId;
  supportsYearRange: boolean;
  supportsVenueFilter: boolean;
  supportsAuthorFilter: boolean;
  supportsMinCitations: boolean;
  supportsOpenAccessFilter: boolean;
  supportsFieldOfStudy: boolean;
  supportsSimilarPapers: boolean;
  supportsReferences: boolean;
  supportsCitations: boolean;
  supportsSortByNewest: boolean;
  supportsSortByRelevance: boolean;
};
```

Initial provider expectations:

| Provider | Best Use | Notes |
| --- | --- | --- |
| OpenAlex | broad default discovery | good general index and metadata |
| arXiv | preprints, ML/CS/math/physics | strong for recent open preprints |
| Semantic Scholar | citations, recommendations, open PDF metadata | good for seed expansion and related work |
| Crossref | DOI and publisher metadata | better for ingestion than discovery |
| PubMed | biomedical literature | important for life science users |

Provider references:

- OpenAlex search: https://developers.openalex.org/guides/searching
- OpenAlex authentication/pricing: https://developers.openalex.org/guides/authentication
- arXiv API manual: https://info.arxiv.org/help/api/user-manual.html
- Semantic Scholar API tutorial: https://www.semanticscholar.org/product/api/tutorial
- Crossref REST API access: https://www.crossref.org/documentation/retrieve-metadata/rest-api/access-and-authentication/
- NLM E-utilities: https://www.nlm.nih.gov/dataguide/eutilities/utilities.html

### OpenAlex v0.1 Capability Decision

OpenAlex is the first provider.

It supports enough of the v0.1 Scout parameters:

| i0i Parameter | OpenAlex Support | v0.1 Decision |
| --- | --- | --- |
| description / keywords | yes, via `search` or `search.semantic` | use `search` first |
| included keywords | yes, via normal/Boolean search | supported |
| excluded keywords | yes, via `NOT` in Boolean search | supported after basic search works |
| exact phrases | yes, quoted phrase search | supported |
| year range | yes, publication year/date filters | supported |
| result limit | yes, `per_page` / paging | supported |
| sort by relevance | yes, relevance score with search | supported |
| sort by newest | yes, publication date/year sort | supported |
| sort by most cited | yes, citation count sort | supported |
| open-access only | yes, open-access filters | supported |
| PDF availability | yes-ish, OA/PDF URL fields | store URL only |
| venue | yes, best via source IDs | later, needs source-name resolution |
| author | yes, best via author IDs | later, needs author-name resolution |
| fields/topics | yes, via OpenAlex topic hierarchy | later, needs topic selection/resolution |
| seed similarity | partial, provider-specific | later; likely stronger with Semantic Scholar |
| references/citations from seeds | possible, provider-specific | later |
| schedule | no, app-owned | later |
| agent loop | no, app-owned | later |
| novelty threshold | no, app-owned | later |

This means v0.1 should treat unsupported or weak parameters as future Scout configuration, not as hidden OpenAlex magic.

### API Key Policy

Use an OpenAlex API key for the first real provider integration.

Development:

```text
OPENALEX_API_KEY=<developer key>
```

Rules:

- read `OPENALEX_API_KEY` in Rust
- never expose the key to Svelte
- never commit `.env`
- fail with a clear setup error if real OpenAlex search is run without a key
- later, support app settings for non-developer users

The app may eventually support unauthenticated or degraded provider modes, but v0.1 should optimize for a configured key.

## Provider Query Compilation

A Scout should compile into provider-specific queries.

Example:

```text
Scout query + filters
  -> OpenAlex query/filter params
  -> arXiv search_query params
  -> Semantic Scholar bulk search params
```

Compilation should be explicit and inspectable.

The user does not need to edit raw provider syntax in v0.1, but the run report should show enough detail to debug surprising results.

## Paper Candidate Shape

Conceptual normalized candidate:

```ts
type PaperCandidate = {
  id: string;
  sourceProvider: SearchProviderId;
  sourceId: string;

  title: string;
  authors: string[];
  abstract?: string;
  year?: number;
  publicationDate?: string;
  venue?: string;
  citationCount?: number;

  doi?: string;
  openalexId?: string;
  arxivId?: string;
  semanticScholarId?: string;
  externalUrl?: string;
  pdfUrl?: string;
  openAccess?: {
    isOpenAccess: boolean;
    status?: string;
  };

  match: {
    score?: number;
    reasons: string[];
    matchedKeywords: string[];
    fromSeedPaperIds: string[];
  };

  alreadyInLibrary: boolean;
};
```

The `match` field is important. Discovery should explain why a paper appeared.

## Search Result Storage

Store enough normalized data to show and audit a Scout Run without depending on a provider response staying available forever.

In v0.1, this storage is session-local UI/run state. Later, persisted Scouts can move this into SQLite.

For each candidate, store:

- source provider
- provider work ID
- title
- authors
- abstract, when available
- publication year/date
- venue/source name
- citation count
- DOI
- provider IDs such as OpenAlex ID, arXiv ID, or Semantic Scholar ID
- landing page URL
- PDF URL, when known
- open-access status
- match score, when provided
- match reasons generated by i0i
- matched keywords
- already-in-library flag at run time

For each run, store:

- Scout ID, if the Scout is persisted
- query text / description
- filters used
- provider queries made
- provider query status and errors
- result counts
- start and finish timestamps
- stop reason

Raw provider responses are not part of the product contract. They may be useful behind a debug flag during adapter development, but the app should depend on normalized candidates.

## Scout Run Shape

Conceptual run:

```ts
type ScoutRun = {
  id: string;
  scoutId: string;
  startedAt: string;
  finishedAt?: string;
  status: "running" | "completed" | "failed" | "cancelled";

  strategyMode: "manual" | "hybrid" | "agentic";
  providerQueries: ProviderQueryLog[];
  candidates: PaperCandidate[];

  summary?: {
    totalCandidates: number;
    newCandidates: number;
    duplicateCandidates: number;
    savedCandidates: number;
    dismissedCandidates: number;
    stopReason: string;
  };

  error?: string;
};
```

Provider query log:

```ts
type ProviderQueryLog = {
  id: string;
  provider: SearchProviderId;
  queryText: string;
  filters: Record<string, unknown>;
  startedAt: string;
  finishedAt?: string;
  status: "completed" | "failed" | "rate_limited";
  resultCount: number;
  error?: string;
};
```

## Proposed User Experience

Scout workspace layout:

```text
left: global Vault Explorer
top of workspace: search header
center: candidate results
right: run status / candidate detail
```

Configuration should start simple:

- `+` new search tab button
- freeform search query
- year range
- result limit
- sort
- open-access only
- run button

Search header:

```text
Discover
[ + ] [ query input                                      ] [ Run ]
[ year from ] [ year to ] [ limit 25 ] [ relevance ] [x] open access
```

Clicking `Run` should always run the search in the current Discover tab and replace that tab's previous results.

Clicking `+` should open a fresh Discover tab with default controls and an empty candidate list.

Advanced configuration can be progressive:

- saved Scout definitions
- keyword chips
- venues
- authors
- citation count
- seed papers
- similarity/reference/citation expansion
- schedule
- strategy mode
- agent budgets

Candidate cards should answer:

- what is this paper?
- where did it come from?
- why did it match?
- is it already in my library?
- can I import/save it?

Discover tab title behavior:

```text
before first run: Discover: Untitled
after run: Discover: <truncated query>
```

## Discovery And Ingestion Boundary

Discovery produces candidates:

```text
PaperCandidate
```

Ingestion creates durable local papers:

```text
PaperCandidate
  -> import/save
  -> LocalPaper
  -> Vault membership
  -> PDF asset when available
```

Scout must not silently create local library papers unless the user explicitly asks it to save/import.

## PDF Storage And Caching

Discovery should not automatically download or cache PDFs for all search results.

During discovery:

```text
store PDF URL / open-access metadata only
```

On explicit preview:

```text
download PDF into a temporary cache
evict by size or age
do not treat it as a durable library asset
```

On explicit save/import:

```text
download or attach PDF as a durable local paper asset
connect it to the imported LocalPaper
make it available to the Reader and extraction pipeline
```

This boundary matters because automatic PDF downloads can waste disk, hit provider limits, slow down search, and create licensing ambiguity. Discovery should stay lightweight; ingestion owns durable assets.

## v0.1 Implementation Slice

The first real Scout implementation should be intentionally small:

- create one transient/manual Scout search form in the UI
- keep the left Vault Explorer visible
- place Scout search controls in the Discover workspace header
- include a `+` control that creates a fresh empty Discover tab
- support multiple transient Discover tabs in one session
- start each Discover tab with an empty candidate list
- support one freeform classical search query
- support year range
- support result limit with options 10, 25, and 50
- support sort by relevance, newest, or most cited
- default result limit to 25
- default sort to relevance
- support open-access only, defaulted on
- use OpenAlex as the only real provider at first
- read `OPENALEX_API_KEY` in Rust
- run OpenAlex search
- normalize candidates
- show candidates in the Discover workspace
- show basic run metadata
- replace the current tab's candidate results on each Run
- update the Discover tab title from the query after a run
- store normalized results in current UI/run state
- do not persist Scout definitions, Scout Runs, or Scout Candidates
- no automatic PDF download/cache
- no agent loop yet
- no schedule yet

Recommended first provider:

```text
OpenAlex
```

Recommended second provider:

```text
arXiv
```

Recommended third provider:

```text
Semantic Scholar
```

## Future Implementation Slices

1. Persist Scout definitions in SQLite.
2. Persist Scout Runs and provider query logs.
3. Add arXiv provider adapter.
4. Add Semantic Scholar provider adapter.
5. Add seed paper expansion.
6. Add duplicate detection across providers.
7. Add save/import flow from candidate to local paper.
8. Add scheduled Scout runs.
9. Add hybrid agent query planning.
10. Add agentic run reports with explanations.

## Architecture Direction

Long-term shape:

```text
Svelte Scout UI
  -> frontend bridge
  -> Tauri command
  -> Rust discovery service
  -> SearchProvider adapters
  -> normalized PaperCandidate[]
  -> SQLite Scout/Run persistence
```

Rust should own provider calls and normalization because:

- network behavior is part of the app backend
- API keys and provider configuration should not leak into UI components
- provider response parsing needs tests
- the frontend should consume one normalized candidate model

## Files Likely Affected Later

Documentation now:

- `docs/rfcs/discovery/0020-scout-discovery-model.md`

Future frontend:

- `src/lib/domain/discovery.ts`
- `src/lib/bridge/discovery.ts`
- `src/lib/features/discover/`

Future Rust:

- `src-tauri/src/commands/discovery.rs`
- `src-tauri/src/discovery/`
- `src-tauri/src/discovery/providers/`
- `src-tauri/src/storage/discovery_store.rs`

Future database:

- `scouts`
- `scout_runs`
- `scout_provider_queries`
- `scout_candidates`

## Risks

- The model can become too complex before real search works.
- Provider APIs have different limits, fields, and reliability profiles.
- Agentic search can feel magical or untrustworthy without an audit trail.
- Seed-paper similarity requires either provider support or a local embedding/index strategy.
- Scheduled runs can create background work, rate-limit issues, and notification noise.
- Too many filters in the first UI can slow learning and implementation.

## Validation Plan

For this RFC:

- Review the markdown diff.
- Confirm that Scout cleanly separates intent, run, provider queries, and candidates.

For the first implementation slice:

```bash
cargo fmt --check
cargo test
pnpm check
pnpm build
```

Manual validation:

- open Discover and see an empty candidate list before the first run
- confirm the left Vault Explorer remains visible
- click `+` in the Discover search header and see a new empty Discover tab
- enter a freeform search query
- confirm typing does not auto-run search
- set year range and result limit
- set sort and open-access-only controls
- confirm result limit defaults to 25
- confirm open-access-only defaults to on
- click Run
- see normalized candidates
- run a second query in the same tab and confirm results are replaced
- confirm another Discover tab keeps its own query/results
- confirm the tab title updates from the query after a run
- inspect why each candidate matched
- confirm no local paper is created until explicit save/import
- confirm no PDF is downloaded until explicit preview/save/import

## Open Questions

- Should the first UI call the object `Scout`, `Discovery Scout`, or just `Scout` everywhere?
- Should seed papers be local-paper only, or can unsaved external candidates be seeds?
- Should scheduled Scouts run only while the desktop app is open?
- Should provider API keys live in app settings, environment variables during development, or both? Current v0.1 decision: `OPENALEX_API_KEY` in development, app settings later.
- Should v0.1 store raw provider responses for debugging, or only normalized candidates? Current decision: normalized candidates by default, optional debug-only raw capture later.
- Should empty year filters mean all years, or should v0.1 default to recent papers?
