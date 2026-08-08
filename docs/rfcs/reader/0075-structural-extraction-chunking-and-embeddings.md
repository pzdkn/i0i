# RFC 0075: Structural extraction, chunking, and chunk embeddings

Status: Proposed
Date: 2026-08-08
Product: i0i
Target: Tauri v2 + SvelteKit (Svelte 5), macOS first
Builds on: RFC 0023 (reader document model), RFC 0025 (extractor evaluation),
RFC 0054 (local ONNX embedding reranker), RFC 0058 (highlight primitive),
RFC 0074 (sticky notes and annotation tools).

## Summary

This is phase 1 of local PDF context and retrieval. It builds the derived-data
layer that hybrid retrieval will sit on, and stops there:

```text
PDF
→ structural extraction     (blocks + spans + geometry — new)
→ semantic chunks           (new)
→ FTS5 lexical index        (new)
→ chunk embeddings          (new, stored but not yet queried)
```

**Explicitly out of scope, deferred to phase 2 and later:** sqlite-vec, vector
KNN, hybrid rank fusion, `searchDocument` / `readDocumentRanges`, the chat
context model, and OCR.

The phase boundary is drawn where it is because embeddings persist as raw
little-endian `f32` blobs — byte-for-byte what `vec0` consumes. Phase 2 builds
an index over rows that already exist. **It does not re-embed anything.** That
is the entire reason splitting here is safe rather than merely convenient.

## Context: the premise that turned out to be false

The originating spec opens with "existing `document_pages`, `document_blocks`,
and `document_spans` remain the source representation of the PDF." That is the
design in RFC 0023. It is not what the code does.

`PdfiumBasicAdapter` (`src-tauri/src/pdf_extraction.rs:327-405`) emits **one
block per page**: the whole page's text as a single `kind: "paragraph"` block,
`block_index: 0`, `bbox_json: None`. And `document_spans` is *never written* —
the table is read into the library snapshot and deleted on re-extract, and no
code path inserts a row.

Two consequences drive this RFC:

1. **Chunking has nothing to respect.** "300–600 tokens respecting paragraphs,
   headings, sections" is unimplementable over one page-sized block. Without
   the extractor upgrade, chunking degenerates to fixed character windows.
2. **A chunk cannot resolve to a rectangle.** The spec's
   `chunk → blocks → spans → bboxes` chain terminates at a block with a null
   bbox. Every downstream promise — highlighting a retrieved passage, jumping
   from an answer back to evidence, citation provenance — depends on geometry
   that is not being captured.

So "updating the PDF processing" is not a supporting task here. It is the
foundation, and it is the largest piece of work in this RFC.

## Decisions taken before drafting

**Extractor: upgrade the in-repo pdfium adapter.** RFC 0025's spike concluded
MinerU is the strongest production candidate, and that conclusion stands — but
MinerU is a Python runtime plus model downloads shipped alongside the app, a
larger lift than everything else in this RFC combined. It gets its own RFC.
`pdfium-render` is already a dependency and exposes enough geometry to produce
real blocks and spans today, with no new runtime.

**Embedding model: ship phase 1 on the existing BGE-small.** The spec names
Qwen3-Embedding-0.6B. `fastembed` cannot run it correctly: it has no Qwen
variant, and its only pooling modes are `Cls` and `Mean`
(`fastembed-4.9.1/src/pooling.rs:4`), while Qwen3-Embedding requires
**last-token** pooling. Loading it through `UserDefinedEmbeddingModel` would
apply the wrong pooling to a decoder's output and produce quietly wrong
vectors — the worst failure mode available, because nothing would look broken.

Running Qwen3 properly means a new `ort`-backed backend with our own
tokenization, last-token pooling, and normalization, plus owning the download
of a ~600 MB ONNX file. That is a real piece of work and it is not this one.
Phase 1 uses the model already loaded and working (BGE-small-en-v1.5, 384-dim),
proves the chunk → embed → store pipeline end to end, and records `model`,
`model_version`, `dimensions`, and `chunk_version` on every row so that
adopting Qwen3 is a re-embed against unchanged schema.

## R1 — Structural extraction

Replace `PdfiumBasicAdapter` with an adapter that produces sub-page structure.

`page.text().segments()` yields `PdfPageTextSegment`, each carrying `.text()`
and `.bounds() -> PdfRect`. From that:

```text
segments
  → group by y-overlap                    → lines
  → group by vertical gap + x-indent      → blocks
  → each segment persisted as-is          → spans (with bbox)
```

Every block gets a `bbox_json` (the union of its spans' rects, normalized to
the page box 0..1, matching how `Locator::PdfRect` already stores rectangles).
Every span gets its own `bbox_json` and its `source_start`/`source_end` into
the canonical `source_text`.

Block `kind` comes from a font-size heuristic over `chars()`: a short line
whose scaled font size sits meaningfully above the page's modal body size
becomes `"heading"`; everything else stays `"paragraph"`. This is deliberately
crude. Real layout classification is what MinerU is for; here we need only
enough signal to give the chunker a boundary worth respecting.

### Canonical offsets

R1 multiplies the `"\n\n"` separators in `source_text`, which makes the offset
contract load-bearing where it previously could not be observed. Stated once:

- `source_text` is block `text` joined by `"\n\n"`, in reading order.
- Each block satisfies `text == source_text[source_start..source_end)`.
- Spans within a block are joined by a single space, and every span's range
  nests inside its block's range.

Without this written down, `chunk → block → span → bbox` drifts by a character
or two per block and surfaces much later as a highlight landing a line off.

### Version bump, but not a rename

`EXTRACTOR_VERSION` (`pdf_extraction.rs:25`) bumps to `0.2.0`.

**The bump alone is inert, which this RFC originally got wrong.** An earlier
draft claimed `stale_document_extractions` re-queues anything whose version does
not match. It does not — it selects on `status = 'extracting'`, i.e. crash
recovery, and never looks at the version. Nor does
`cached_pdf_sources_without_ready_extraction`, which skips any source holding
*a* ready extraction whatever version produced it. The error survived every unit
test and was caught only by running against the reference vault, where all 11
extractions sat at `0.1.0` with page-sized blocks and zero spans while the new
code reported success.

So the migration needs a third sweep, `outdated_document_extractions`, which
`recover_and_queue_startup_extractions` runs with `force = true` (the extraction
is `ready`, so the non-forced path would skip it). Forcing deletes the old
extraction; its pages, blocks, spans, chunks, and embeddings cascade away with
it, which is correct — all of it is derived from the PDF.

`EXTRACTOR` stays `"pdfium_basic"` even though the adapter is no longer basic,
and the name is doing more work than it looks. It is a component of two derived
identifiers — `document_extraction_id` = `extraction:{extractor}:{source_id}`
(`library_store.rs:3833`) and `annotation_source_id` = `{EXTRACTOR}:{source_id}`
(`pdf_extraction.rs:493`). Renaming it does not re-key existing extractions; it
mints *new* ones beside them, and the originals become invisible to
`stale_document_extractions`, which filters by extractor name. They would never
be cleaned up.

Marks survive a rename either way — they carry `document_sources.id`, which
`ReaderDocument.source_id` exposes (`reader_service.rs:591`), not the annotation
source id — but there is no reason to leave orphans behind for a cosmetic
change. A rename is a migration, and it is not this RFC's.

### This cannot mis-anchor an existing mark

The obvious fear — re-extracting every PDF invalidates the reference vault's
75 marks — does not apply, and it is worth writing down why rather than
re-deriving it later.

PDF marks anchor as `Locator::PdfRect { source_id, page_index, rects_json }`
(`src-tauri/src/domain/highlight.rs:39-47`): a page index and normalized
rectangles. They do not reference blocks, spans, or text offsets. Nothing about
block granularity can move them.

`source_text` — the `"\n\n"` join of block text — is derived at read time
(`src-tauri/src/services/reader_service.rs:558`) and never persisted, so no
stored offset is measured against it. The `Locator::TextOffset` and
`TextPoint` variants are the HTML ingestion path, which this RFC does not
touch — and that holds for R3 as well, because `finish_document_extraction`
has exactly one caller, `pdf_extraction.rs:269`. HTML documents never reach it,
so putting chunking inside it cannot reach HTML marks. Chunking HTML is
probably worth doing later; it is not something this RFC should acquire by
accident.

**Risk that does remain:** the reader renders its text layer from blocks. Going
from 1 block per page to ~40 changes what it draws. Verifying the reader
against the reference vault is part of this work, not an afterthought.

## R2 — De-scope the library snapshot

`read_library` (`src-tauri/src/storage/library_store.rs:2125-2136`) loads
**every block and span in the entire library** into one `LibrarySnapshot`, and
`reader_service` then filters by `extraction_id` in memory
(`reader_service.rs:503, 522`).

`get_library` is a Tauri command (`commands/library.rs:20`), so the snapshot is
not merely built in memory — it is serialized to JSON, pushed across IPC, and
parsed by the frontend. And it is not only a startup cost: six store mutations
return a full snapshot — `create_vault`, `rename_vault`, `delete_vault`,
`remove_paper_from_vault`, `delete_paper_globally`, `add_local_pdf_to_vault`.

Measured against the current reference library (12 papers, 269 pages, 269
blocks, 0 spans, 975 KB of block text), **renaming a vault already ships ~1.3 MB
of PDF body text through IPC to redraw a label.** So this is not a purely
pre-emptive fix; R1 makes an existing problem acute.

The identity columns are what scale badly. Measured averages: block id ~70
chars, `extraction_id` ~59, `source_id` ~35, `paper_id` ~18. A span row carries
all of those plus a `block_id`, so it costs ~250 bytes of identifiers and ~120
bytes of JSON field names — **~370 bytes before any text**. At ~10 blocks and
~60 line segments per page:

| | today | after R1 |
| --- | ---: | ---: |
| blocks | 269 | ~2,700 |
| spans | 0 | ~16,000 |
| identifier + field-name overhead | ~90 KB | ~6.8 MB |
| text (duplicated at span level) | 975 KB | ~1.9 MB |
| **snapshot JSON per call** | **~1.3 MB** | **~8.7 MB** |

That is twelve papers, and it scales linearly: a hundred-paper library
approaches 70 MB per call.

The waste is structural rather than incidental. Every consumer already narrows
to a single `extraction_id`, so rendering one open paper loads all twelve and
discards eleven-twelfths.

`document_blocks` and `document_spans` come out of `LibrarySnapshot`. The
reader loads them per-extraction through a new store method — which is what the
callers were doing by hand anyway. This is scope this RFC adds to the
originating spec; R1 is not shippable without it.

Paginating or caching the snapshot would treat the size as the problem. The
problem is that document-scoped data is living in a library-scoped struct.

`LibrarySnapshot` is a bridge contract, not only a Rust struct — removing two
fields changes `src/lib/bridge/library.ts` and every consumer of the snapshot
type. Small, but it is a frontend change hiding inside a storage refactor.

## R3 — Chunking

A pure function: ready blocks in reading order → chunks.

```text
walk blocks in reading order
  accumulate into the current chunk
  never split a block across chunks

  hard boundary   the next block is a heading
                  → always close; the heading opens the next chunk
  hard boundary   the token estimate reaches the cap
  soft boundary   the next block starts a new page
                  → close only if the chunk already meets the minimum
```

Three details the shape above is carrying:

A heading **opens** the chunk that follows it rather than closing the one
before it. A heading stranded at the tail of the preceding chunk describes text
it does not introduce, which is exactly backwards for retrieval. `heading_path`
carries the nearest heading in force, so every chunk knows which section it came
from even when the heading itself sits several chunks back. (True section
nesting waits for an extractor that reports heading *levels*; the font-size
heuristic in R1 does not.)

**A heading does not close a chunk that is only headings.** Consecutive headings
are everywhere — "4 Experiments" then "4.1 Setup", and a title page where title,
authors, and affiliation are all large type. Without this guard each becomes its
own chunk: on the reference vault, **259 of 1,032 chunks came back under 20
tokens**, each costing an embedding and competing for a slot in the results
while carrying nothing retrievable. With it, 18 of 786. The label snapshots to
the *nearest* heading while the chunk is still headings-only, and fixes once
body text arrives.

A page break is **soft**, not hard. Making it hard would guarantee no chunk
spans pages — tidy, and it would render `page_start`/`page_end` permanently
equal — but a paragraph continuing across a page break is the single most
common structure in a paper, and hard-breaking there produces a stub chunk at
every page top. Closing only when the chunk already meets its minimum keeps the
common case whole and still prefers page-aligned boundaries when it can have
them for free.

Token count is estimated at 4 characters per token. A real tokenizer is not
worth a dependency for a bound this soft.

**Size target: 300–450 tokens (~1200–1800 characters), minimum 150, hard cap
500.** The
originating spec says 300–600. BGE-small truncates input at 512 tokens, so a
600-token chunk would be embedded with its tail silently discarded — the text
would still be in FTS5 and still be readable, and the vector would quietly
describe only part of it. The ceiling is a property of the model, not of the
document, which is why it is recorded in `chunk_version`. Adopting Qwen3
(32k context) later permits larger chunks as a re-chunk, with no schema change.

Chunking runs **synchronously inside `finish_document_extraction`, in the same
transaction as pages, blocks, and spans**. An extraction is therefore never
`ready` without its chunks. This is worth a moment: the alternative — a second
queue with its own status column and its own recovery path — buys nothing.
Chunking needs no model and no network, and a 20-page paper chunks in well
under 500 ms. Making it an invariant of a ready extraction removes an entire
state machine from the system.

`finish_document_extraction` (`library_store.rs:419`) grows parameters for
spans and chunks alongside pages and blocks.

## R4 — Schema

```sql
create table if not exists document_chunks (
  id text primary key,
  paper_id text not null,
  source_id text not null,
  extraction_id text not null,
  chunk_index integer not null,      -- reading order within the extraction
  chunker text not null,             -- 'structural'
  chunk_version integer not null,
  page_start integer not null,
  page_end integer not null,
  heading_path text,                 -- 'Introduction > Motivation', nullable
  text text not null,
  token_estimate integer not null,
  source_start integer not null,
  source_end integer not null,
  created_at text not null,
  unique (extraction_id, chunk_index),
  foreign key (paper_id) references papers(id) on delete cascade,
  foreign key (source_id) references document_sources(id) on delete cascade,
  foreign key (extraction_id) references document_extractions(id) on delete cascade
);

create index if not exists idx_document_chunks_extraction_id
  on document_chunks(extraction_id);

create table if not exists document_chunk_blocks (
  chunk_id text not null,
  block_id text not null,
  ordinal integer not null,
  primary key (chunk_id, block_id),
  foreign key (chunk_id) references document_chunks(id) on delete cascade,
  foreign key (block_id) references document_blocks(id) on delete cascade
);

-- The reverse lookup: which chunk covers this block? Needed to highlight a
-- retrieved chunk, and to answer "what is the chunk for the passage I selected".
create index if not exists idx_document_chunk_blocks_block_id
  on document_chunk_blocks(block_id);

create virtual table if not exists document_chunks_fts using fts5(
  chunk_id unindexed,
  text,
  tokenize = 'unicode61 remove_diacritics 2'
);

create table if not exists document_chunk_embeddings (
  chunk_id text primary key,
  model text not null,
  model_version text not null,
  dimensions integer not null,
  chunk_version integer not null,
  embedding blob not null,           -- little-endian f32, dimensions * 4 bytes
  created_at text not null,
  foreign key (chunk_id) references document_chunks(id) on delete cascade
);

create index if not exists idx_document_chunk_embeddings_model
  on document_chunk_embeddings(model, model_version);
```

FTS5 availability is not an assumption: `libsqlite3-sys 0.37.0`'s bundled build
compiles with `-DSQLITE_ENABLE_FTS5` unconditionally (`build.rs:132`), along
with `-DSQLITE_ENABLE_LOAD_EXTENSION=1`, which is what will let phase 2 load
sqlite-vec. A test asserts the FTS5 round trip rather than trusting the flag.

### Chunks carry offsets, not geometry

The originating spec says chunks need not duplicate spatial geometry. That is
right for *rectangles* — a chunk resolves to blocks, blocks resolve to spans,
spans carry bboxes — and wrong for *offsets*. `document_chunk_blocks` alone
cannot locate a chunk that covers part of its first or last block, and the
offsets are already being maintained during extraction
(`pdf_extraction.rs:378-388`). Storing them costs two integers and removes a
join plus a reconstruction step from every read.

### FTS5 is standalone, not external-content

The conventional choice is `content='document_chunks'`, which avoids storing
the text twice. It keys rows on `rowid`, and this repository rebuilds tables:
`relax_highlight_color_not_null` (`library_store.rs:3905`) drops and recreates
`highlights` wholesale, and the same pattern will be needed again, because
`create table if not exists` is inert on an existing database. A rebuild
silently renumbers `rowid` and invalidates every external-content FTS row, with
no error — searches simply return wrong rows.

A standalone table storing `chunk_id` and a copy of the text is immune to that,
needs no sync triggers, and costs one extra copy of text that is disposable
derived data. Queries join back on `chunk_id`, which is the primary key.

## R5 — Embedding worker

`services/embedding/build()` (`src-tauri/src/services/embedding/mod.rs:30`)
currently loads the model and wraps it in an `EmbeddingReranker`. It is
refactored to return the loaded `Arc<dyn TextEmbedder>` once, feeding both the
reranker and a new chunk-embedding worker. Loading BGE twice would waste ~130 MB
for no reason, and `TextEmbedder` is already the right seam — no new abstraction
is introduced.

The worker sweeps for chunks with no `document_chunk_embeddings` row at the
current model, model version, and chunk version; embeds them in batches inside
`spawn_blocking`; writes the vectors. Absence of a row *is* the "not yet
embedded" state — no status column, no state machine, nothing to recover.

It runs at two moments, mirroring how extraction is already driven
(`pdf_extraction.rs:158, 197`):

- **On startup** — one sweep, which picks up chunks left behind by a crash, a
  model change, or a `chunk_version` bump.
- **On extraction ready** — woken by the `document_extraction_updated` event
  that the extractor already emits, so the extractor never has to know this
  worker exists.

**Both are required, and the second is easy to skip.** On the launch that
migrates an upgraded library, the startup sweep runs *before* re-extraction has
written any new chunks — so it embeds nothing, and re-extraction then cascades
away the embeddings that did exist. The reference vault landed at 1,032 chunks
and **zero** embeddings exactly that way. A `tokio::sync::Notify` coalesces the
burst, so eleven extractions finishing together produce one sweep.

Because the work list is a query rather than a queue, both triggers are the same
code path and running them twice is harmless.

### Failure policy: two contracts, deliberately

The originating spec says the system should "fail visibly rather than silently
degrading." The existing embedding module says the opposite: on any failure,
return empty and let ranking fall back to legacy weights, plus a runtime kill
switch at `search.reranker_enabled`. These are about to live in one module, so
the split is stated rather than left to whoever reads it next.

**The reranker's contract is unchanged.** A failed rerank costs relevance on one
query. Falling back is correct and the user loses nothing they can name.

**Chunk embedding reports coverage.** It is a persistence job with a definite
completion state, so it can be counted: `embedded / total` chunks per paper. A
missing embedding is a hole in retrieval that a user could never see on their
own, which is precisely why it must be counted rather than swallowed. Failures
log and retry on the next sweep. Model-load failure logs an error and reports
zero coverage; it does not crash the app, because a paper with no embeddings is
still readable and still fully searchable through FTS5.

This is not a contradiction of the spec's "fail visibly." Visible means
*counted and reportable*, and in phase 1 zero coverage costs nothing — FTS5
answers every query on its own. When phase 2 makes vector search load-bearing,
coverage is already there to gate on.

## R6 — Teardown and backfill

**Teardown.** `clear_extraction_children` (`library_store.rs:3837`) deletes
derived rows by `extraction_id`. `document_chunks` joins it; foreign keys
cascade to `document_chunk_blocks` and `document_chunk_embeddings`.
`document_chunks_fts` needs an **explicit delete** — virtual tables do not
participate in foreign key cascades. Missing this orphans FTS rows pointing at
chunk ids that no longer exist, and the orphans stay searchable.

**Backfill.** Every existing extraction has zero chunks. A startup sweep finds
ready extractions with no chunks at the current `chunk_version` and re-chunks
them, mirroring `recover_and_queue_startup_extractions`
(`pdf_extraction.rs:197`). In practice the `EXTRACTOR_VERSION` bump in R1 makes
every extraction re-run anyway on first launch after this ships; the sweep is
what keeps a later chunker change from requiring a full re-extraction.

## Testing

| Unit | Test |
| --- | --- |
| Block grouping | Synthetic segments with known geometry group into expected lines and blocks |
| Heading heuristic | A large short line classifies as heading; body text does not |
| Chunker | Pure function over synthetic blocks: respects target size, never splits a block, a heading opens the next chunk, a page break below the minimum does not close one |
| FTS5 | Round trip through a real connection — asserts the build actually has FTS5 |
| Teardown | Re-extract leaves no orphaned chunk, chunk_block, embedding, or FTS row |
| Embedding worker | Stub `TextEmbedder`; asserts coverage accounting and that a failure leaves the chunk retryable |
| Snapshot | `get_library` no longer returns blocks or spans; the reader still resolves them |

The chunker being a pure function over blocks is the point of the whole design:
the part most likely to need tuning is the part testable without a PDF, a
model, or a database.

## Budgets

| Operation | Target |
| --- | --- |
| Chunk a 20-page paper | < 500 ms |
| Embed a 20-page paper (BGE-small, CPU) | < 30 s |
| `get_library` after R2 | no worse than today |

These are pass/fail for phase 1, not aspirations. The embedding budget is the
one to watch when Qwen3 arrives: ~18x the parameters and 1024 dimensions is a
different cost class, and that RFC will need to restate this number rather than
inherit it.

## What phase 2 inherits

```text
document_chunks            text + provenance + offsets
document_chunk_blocks      chunk → block → span → bbox
document_chunks_fts        lexical retrieval, working
document_chunk_embeddings  f32 blobs, vec0-ready
```

Phase 2 is: load sqlite-vec, verify at startup and fail visibly if absent,
build `document_chunk_vectors` from stored blobs, and fuse the two rankings.
No re-extraction, no re-chunking, no re-embedding.

## Design principle

Unchanged from the originating spec, and the reason the phase boundary sits
where it does:

```text
PDF structure first
retrieval second
context third
```

Search indexes and embeddings are disposable and regenerable from extracted
document structure. Everything this RFC adds can be dropped and rebuilt from
the PDF.

## Alternatives considered

**A new `docs/rfcs/retrieval/` folder.** This RFC is mostly about the reader's
document model, and it builds directly on RFC 0023 and RFC 0025. It stays in
`reader/`. A retrieval folder is worth creating when phase 2 gives it a second
occupant.

**Chunking as its own queued worker.** Rejected: it needs no model and no
network, so making it an invariant of a ready extraction is strictly simpler
than a second status column with its own recovery path.

**Deferring geometry and chunking over page text with character offsets.**
Cheapest path, and it makes every retrieved chunk highlight a whole page. Since
the point of local retrieval is jumping from an answer back to the exact
evidence, this defers the feature rather than the work.
