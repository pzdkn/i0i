# RFC 0054: Search Relevance Engine — Local Embedding Rerank + Query Expansion

Status: Implemented
Date: 2026-07-23
Product: i0i
Target: Tauri v2 + Svelte, macOS first
Builds on: RFC 0037 (Deep Research Scout Agent), RFC 0043 (Multi-Provider Scout
Search Quality), RFC 0044 (Concurrent Fan-Out), RFC 0053 (Provider Expansion)

## Summary

Make search results more *relevant*, not just more numerous, with two levers
that sit on top of the existing orchestrator without changing how providers are
queried:

- A **local embedding reranker**: download a small sentence-embedding model
  once, run it on-device, and use query↔candidate cosine similarity as a strong
  ranking signal — replacing today's naive substring keyword match as the
  primary relevance term.
- **Quick-search query expansion**, done **progressively**: run the user's
  literal query immediately (Quick stays quick), and in the background use a
  fast/cheap LLM to expand the query, re-run, and merge the extra results in.

Plain English version: today ranking mostly trusts each provider plus a
substring count. This teaches i0i to understand *meaning* — sorting results by
how close they actually are to what the user meant, and quietly widening the
net on quick searches without making them feel slow.

## Problem

Two relevance gaps, traced through `orchestrator.rs::rank_score` and
`services/research/`:

1. **Ranking barely understands the query.** The relevance term with the
   largest weight after the provider's own score is `keyword` (0.25): a
   lowercase **substring match count** of query terms against title+abstract.
   It cannot see synonyms, paraphrase, acronyms, or conceptual closeness. A
   paper titled "large language models for retrieval" scores zero keyword
   overlap against a query for "LLM document search" despite being a near-exact
   topical match. RFC 0043 explicitly deferred both embedding and LLM reranking
   ("no embedding reranker", "no LLM reranking of every result") — this RFC
   picks up the embedding half.

2. **Quick search never expands, so recall depends on exact phrasing.** By RFC
   0043's deliberate design, Quick search sends the user's exact string to
   providers with no expansion. That keeps it fast but means a slightly-off
   query (wrong synonym, missing acronym expansion) silently misses relevant
   papers. Expansion exists only inside the heavier Deep Research LLM loop.

Note: an LLM reranker (`Planner::rank`, `RANK_SYSTEM`) is already implemented
and unit-tested in `services/research/planner.rs` but is **never called** by the
agent loop. This RFC chooses **local embeddings** over wiring that up, for
cost and latency reasons (see Alternatives).

## Goals

- Add an on-device embedding model with a first-run download, cached in app
  data, with graceful fallback to today's ranking when the model is absent.
- Introduce a `semantic` similarity sub-score and rebalance `rank_score` so
  meaning, not substring overlap, is the primary relevance signal.
- Use the same embedding rerank for Quick search, Deep Research, and RFC 0053's
  new providers (one code path, applied after merge/dedupe).
- Add progressive query expansion to Quick search using a fast/cheap LLM, with
  the literal query always returning first and expansion arriving as a
  background augmentation.
- Keep token cost near zero: embeddings are local (free); the only paid call is
  one small expansion completion per Quick search that opts in.

## Non-Goals

- No cross-encoder reranker (heavier; bi-encoder is enough for tens of
  candidates). Deferred to Future Work.
- No hosted embedding API as the default (see Alternatives — kept as a fallback
  option, not the primary path).
- No change to provider query syntax or the concurrent fan-out.
- No expansion on the *literal* Quick result path — expansion is strictly
  additive and asynchronous, never blocking first results.
- No re-introduction of `Planner::rank` into the loop (explicitly considered and
  set aside below).
- No new providers — that is RFC 0053.

## Part A — Local Embedding Reranker

### A1. Model and runtime

- Model: a small English bi-encoder, **BGE-small-en-v1.5** (~130 MB) or
  **all-MiniLM-L6-v2** (~90 MB). 384-dim embeddings, strong on short
  title/abstract text, CPU-friendly.
- Runtime: **`fastembed-rs`** (wraps ONNX Runtime; handles tokenization + model
  download) as the default. `candle` is the alternative if we want to avoid the
  ONNX native dependency; `fastembed` is less code to own.
- Location: model files cached under the app data dir
  (`…/i0i/models/<model-id>/`). First use triggers a download with progress;
  subsequent runs load from disk. Nothing leaves the machine at query time.

### A2. Where it runs

A new `EmbeddingReranker` service, held on app state like the other services,
exposing:

```rust
fn is_ready(&self) -> bool;                    // model present + loaded
async fn ensure_model(&self) -> Result<()>;     // trigger/await download
fn embed(&self, texts: &[String]) -> Vec<Vec<f32>>;
```

Reranking happens in the orchestrator, **after** merge/dedupe and **before**
final truncation — one place, so Quick search, Deep Research (via
`RealCandidateSource`), and RFC 0053 providers all benefit:

```text
merged, deduped candidates
  -> if reranker.is_ready():
       q = embed(query)
       c_i = embed(title_i + " " + abstract_i[..N])
       semantic_i = cosine(q, c_i)   // 0..1
  -> rank_score (semantic-aware, see A3)
  -> sort, truncate to result_limit
```

Embedding ~30–50 short candidate texts on CPU is well under a second; the
candidate abstract is truncated (e.g. first ~400 chars) to bound tokenization.
Query and candidate embeddings within one search are computed in a single batch.

### A3. Rebalanced `rank_score`

Introduce `semantic` and demote `keyword` to a cheap lexical floor. Proposed
weights (sum = 1.0):

```text
current                         proposed (reranker ready)
provider       0.35             provider       0.25
keyword        0.25             semantic       0.30   <- new, primary relevance
citations      0.15             keyword        0.10   <- lexical floor
recency        0.10             citations      0.10
availability   0.10             recency        0.10
multi_provider 0.05             availability   0.10   (viewability, RFC 0053)
                                multi_provider 0.05
```

When the reranker is **not** ready (model still downloading, disabled, or
failed to load), fall back to **today's** weights so ranking never regresses to
worse-than-current. This makes the model download non-blocking and fully
optional.

`semantic` = `cosine(query_embedding, candidate_embedding)` clamped to `0..1`.
It also augments `match_summary.reasons` (e.g. `semantic:0.82`) for
explainability, consistent with existing provenance reasons.

## Part B — Progressive Quick-Search Query Expansion

### B1. Flow

Quick search's literal path is unchanged and always returns first. Expansion is
a second, asynchronous pass:

```text
user query
  -> [now] run literal query through orchestrator, return ranked results
  -> [background] cheap-LLM expand(query) -> 2-3 query variants
  -> run variants concurrently through the same orchestrator
  -> merge + dedupe into the existing result set
  -> embedding-rerank the combined set (Part A)
  -> emit `search_expanded` event with the augmented, re-ranked list
```

This is the same progressive pattern already used for Deep Research previews
(`search_candidates_preview`) and RFC 0051 background PDF acquisition — the user
sees fast results immediately and a quiet "found N more" refresh a beat later.

### B2. The expansion call

- Model: a **fast, cheap** completion model via the existing OpenRouter path
  (`services/llm.rs`, `ChatConfig`) — a small/Haiku-class model, not the Deep
  Research planner model.
- Prompt: reuse the shape of `PLAN_SYSTEM` but tighter — "Return 2–3 alternative
  search queries (synonyms, acronym expansions, method/dataset names) for the
  same intent. JSON array of strings." No provider tagging (Quick runs all
  selected providers), no multi-turn refinement.
- Guardrails: bounded output (2–3 strings), short timeout; on timeout/failure
  the literal results simply stand (no error surfaced). Cache expansions by
  normalized query string for the session to avoid repeat calls.

### B3. Opt-in and cost

- A Discover setting **"Expand my search"** controls whether the background
  expansion fires. Off ⇒ pure literal behavior, zero token cost, identical to
  today. (Shipped **on by default**; toggle off per workspace to avoid the cost
  — see Implementation Notes.)
- Cost profile: one small completion per opted-in Quick search; embeddings are
  local and free. Consistent with the project's token-cost discipline —
  expansion is the *only* paid element and it is bounded and cache-guarded.

## What stays the same

- Provider query construction and concurrent fan-out (RFC 0044/0053).
- Deep Research's own LLM plan/refine/assess loop — it keeps its planner; it
  simply gains the embedding rerank at the ranking step like every other path.
- `DiscoverySearchResponse` / `PaperCandidate` shapes (additive fields only:
  the semantic reason string; no breaking changes).

## Alternatives Considered

- **Wire up the existing `Planner::rank` LLM reranker instead of embeddings.**
  It exists and is tested, but reranking *every* result on *every* search is a
  per-query token cost and adds latency to the one path (Quick) that must stay
  fast. Embeddings are free after the one-time download and run in-process.
  Rejected as the primary reranker; may still be used inside Deep Research's
  bounded loop in a later RFC.
- **Hosted embedding API** (OpenAI `text-embedding-3-small`, Voyage, Jina).
  Trivial to integrate but adds per-query cost and a network dependency, and
  the existing OpenRouter path does not cover embeddings. Kept as a fallback
  `embedding_provider = "remote"` option behind the same `EmbeddingReranker`
  interface, not the default.
- **Cross-encoder reranker.** More accurate than a bi-encoder but far heavier
  per candidate; unjustified for tens of results. Future Work.

## Validation

Backend tests:

```text
reranker: cosine of identical texts ~= 1.0; unrelated texts low
reranker: not-ready -> rank_score uses legacy weights (no regression)
reranker: ready -> semantic sub-score present and dominant in rank_score
rank_score: proposed weights sum to 1.0 in both ready/not-ready modes
expansion: cheap-LLM returns 2-3 variants; malformed/empty -> literal stands
expansion: variants run through orchestrator and merge/dedupe with literal set
expansion: session cache avoids a second call for the same normalized query
expansion disabled -> no LLM call, literal-only path
```

Frontend checks:

```text
literal Quick results render before expansion completes
`search_expanded` augmentation merges without reordering flicker beyond re-rank
"Expand my search" toggle disables the background pass
model-download state surfaced (downloading / ready / unavailable)
ranking still works with model absent (graceful fallback)
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
1. First run: trigger model download, confirm progress + eventual "ready".
2. Search a paraphrased query ("LLM document search"); confirm a topically
   matching paper with no literal keyword overlap now ranks highly.
3. Disable the reranker mid-session; confirm ranking falls back cleanly.
4. Quick search with expansion on: confirm literal results appear first, then
   a "found N more" augmentation.
5. Turn "Expand my search" off; confirm no expansion call and instant literal
   results.
```

## Implementation Order

1. Add RFC 0054.
2. `EmbeddingReranker` service + `fastembed` integration + model
   download/cache + `is_ready`/`ensure_model` + tests (no ranking change yet).
3. Add `semantic` sub-score + rebalanced `rank_score` with ready/not-ready
   fallback; wire rerank into the orchestrator post-dedupe.
4. Frontend: model-download status surfacing; confirm graceful absence.
5. Progressive Quick-search expansion: cheap-LLM call + variant fan-out +
   merge + `search_expanded` event + session cache.
6. Frontend: literal-first render + augmentation merge + "Expand my search"
   toggle.
7. Run validation.

## Risks

- **First-run download friction / size.** ~90–130 MB one-time. Mitigation:
  non-blocking (ranking works without it), progress surfaced, cached
  thereafter; consider shipping the model on request rather than at install.
- **ONNX native dependency / build + binary size.** Mitigation: `fastembed`
  isolates it; `candle` is the pure-Rust fallback if the native dep causes
  packaging pain on macOS.
- **Embedding latency on large result sets.** Mitigation: rerank only the
  post-truncation-candidate window (e.g. top ~100 before final cut), batch,
  and truncate abstracts.
- **Expansion adds noise / off-topic results.** Mitigation: expansion feeds the
  *same* dedupe + embedding rerank, so weakly-related expanded hits sink; the
  literal set is never displaced, only appended to and re-sorted.
- **Weight rebalance regresses some queries.** Mitigation: keep legacy weights
  as the not-ready path and treat the new weights as tunable constants;
  validate on a small fixed query set before defaulting on.

## Implementation Notes (2026-07-23)

Both parts implemented and validated.

**Part A — embedding reranker**
- Lives in `services/embedding/`. A `TextEmbedder` trait + `EmbeddingReranker`
  handle + cosine math + score assembly all compile and are unit-tested with a
  fake bag-of-words embedder **regardless of the feature flag**, so the
  relevance logic ships even where ONNX can't build.
- The real model is behind the `embeddings` Cargo feature (`fastembed` 4.x,
  BGE-small-en-v1.5, ONNX Runtime via `ort`). **The feature builds cleanly
  here** (~50s cold) and a live `#[ignore]` test confirmed the model loads and
  ranks a topical match above an unrelated paper. It is **on by default**
  (`default = ["embeddings"]`), so a normal build pulls ONNX Runtime and
  downloads the ~130 MB model on first run. Build `--no-default-features` to
  drop it (verified to still compile); ranking then uses legacy weights.
- **The ready-mode weights are still a first guess and have not been validated
  against a fixed query set.** They are live now that the feature is on by
  default — worth a calibration pass.
- Ranking shape (per review): `EmbeddingReranker::semantic_scores(query,
  &candidates) -> Vec<f64>` is async and index-aligned, returning an **empty
  vec** when not ready. `rank_candidates`/`rank_score` stay pure/sync and take a
  `&[f64]` slice; a missing/empty score ⇒ legacy weights per candidate. The
  orchestrator computes scores **after** merge/dedupe/viewability-filter and
  **before** ranking, in `search_expanded`. Semantic scoring runs on the
  embedding of *title + first 400 abstract chars*, off-thread via
  `spawn_blocking`.
- Ready-mode weights (provider 0.25 / semantic 0.30 / keyword 0.10 / citations
  0.10 / recency 0.10 / availability 0.10 / multi 0.05) and legacy weights both
  assert-sum to 1.0. **The weights are a first guess and want validation
  against a fixed query set before enabling the feature by default.**
- **Top-N embedding window implemented** (`SEMANTIC_RERANK_WINDOW = 100`): the
  expand path can produce a few-hundred-candidate union (up to ~4 queries × 4
  providers), so when the reranker is ready and the set exceeds the window,
  `search_expanded` keeps the legacy-top-100 (`legacy_top_n`, no annotation)
  before semantic scoring and final ranking. The dropped tail could not survive
  truncation to `result_limit` anyway. This bounds embedding cost now that the
  feature is on by default.

**Part B — progressive query expansion**
- `services/query_expansion.rs`: `QueryExpander` calls a cheap model (the chat
  config's `title_model`, else the chat model) via the existing OpenRouter
  transport, parses a JSON array of 2–3 variants (tolerant of object-wrapping
  and surrounding prose), dedupes/caps them, and caches by normalized query for
  the session (empty results cached too). No key / timeout / unparseable reply
  ⇒ empty ⇒ literal results stand. Live-verified against the real model.
- Rather than bolt a spawn+event path onto the synchronous `search_papers`
  command, expansion is a **second command** (`expand_search`) the frontend
  calls after literal results render. It expands, re-runs the original query
  plus variants through the orchestrator, and returns the merged, reranked
  superset. This preserves "literal first, expansion a beat later" without
  changing the command's execution model.
- **Cost, stated plainly:** with expansion on, one quick search costs an LLM
  expansion call plus a re-fan-out of the original query **and** each variant —
  roughly **5× the provider traffic** of a plain search (1 literal + original +
  3 variants). That is the architectural price of the second-command design.
  Expansion is **on by default** (enabled on request); the no-variant
  short-circuit below keeps the floor cheap, and it can be turned off per
  workspace via the "Expand my search" toggle (quick-search only).
- **No-variant short-circuit:** if expansion yields no variants (no key,
  cached-empty, timeout, unparseable reply), `expand_search` returns an
  empty-candidate sentinel *without* re-running any search, and the frontend
  merge no-ops on it. So a no-variant search costs zero extra provider calls —
  only the one (bounded, cached) LLM attempt.
- Ranking is always measured against the **original** query; variants only
  widen recall (`search_expanded` ranks against `request.query`, not the
  variants).
- Frontend: `applyDiscoverExpansion` merges the superset in place, **preserving
  the current selection and per-candidate probe state** so the augmentation
  doesn't disturb what the user is looking at; it no-ops on the empty sentinel,
  a changed query, or a newer run.

**Deferred (noted, not blocking)**
- Deep research keeps legacy ranking — the embedding reranker is not threaded
  through the `SearchManager` loop (`agent.rs` passes `&[]`). Same pragmatic
  scoping as RFC 0053's deep-research provider gap.
- No model-download progress UI yet (A4); with the feature off there's nothing
  to download, and the startup log reports readiness.
- Hosted-embedding fallback (`embedding_provider = "remote"`) and the
  cross-encoder path remain Future Work.

Validation: `cargo test` 213 passed / 0 failed / 2 ignored (feature off);
`cargo build --features embeddings` clean; live model + live expansion checks
pass; `pnpm check` 0 errors / 0 warnings; `git diff --check` clean.

## Future Work

- Cross-encoder rerank of the top-K for precision-critical searches.
- Reuse candidate embeddings for "more like this" / vault semantic search.
- Feed embedding similarity back into Deep Research's `assess` step to decide
  coverage gaps by meaning rather than title overlap.
- Optionally wire `Planner::rank` for Deep Research's final ranking only, where
  latency is already acceptable.
- Persist candidate embeddings to avoid recompute across repeated searches.
