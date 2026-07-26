# RFC 0057: Deep Research Semantic Ranking — Wire the Biencoder + Semantic Floor

Status: Implemented (§1 + §2; §3 weight tune deferred)
Date: 2026-07-26
Product: i0i
Target: Tauri v2 + Svelte, macOS first
Builds on: RFC 0037 (Deep Research Scout Agent), RFC 0043 (Search Quality),
RFC 0054 (Search Relevance Engine — local embedding rerank)

## Summary

RFC 0054 built a local embedding reranker and wired it into **quick search**,
but explicitly deferred wiring it into **deep research**. As a result deep
research still ranks purely on deterministic signals (keyword substring +
citations + recency), so a goal like *"introductory works on distributed
training for LLMs"* returns highly-cited generic surveys ("Deep Residual
Learning", "Review of Deep Learning") and buries the papers actually meant —
FSDP, ZeRO, Megatron-LM, `torch.distributed`.

This RFC finishes the 0054 follow-up with two changes, no new model and no new
providers:

1. **Feed the biencoder into deep-research ranking.** Compute query↔candidate
   cosine on the pool and pass it into the existing `rank_candidates` semantic
   branch — the same call the quick path already uses.
2. **Add a semantic floor.** Drop candidates whose similarity to the goal is
   below a threshold *before* truncation, so off-topic-but-famous papers are
   removed, not merely re-sorted down.

Plain English: deep research currently rewards fame and word overlap. This makes
it rank by *meaning* — the thing the user actually searched for — and throws out
results that are clearly off-topic no matter how cited they are.

## Problem

Traced in `services/research/agent.rs:161`:

```rust
// Deep research keeps legacy ranking for now; wiring the embedding reranker
// through the SearchManager loop is a follow-up (RFC 0054 notes).
let ranked = rank_candidates(new_candidates, inputs.goal, constraints.target_count, &[])
```

The `&[]` is the `semantic_scores` argument. Empty ⇒ `rank_score` takes the
`None` branch (`orchestrator.rs`): `PROVIDER 0.35`, `KEYWORD 0.25`,
`CITATION 0.15`, `RECENCY 0.10`, `AVAILABILITY 0.10`, `MULTI 0.05`. No term
understands meaning, and citations reward famous-but-irrelevant papers. The
`SEM_*` weight branch — which gives semantic similarity 0.30 — is never reached
on this path, and no `semantic:x.xxx` reason is ever attached, so there is no
score to inspect.

The reranker itself is already a managed service (`app.manage(embedding_reranker)`
in `lib.rs`) with a ready-to-use, failure-safe method:

```rust
pub async fn semantic_scores(&self, query: &str, candidates: &[PaperCandidate]) -> Vec<f64>
```

It returns an **empty vec** when the model is unbuilt, the runtime kill-switch is
off (`search.reranker_enabled`), or embedding fails — and `rank_candidates`
already treats empty as "no signal, use legacy weights." So wiring it in cannot
regress ranking when the reranker is unavailable.

## Proposal

### 1. Thread the reranker into the deep-research loop

- Add `Arc<EmbeddingReranker>` to `SearchManager` (constructed in `lib.rs`
  setup alongside the reranker that already exists there).
- Pass it into `agent::run` (via `RunInputs` or a `run` parameter).
- Just before ranking, compute scores over the *new* candidates only (the set
  already handed to `rank_candidates`):

  ```rust
  let semantic = reranker.semantic_scores(inputs.goal, &new_candidates).await;
  let new_candidates = apply_semantic_floor(new_candidates, &semantic); // §2
  let ranked = rank_candidates(new_candidates, inputs.goal, target_count, &semantic);
  ```

  This activates the existing `SEM_*` branch and attaches `semantic:x.xxx`
  reasons — so deep-research candidates now carry an inspectable semantic score,
  directly answering "where are the scores."

- **Bound the work.** Embed at most the top-K (reuse `SEMANTIC_RERANK_WINDOW`,
  100) candidates by legacy score before reranking, so a large pool doesn't
  trigger hundreds of forward passes. Deep research is already async/background,
  so K=100 is comfortable.

### 2. Semantic floor

When a semantic signal is present (non-empty scores, i.e. reranker ready), drop
candidates whose cosine to the goal is below `SEMANTIC_FLOOR` (proposed **0.30**)
before truncating to `target_count`. Guards:

- **Never below a minimum.** If the floor would leave fewer than
  `min(target_count, SEMANTIC_MIN_KEEP)` candidates, keep the top scorers
  instead — a strict floor must never return an empty run.
- **Signal-gated.** With no semantic signal (reranker off/unavailable), the
  floor is skipped entirely; behavior is exactly today's.

The floor removes the "famous but off-topic" failure mode that re-sorting alone
can't fix when citation weight is high.

### 3. Weight tune (optional, small)

`rank_score`'s `SEM_*` constants are **shared** with quick search, so any tune
affects both paths — intended, but called out. Current `SEM_*` sums to 1.00.
Proposed shift toward meaning and away from fame:

| Term | Now | Proposed |
|---|---|---|
| Semantic | 0.30 | **0.42** |
| Provider | 0.25 | 0.22 |
| Keyword | 0.10 | 0.08 |
| Citation | 0.10 | **0.05** |
| Recency | 0.10 | 0.08 |
| Availability | 0.10 | 0.10 |
| Multi-provider | 0.05 | 0.05 |

This is a knob, not load-bearing; ship §1+§2 first and tune from observed
results.

## Non-goals

- **No query rewriting.** Decomposing the natural-language goal into technical
  entities (FSDP/ZeRO/…) is a real lever but a separate change (planner prompt),
  out of scope here.
- **No new providers, no new model, no vector DB** — brute-force cosine over a
  bounded window, exactly as quick search does today.
- No change to how providers are queried or to the quick-search path's wiring.

## Testing

- Unit: `apply_semantic_floor` — drops below-threshold, respects the min-keep
  guard, is a no-op on empty scores.
- Unit: `agent::run` with a stub `TextEmbedder` (the existing bag-of-words test
  embedder) asserts a relevant candidate outranks a high-citation off-topic one,
  and that `semantic:` reasons are attached.
- Regression: with the reranker disabled, deep-research ordering is byte-for-byte
  today's (empty scores ⇒ legacy branch, floor skipped).

## Risks

- **Shared weights.** The §3 tune moves quick search too. Mitigation: land
  §1+§2 (pure additions on the deep path) first; treat §3 as a follow-up commit.
- **Embedding latency on large pools.** Mitigated by the top-K window bound.
- **Over-aggressive floor** returning too few results. Mitigated by the
  min-keep guard; 0.30 is deliberately conservative and tunable.

## Rollout

1. Thread reranker → `SearchManager` → `agent::run`; compute scores; pass into
   `rank_candidates` (§1). Verify `semantic:` reasons appear.
2. Add `apply_semantic_floor` + guards + tests (§2).
3. Observe on real goals (the distributed-training query is the canonical
   check); apply the §3 weight tune if fame still leaks through.
