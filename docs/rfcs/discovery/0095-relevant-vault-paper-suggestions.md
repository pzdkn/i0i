# RFC 0095: Relevant vault paper suggestions

Status: Implemented
Date: 2026-08-20
Product: i0i
Target: Tauri v2 + SvelteKit (Svelte 5), macOS first
Builds on: RFC 0091 (vault paper suggestions), RFC 0094 (suggestion controls),
RFC 0057 (semantic ranking), RFC 0075 (local embeddings)

## Summary

RFC 0094 made vault suggestions controllable: the user can set a focus and year
range, review several proposed query paths and run only the paths they approve.
Retrieval now fans those paths across OpenAlex and arXiv concurrently.

The remaining problem is ranking quality. The final ranker still treats the
whole vault as one broad centroid, gives citation-graph rank three votes, gives
raw citation count another vote, forgets which query paths found a paper and
fills the requested result count without a strict relevance threshold. Those
choices favor old, central and general papers, especially surveys.

This RFC makes the ranking objective explicit: suggest a small number of
specific papers that advance at least one paper or question in the active
vault, even when that means returning fewer papers than requested.

## Implemented baseline

RFC 0094 currently provides this pipeline:

1. Build a bounded profile from up to 24 vault papers and the optional Focus.
2. Ask one planner call for 1, 3 or 5 distinct query paths without contacting
   providers.
3. Let the user approve the paths to run.
4. Search each selected path through OpenAlex and arXiv concurrently under a
   shared bound. Provider and year constraints apply at retrieval time.
5. Merge and deduplicate provider candidates. Path provenance is currently
   discarded during this merge.
6. Apply the existing RFC 0057 semantic pool reduction against the combined
   vault profile. Its minimum-keep guard deliberately retains up to five weak
   candidates, so it is not a final relevance threshold.
7. Add a bounded two-hop citation neighbourhood and reapply year, owned and
   dismissed filters to graph candidates.
8. Fuse retrieval order, vault-centroid similarity, citation-graph rank three
   times and raw citation-count rank once using reciprocal-rank fusion.
9. Return at most the configured 3, 5 or 10 results.

The query planner, explicit path selection, bounded concurrent retrieval, year
filtering, partial provider failure and run progress belong to RFC 0094 and are
not reimplemented here. RFC 0095 changes the evidence retained from retrieval,
the candidate filters and the final ranker.

## Ranking policy

### Candidate evidence survives deduplication

Each provider result is associated with its query-path id before candidates are
deduplicated. The merged candidate retains a set of distinct path ids that
found it. Two providers returning the same paper for the same path count as one
path hit, not two independent votes.

Provider completion order is not ranking evidence. Concurrent requests finish
in a nondeterministic order, so the final ranker must use the rank within each
provider/path result and merge those ranks deterministically. Stable paper
identity breaks exact ties.

The ranking service maintains one internal evidence record per deduplicated
paper:

- provider/path ranks;
- distinct query-path ids;
- distinct vault-paper citation origins;
- similarity to the nearest vault paper;
- optional similarity to Focus.

This evidence may remain internal to a run. The persisted suggestion reason
records the strongest user-facing evidence; this RFC does not require a new
general-purpose provenance schema.

### User intent narrows the vault

RFC 0094 already applies year bounds during provider retrieval and after graph
expansion. RFC 0095 preserves that invariant.

Focus currently contributes to the combined planning profile. RFC 0095 also
embeds Focus independently and scores every final candidate against it. Focus
does not replace the vault: a candidate must still have evidence connecting it
to at least one paper already present.

### Relate to papers, not only a centroid

A single centroid flattens a mixed-topic vault. Instead, embed each candidate
once and compare it with the available representative embedding for each vault
paper. The candidate's vault-fit score is its highest paper-level similarity,
and the closest paper is retained for its explanation.

This answers the useful question, "Which paper in this vault does this advance?"
It also lets a focused niche survive beside unrelated vault topics. The global
vault centroid may remain an inexpensive retrieval-pool heuristic, but it is
not a final ranking or eligibility signal.

### Prefer specific contributions

The planner prompt asks for concrete primary contributions, methods, findings
and direct extensions of vault papers. It must not add `survey`, `review` or
other broad paper types merely to improve topical coverage.

RFC 0094 already carries an `include_reviews` run option but intentionally does
not expose or apply it. This RFC completes that option. With **Include reviews**
off, a documented, deliberately small title classifier excludes candidates
whose title contains these normalized whole words or phrases:

- `survey`
- `review` or `systematic review`
- `meta-analysis`
- `bibliometric analysis`

The classifier does not use arbitrary substrings and does not claim to identify
every review paper. With Include reviews on, matching papers remain eligible but
receive no ranking boost. The control appears in the Suggestions inspector only
when the filtering behavior ships with it.

### Signals and fusion

Final reciprocal-rank fusion uses each applicable signal once:

1. deterministic merged provider/path retrieval rank;
2. nearest-vault-paper semantic rank;
3. optional Focus-similarity rank;
4. citation-graph distinct-origin rank;
5. distinct-query-path coverage rank, when more than one path ran.

Path coverage contributes at most one rank list regardless of how many paths or
providers returned the paper. Citation-graph rank also contributes once.

Raw global citation count is removed completely from fusion and tie-breaking.
It remains display metadata, not evidence that a paper belongs in this vault.
Exact ties use stable paper identity so identical inputs produce identical
ordering.

Do not introduce learned weights or an additional LLM reranking call in this
RFC. The transparent ranker is cheaper to run, easier to diagnose and suitable
for the fixed-corpus evaluation below.

### Strict relevance floor and result count

`result_count` is a maximum, not a quota. A candidate is eligible only if:

- it passes owned, dismissed, year and review filters; and
- it has a citation edge from at least one vault paper, clears the configured
  nearest-paper similarity floor, or was found through at least two distinct
  approved query paths; and
- when Focus is present, it also clears the configured Focus-similarity floor.

The existing RFC 0057 pool-reduction guard may still keep five candidates for
later scoring. It must not bypass this final eligibility gate. If Focus scoring
is required but unavailable, the run fails clearly and preserves the previous
suggestions rather than silently ignoring Focus. Without Focus, graph or
multi-path evidence may still admit candidates when paper embeddings are
unavailable; the activity feed reports that semantic evidence was unavailable.

Initial thresholds live in `i0i.config.toml` under `[vault_suggestions]` with
concise comments
and conservative defaults. They are backend configuration, not inspector
controls. A run may return zero, two or five candidates; the UI says **No strong
suggestions** when none clear the floor.

### Better reasons

The visible reason names the strongest concrete evidence rather than a generic
topic:

- `Cited by 3 papers in this vault`
- `Most similar to "Sparse Transformers" (86%)`
- `Matches focus "empirical methods" and "Sparse Transformers"`
- `Found through 2 paths: token pruning + efficient inference`

Do not expose the fused score. If only weak generic wording can be produced,
the candidate should probably have failed the relevance floor.

## Evaluation

Ranking changes require a small fixed evaluation before replacing the RFC 0094
ranker. Create fixtures from at least three real vault shapes:

- one narrow methods vault;
- one mixed-topic vault;
- one vault where surveys are intentionally useful.

For each vault, freeze the candidate pool together with provider/path ranks,
path ids, graph origins, paper-level similarities and human labels
`strong | plausible | irrelevant`. Also label whether each candidate is a
review. Compare the RFC 0094 and RFC 0095 rankers on:

- precision among the first five;
- number of irrelevant candidates shown;
- review share with Include reviews off;
- useful result count after the strict relevance floor;
- deterministic output after input arrival order is shuffled.

The candidate pool is fixed so the test measures ranking policy rather than
provider drift. Network retrieval remains covered separately by RFC 0094's
integration tests.

## Acceptance criteria

1. Deduplication preserves distinct query-path and graph-origin evidence for
   each candidate.
2. Shuffling provider completion order does not change final suggestions.
3. Final vault similarity uses the nearest vault paper rather than the global
   vault centroid and retains that paper for explanations.
4. Focus is scored independently and acts as an eligibility gate when present.
5. Review-like papers are excluded by default and can be restored with Include
   reviews.
6. Raw citation count no longer contributes to rank or tie-breaking, and graph
   and path-coverage signals each contribute at most once.
7. Weak candidates are omitted rather than used to fill the requested maximum;
   the RFC 0057 minimum-keep guard cannot bypass the final relevance floor.
8. Suggestions show a specific vault-based reason and zero eligible results use
   the **No strong suggestions** state.
9. Fixed-corpus evaluation improves top-five precision over the RFC 0094
   baseline and does not regress the review-enabled fixture.
10. Tests cover provenance-preserving deduplication, arrival-order independence,
    review classification, hard filters, nearest-paper and Focus thresholds,
    missing-semantic behavior, signal composition and result limits.
11. The implemented and verified feature updates this RFC's status and the
    repository changelog.

## Out of scope

- A trained recommendation model
- An additional LLM judging call after retrieval
- Collaborative filtering across users or vaults
- Automatic clustering or splitting of mixed-topic vaults
- Provider-specific paper-type taxonomies
- Changes to query preparation, concurrency, layout or progress UI, which
  belong to RFC 0094

## Verification

- `cargo test --lib`: 444 passed, 5 intentionally ignored
- `pnpm check`: 0 errors and 0 warnings
- `pnpm build`: completed successfully; the existing large-chunk warning remains
- The frozen three-vault corpus improves precision for narrow and mixed vaults
  and preserves the useful review in the review-enabled vault
