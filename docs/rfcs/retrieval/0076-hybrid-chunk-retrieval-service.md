# RFC 0076: Hybrid chunk retrieval service

Status: Proposed
Date: 2026-08-08
Product: i0i
Target: Tauri v2 + SvelteKit (Svelte 5), macOS first
Builds on: RFC 0075 (structural extraction, chunking, chunk embeddings),
RFC 0054 (local ONNX embedding model), RFC 0023 (reader document model).

## Summary

RFC 0075 built the derived-data layer and stopped before querying it. This RFC
queries it.

One entry point:

```text
search(query, paper_ids, vault_ids, mode) → scored chunks
```

Lexical retrieval through FTS5/BM25, semantic retrieval through cosine over the
`f32` blobs RFC 0075 already stores, and the two fused by Reciprocal Rank
Fusion. Every hit carries its provenance and its per-signal scores.

**Out of scope, still deferred:** `readDocumentRanges`, the chat context model
(explicit / ambient / retrieved), neural reranking, context compression, and
OCR. This RFC delivers retrieval; wiring retrieval *into* chat is the next one.

## Naming: this is retrieval, not search

`SearchManager` already exists (`services/research/manager.rs:30`), along with
`searches`, `search_runs`, `search_candidates`, and a `search_papers` command.
In this codebase **"search" means deep-research discovery** — finding papers
that are not in the library yet.

What this RFC builds is the opposite motion: finding passages *inside* papers
already held. Calling it `SearchService` would put two unrelated concepts one
autocomplete apart, in a codebase where one of them already owns four tables.

So: module `services/retrieval`, type `RetrievalService`, command
`retrieve_chunks`. The user-facing verb stays "search" — that is what the
operation is called in the product — but the code says retrieval.

This RFC also opens `docs/rfcs/retrieval/`. RFC 0075 stayed in `reader/`
because it was mostly about the document model; from here the series is
retrieval proper.

## The API

```rust
pub struct RetrievalRequest {
    pub query: String,
    pub paper_ids: Vec<String>,
    pub vault_ids: Vec<String>,
    pub mode: RetrievalMode,   // Lexical | Semantic | Hybrid (default)
    pub limit: usize,          // default 12
}

pub struct RetrievalResponse {
    pub hits: Vec<ChunkHit>,
    pub scope: ScopeSummary,
    pub semantic: SemanticStatus,
}

pub struct ChunkHit {
    pub chunk: DocumentChunk,     // text, page range, heading path, block ids
    pub score: f64,               // the ranking score actually used
    pub lexical: Option<Signal>,  // rank + BM25, when lexical ran and matched
    pub semantic: Option<Signal>, // rank + cosine, when semantic ran and matched
}

pub struct Signal {
    pub rank: usize,
    pub score: f64,
}
```

### Scope, and a conflict in the original spec

The originating spec said `paper_ids` defaults to the current paper and
`vault_ids` defaults to the current vault. Those two defaults contradict each
other: applied together they resolve to the whole current vault, which makes
the paper default dead code, and every "search this PDF" call would silently
search twelve papers.

Resolved as:

- **Empty request scope means the current paper.** Narrowest and most
  predictable; it is what "search this document" must mean.
- **`vault_ids` is opt-in.** Naming a vault widens the scope; nothing widens it
  implicitly.
- **Papers and vaults union.** Naming a vault *and* a paper outside it means
  "search both" — that is what someone assembling a context expects.

And the part that cannot live in the backend: **"current" is not a concept the
store has.** The service takes an explicit scope; the Tauri command layer fills
in the current paper from reader state. An empty scope reaching the service
searches nothing and says so in `ScopeSummary`, rather than quietly falling
back to the entire library — a query that silently searches 500 papers instead
of one is worse than a query that returns nothing.

## Semantic search without sqlite-vec

The originating spec was explicit: use sqlite-vec, verify the extension loads
at startup, fail visibly if it does not. This RFC does not, and the reason is
worth stating plainly rather than burying.

**Brute-force cosine is faster than the index at this library's size.** The
vectors are already stored as little-endian `f32` (RFC 0075). Loading a scope's
worth and scoring them in Rust costs:

| scope | chunks | vector bytes | cosine cost |
| --- | ---: | ---: | ---: |
| one paper | ~60 | ~92 KB | ~0.1 ms |
| one vault (12 papers) | ~700 | ~1.1 MB | ~1 ms |
| whole library today | ~700 | ~1.1 MB | ~1 ms |
| 100 papers | ~6,000 | ~9 MB | ~10 ms |

384 dimensions at 4 bytes each is 1.5 KB per chunk. A dot product over 6,000 of
them is a few million flops — below the noise floor of the IPC round trip that
delivers the result.

What sqlite-vec would add today: a native extension to bundle and load per
platform, a startup health check, a second index to keep in sync with
`document_chunks`, and a new failure mode where the app refuses to start
because a `.dylib` did not load. What it would buy: nothing measurable until
roughly **100k chunks**, around 1,500 papers.

So the exclusion is not a deferral of quality, it is a deferral of operational
cost, and it is reversible for free — the stored blob format is exactly what
`vec0` consumes, so adopting it later is an index build over existing rows with
no re-embedding. **The threshold is written into the code as a logged warning**
when a scope exceeds 50k chunks, so the decision revisits itself rather than
depending on someone remembering this document.

This does mean the originating spec's "verify sqlite-vec at startup and fail
visibly" has no subject in v1. Its intent — never silently degrade to lexical
while pretending to be hybrid — is served instead by `SemanticStatus` below.

## Fusion

Reciprocal Rank Fusion, `k = 60`:

```text
score(chunk) = Σ  1 / (k + rank_in_that_ranking)
             signals
```

RRF because **BM25 and cosine cannot be compared numerically.** BM25 is
unbounded and depends on corpus statistics; cosine is bounded in `[-1, 1]` and
depends on the model. Any weighted blend of the two raw numbers requires a
calibration nobody has measured, and it would silently change meaning as the
library grows and BM25's IDF terms shift. RRF discards the magnitudes and uses
only the ordering, which is the only part of each signal that is trustworthy
across both.

`k = 60` is the standard constant from the original RRF paper. It flattens the
difference between ranks 1 and 2 relative to the difference between "ranked at
all" and "absent", which is the behaviour we want: a chunk both signals agree
on should beat a chunk that one signal loved and the other did not see.

Each signal retrieves `limit * 4` candidates before fusion, so a chunk ranked
15th lexically and 3rd semantically still surfaces in a top-12 request.

### What `score` means

`score` is **the number this response was sorted by, comparable only within
this response.** In `Hybrid` it is the RRF sum; in `Lexical` it is the negated
BM25; in `Semantic` it is the cosine.

Deliberately not normalized to a friendly 0–100. A normalized score invites
comparison across queries and across modes, and every such comparison would be
meaningless. The per-signal `lexical` and `semantic` fields are there so a UI
can explain *why* a hit ranked where it did, which is the honest version of
what a percentage pretends to offer.

## Degradation is reported, not hidden

Continuing RFC 0075's policy: the reranker degrades silently, retrieval counts
its holes.

```rust
pub enum SemanticStatus {
    Ran { coverage: EmbeddingCoverage },
    ModelUnavailable,       // no embedder loaded
    NotRequested,           // mode = Lexical
    NoEmbeddings,           // scope has chunks, none embedded yet
}
```

A `Hybrid` request against a paper whose embeddings are still being written
returns lexical results and says `NoEmbeddings` — it does not pretend to have
run a hybrid search. This is the case that will actually happen: the startup
sweep takes minutes on an existing library, and a user opening a PDF in that
window would otherwise get quietly worse results with no indication why.

## Query embedding

Every semantic request embeds the query — one forward pass, ~10 ms on CPU for
BGE-small. Not cached. A cache would need invalidation on model change and
would save single-digit milliseconds on repeated identical queries, which is
not a thing users do.

The embedding runs in `spawn_blocking`, like every other use of the model.

## Provenance, and getting back to the page

Each hit carries what RFC 0075 stored: `paper_id`, `page_start`/`page_end`,
`heading_path`, `source_start`/`source_end`, and `block_ids`.

That is enough to *name* the location but not to draw it. Turning a hit into
rectangles is `block_ids → document_spans → bbox_json`, which is a second query
and a payload several times the size of the text. Retrieval does not do it — a
result list shows text and a page number, and only a click needs geometry.

The resolve step (`chunk_rects(chunk_id) → Vec<PdfRect>`) belongs with the
chat-context RFC that will use it. Naming it here so the chain RFC 0075 built
is visibly complete: **chunk → blocks → spans → rectangles** works today; this
RFC simply does not walk it on every search.

## Storage additions

None. This RFC is queries over RFC 0075's schema.

New store methods:

| method | purpose |
| --- | --- |
| `resolve_search_scope(paper_ids, vault_ids)` | union of papers and vault members |
| `lexical_chunk_ranking(paper_ids, query, limit)` | chunk ids + negated BM25 |
| `chunk_vectors_for_papers(paper_ids, model, version)` | vectors for in-memory cosine |
| `chunks_by_ids(ids)` | hydrate the fused ranking |

The split between "rank ids" and "hydrate text" is deliberate: fusion needs
only ids and scores, so text is loaded once, for the ~12 chunks that survive,
instead of for the ~100 candidates each signal produced.

### FTS5 query escaping

FTS5 `MATCH` takes a query *language* — bare `AND`/`OR`/`NEAR`, `"`, `*`, and
`(` all have meaning. A user typing `BLEU (revised)` or `C++` gets a syntax
error, not a search. Every token is quoted before it reaches `MATCH`, making
each a literal phrase and the whole query an implicit AND. Already implemented
and tested in RFC 0075 (`fts_match_query`).

## Testing

| Unit | Test |
| --- | --- |
| Scope resolution | Papers ∪ vault members, deduplicated; empty scope stays empty |
| Lexical ranking | Known corpus, expected order; BM25 returned positive-is-better |
| Semantic ranking | Stub embedder with hand-built vectors; nearest chunk ranks first |
| RRF | Pure function over two synthetic rankings, including disjoint ones |
| Mode | `Lexical` never loads vectors; `Semantic` never touches FTS |
| Degradation | No embedder → lexical hits plus `ModelUnavailable`, not an error |
| Partial coverage | Half-embedded scope reports `Ran { coverage }` with the gap |
| Escaping | `(`, `*`, `AND`, unbalanced quotes all return results, never an error |
| Hydration order | Fused rank order survives `chunks_by_ids` |

Fusion and scoring are pure functions over synthetic rankings — no database, no
model — for the same reason the chunker is: the part most likely to need tuning
should be the part testable in microseconds.

## Budgets

| Operation | Target |
| --- | --- |
| Lexical search, one paper | < 10 ms |
| Hybrid search, one paper | < 50 ms (dominated by query embedding) |
| Hybrid search, one vault | < 100 ms |

## Alternatives considered

**sqlite-vec now.** Covered above: real operational cost, no measurable benefit
below ~100k chunks, and free to adopt later because the blob format already
matches.

**Weighted score blending instead of RRF.** Requires calibrating two
incomparable scales, and the calibration drifts as the corpus grows. RRF needs
no constants beyond `k`.

**Returning rectangles with every hit.** Multiplies the payload for data a
result list does not draw. Deferred to a resolve call.

**Defaulting an empty scope to the whole library.** Rejected — the failure is
silent and expensive, and it turns "search this PDF" into a library scan when a
caller forgets to pass scope.
