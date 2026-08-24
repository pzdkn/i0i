# RFC 0037: Deep-Research Agentic Search

Status: Stale
Date: 2026-06-28
Product: i0i
Target: Tauri v2 + Svelte, macOS first

## Summary

Turn Discover from a one-shot keyword box into a **deep-research agent**. The user writes
a natural-language **goal** ("find recent explainable-AI papers relevant to my
interpretability work, methods not surveys") plus a few structured constraints; a
Rust-orchestrated agent loop composes a small set of **typed research primitives**
(`plan_queries → search → filter → dedup → diff → assess → refine → rank`) to plan
provider queries, search, dedup, iterate to fill coverage gaps, then rank the results
against the goal with a one-line rationale per paper. The result is a **saved search**: a
durable, inspectable artifact whose candidate pool **stacks** over time. A saved search
can be put on a recurring schedule so new matching literature flows in on its own — each
run appends only the *new* papers and flags them.

This realizes the vision's "Configurable Search Agent" and "Source scout / briefer"
([`future-vision.md`](../../overview/future-vision.md)) and the agentic + scheduled
modes sketched in [RFC 0020](0020-scout-discovery-model.md), built on the provider,
LLM, background-job, and persistence machinery that already exists in the codebase.

**Out of scope here (deliberately):** the *briefer* — a written synthesis report over a
run — is reserved as a future step layered over a stored search (see Future Work). This
RFC delivers ranked candidates with rationales, not prose.

## Decisions

These are settled for this RFC. Rationale follows in the Design section.

1. **The durable unit is a saved Search; runs stack onto it.** "Run deep research"
   creates (or reuses) a `Search` and starts one run. A manual re-run and a scheduled run
   are the same path: each run **dedups against the existing pool and appends only the new
   candidates** ("stacking"). There is no separate ad-hoc code path.
2. **Rust orchestrates a bounded loop over typed primitives; the LLM does not pick
   control flow.** Rust drives `plan → search → filter → dedup → diff → assess →
   refine → rank`. The loop is bounded by `max_iterations` (set by the depth preset); the
   LLM, via `assess`, may **break early** when coverage is sufficient. The planner LLM is
   called *inside* primitives and only parameterizes them. This is **not** LLM-native
   tool-calling — chosen for inspectability, testability, and hard budget control, all of
   which the journey docs make non-negotiable.
3. **The research domain is a vocabulary of typed primitives, each behind a seam.**
   Primitives are pure, LLM-backed, or provider-backed (see "The research primitives").
   Real impls call OpenRouter / providers / the system clock; fakes return canned data.
   `spawn_agent` is **named in the vocabulary but not implemented** in this RFC — it is the
   one item that is an explicit non-goal (multi-agent recursion).
4. **Background execution mirrors `PdfExtractionManager`.** A `SearchManager` owns a
   queue, an `async_runtime::spawn` worker, a persisted status lifecycle, `search_updated`
   events, and startup recovery. Scheduling is a `tokio::time::interval` ticker that
   enqueues due Searches.
5. **Candidates are never invented.** They only ever come from real provider results. The
   LLM expands queries, ranks, and explains; it does not fabricate papers or metadata.
6. **Ranking is goal-relative now, profile-relative later.** This RFC ranks against the
   goal and hard constraints. The learning-profile moat (rank by marginal value to *you*)
   plugs into the same `rank` primitive in a later RFC; it is an explicit non-goal here.

## Context

Today (after RFCs 0020/0030/0031) Discover is: one freeform query → pick one provider
from a dropdown → year/sort/limit/OA → a transient candidate feed
(`searchPapers` → `DiscoveryService::new(provider).search(request)` → normalized
`PaperCandidate[]`). Nothing persists; there is no cross-query reasoning, no ranking
beyond the provider's own order, and no scheduled inflow.

The pieces the agent needs already exist and are well isolated:

- **Providers** sit behind a clean trait and are callable programmatically:
  `DiscoveryService::new(provider).search(request).await` (provider is owned/cloneable;
  `search` is async and takes `&DiscoverySearchRequest`). See
  `src-tauri/src/commands/discovery/{provider.rs,service.rs}`.
- **LLM access** is OpenRouter via `openrouter::complete` (non-streaming) and
  `complete_streamed`, with `CompletionRequest { model, messages, stream, max_tokens }`
  and `WireMessage { role, content }`. Model + env-var API key come from a config block
  loaded once at startup; the `title_model` field shows the pattern for a second, cheaper
  model. See `src-tauri/src/services/chat/{openrouter.rs,config.rs}`.
- **Background jobs** have a proven shape: `PdfExtractionManager` /
  `PdfDownloadManager` use `queued_or_active: Arc<Mutex<HashSet<String>>>`, a worker via
  `tauri::async_runtime::spawn`, SQLite-persisted statuses, `self.app.emit(...)` events,
  and `recover_and_queue_startup_*` recovery wired in `lib.rs::setup`. See
  `src-tauri/src/{pdf_ingestion.rs,pdf_extraction.rs,lib.rs}`.
- **Persistence** is a cloneable `LibraryStore { db_path }` with one-connection-per-call,
  `create table if not exists` schema, self-guarding migrations, `add_column_if_missing`,
  and `timestamped_id`. See `src-tauri/src/storage/library_store.rs`.
- **Frontend** has a Discover workspace/tab model in `library-cache.svelte.ts`, a bridge
  convention wrapping `invoke`, a streaming idiom (`streamAsk` over a Tauri `Channel`),
  a `listen(...)`-in-`onMount` event idiom, and an add-to-vault path
  (`paperDraftFromDiscoverCandidate` → `addPaperToVaults`).

## Goals

- A natural-language goal plus structured constraints produces a ranked, deduped
  candidate set, with a rationale and provenance for every candidate.
- The search is a durable, inspectable artifact: what was searched, which queries ran,
  why the run stopped, why each paper surfaced. Re-runs stack new papers onto it.
- The agent loop is bounded by hard budgets and explicit stopping criteria, can stop
  early when the LLM judges coverage sufficient, is cancellable mid-run, and is
  unit-testable without network or real time.
- Saved Searches can run on a recurring schedule while the app is open, with bounded
  startup catch-up; scheduled runs stack new papers and flag them as unread.
- Maximum reuse of existing provider, LLM, background-job, persistence, and Discover-UI
  machinery; targeted, justified refactors only.

## Non-Goals

- **The briefer (written synthesis).** Reserved as a future step over a stored search
  (Future Work). This RFC ships ranked candidates + per-candidate rationale, not prose.
- **Profile-relative ranking** (the learning-profile moat). Designed for, not built here.
- **`spawn_agent` / multi-agent recursion.** Named in the primitive vocabulary as a
  reserved seam; not implemented. The classic scope-creep trap the journey docs warn of.
- **LLM-native tool-calling.** Rust owns control flow. A future option, noted below.
- **Cross-provider entity resolution.** This RFC dedups *within the pool* by exact DOI /
  arXiv id / normalized title (the same paper from two queries / two runs). It does not
  merge "the same paper across providers," an SLC v0.1.0 non-goal.
- **Headless browser / open-web scraping.** No Playwright/Puppeteer, no general
  web-search/fetch, no Google Scholar scraping. See "Web navigation, browsing, and full
  text."
- **Automatic PDF download or import.** Discovery stays lightweight; candidates carry PDF
  URLs only, and ingestion happens on explicit add-to-vault (unchanged).
- **A full activity-log subsystem.** This RFC emits a couple of run-level activity
  signals as a hook; the durable activity-log read-model is a separate RFC.
- **Background execution while the app is closed.** Desktop scheduling fires only while
  i0i runs; see the honest discussion under Scheduling.

## Why an agent beats one good keyword search

- **Query/synonym expansion** catches papers a single phrasing misses ("sparse
  autoencoder" vs "dictionary learning").
- **Iteration** fills coverage gaps: a weak first pass triggers refined queries instead
  of returning thin results.
- **Ranking is against the goal**, not citation count or raw provider order — the agent
  judges fit to the stated goal and constraints, reading each candidate's abstract.
- **Stacking** turns a search into a living artifact: re-run it (manually or on a
  schedule) and only the genuinely new papers arrive, flagged as new.

What it is *not*, yet: ranking is **goal-relative**, not **profile-relative**. It does
not know what you already understand. That is the moat (`learning-journeys/README.md`)
and it attaches to the `rank` primitive later (see Future Work).

## The research primitives

This is the core of the design: a small, typed vocabulary for the domain of
research-paper finding. Each primitive is a **seam** with a clear signature, classified by
how it is backed — **pure** (no I/O), **LLM** (OpenRouter), or **provider** (discovery
APIs). The Rust loop composes them; the live progress timeline is literally the trace of
these calls (see Progress reporting).

| Primitive | Kind | Signature (sketch) | Notes |
|---|---|---|---|
| `plan_queries` | LLM | `(goal, state) → Vec<Query>` | first-pass query expansion from the goal + seeds |
| `refine_queries` | LLM | `(goal, state, gaps) → Vec<Query>` | gap-driven follow-up queries on later iterations |
| `search` | provider | `(Query) → Vec<Candidate>` | the `CandidateSource` seam; `Query` carries venue/author/field as **query-time params**; dispatch + throttle |
| `lineage` | provider | `(paper, Refs\|CitedBy) → Vec<Candidate>` | seed expansion via OpenAlex `referenced_works`/`cited_by` (**net-new** provider capability) |
| `filter` | pure | `(Pool, Constraints) → Pool` | post-hoc guards only — year clamp, OA flag, drop already-seen. **Not** venue/author/field (see note) |
| `dedup` | pure | `(Pool) → Pool` | collapse by DOI / arXiv id / normalized title |
| `diff` | pure | `(Pool, existing) → Vec<Candidate>` | the *new* candidates vs the saved search — powers stacking |
| `read_papers` | LLM | `(Vec<Candidate>) → annotated` | abstract-level relevance signal; **P1: folded into `rank`** (see below) |
| `assess` | LLM | `(goal, state) → Assessment` | `{ coverage, gaps, stop?: StopReason }` — the early-break point |
| `rank` | LLM | `(Vec<Candidate>, goal) → ranked + rationale` | scores + one-line rationale per candidate |
| `spawn_agent` | — | *reserved* | named for the future; **not implemented** |

`read_papers` exists in the vocabulary as the act of feeding abstracts to the LLM for
per-candidate relevance signals. Abstracts arrive **inside provider records already** (no
extra fetch — `PaperCandidate.abstract_text` is populated today: OpenAlex reconstructs its
inverted index, arXiv uses the summary), so in Phase 1 this is **folded into `rank`** (which
reads title + abstract + metadata directly) to keep LLM-call counts down; a separate
deep-read pass is reserved.

**Why venue/author/field are query-time, not `filter` (a correction from a codebase spike).**
You cannot post-filter your way to papers a keyword search never returned: if the OpenAlex
query didn't ask for a venue, those papers are simply absent from the pool. So venue, author,
and field-of-study must be **request parameters** on `Query` / `CandidateSource::search` /
`DiscoverySearchRequest`, applied at query time per provider (OpenAlex rich; arXiv =
category≈field + author, no venue). The pure `filter` primitive is therefore limited to
post-hoc guards (year clamping, OA flag, dropping already-pooled ids). Year may be applied
both ways (providers already accept `year_from`/`year_to`).

**Net-new vs reused (codebase spike).** Reused as-is: abstracts in `PaperCandidate`,
`CandidateMatch.from_seed_paper_ids` (provenance already exists), the `complete` /
`CompletionRequest` / `WireMessage` / `SseDecoder` helpers (a `pub(super)` → `pub(crate)`
visibility change). **Net-new and not yet in the codebase:** (1) `response_format` on
`CompletionRequest` (absent today); (2) lineage query shape (`referenced_works`/`cited_by`)
in the OpenAlex provider; (3) venue/author/field params on `DiscoverySearchRequest` and the
OpenAlex/arXiv query builders. Items (2) and (3) are a **provider-extension plan that lands
early**, before the agent loop can call `search`/`lineage` with the full `Query`.

### Wiring (the loop)

Rust composes the primitives in a designed pipeline, bounded by the budget, with one
LLM-controlled early exit (`assess`):

```text
state = RunState::new(goal, constraints)
state.existing = load_candidates(search_id)        // for diff / stacking
budget = Budget::from(depth_preset)                 // max_iterations, max_provider_queries, max_llm_calls

for iteration in 0..budget.max_iterations:
  if cancelled(): stop(Cancelled)
  if evaluate_stop(state, budget) is Some(r): stop(r)            // hard budget rail

  queries = if iteration == 0 { plan_queries(goal, state) }      // LLM
            else            { refine_queries(goal, state, gaps) } // LLM
  for q in queries (respecting budget.max_provider_queries):
    if cancelled(): stop(Cancelled)
    raw = search(q)                                  // provider (+ lineage for seed queries)
    state.pool.extend(raw)
    persist search_queries audit row + emit search_updated

  state.pool = filter(state.pool, constraints)       // pure
  state.pool = dedup(state.pool)                      // pure
  assessment = assess(goal, state)                    // LLM
  if assessment.stop: stop(assessment.stop)          // <-- LLM early break (coverage sufficient)
  gaps = assessment.gaps

new = diff(state.pool, state.existing)               // pure — stacking: only the new ones
ranked_new = rank(new, goal)                          // LLM — rank-new-only
persist ranked_new (rank, score, rationale, first_seen_run_id) ; mark new=unseen
set search status=ready, stop_reason, summary{ found, unique, new }
```

- **Bounded + early-break.** `max_iterations` caps the loop; `assess` can end it sooner.
  This is exactly the control model agreed: Rust drives, the LLM may say "enough."
- **Stacking is `diff` + rank-new-only.** A run ranks and persists only the candidates not
  already in the pool, tagging them with `first_seen_run_id`. Existing rows keep their
  rank. The pool is displayed sorted by `(first_seen_run desc, rank asc)` — newest batch on
  top, ranked within itself.
- **Stopping criteria** (one `stop_reason` recorded): target candidate count reached,
  coverage sufficient (LLM `assess`), max iterations, max provider queries, max LLM calls,
  provider failures exhaust budget, or user cancellation.
- **Budgets are safety rails, not preferences.** The loop enforces `max_iterations`,
  `max_provider_queries`, `max_llm_calls` independent of what the planner asks for. A
  runaway planner cannot exceed them.

### Depth presets

"Depth" is the one user-facing knob; it expands to the internal budget. Don't expose the
raw numbers.

| Depth (UI) | max_iterations | ~max provider queries | feel |
|---|---|---|---|
| Quick | 1 | 3 | one good pass |
| Standard | 2–3 | 8 | iterate once or twice on gaps |
| Thorough | 4 | 16 | dig hard |

## Web navigation, browsing, and full text (why no headless browser)

"Deep research" names two different things. **General-web** deep research (OpenAI/Gemini
Deep Research, Perplexity) treats the open web as its corpus and needs a headless browser
and a web-search API. **Scholarly** deep research (Elicit, Undermind, Consensus, scite)
runs over structured scholarly indices via APIs plus an embeddings/RAG layer — no browser.
This feature is the second kind, so it needs **no Playwright/Puppeteer and no general
web-search/fetch**:

- **Search engine → provider search APIs.** OpenAlex/arXiv `search` *is* the agent's
  search tool; they return clean structured records (title, abstract, authors, year,
  venue, citations, OA PDF URL). The `search` primitive (`CandidateSource` seam) is that.
- **Following links → the citation graph as data.** The `lineage` primitive
  (`referenced_works` / `cited_by`) is fetched via API, not by scraping pages.
- **Reading the paper → abstracts inline, full text via the existing pipeline.** Provider
  APIs return abstracts (used by `rank`); full text, when ever needed, comes from the
  OA-PDF download (`pdf_ingestion`) + extraction (`pdf_extraction`, RFC 0035) — an HTTP GET
  plus our extractor, no browser.

**Honest limitation:** Phase 1 ranks on title + abstract + metadata, not full text — a
deliberate, cheaper tradeoff (no per-candidate fetch mid-run). Full-text-aware ranking is
future work that reuses the existing extractor, still browser-free.

**Where browsing would enter, and why it's deferred:** only open-web or paywalled sources
would require fetching/scraping — explicitly out of scope (OA-only; RFC 0020 rules out
Google Scholar scraping). If ever wanted, such a source slots in as **another
`CandidateSource` implementation behind the same `search` seam**; the loop, budgets,
dedup, and ranking are unchanged.

## Architecture

```text
Svelte Find view (Deep mode)
  -> bridge (invoke + listen)
  -> Tauri commands (create_search, run_search, cancel_search_run, list/get, set_schedule)
  -> SearchManager (queue + worker + interval scheduler)
       -> SearchAgent::run(search, deps)
            bounded loop over primitives (cancellable, early-break via assess):
              plan_queries / refine_queries --(JSON)--> Vec<Query>
              search / lineage              (dispatch by provider + throttle)
              filter / dedup / diff         (pure)
              assess                        --(JSON)--> continue | stop(reason)
              rank                          --(JSON)--> ranked new candidates + rationale
       -> LibraryStore (searches / search_runs / search_provider_queries / search_candidates)
       -> app.emit("search_updated", ...)
```

### Module layout (Rust)

```text
src-tauri/src/services/research/
  mod.rs          re-exports SearchManager, SearchAgent, config
  config.rs       ResearchConfig::load() (planner model, depth→budget map, schedule tick)
  agent.rs        SearchAgent::run(...) — the bounded loop wiring the primitives
  primitives.rs   the typed primitive signatures (Pool, Query, Assessment, RunState)
  planner.rs      Planner trait (plan_queries/refine_queries/assess/rank) + OpenRouterPlanner + JSON types
  source.rs       CandidateSource trait + RealCandidateSource (dispatch + throttle + lineage) + fake
  dedup.rs        pure: dedup, diff
  filter.rs       pure: apply Constraints
  budget.rs       pure: Budget, StopReason, evaluate_stop(state, budget)
  schedule.rs     pure: due_searches(searches, now) -> Vec<SearchId>; next_run_at(freq, now)
  manager.rs      SearchManager — queue, worker, interval ticker, events, recovery
  clock.rs        Clock trait + SystemClock
src-tauri/src/commands/research.rs   Tauri commands
```

The LLM wire helpers (`complete`, `CompletionRequest`, `WireMessage`, `SseDecoder`) are
currently `pub(super)` inside `services/chat`. Promote them to `pub(crate)` (or lift into a
small shared `services/llm.rs`) so both `chat` and `research` share one OpenRouter client
path. A targeted refactor justified by the new consumer.

### The seams (what makes it testable)

```rust
// planner.rs — every LLM primitive behind one trait
#[async_trait]
pub trait Planner {
    async fn plan_queries(&self, goal: &Goal, state: &RunState)
        -> Result<Vec<Query>, ResearchError>;
    async fn refine_queries(&self, goal: &Goal, state: &RunState, gaps: &[Gap])
        -> Result<Vec<Query>, ResearchError>;
    async fn assess(&self, goal: &Goal, state: &RunState)
        -> Result<Assessment, ResearchError>;        // { coverage, gaps, stop: Option<StopReason> }
    async fn rank(&self, goal: &Goal, candidates: &[PaperCandidate])
        -> Result<Vec<RankedCandidate>, ResearchError>; // rank + score + one-line rationale
}

// source.rs — the provider seam the loop depends on
#[async_trait]
pub trait CandidateSource {
    async fn search(&self, query: &Query) -> Result<Vec<PaperCandidate>, ResearchError>;
    async fn lineage(&self, paper: &PaperRef, rel: Lineage)
        -> Result<Vec<PaperCandidate>, ResearchError>;
}

// clock.rs
pub trait Clock { fn now(&self) -> DateTime<Utc>; }
```

- `OpenRouterPlanner` is the real `Planner`; a `FakePlanner` returns canned JSON.
- **Why `CandidateSource` rather than `DiscoveryProvider` directly:** provider selection
  is a concrete `match` on `DiscoveryProviderChoice` inside `search_papers`, each arm
  building `DiscoveryService::new(providers.<p>.clone())` over a *monomorphized* type, and
  `DiscoveryProvider` is declared with native `async fn` (no `#[async_trait]`), so
  `Box<dyn DiscoveryProvider>` is **not dyn-compatible**. `CandidateSource` is the
  injectable seam: `RealCandidateSource` does the `DiscoveryProviderChoice` →
  concrete-provider dispatch, the per-provider throttle, and the lineage calls;
  `FakeCandidateSource` returns canned candidates. The loop depends on
  `&dyn CandidateSource`, leaving the existing concrete dispatch untouched.
- `SystemClock` vs a `FakeClock` makes scheduling and "due" logic deterministic.

With these seams, `dedup`, `diff`, `filter`, `evaluate_stop`, `due_searches`,
`next_run_at` are pure and tested directly; `SearchAgent::run` is tested end-to-end with
`FakePlanner` + `FakeCandidateSource` + `FakeClock` (no network, no real time, no sleeps).

### Structured output (JSON)

The LLM primitives must return machine-parseable JSON. Because `response_format` does not
exist in the codebase today and `json_object` support for Claude-via-OpenRouter is
unverified, the **tolerant-parse path is the primary mechanism we build first**: a strict
prompt embedding the JSON schema plus a tolerant extractor (strip prose/code fences, parse,
and on failure re-ask once with the parser error). `response_format: { type: "json_object" }`
is then added as an **optimization** once a live call against `anthropic/claude-sonnet-4.5`
confirms support; it is layered on top of (not a replacement for) the tolerant parser, which
stays as the safety net regardless.

`CompletionRequest` gains an optional `response_format` field
(`#[serde(skip_serializing_if = "Option::is_none")]`) so the chat path is unchanged.

## Data Model

New tables, following existing conventions (`create table if not exists`, FK cascade,
self-guarding migration, `timestamped_id`, text timestamps). Created in `create_schema`;
no changes to existing tables.

```sql
create table if not exists searches (
  id            text primary key,          -- search_<nanos>
  title         text not null,             -- derived from goal, or user-named
  goal          text not null,             -- natural-language instruction
  constraints   text not null,             -- json: { year_from, year_to, providers[], open_access,
                                           --         target_count, venues[], authors[],
                                           --         fields_of_study[], seed_paper_ids[] }
  strategy      text not null,             -- json: { depth, max_iterations, max_provider_queries, max_llm_calls }
  schedule      text,                      -- json: { enabled, frequency, next_run_at, last_run_at } | null
  status        text not null,             -- status of the latest run (idle until first run)
  stop_reason   text,
  summary       text,                      -- json: latest-run counts { found, unique, new, total }
  error         text,
  created_at    text not null,
  updated_at    text not null
);

create table if not exists search_runs (        -- thin ledger, NOT candidate storage
  id            text primary key,          -- run_<nanos>
  search_id     text not null,
  status        text not null,             -- queued|planning|searching|assessing|ranking|ready|failed|cancelled
  stop_reason   text,
  iteration     integer not null default 0,
  added_count   integer not null default 0,-- how many NEW candidates this run added
  total_count   integer not null default 0,-- pool size after this run
  started_at    text,
  finished_at   text,
  error         text,
  created_at    text not null,
  foreign key (search_id) references searches(id) on delete cascade
);
create index if not exists idx_search_runs_search_id on search_runs(search_id);

create table if not exists search_provider_queries (  -- audit / durable progress trace
  id           text primary key,
  run_id       text not null,
  iteration    integer not null,
  provider     text not null,
  query_text   text not null,
  filters      text,                       -- json
  status       text not null,              -- completed|failed
  result_count integer not null default 0,
  error        text,
  created_at   text not null,
  foreign key (run_id) references search_runs(id) on delete cascade
);
create index if not exists idx_search_provider_queries_run_id on search_provider_queries(run_id);

create table if not exists search_candidates (        -- the stacked pool (one row per paper per search)
  id                  text primary key,
  search_id           text not null,       -- pool belongs to the SEARCH (stacking), not a single run
  first_seen_run_id   text not null,       -- which run first surfaced it
  rank                integer not null,
  score               real,
  rationale           text,                -- why this paper, goal-relative
  doi                 text,
  arxiv_id            text,
  title               text not null,
  candidate_json      text not null,       -- full normalized PaperCandidate
  from_seed_paper_ids text,                -- json: provenance for seed-derived candidates
  already_in_library  integer not null default 0,
  saved               integer not null default 0,
  seen                integer not null default 0,  -- unread marker; 0 until the user views it
  first_seen_at       text not null,
  created_at          text not null,
  foreign key (search_id) references searches(id) on delete cascade
);
create index if not exists idx_search_candidates_search_id on search_candidates(search_id);
```

Candidates belong to the **search** (the stacked pool), not to a single run; each remembers
the run that first surfaced it (`first_seen_run_id`) and carries `seen` for the unread
badge. `dedup`/`diff` operate against this pool. The full normalized `PaperCandidate` rides
in `candidate_json` (the "depend on normalized candidates" contract, RFC 0020);
`from_seed_paper_ids` carries provenance forward.

## Backend Changes

- **Provider extensions (net-new, lands early)** — `domain/discovery.rs`: add optional
  `venues`, `authors`, `fields_of_study` to `DiscoverySearchRequest`; wire them into the
  OpenAlex (`providers/openalex/search.rs`) and arXiv (`providers/arxiv/search.rs`) query
  builders (per-provider, asymmetric). Add a **lineage** capability to the OpenAlex provider
  (`referenced_works`/`cited_by` query shape) surfaced through `CandidateSource::lineage`.
- **`services/research/`** — new module per the layout above.
- **`services/chat/openrouter.rs`, `chat/mod.rs`** — promote `complete` /
  `CompletionRequest` / `WireMessage` / `SseDecoder` to `pub(crate)` (or lift to
  `services/llm.rs`); add optional `response_format` (net-new) to `CompletionRequest`.
- **`storage/library_store.rs`** — add the four tables to `create_schema`; add CRUD:
  `create_search`, `update_search`, `list_searches`, `get_search`, `create_search_run`,
  `set_search_run_status`, `append_provider_query`, `append_new_candidates`
  (the stacking insert: dedup against pool, insert only new), `list_search_candidates`,
  `mark_candidate_saved`, `mark_candidates_seen`, plus schedule helpers (`set_schedule`,
  `searches_due(now)`). One-connection-per-call; a transaction for the final
  "append new candidates + set status=ready + write run ledger" commit.
- **`commands/research.rs`** — `create_search`, `run_search` (enqueues, returns run id
  immediately), `cancel_search_run`, `list_searches`, `get_search`, `list_search_candidates`,
  `set_search_schedule`. Registered in `lib.rs::invoke_handler`.
- **`lib.rs::setup`** — construct `SearchManager::new(app.handle().clone(), store.clone(),
  research_config)`, call `recover_and_queue_startup_runs()` (reset stale running statuses;
  bounded schedule catch-up), start the interval ticker, then `.manage()` — mirroring the
  extraction-manager wiring.

### Cancellation

`SearchManager` owns an `Arc<Mutex<HashMap<RunId, CancellationToken>>>`.
`cancel_search_run` looks up the run's token and trips it; the loop checks the token
between iterations and before each LLM/provider call and stops with `StopReason::Cancelled`,
keeping whatever was found. The entry is removed when the run finishes. Net-new versus the
extraction manager (which has no mid-flight cancel); designed in, not bolted on.

### Rate-limiting

Provider calls within a run are serialized per provider with a configured delay (arXiv
~3s; OpenAlex modest), behind the `CandidateSource` seam, so a single run is polite and
scheduled runs cannot stampede a provider. Cross-run concurrency is bounded by the single
worker.

### Progress reporting: events as a primitive trace

Chat streams over a `Channel` tied to one `invoke`. A search run is a background job that
also fires on a schedule with no open call, so it reports progress like the extraction
manager — `self.app.emit("search_updated", ...)` (camelCase payload). The payload is shaped
as a **primitive-call trace** so the UI can pretty-print it directly:

```ts
{ searchId, runId, phase,            // queued|planning|searching|assessing|ranking|ready|failed|cancelled
  iteration,
  message,                           // human line, e.g. 'search(OpenAlex, "…") → 37'
  counts: { found, unique, new },
  query?: { provider, text, resultCount } }   // present on per-`search`-call events
```

`run_search` returns the run id; the UI subscribes to events. Each primitive call the loop
makes emits one event; the durable version of the trace is the `search_provider_queries`
audit table.

## Scheduling (honest constraints)

A desktop app cannot run cron while closed. This RFC is explicit about that:

- A `tokio::time::interval` ticker (e.g., 60s) runs inside `SearchManager`. On each tick it
  asks the store for `searches_due(clock.now())` and enqueues a run for each, then sets
  `next_run_at` via `next_run_at(frequency, now)` (pure).
- A scheduled run uses the **same stacking path**: it dedups against the existing pool and
  appends only new candidates, flagged `seen = 0`. This is what produces the "+5 new"
  unread marker — and quietly does the old briefer's "what arrived" job without prose.
- On startup, `recover_and_queue_startup_runs` performs **bounded catch-up**: a Search whose
  `next_run_at` is in the past gets **at most one** make-up run (not one per missed
  interval), and total catch-up runs at startup are capped, so launching after a vacation
  does not fire a stampede.
- The UI states plainly that scheduled Searches run while i0i is open and catch up on next
  launch. No background daemon, no push.

`frequency` is `daily | weekly | monthly` (RFC 0020). The ticker + `Clock` seam make "which
Searches are due at time T" a pure, tested function.

## Frontend Changes

### Information architecture: one Find view, not a new tab kind

Quick (keyword) search and Deep (agentic) research both produce a ranked candidate list;
the only difference is how intent is expressed (explicit filters vs an NL goal + agent
loop). So they live in **one Find view** (today's Discover), not separate tab kinds. There
is **no `search`/`scout` tab kind**.

The Find view's **left rail is contextual to the view** — the IDE pattern where the side
panel changes per mode (Explorer / Search / Git). In Find, the left rail lists **Searches**,
not vaults:

```text
┌ Searches ─────┐┌──────────── Find (main column) ───────────┐┌──── right rail ────┐
│ + New search  ││ SEED BAR: [Quick | Deep] mode toggle       ││ DURING A RUN:      │
│ ───────────── ││   Deep: goal textarea + constraints +      ││  live primitive    │
│ ● XAI methods ││         seed papers + depth + Run           ││  trace + Cancel    │
│   (weekly •+5)││   Quick: query + year/sort/limit + Run     ││ ─────────────────  │
│ ○ sparse attn ││ ─────────────────────────────────────────  ││ AFTER / ON SELECT: │
│ ○ run 2d ago  ││ SEARCH HEADER (goal · counts · +N new ·     ││  candidate detail  │
│               ││                stop reason)                 ││  (DiscoverInspector│
│               ││ RANKED CANDIDATE LIST (newest batch on top) ││   + why it surfaced│
│               ││   1. title  score  "rationale"  [+ Vault]  ││   + provenance)    │
│               ││   2. title  score  "rationale"  [✓ vault]  ││                    │
└───────────────┘└────────────────────────────────────────────┘└────────────────────┘
```

- **Center = ranked candidate list** with a compact **search header** (goal recap, counts,
  `+N new`, stop reason). No briefer prose in this RFC.
- **Right rail does a temporal swap.** While a run is in flight it shows the **live
  primitive trace** (planning → chosen queries → searching → dedup → assessing → ranking)
  with a **Cancel** button; once the run resolves or a row is selected it reverts to
  **candidate detail** (`DiscoverInspector` reused almost verbatim). Progress is ephemeral,
  detail is persistent — they never coexist.
- **Left rail = Searches.** Saved searches (with a schedule marker + unread count) and
  recent runs, newest first, with `+ New search` on top. Clicking one loads its ranked pool
  into the center. This rail **replaces both the run-history switcher and the
  multi-Discover-tab model**.

**Persistence rule:** a **Deep search** is a durable artifact and appears in the Searches
rail; a **Quick search** is transient — it replaces the current results and adds no rail
entry, so the list does not fill with throwaway keyword queries.

**IA change vs RFC 0020.** RFC 0020 kept the Vault Explorer "always visible and global."
This RFC supersedes that with a **per-view contextual left rail** (Find shows Searches;
Vault shows Vaults). Losing the vault tree inside Find is safe because add-to-vault already
uses the `DiscoverTargetEditor` type-to-filter popover.

### Seed-paper picker

In Deep mode the seed bar has a **Seed papers** field. The user types `<vault-name>/…`;
autosuggestions of papers in that vault pop up (capped at a configurable max, default 5);
the user finishes typing the title or picks from the suggestions. Same type-to-filter idiom
as `DiscoverTargetEditor`, scoped by the vault prefix. Selected seeds drive the `lineage`
primitive and bias `rank`.

### Code changes

- **Domain** (`src/lib/domain/research.ts`): `Search`, `SearchConstraints`,
  `SearchSchedule`, `SearchRun`, `SearchRunStatus`, `SearchCandidate`, `SearchUpdated`.
- **Bridge** (`src/lib/bridge/research.ts`): thin `invoke` wrappers (`createSearch`,
  `runSearch`, `cancelSearchRun`, `listSearches`, `getSearch`, `listSearchCandidates`,
  `setSearchSchedule`) per the existing convention.
- **State** (`library-cache.svelte.ts`): hold the Searches list and the active run; an
  `applySearchUpdated(payload)` reducer mirroring `applyDiscoverSearchResponse`; reuse
  `markDiscoverOwnership` / `getCandidateVaultTargets` for candidates.
- **`DiscoverSeedBar.svelte`** — add a `Quick | Deep` mode toggle. Quick keeps today's
  controls; Deep shows the goal textarea + constraints (year range, providers, target
  count, venues, authors, fields-of-study) + the seed-paper picker + a **depth** selector
  (Quick/Standard/Thorough) + a schedule dropdown (off by default) + `Run deep research`.
- **Find left rail** — new `SearchList.svelte`: `+ New search`, saved searches (schedule
  marker + unread `+N`), recent runs; selecting one loads it.
- **Find center** — a compact `SearchHeader.svelte` (goal · counts · `+N new` · stop
  reason) above the existing `DiscoverFeed` candidate list.
- **Find right** — reuse `DiscoverInspector` for candidate detail; add `RunProgress.svelte`
  for the in-flight primitive trace (driven by `search_updated` via the
  `listen(...)`-in-`onMount` idiom), shown only while a run is active.
- **Add-to-vault** unchanged: `DiscoverTargetEditor` popover →
  `paperDraftFromDiscoverCandidate` → `addPaperToVaults`; double-click a row opens it in
  the Reader.

### Provider asymmetry (filters)

Filters are applied **per-provider where the provider supports them**: **OpenAlex** filters
richly on venue, author, and concept/field; **arXiv** supports category (≈ field) and
author but **not** venue. The UI notes that, e.g., a venue filter narrows OpenAlex but not
arXiv.

### User journey (click level)

**Journey 1 — first deep research (the core path).**

1. **Start.** Click `+ New search` in the Find left rail, then flip the seed-bar toggle to
   **Deep**.
2. **Write the goal:** *"Recent explainable-AI papers relevant to my interpretability work,
   methods not surveys."*
3. **Set constraints:** `from 2023`, providers `OpenAlex + arXiv`, target `20`, depth
   `Standard`; optionally add **seed papers** by typing `interp-vault/…` and picking two.
4. **Launch.** Click **`Run deep research`**. The button becomes **Cancel**; the right rail
   switches to the live primitive trace.
5. **Watch (right rail, via `search_updated`).** Primitive calls stream in — *plan_queries →
   `search(OpenAlex, "…") → 37` → `search(arXiv, "…") → 18` → `dedup 55 → 41` → `assess:
   gap long-context, refining` → iteration 2 → `rank 41`*. Cancel stops cleanly and keeps
   what was found.
6. **Results land.** The trace clears; the **search header** shows `12 papers · stopped:
   target reached`, the **ranked list** fills below, and a new entry appears in the left
   Searches rail.
7. **Triage** the ranked list: rank, title, year/venue, citation signal, a one-line
   rationale, an ownership badge.
8. **Inspect.** Click a row → right rail shows candidate detail + *why it surfaced*
   (query/iteration + rationale + seed provenance).
9. **Act.** Click **`+ Add to Vault`** → `DiscoverTargetEditor` popover → pick a vault →
   the row flips to `✓ in Vault`. Double-click opens the paper in the Reader.

**Journey 2 — put it on a schedule.** From a saved search, open the seed-bar **Schedule**
dropdown → **Weekly**. A plain line appears: *"Runs weekly while i0i is open; catches up on
next launch."* The Searches rail marks it with a schedule indicator.

**Journey 3 — morning inflow (stacking).** On launch, a scheduled run has fired (or catches
up) and **stacked new papers** onto the saved search. The Searches rail shows `+5` unread
on that search; clicking it loads the pool with the **5 new papers on top, flagged new**.
Same surface as Journey 1 — one mental model for manual and scheduled runs. (This is the
briefer's "what arrived this week" value, delivered as a flagged list instead of prose.)

## Testing & Validation

```bash
cargo fmt --check && cargo clippy && cargo test
pnpm check && pnpm build
```

Unit (pure, no network/time):

- `dedup.rs`: same DOI / arXiv id / normalized title collapses; `diff` returns only papers
  not already in the pool (stacking).
- `filter.rs`: each constraint enforced; provider asymmetry respected.
- `budget.rs`: `evaluate_stop` fires on each ceiling; planner cannot exceed `Budget`.
- `schedule.rs`: `due_searches` and `next_run_at` for daily/weekly/monthly with a
  `FakeClock`; bounded catch-up returns at most one make-up run per Search and respects the
  global cap.
- `planner.rs`: JSON parsing — clean object, fenced/prose-wrapped object (fallback path),
  malformed → one re-ask.

Loop (with `FakePlanner` + `FakeCandidateSource` + `FakeClock`):

- Happy path: plan → search → dedup → assess(stop) → ranked new persisted, `status=ready`,
  exactly one `stop_reason`.
- Early break: `assess` returns `stop` before `max_iterations` is hit.
- Iteration: `assess(refine)` drives a second query round via `refine_queries`, then stops.
- Stacking: a second run on the same search adds only new candidates with a new
  `first_seen_run_id`; existing rows keep their rank; unread flags set.
- Cancellation between iterations yields `status=cancelled`, partial pool kept.
- Budget exhaustion yields the right `stop_reason`.

Manual:

- Run a goal with seeds; watch the live primitive trace; see a ranked list with rationales;
  add a candidate to a vault; confirm "in vault" updates.
- Cancel a run mid-flight.
- Enable a daily schedule; advance the clock (or shorten the tick in a dev build); confirm a
  scheduled run stacks new papers with unread flags; relaunch and confirm bounded catch-up.

## Phasing

**Phase 1 — manual agentic search (this RFC, implementation depth).** Search model, the
bounded primitive loop with seams/budgets/cancellation/early-break, JSON planner, the four
tables, **stacking on manual re-run** (`diff` + rank-new-only + unread flags), seed papers,
the rich structured filters, events as a primitive trace, and the Find-view UI (Deep mode in
the seed bar, center ranked list + search header, right-rail trace↔detail, Deep searches in
the left Searches rail). Schedule control present but defaults off (a no-op beyond storing
intent).

**Phase 2 — scheduling automation (design depth here, its own plan).** The interval ticker,
`searches_due` / `next_run_at`, bounded startup catch-up, the Searches rail's
saved/scheduled markers and unread state surfacing on launch. The stacking machinery it
relies on is already built in Phase 1, so Phase 2 is mostly the ticker + catch-up.

Phase 1 is fully designed above; Phase 2 is sketched deliberately to keep this RFC to a
single implementable plan and avoid the "four 70%-done verticals" failure the journey docs
warn about.

**Implementation order (layered, bottom-up).** The Phase-1 build decomposes into ordered,
independently-testable plans: (0) **provider extensions** — structured-filter params +
lineage on OpenAlex/arXiv; (1) **LLM client sharing** — `pub(crate)` promotion +
`response_format`; (2) **persistence** — the four tables + CRUD + stacking insert; (3) **pure
core** — `dedup`/`diff`/`filter`/`budget`/`schedule`/`clock`; (4) **seams** — `Planner`
(+ tolerant JSON parser) and `CandidateSource` with fakes; (5) **agent loop** —
`SearchAgent::run` (bounded + early-break + cancellation), tested with fakes; (6) **manager +
commands + `lib.rs` wiring**; (7) **frontend** — Find Deep mode, Searches rail, progress
trace, seed picker. Each plan is authored just-in-time, not all up front.

## Risks

- **JSON reliability.** Tolerant-parse is the primary path (strip fences, parse, single
  re-ask); `response_format` is added only after a live check confirms support. Parsing is
  unit-tested.
- **Provider extensions.** Lineage and structured-filter params are net-new; isolated to the
  provider layer behind `CandidateSource` and landed in an early, independently-testable plan.
- **Cost / runaway loops.** Hard `Budget` ceilings enforced by the loop independent of the
  planner; depth presets; bounded catch-up; single worker.
- **Provider rate limits.** Per-provider serialized delay behind the seam; single-worker
  concurrency.
- **Desktop scheduling expectations.** Stated honestly in UI; bounded catch-up prevents
  stampedes.
- **Scope creep toward the profile / multi-agent.** Explicit non-goals; `spawn_agent`
  reserved but unbuilt; ranking isolated to the `rank` primitive.
- **Refactor blast radius** from promoting the OpenRouter helpers. Contained: visibility
  change plus one optional field; chat path unchanged and covered by existing tests.

## Future Work

- **The briefer** — a `synthesize` step over a stored search's pool producing a written
  report; layers cleanly on top of the ranked candidates this RFC delivers.
- **Profile-relative ranking** — extend `rank` to weight by marginal value to the learning
  profile (the moat).
- **`spawn_agent` / LLM-native tool-calling** — sub-searches per gap, or handing control
  flow to the LLM, once the plumbing and need exist. Same primitives, swapped driver.
- **Full-text-aware ranking** — fetch top-N OA PDFs, feed extracted text (RFC 0035) to
  `rank`; still browser-free.
- **Activity-log read-model** consuming the run-level signals this RFC emits.
- **Semantic Scholar / richer lineage** as additional `CandidateSource` capability once a
  usable S2 key path exists (S2 is currently rate-limited; OpenAlex + arXiv are Phase 1).

## Open Questions

- Default `planner_model`: reuse `chat.model` (`anthropic/claude-sonnet-4.5`) or add a
  dedicated, possibly cheaper, `research.planner_model`? Proposal: dedicated field, default
  to `chat.model` if unset.
- `read_papers` in Phase 1: keep folded into `rank` (abstracts read during ranking), or run
  it as a distinct enrichment pass? Proposal: folded into `rank` for P1 to bound LLM calls;
  separate deep-read reserved.

(Resolved across review: briefer dropped for now; `brief → goal`; scout/run collapsed to a
**Search** with a stacked candidate pool; runs stack via `diff` + rank-new-only; scheduling
kept (Phase 2 automation); seed papers in scope with a vault-prefix picker; venue/author/
field filters added with provider asymmetry noted; depth presets replace raw budget knobs;
the research domain modeled as a vocabulary of typed primitives wired by a bounded
Rust loop with an LLM early-break; progress surfaced as the primitive-call trace.)
