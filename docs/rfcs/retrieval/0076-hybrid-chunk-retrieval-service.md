# RFC 0076: SearchService — hybrid retrieval over our own papers

Status: Stale
Date: 2026-08-08
Product: i0i
Target: Tauri v2 + SvelteKit (Svelte 5), macOS first
Builds on: RFC 0075 (structural extraction, chunking, chunk embeddings),
RFC 0054 (local ONNX embedding model), RFC 0023 (reader document model).

## Summary

RFC 0075 built the derived-data layer and stopped before querying it. This RFC
queries it, through one service with one entry point:

```text
search(query, paper_ids, vault_ids, mode) → scored chunks
```

Lexical retrieval through FTS5/BM25, semantic retrieval through **sqlite-vec**
KNN, fused by Reciprocal Rank Fusion. Every hit carries its provenance and its
per-signal scores.

**Out of scope:** `readDocumentRanges`, the chat context model (explicit /
ambient / retrieved), neural reranking, context compression, OCR. This RFC
delivers the retrieval primitive; wiring it into chat is the next one.

## The service is a shared primitive

SearchService means *search within papers we already hold* — distinct from
deep-research discovery, which finds papers we do not have.

It is deliberately **agnostic about its caller**. The known consumers already
differ in scope and intent:

```text
reader          search inside the open PDF
vault view      search across one vault
global search   search everything in the library
agent           search to enrich its own context
```

Three consequences follow, and they shape every decision below:

1. **No "current" anything.** Current paper and current vault are UI state. The
   service takes an explicit scope; the command layer fills it in from whatever
   "current" means to that caller. A service that knew about the open PDF could
   not serve the agent or global search.
2. **Scope is a filter, not a mode.** There is no `searchDocument` versus
   `searchLibrary`. One call, and the scope narrows it.
3. **No caller-specific ranking.** Everyone gets the same ordering. If the agent
   eventually needs different behaviour, that is a parameter, not a fork.

### On the name

`SearchManager` already exists (`services/research/manager.rs:30`) with
`searches`, `search_runs`, `search_candidates`, and a `search_papers` command —
all deep-research discovery. Two things called "search" will sit one
autocomplete apart.

Keeping `SearchService` as the name (it is what the operation is called in the
product) but placing it at `services/search/` with a module doc that states the
distinction in its first line. The collision is noted here so it is a known
cost rather than a surprise.

## Scope is a conjunction

`paper_ids` and `vault_ids` are **AND-ed filters**, each empty meaning
"unconstrained on this dimension":

| `paper_ids` | `vault_ids` | resolves to |
| --- | --- | --- |
| `[]` | `[]` | every paper in the library — global search |
| `[p]` | `[]` | paper `p`, wherever it lives |
| `[]` | `[v]` | every paper in vault `v` |
| `[p]` | `[v]` | `p` **if** `p ∈ v`, otherwise **nothing** |
| `[p, q]` | `[v, w]` | those of `p, q` that are in `v` or `w` |

Within a list the members are OR-ed; across the two lists they intersect. This
is ordinary filter semantics, and it composes: "this PDF in this vault" is the
reader's call, "this vault" is the vault view's, "everything" is global search,
and the agent can express any subset without a new endpoint.

### The empty intersection warns, it does not throw

Asking for a paper that is not in the named vault is a legitimate question with
the honest answer "nothing". It is not an error:

```rust
if resolved.is_empty() && !(paper_ids.is_empty() && vault_ids.is_empty()) {
    log::warn!("search scope is empty: papers {paper_ids:?} ∩ vaults {vault_ids:?}");
}
```

The response reports it explicitly rather than looking like a query that simply
found no matches:

```rust
pub struct ScopeSummary {
    pub paper_ids: Vec<String>,   // after resolution
    pub empty_reason: Option<EmptyScope>,  // NoIntersection | NoPapers
}
```

Throwing would be wrong: it turns a routine UI state — a paper open outside the
active vault — into an error dialog. Returning zero hits silently would also be
wrong: the caller cannot tell "nothing matched" from "you asked for an
impossible scope".

## Semantic search: sqlite-vec

Three spikes ran before this section was written, and all three pass in
`library_store.rs`'s test module:

| Spike | Result |
| --- | --- |
| `sqlite_vec_registers_and_answers_knn` | `vec_version()` resolves; `vec0` KNN returns the nearest vector |
| `vec0_partition_key_scopes_knn_per_paper` | `k = 2` within one partition returns that paper's chunks, not the globally nearer ones |
| `vec0_partition_filter_accepts_a_set_of_papers` | `paper_id in (...)` is honoured — out-of-scope partitions do not leak |

That third one is what makes the scope design work with a single query. Without
it, a multi-paper scope would need one KNN per paper, merged.

### It links statically

`sqlite-vec = "=0.1.9"`, registered through `sqlite3_auto_extension` before any
connection opens. **There is no `.dylib` to bundle, sign, or locate at
runtime** — the extension compiles into the binary. The bundled SQLite already
sets `SQLITE_ENABLE_LOAD_EXTENSION=1` (`libsqlite3-sys` `build.rs:132`), and
static registration does not even need it.

Version pinned exactly: **`0.1.10-alpha.4` ships a broken package** — its
`sqlite-vec.c` includes `sqlite-vec-diskann.c`, which is not in the published
crate, so it fails to compile. `0.1.9` builds and passes all three spikes. The
`=` pin is deliberate; a caret range would drift onto that alpha.

### Partitioning is the design, not a detail

```sql
create virtual table document_chunk_vectors using vec0(
  paper_id  text partition key,
  chunk_id  text primary key,
  embedding float[384]
);
```

`paper_id` as partition key rather than a metadata column. A plain `k = n` KNN
searches globally and *then* filters, so scoping to one paper could return zero
hits when that paper's chunks all rank below the global top-n — the failure
would look like "this PDF has nothing relevant" rather than a bug. With a
partition key, `k` means "k within this scope", which is what every caller
actually wants.

Dimensions are fixed in the table definition, so the column encodes the current
model. Changing models is a table rebuild plus a re-embed, which RFC 0075
already treats as the expected path — `model`, `model_version`, and
`dimensions` are on every `document_chunk_embeddings` row precisely so this is
detectable.

### Two tables for one fact, and why

`document_chunk_embeddings` stays as the canonical store; `document_chunk_vectors`
is the index built from it. Not redundancy — a division of labour:

- The table survives a model change, a `vec0` format change, and a corrupted
  index. The index is rebuildable from it in one pass, with no re-embedding.
- `vec0` is a virtual table: it cannot hold a foreign key, cannot cascade, and
  cannot be queried for anything but KNN.

Which raises the same hazard RFC 0075 hit with FTS5 — and the same fix.
`document_chunks` is reachable by cascade from papers, sources, and
extractions, and a cascade never runs the code at the call site. So the vector
index gets a trigger, exactly like `trg_document_chunks_fts_delete`:

```sql
create trigger trg_document_chunk_vectors_delete
after delete on document_chunk_embeddings
begin
  delete from document_chunk_vectors where chunk_id = old.chunk_id;
end;
```

Tested against both cascade paths, as the FTS trigger already is. An orphaned
vector is worse than an orphaned FTS row: it returns a chunk id that no longer
resolves, and the hydration step drops it, so a search silently returns fewer
results than it found.

### Startup verification

The originating spec asked to verify the extension at startup and fail visibly.
With static linking a missing extension is a compile error rather than a
runtime one, but the check is one query and it also catches a `vec0` table that
failed to create:

```text
select vec_version()  →  ok    semantic search enabled
                      →  err   log error, SemanticStatus::Unavailable, keep serving lexical
```

**Visible, not fatal.** Refusing to start would take away a reader and a
lexical search that both work perfectly, over a feature that degrades cleanly.
Visibility comes from `SemanticStatus` on every response, so a caller can say
"semantic search is unavailable" instead of quietly returning worse results.

## The API

```rust
pub struct SearchRequest {
    pub query: String,
    pub paper_ids: Vec<String>,
    pub vault_ids: Vec<String>,
    pub mode: SearchMode,   // Lexical | Semantic | Hybrid (default)
    pub limit: usize,       // default 12
}

pub struct SearchResponse {
    pub hits: Vec<ChunkHit>,
    pub scope: ScopeSummary,
    pub semantic: SemanticStatus,
}

pub struct ChunkHit {
    pub chunk: DocumentChunk,     // text, page range, heading path, block ids
    pub score: f64,               // the ranking score actually used
    pub lexical: Option<Signal>,  // rank + BM25, when lexical ran and matched
    pub semantic: Option<Signal>, // rank + distance, when semantic ran and matched
}

pub struct Signal { pub rank: usize, pub score: f64 }

pub enum SemanticStatus {
    Ran { coverage: EmbeddingCoverage },
    Unavailable,     // sqlite-vec or the model failed to initialize
    NotRequested,    // mode = Lexical
    NoEmbeddings,    // scope has chunks, none embedded yet
}
```

## Fusion

Reciprocal Rank Fusion, `k = 60`:

```text
score(chunk) = Σ  1 / (k + rank_in_that_ranking)
             signals
```

RRF because **BM25 and vector distance cannot be compared numerically.** BM25 is
unbounded and depends on corpus statistics that shift as the library grows;
`vec0` returns an L2 distance where lower is better. Any weighted blend of the
two needs a calibration nobody has measured, and it would silently change
meaning over time. RRF discards magnitudes and uses only ordering — the part of
each signal that is trustworthy across both.

`k = 60` is the constant from the original RRF paper. It flattens the gap
between ranks 1 and 2 relative to the gap between "ranked" and "absent", which
is the behaviour we want: agreement between signals should outrank a single
signal's enthusiasm.

Each signal fetches `limit * 4` candidates before fusion, so a chunk ranked 15th
lexically and 3rd semantically still reaches a top-12 response.

### What `score` means

**The number this response was sorted by, comparable only within this
response.** RRF sum in `Hybrid`, negated BM25 in `Lexical`, negated distance in
`Semantic`.

Not normalized to a friendly 0–100 — that invites comparison across queries and
modes, and every such comparison is meaningless. The per-signal fields let a UI
explain *why* something ranked where it did, which is the honest version of
what a percentage pretends to offer.

## Degradation is reported, not hidden

Continuing RFC 0075's split: the reranker degrades silently, retrieval counts
its holes.

A `Hybrid` request against a paper whose embeddings are still being written
returns lexical hits and reports `NoEmbeddings`. It does not claim to have run
a hybrid search. This case *will* happen — RFC 0075's startup sweep takes
minutes on an existing library, and a user opening a PDF in that window would
otherwise get quietly worse results with nothing to explain it.

## Query embedding

Every semantic request embeds the query: one forward pass, ~10 ms on CPU for
BGE-small, in `spawn_blocking` like every other use of the model. Not cached —
a cache needs invalidation on model change and saves single-digit milliseconds
on repeated identical queries, which is not a thing users do.

## Provenance, and getting back to the page

Each hit carries what RFC 0075 stored: `paper_id`, `page_start`/`page_end`,
`heading_path`, `source_start`/`source_end`, `block_ids`.

Enough to *name* a location, not to draw it. Rectangles are
`block_ids → document_spans → bbox_json`: a second query and a payload several
times the size of the text. Search does not walk it — a result list shows text
and a page number; only a click needs geometry.

`chunk_rects(chunk_id) → Vec<PdfRect>` belongs with the chat-context RFC that
will use it. Named here so the chain RFC 0075 built is visibly complete:
**chunk → blocks → spans → rectangles** works; this RFC just does not walk it on
every search.

## Storage

One new virtual table, one trigger, one backfill. `document_chunk_embeddings`
is unchanged — the index is derived from it.

The backfill mirrors RFC 0075 R6: a startup pass inserting into
`document_chunk_vectors` any embedding row with no vector, so an existing
library indexes itself without re-embedding. The embedding worker also inserts
into both on each write.

New store methods:

| method | purpose |
| --- | --- |
| `resolve_search_scope(paper_ids, vault_ids)` | the conjunction above |
| `lexical_chunk_ranking(paper_ids, query, limit)` | chunk ids + negated BM25 |
| `semantic_chunk_ranking(paper_ids, vector, limit)` | chunk ids + distance, via `vec0` |
| `chunks_by_ids(ids)` | hydrate the fused ranking |
| `index_missing_chunk_vectors()` | backfill |

Ranking and hydration are split deliberately: fusion needs only ids and scores,
so text is loaded once for the ~12 survivors instead of the ~100 candidates the
two signals produced.

### FTS5 escaping

Already implemented and tested in RFC 0075 (`fts_match_query`). FTS5 `MATCH`
takes a query *language* — `AND`, `OR`, `NEAR`, `"`, `*`, `(` all have meaning,
so `BLEU (revised)` is a syntax error, not a search. Every token is quoted,
making each a literal phrase and the whole query an implicit AND.

## Testing

| Unit | Test |
| --- | --- |
| Scope conjunction | Every row of the table above, including `p ∉ v` → empty |
| Empty scope | Warns and reports `EmptyScope`; never returns `Err` |
| Global scope | Both lists empty searches the whole library |
| Lexical ranking | Known corpus, expected order, positive-is-better |
| Semantic ranking | Stub embedder with hand-built vectors; nearest ranks first |
| Partition scoping | A paper's chunks are returned even when globally out-ranked |
| RRF | Pure function over synthetic rankings, including disjoint ones |
| Mode | `Lexical` never touches `vec0`; `Semantic` never touches FTS |
| Vector teardown | Both cascade paths leave no orphaned vector |
| Backfill | Embeddings without vectors get indexed; running twice is a no-op |
| Degradation | No `vec0` → lexical hits plus `Unavailable`, not an error |
| Partial coverage | Half-embedded scope reports `Ran { coverage }` with the gap |
| Hydration order | Fused rank order survives `chunks_by_ids` |

Fusion and scope resolution are pure functions — no database, no model — for
the same reason the chunker is.

## Budgets

| Operation | Target |
| --- | --- |
| Lexical, one paper | < 10 ms |
| Hybrid, one paper | < 50 ms (dominated by query embedding) |
| Hybrid, one vault | < 100 ms |
| Hybrid, whole library | < 250 ms |
| Vector backfill, existing library | < 5 s |

## Alternatives considered

**Brute-force cosine over stored blobs instead of `vec0`.** Genuinely viable at
today's size — a vault is ~1.1 MB of vectors and cosine over it is ~1 ms. It was
the earlier draft of this RFC. Rejected because the operational cost that
argument rested on turned out not to exist: the crate links statically, so there
is no library to bundle or locate, and the spikes proved partition-scoped KNN
works. Brute force would have to be replaced at ~100k chunks anyway, and the
index costs one table and one trigger now.

**A metadata column instead of a partition key.** Filters after the KNN, so a
scoped search can return zero hits when the scope's chunks rank below the global
top-k — a failure that reads as "nothing relevant here".

**Weighted score blending instead of RRF.** Needs a calibration between two
incomparable scales, and the calibration drifts as the corpus grows.

**Union instead of intersection for scope.** Would make "this paper in this
vault" mean "this paper, plus everything in the vault" — the opposite of the
reader's intent, and it makes narrowing impossible to express.

**Returning rectangles with every hit.** Multiplies the payload for data a
result list does not draw.
