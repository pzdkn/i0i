# RFC 0077: ContextManager — the agent's working memory

Status: Implemented
Date: 2026-08-08
Product: i0i
Target: Tauri v2 + SvelteKit (Svelte 5), macOS first
Builds on: RFC 0075 (structural extraction, chunking, embeddings),
RFC 0076 (SearchService), RFC 0032 (chat with current paper),
RFC 0034 (threads), RFC 0061 (annotated passage / note-ask separation).

Filed under `chat/` because ChatAgent is the only consumer. If a second
consumer appears (study mode, graph), move it to its own directory.

## Summary

Today every ask rebuilds the same prompt: the whole paper's text, cut at 32,000
characters, plus the thread's entries. `build_context` in
`services/chat/context.rs` does this in 60 lines and does it the same way every
turn. No retrieval, no memory, no control.

This RFC replaces that with a **ContextManager**: the thing that decides what
the model sees. ChatAgent asks it for a prompt and never calls SearchService
itself — ContextManager is the only door to retrieval.

```text
ChatAgent → ContextManager → SearchService → chunks
                    ↓
        persistent items + ephemeral → prompt
```

Five methods:

| Method | Does | Calls the model? |
|---|---|---|
| `search(query, scope, limit)` | pass through to SearchService, return candidates | no |
| `add_context(thread_id, chunk_id)` | commit a chunk to persistent context | no |
| `delete_context(key)` | drop one item, by item id **or** chunk id | no |
| `get_context(thread_id, ephemeral)` | assemble the prompt under a budget | **no** |
| `compact_context(thread_id)` | summarize and set a watermark | **yes** |

`add_context` takes only a chunk id: the manager reads the chunk to derive its
durable anchor and token estimate, so no caller has to know that layout.

`get_context` takes `Option<&str>` for the thread — the anchor path has no
thread yet (see Keying).

**Out of scope:** tool-calling during the answer stream (see Risks), context
sharing between threads, automatic compaction.

Context **can** span papers, because search scope can. Nothing here assumes one
paper: items carry their own `paper_id` and citations resolve per item. What is
out of scope is a *UI* for cross-paper context — phase 1 scopes retrieval to
the thread's paper, and the schema does not have to change when that widens.

## Two kinds of context

The distinction the user asked for, made concrete:

| | Ephemeral | Persistent |
|---|---|---|
| Examples | current selection, the open paper, this turn's retrieval | chunks the agent or user added |
| Lifetime | one `get_context` call | until deleted or compacted |
| Where it lives | **a function parameter** | a SQLite table |
| Survives restart | no | yes |
| Survives a selection change | no | yes |

**Ephemeral context is never stored.** "Dropped when the selection changes" is
free if it was never written down — the caller passes the current selection into
`get_context` and that is the whole lifecycle. Every invalidation bug in the
alternative design (stale selection, stale page, selection belonging to a
different paper) cannot happen.

Persistent context must be a table, not memory: "persists unless compacted or
deleted" has to mean *across restarts*, and a process-lifetime `HashMap` loses
it on quit.

```rust
pub struct EphemeralContext {
    pub paper_id: String,
    pub selection: Option<String>,
}
```

No `page_index`: nothing produces the current page at ask time, and a field no
caller fills is a field that lies. Ambient page context is a later change.

## Schema

```sql
create table if not exists chat_context_items (
  id text primary key,
  thread_id text not null references chat_threads(id) on delete cascade,
  position integer not null,          -- insertion order, dense from 0
  kind text not null,                 -- 'chunk' | 'summary'

  -- chunk items: a hint plus a durable anchor (see below)
  chunk_id text,
  paper_id text,
  source_start integer,
  source_end integer,

  -- summary items
  body text,
  covers_through_entry_id text,

  token_estimate integer not null,
  created_at text not null
);

create unique index if not exists idx_chat_context_items_position
  on chat_context_items(thread_id, position);
```

`chat_entries` already cascades from `chat_threads`, so this follows an existing
precedent rather than establishing one. Deleting a thread takes its context with
it.

No `score` column. `ChunkHit.score` is comparable only within one response — its
own doc comment says so. Persisting it would invite ordering context by numbers
from different queries, which means nothing.

### Chunk ids are not durable; source offsets are

`CHUNK_VERSION` is already at 2, and `rechunk_extraction` /
`extractions_needing_rechunk` exist and run. A rechunk deletes and re-mints
chunk rows, so a stored `chunk_id` can dangle through no fault of the user.

So store both:

```text
chunk_id                          fast path — try this first
paper_id + source_start/end       durable — survives rechunking
```

`get_context` resolves each item as:

1. `chunk_id` still exists → use it.
2. Otherwise re-resolve: the chunks of `paper_id` overlapping
   `[source_start, source_end)`, joined in reading order.
3. Neither → the item is reported as `unresolved` in the summary and skipped.
   Never silently dropped.

Case 2 can return more text than originally added if chunk boundaries moved.
That is the right trade: the *passage* the user chose is preserved, its exact
packaging is not.

### Three ids, three lifetimes

"We need some id to reference the chunks, they can be temporary" — right, and
the temporary one is a *different* id from the durable one. Three layers, each
doing one job:

| Id | Lives | Job |
|---|---|---|
| `[C1]`, `[C2]` … handle | one `get_context` call | what the **model writes** in its answer |
| `chunk_id` | until the next rechunk | fast lookup, and the key for rectangles |
| `paper_id` + source range | forever | survives rechunking |

The handle is assigned at assembly time, densely from 1, in emission order. The
model never sees a UUID — it sees `[C3]` and cites `[C3]`. Handles are not
stored, because they are only meaningful inside the assembly that minted them:
the same chunk can be `[C3]` this turn and `[C1]` after a compaction.

### Clickable citations

`get_context` returns the map alongside the prompt, so the answer's `[C3]` can
resolve to a place in the PDF:

```rust
pub struct ContextCitation {
    pub handle: String,       // "C3"
    pub item_id: String,
    pub paper_id: String,
    pub page_start: i32,
    pub heading_path: Option<String>,
    pub chunk_id: Option<String>,
    pub rects_json: String,   // a JSON array of PageRects
}
```

Rectangles come from the chain RFC 0075 built and RFC 0076 deferred using:

```text
chunk → document_chunk_blocks → document_blocks.bbox_json → rects
```

This needs one new store method, `chunk_rects(chunk_id) -> Vec<PageRects>`.
Blocks already carry `bbox_json` normalized to reader space, so the reader can
scroll to the page and paint the passage with what it already knows how to do
for highlights (RFC 0058).

Two consequences worth stating:

- **Block-level, not span-level.** A chunk resolves to whole blocks, so the
  jump lands on the paragraph, not the sentence. Span-level precision exists in
  the data but is not needed to make a citation clickable.
- **A re-resolved item may have no rectangles.** If the chunk was re-resolved
  through the durable anchor after a rechunk, we still find blocks by source
  range — but if extraction itself changed, `rects` can come back empty. Then
  the citation degrades to a page number rather than failing.

The answer text is persisted with `[C3]` markers in it, and the citation map is
persisted next to it on the answer entry. Without that, reopening a thread would
show markers that resolve to nothing.

The "cite as `[C1]`" instruction is added to the system prompt **only when
there are citable items**. A thread with no context items gets today's prompt
unchanged — which is what makes the byte-identical success criterion below
achievable rather than aspirational.

### Deleting by either key

You asked for `delete_context(chunk_id or some context_id)`. Both work:

```rust
pub enum ContextKey<'a> { Item(&'a str), Chunk(&'a str) }
```

A chunk id resolves to the item holding it — the caller that just saw a search
hit knows the chunk id, the caller looking at a context list knows the item id,
and neither should have to translate.

## `get_context` does no I/O to a model

Pure assembly: read the persistent items, read the entries after the watermark,
take the ephemeral parameter, fit them to a budget, return a prompt and a
report of what did not fit.

```rust
pub struct AssembledContext {
    pub system_prompt: String,
    pub messages: Vec<WireMessage>,
    pub summary: ChatContextSummary,
}
```

This must stay fast and deterministic. If `get_context` could trigger
compaction, some asks would silently take an extra model round-trip before the
first token — unpredictable latency for no visible reason. Compaction is
explicit, and only `compact_context` calls the model.

### Budget and precedence

The currency is **tokens**. Chunks already carry `token_estimate`; the config
carries `max_context_chars` capped at 32,000. Convert with the chunker's
existing `CHARS_PER_TOKEN = 4`:

```text
32,000 chars / 4 = 8,000 tokens
```

Fill in this order, stopping when the budget is spent:

| # | Item | Why here | In the budget? |
|---|---|---|---|
| 0 | Ephemeral selection | the question is about *this* | no — see below |
| 1 | Compaction summaries | the only record of what was dropped | yes |
| 2 | Entries after the watermark | the live conversation | no — as today |
| 3 | Persistent chunks, newest first | added deliberately, but replaceable | yes |
| 4 | Retrieved chunks (this turn) | ephemeral, re-selected each turn | yes |
| 5 | Paper head text | today's behaviour, with what is left | takes the remainder |

**The budget governs context items, not the whole prompt.** Entries are not
charged, because they never have been: the thread's turns have always been sent
in full. Charging them would shrink the paper text on any thread with history,
which is a regression dressed as a budget. Compaction still bounds them — via
the watermark, not the budget.

Row 0 is outside the budget because that is what happens today:
`build_context` gives the paper its full 32,000 chars *and* adds the anchor
passage on top — the test
`anchor_passage_is_foregrounded_even_when_paper_truncated` pins exactly this.
Charging the selection against the budget would shrink the paper text on every
anchored ask, which is a silent regression on the most common ask in the app.

Row 3 has two different orders. Chunks are *selected* newest-first when the
budget is tight, and *emitted* in `position` ascending so the prompt reads in
the order the context was built. `added_chunks_appear_as_numbered_citable_passages`
pins the emission order.

Row 5 keeps the current experience intact for a thread with no context items —
which is every existing thread. Without it, this RFC would be a regression on
day one.

`ChatContextSummary` grows to report the outcome:

```rust
pub struct ChatContextSummary {
    pub paper_title: String,
    pub included_chars: usize,
    pub truncated: bool,
    #[serde(default)] pub context_items: usize,
    #[serde(default)] pub dropped_items: usize,
    #[serde(default)] pub unresolved_items: usize,
    #[serde(default)] pub compacted: bool,
}
```

`#[serde(default)]` on each new field: these rows are already serialized into
`chat_entries.context_json`, and every existing answer must keep deserializing.
Old rows read as zeros and `compacted: false`, which is true.

## Compaction does not delete the chat

The user wrote "all explicit chunks, and chat history is removed and replaced by
a context summary." That is about **context**, not about their scrollback. The
thread view keeps showing every entry; only what `get_context` emits shrinks.

`compact_context(thread_id)`:

1. Assemble the current context (no model call).
2. Ask the model for a summary — the cheap `annotation_model`, not the answer
   model. Summarizing is not answering.
3. Write one `kind='summary'` item with `covers_through_entry_id` = the newest
   entry at the time.
4. Delete every `kind='chunk'` item that existed when compaction started.
   Chunk items carry no entry id, so "covered" is defined by time, not by
   watermark. Items added *while* the summary was being generated survive.

`get_context` then emits `summary + entries after the watermark`. Nothing in
`chat_entries` is touched.

```text
before:  [c1][c2][c3]  e1 e2 e3 e4 e5        → 7,400 tokens
after:   [summary@e5]  (e6 e7 as they come)  →   380 tokens
```

If the summarization call fails, nothing is written. A failed compaction leaves
the thread exactly as it was — the same invariant `ask_at_anchor_streamed`
already holds.

## Keying: thread_id, and the anchor path runs ephemeral-only

Persistent context is keyed by `thread_id`. This has one consequence worth
naming: `ask_at_anchor_streamed` persists nothing until the reply lands, so at
prompt-assembly time **there is no thread yet**. That path gets ephemeral
context only — selection, page, paper — and no persistent items.

That is correct, not a gap. A first ask at a highlight has no accumulated
context by definition. The alternative — keying by paper so context leaks across
every thread on that paper — would break the "nothing persisted on a failed ask"
invariant and mix unrelated conversations.

## `search` commits nothing

`ContextManager.search` is a thin pass-through to `SearchService`. It returns
candidates; `add_context` is a separate call. Two reasons:

- It matches the original RFC's `searchDocument` → `readDocumentRanges`
  two-step: discover, then load selectively.
- Auto-adding every hit would fill persistent context with results the agent
  looked at and rejected.

The pass-through keeps SearchService caller-agnostic — ContextManager is just
another caller, with no privileged ranking.

## Implementation order

One RFC, but built and verified in this order so a failure is never ambiguous:

| Step | Delivers | Verified by |
|---|---|---|
| 1 | schema + store methods | migration and cascade tests |
| 2 | `add_context` / `delete_context` / `get_context`, budget, ephemeral | prompt assembly tests, no model |
| 3 | `compact_context` | summary + watermark tests with a stub model |
| 4 | `search` pass-through and pre-answer retrieval in the ask path | existing ask tests still pass |
| 5 | `chunk_rects`, citation handles, clickable jump | the only step that touches the reader |

Step 1 is the only database migration. Step 5 is the only UI risk.

### What landed

| Piece | Where |
|---|---|
| `chat_context_items` + cascade | `storage/library_store.rs` |
| `chunk_rects`, `chunks_overlapping` | `storage/library_store.rs` |
| the five methods | `services/chat/context_manager.rs` |
| pre-answer retrieval, compaction call | `services/chat/service.rs` |
| commands | `commands/context.rs` |
| `[C1]` rendering + jump | `features/reader/CitedAnswer.svelte`, `cited-answer.ts`, `ReaderView.svelte` |
| transient passage flash | `PdfPage.svelte` → `PdfRenderedPage.svelte` |

## Risks

**R1 — Tool calls in the answer path.** `annotate_streamed`'s doc comment says
the answer path is deliberately kept free of tool-call generation so the reply
streams clean and fast; marking runs as a separate cheap pass. If ChatAgent
calls `search` *mid-turn*, tool calls come back into the answer path and that
property is lost.

Phase 1 therefore does **pre-answer retrieval**, which you confirmed: before the
ask, run one `search` with the user's question, add the top hits as ephemeral
retrieved context, then stream the answer with no tools. Agentic mid-turn
retrieval is a later RFC and should be measured against the streaming
regression it causes.

Note what this makes retrieved context: **ephemeral**. It is selected fresh for
each turn — the original RFC's "retrieved context is temporary and selected
independently for each model turn." It only becomes persistent if something
calls `add_context` on it.

**R2 — Compaction loses the thing you needed.** A summary is lossy by
construction. Mitigated by: it is never automatic, the entries survive in the
thread, and `dropped_items` is reported. Not fully solved.

**R3 — Re-resolution widens a passage.** Covered above: after a rechunk, an item
may resolve to more text than was added. Bounded by the same token budget.

**R4 — Budget row 5 masks an empty context.** If the paper-text fallback always
fills the remaining space, a user may not notice their context items were
dropped. `context_items` / `dropped_items` in the summary is what the UI needs
to show it.

## Success criteria

| | Target |
|---|---|
| `get_context` | < 20 ms, no network |
| Existing threads with no context items | byte-identical prompt to today, anchored or not |
| Old `chat_entries.context_json` rows | deserialize unchanged |
| Compaction | ≥ 5× token reduction on a 10-entry thread |
| Failed compaction | zero rows written |
| Rechunked paper | context items resolve, none silently vanish |
| Clicking a `[C3]` citation | reader opens the right page, passage painted |

## Open decisions

Chosen on your behalf — say the word on any of them:

1. **Compaction keeps `chat_entries`.** I read "chat history is removed" as
   about context, not your scrollback. If you meant the entries themselves,
   this changes.
2. **Ephemeral is a parameter, never a row.** No `context_state` object holding
   both kinds.
3. **Persistent context is keyed by thread**, so anchor-asks get ephemeral only.
4. **Compaction is manual.** No token-threshold auto-trigger in phase 1.
5. **Compaction uses `annotation_model`**, not the answer model.
6. **Tokens are the currency**, converted at 4 chars/token.
7. **Row 5 exists** — the paper-text fallback stays, so nothing regresses, and
   the selection stays outside the budget as it is today. Chat entries are not
   charged against the budget either, for the same reason.
8. **Phase 1 is pre-answer retrieval**, not mid-turn tool calls (R1).
9. **Citations are block-level**, so a click lands on the paragraph, not the
   sentence. Sentence precision would need span offsets the chunk does not
   carry.
10. **Citation handles are `[C1]`-style and not stored**; the map that resolves
    them is stored on the answer entry.
11. **`get_context` is provider-neutral** — resolved the other way during
    implementation. It returns `AssembledContext { system_prompt, entries,
    summary }`; `chat/service.rs` converts to OpenRouter `WireMessage`s. Same
    instinct that kept SearchService caller-agnostic.
12. **`EphemeralContext` carries `paper_id` and `selection`, not `page_index`.**
    Nothing produces the current page at ask time, and a field no caller fills
    is a field that lies. Ambient page context is a later change.
13. **Citations live on `ChatContextSummary`**, which is already serialized into
    `chat_entries.context_json` — so the map that resolves `[C1]` is stored with
    the answer that wrote it, with no schema change.
