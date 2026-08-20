# RFC 0095: Relevant vault paper suggestions

Status: Draft
Date: 2026-08-20
Product: i0i
Target: Tauri v2 + SvelteKit (Svelte 5), macOS first
Builds on: RFC 0091 (vault paper suggestions), RFC 0094 (suggestion controls),
RFC 0088 (deep research), RFC 0057 (semantic ranking)

## Summary

Vault suggestions are currently broad and disproportionately contain survey
papers. This is not primarily a presentation problem. The current retrieval and
ranking policy has no year or topic focus, turns the entire vault into one broad
profile, gives citation-graph rank three votes, gives raw citation count another
vote, applies no minimum relevance threshold and always fills the five result
slots. Those choices systematically reward old, central, general papers.

A second problem is query compression. One machine-written query for an entire
vault can erase distinctions between methods, applications and open questions.
The replacement uses several small query paths concurrently, then combines
their evidence without letting duplicate results dominate.

This RFC makes the ranking objective explicit: suggest a small number of
specific papers that advance the active vault, even when that means returning
fewer than requested.

## Current pipeline

For clarity, the implemented RFC 0091 pipeline is:

1. Build one bounded profile from up to 24 vault papers using titles, abstracts
   and representative extracted passages.
2. Run Standard Deep Research against OpenAlex and arXiv with no year bounds,
   a target pool of 25 and the vault papers as seeds.
3. Expand a bounded two-hop OpenAlex citation neighbourhood.
4. Compare candidates with the mean of available vault chunk embeddings.
5. Fuse Deep Research rank, semantic rank, citation-graph rank three times and
   raw citation-count rank once using reciprocal-rank fusion.
6. Return the top five after owned and dismissed papers are removed.

The likely failure modes follow directly:

- a multi-topic vault becomes one vague query;
- graph centrality and citation count reward canonical surveys;
- rank fusion can promote a weak candidate without requiring actual similarity;
- missing year bounds favors old papers;
- filling every slot makes low-confidence results look endorsed.

## Ranking policy

### User intent narrows the vault

RFC 0094's optional **Focus within this vault** is appended to the bounded vault
profile as a strong instruction to the Deep Research planner. It is also used
as an independent semantic ranking query. It does not replace the vault: a
candidate must still relate to at least one paper already present.

Year bounds flow into `SearchConstraints` and are reapplied after retrieval,
because graph expansion can introduce papers that did not pass through provider
search filters.

### Parallel query paths

Before retrieval, the planner generates the `query_path_count` requested by RFC
0094. Each proposed path must express one distinct search angle in a
provider-friendly query, not a paraphrase of the whole vault profile. For
example, a vault spanning sparse attention might produce:

1. `empirical sparse attention language models`
2. `efficient transformer inference long context`
3. `learned token pruning attention`

The proposal is returned before any provider call. The user reviews it and may
deselect paths that are too broad, redundant or simply uninteresting. Only the
selected paths become independent retrieval units. OpenAlex and arXiv searches
for those paths may run concurrently, bounded by one service-level semaphore so
selecting five paths does not create an uncontrolled burst. A provider failure
affects only that provider/path pair; the remaining work continues.

Each path is deliberately lighter than the current Standard Deep Research run:

- one focused provider query per provider;
- a small per-path candidate limit derived from the overall pool budget;
- no separate multi-round reflection loop inside every path;
- one shared citation expansion and final ranking after all paths settle.

This preserves nuance without multiplying the full agent loop five times. The
planner runs once to create the reviewable proposal; after explicit user
approval, retrieval fans out and fusion happens once. The default is three
proposed paths. One remains available for narrow vaults or lower-latency
searches, and five is the explicit high-breadth option.

Candidates are deduplicated across path and provider identity before expensive
scoring. The service retains which paths found each candidate. Appearing in
multiple genuinely distinct paths is useful evidence, but it contributes at
most one additional rank signal; duplicate provider records or paraphrased
paths cannot repeatedly vote for the same paper.

### Prefer specific contributions

The planner prompt asks for concrete primary contributions, methods, findings
and direct extensions of vault papers. It must not use `survey`, `review` or
other broad paper types merely to improve topical coverage.

With **Include reviews** off, candidates whose title clearly identifies them as
a review are excluded using a documented, deliberately small title classifier:

- `survey`
- `review` or `systematic review`
- `meta-analysis`
- `bibliometric analysis`

Matching uses normalized whole words and phrases, not arbitrary substrings.
This is an understandable first version; it must not pretend OpenAlex or arXiv
provide a reliable universal primary-research classification. With the option
on, these candidates remain eligible but receive no special boost.

### Signals and fusion

Final reciprocal-rank fusion uses each of these once:

1. merged query-path retrieval rank
2. vault semantic-similarity rank
3. optional focus-similarity rank
4. citation-graph distinct-origin rank
5. distinct-query-path coverage rank, when more than one path ran

Raw global citation count is removed from fusion. It remains display metadata
and may break an exact final tie, but it is not evidence that a paper belongs in
this vault. Citation-graph rank is no longer repeated three times. Reaching a
candidate from several distinct vault papers remains meaningful; global
popularity does not.

Do not introduce learned weights or an LLM reranking call in this RFC. The
transparent ranker is easier to diagnose and the existing Deep
Research pass has already paid for semantic planning. A later evaluation may
justify a trained or LLM judge, but it should not conceal the present failure.

### Relevance floor and result count

`result_count` is a maximum, not a quota. A candidate is eligible only if:

- it passes owned, dismissed, year and review filters; and
- it has direct graph evidence from a vault paper, or clears a configured
  semantic-similarity floor against the vault; and
- when a focus is present, it also clears a focus-similarity floor.

The initial thresholds live in the normal application YAML with comments and
safe defaults. They are backend configuration, not inspector controls. A run
may return zero, two or five candidates; the UI says **No strong suggestions**
when none clear the floor.

### Better reasons

The visible reason names the strongest concrete evidence rather than a generic
topic:

- `Cited by 3 papers in this vault`
- `Directly extends “Sparse Transformers”`
- `86% similar to this vault · matches focus “empirical methods”`
- `Found through 2 search paths · token pruning + efficient inference`

Do not expose a fused score. If only weak generic wording can be produced, the
candidate should probably have failed the relevance floor.

## Evaluation

Ranking changes need a small fixed evaluation before replacing the current
policy. Create fixtures from at least three real vault shapes:

- one narrow methods vault;
- one mixed-topic vault;
- one vault where surveys are intentionally useful.

For each vault, record a frozen candidate pool and human labels
`strong | plausible | irrelevant`, plus whether each candidate is a review.
Compare current and proposed rankers on:

- precision among the first five;
- number of irrelevant candidates shown;
- review share with Include reviews off;
- useful result count after the relevance floor.

The candidate pool is fixed so the test measures ranking policy rather than
provider drift. Network retrieval remains covered separately by integration
tests.

## Acceptance criteria

1. Year bounds and optional focus influence both retrieval and final filtering.
2. The planner emits the configured number of distinct query paths without
   contacting providers, and only paths selected by the user are searched.
3. Selected provider searches run concurrently under a shared bound and a
   failed path does not cancel successful siblings.
4. Candidates are deduplicated before scoring while preserving distinct-path
   provenance for ranking and explanations.
5. Review-like papers are excluded by default and can be restored with Include
   reviews.
6. Raw citation count no longer contributes a rank and graph rank contributes
   once.
7. Weak candidates are omitted rather than used to fill the requested count.
8. Suggestions show a specific vault-based reason.
9. Fixed-corpus evaluation improves top-five precision over the RFC 0091
   baseline and does not regress the review-enabled fixture.
10. Unit tests cover query-path planning, explicit selection, bounded
    concurrency, partial provider failure, deduplication, review classification,
    hard filters, relevance thresholds, signal composition and result limits.

## Out of scope

- A trained recommendation model
- An additional LLM judging call after retrieval
- Collaborative filtering across users or vaults
- Automatic clustering or splitting of mixed-topic vaults
- Provider-specific paper-type taxonomies
- Changes to the Suggestions layout or progress UI, which belong to RFC 0094
