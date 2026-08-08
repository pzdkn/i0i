# RFC 0078: ChatAgent — the agent decides what it needs

Status: Implemented
Date: 2026-08-08
Product: i0i
Target: Tauri v2 + SvelteKit (Svelte 5), macOS first
Builds on: RFC 0077 (ContextManager), RFC 0076 (SearchService),
RFC 0059 (agent-authored highlights), RFC 0034 (threads).

## Summary

RFC 0077 gave the agent a memory it does not control. Retrieval runs on **every**
ask whether or not the question needs it, and the agent has no say in what ends
up in front of it.

This RFC hands over the wheel. The agent searches when it decides it needs to,
keeps what it decides is worth keeping, and cites what it used.

The mechanism is a **two-phase turn**:

```text
phase 1  retrieval loop     tools on,  no prose      (0-3 round trips)
phase 2  the answer         tools off, prose streams (1 round trip)
```

RFC 0077 R1 said the answer path must stay free of tool-call generation so
replies stream clean. That constraint survives — it just moves. All tool calling
happens **before** the prose starts. Phase 2 is the same clean stream it is
today.

**Out of scope:** multi-paper agentic search (the scope stays the thread's
paper), agent-initiated compaction, tool use during annotation.

## What changes for you

| Today (0077) | With this RFC |
|---|---|
| every ask searches, need it or not | the agent searches only when it decides to |
| retrieved passages vanish after the turn | the agent can keep one, visibly |
| citations exist but nothing chose them | the agent cites what it actually used |
| one round trip | one to four, with a progress line |

## Phase 1: the retrieval loop

The model gets tools and a **reduced** system prompt: paper metadata, the
current passage list, the question — no paper body text. It answers with tool
calls or with nothing.

```text
┌─ loop, max 3 iterations ─────────────────────┐
│  model → tool calls?                         │
│    no  → break                               │
│    yes → run them, append results, iterate   │
└──────────────────────────────────────────────┘
                  ↓
   full assembly (ContextManager::get_context)
                  ↓
        phase 2: stream the answer
```

Three tools:

| Tool | Does | Writes? |
|---|---|---|
| `search_context(query, limit)` | hybrid search in the thread's paper | no |
| `add_context(chunk_id, why)` | keep a passage for later turns | yes |
| `drop_context(item_id)` | remove a passage **it added** | yes |

`search_context` returns id, page, heading, and a ~300-character preview — not
full text. Full text arrives through the final assembly, once, rather than
being paid for in every loop iteration.

### The agent may delete only what it added

`chat_context_items` grows an `origin` column (`'user' | 'agent'`). `drop_context`
refuses an item the user kept.

You curated that passage on purpose; an agent quietly removing it is the kind
of surprise that makes a feature untrustworthy. The agent can always *say* a
passage looks unhelpful. It cannot act on that alone.

Agent-added items are **persistent and labelled**, not hidden. The RFC 0077
context panel shows an `agent` tag next to them, and the ✕ works the same.
That is the honest reading of "the agent adds it to state": visible, attributed,
and reversible by you.

### Bounds, and what happens at them

| Bound | Value | On hitting it |
|---|---|---|
| iterations | **1** | one look, then answer |
| tool calls in that round | 2 | ignore the rest, report |
| chunks retrieved per turn | 6 | further searches return "budget spent" |

**Revised down after using it.** The loop was 3 iterations and it made answers
feel slow: every iteration is a full round trip the reader waits through before
the first word of prose, and refining a query a second time is worth much less
than answering sooner. One round still lets the agent *decide* — it just gets
one look, and may issue two queries in it. The deciding round also runs on the
cheap `annotation_model` with a 512-token ceiling, since choosing a search query
is far lighter than writing the answer and this sits on the critical path.

`capped` now means only "we refused work the agent asked for". Stopping after
the single round is the design, not a limit hit, so it raises no notice — a
notice on every searching turn is a notice you learn to ignore.

Every cap is **reported**, never silent. A capped turn sets `retrieval_capped`
on the context summary and the UI says so. RFC 0076 counts its holes and RFC
0077 counts dropped items; a loop that quietly stopped short would read as an
agent that decided it had enough.

## Handles must be stable across the loop

This is the subtle one, and the reason to write it down before building.

RFC 0077 mints `[C1]`, `[C2]` … at assembly time, in emission order. In a loop
that looks like a bug waiting to happen: the agent retrieves passage X in
iteration 1, retrieves passage Y in iteration 3, and the final assembly
renumbers both. A marker the model formed an intention about now points at a
different passage.

**Resolved by never showing a handle during the loop.** Tool results identify
passages by `chunk_id`, not by `[Cn]`:

```text
- id=chunk_88:4 p7 · Method · Scaling :: We divide by sqrt(d_k) because…
```

Handles are still minted once, at final assembly. The model sees ids while
*deciding* and markers while *writing*, and the two never overlap — so there is
nothing to keep stable. Freezing handles per turn would also have worked; this
is the same guarantee with no machinery.

## What has to change underneath

### `WireMessage` grows two optional fields

```rust
pub(crate) struct WireMessage {
    pub role: String,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<WireToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}
```

`skip_serializing_if` on both, so every existing call site emits a
byte-identical payload. `annotate_streamed` consumes tool calls fire-and-forget
and never replays them, so it is untouched.

### The tool-call id is currently thrown away

A gap found while writing this, not a detail to discover later.
`ToolCallDelta.id` is parsed and then dropped:

```rust
#[allow(dead_code)] // Present in the wire format; not needed for assembly.
id: Option<String>,
```

`AssembledToolCall` is `{name, arguments}`. A loop **must** send each result back
under its `tool_call_id`, so the decoder has to keep the id and
`AssembledToolCall` has to carry it. Small change; blocking.

### Progress events

`ChatStreamEvent` is a tagged enum, so this is additive:

```rust
Searching { query: String },
Retrieved { count: usize, total: usize },
```

Without them the panel sits dead through up to three round trips and the app
looks hung. This is the difference between "thinking" and "broken".

## References are what the agent used, not what it was given

This corrects shipped RFC 0077 behaviour, not just a plan.

Today the assembly mints a citation for **every** passage in the prompt, stores
all of them on the answer, and the `keep:` row lists all of them. So an answer
that leaned on one passage shows five references, and four of them are noise
that looks like evidence.

The prompt still offers every passage a handle — the model has to be able to
cite anything it reads. But when the answer lands:

```text
answer text → scan for [Cn] markers → keep only those citations
```

Everything else is discarded before the answer is persisted. A passage the model
read and did not cite leaves no trace, which is the honest record: it was
context, not evidence.

Consequences worth stating:

- **An answer that cites nothing has no references.** Correct. Some questions
  are answered from the paper text, and a reference list would be inventing
  provenance.
- **A paraphrase without a marker is invisible to us.** The model can use a
  passage and not cite it, and nothing here detects that. The tool description
  asks for markers on anything load-bearing; that is the whole lever.
- **`keep:` chips shrink to cited passages only.** You can no longer keep
  something the agent read past — which is the point.

## The context panel is collapsed by default

Same principle one level up. The panel currently lists every persistent item
whenever a thread has any. It becomes a single line:

```text
Context (3) ▸                                    compact
```

Click to expand, ✕ to drop, `agent` tags on the agent's additions. Closed is the
resting state: context is plumbing, and plumbing you have to look at every time
you read an answer is a leak.

**One exception.** If anything is `unresolved` or was dropped over budget, the
line says so and opens itself:

```text
Context (3) · 1 unresolved ▾
```

RFC 0076 counts its holes and RFC 0077 counts dropped items precisely so they
cannot hide. Collapsing the panel must not undo that — a hole you never see is
the failure mode both RFCs were written against.

## Phase 2 and the rest of the UI

Unchanged from RFC 0077: `[C1]` markers render as buttons, clicking scrolls to
the page and flashes the passage.

One addition: **a references block under each answer**, listing only the cited
passages — handle, page, heading, first line — so you can see what the answer
rested on without hunting for markers in the prose. Clicking a row does what
clicking `[C1]` does.

## Cost

Each loop iteration is a round trip with the reduced prompt (~600 tokens), not
the full assembly (~8,000). A three-iteration turn costs roughly

```text
1 × 600  +  8,000        ≈  8,600 tokens
```

against 8,000 before — and the 600 is on the cheap model. A turn that needs no
lookup costs *less* than the old unconditional search.

The loop was originally specified to run on the answer model, on the grounds
that deciding what evidence a question needs is judgment. Using it changed that:
the wait in front of the first token is the thing the reader actually feels, and
picking a search query is light enough for the fast model.

## The smaller alternative

Worth naming, because it is genuinely cheaper to build and to bound:

| | Single round | Full loop (recommended) |
|---|---|---|
| Agent decides *whether* to search | yes | yes |
| Agent refines after seeing results | no | yes |
| Agent manages context | no | yes |
| Round trips | 1-2, fixed | 1-4 |
| New machinery | tool-call id | tool-call id, message replay, bounds |

Single-round gets the biggest win — no retrieval on questions that do not need
it — for a fraction of the work. But it cannot do the thing you actually asked
for: search, look, search again, keep the good one. If the loop turns out to be
slow or unpredictable in practice, capping iterations at 1 turns it back into
this without a redesign.

## Risks

**R1 — Latency becomes unpredictable.** A turn is now 1 to 4 round trips.
Mitigated by the iteration cap and progress events; not eliminated. Measure
before widening the cap.

**R2 — The agent searches when it should just answer.** "What is this paper
about?" needs no retrieval. The tool description has to say so explicitly, and
`retrieval_capped` plus the searched-query log make over-searching visible
rather than invisible.

**R3 — Agent-added context accumulates.** Every turn can add. Bounded by the
per-turn chunk cap and by compaction, and every item is visible with an ✕.
A per-thread ceiling is the obvious next lever if it turns out to be needed.

**R4 — A tool call the model malforms.** Invalid JSON arguments, an unknown
chunk id, a `drop_context` on a user item. Each returns a tool *result*
describing the failure rather than aborting the turn — the model can recover,
and a turn that dies because the model mistyped an id is worse than one that
answers slightly less well.

## Success criteria

| | Target |
|---|---|
| A question needing no retrieval | zero tool calls, one round trip |
| Existing single-turn asks | same answer quality, no slower |
| Handles | never shown during the loop, so they cannot go stale |
| Capped turn | answers, and says it was capped |
| Malformed tool call | turn completes |
| An answer citing 1 of 5 passages | 1 reference stored, not 5 |
| An answer citing nothing | no references block at all |
| A thread with an unresolved item | panel opens itself and says so |
| `drop_context` on a user item | refused, and the agent is told why |
| Existing `annotate_streamed` | byte-identical request payload |

### What landed

| Piece | Where |
|---|---|
| `tool_calls` / `tool_call_id` on the wire, call ids kept | `services/llm.rs` |
| the loop, tools, bounds | `services/chat/agent_loop.rs` |
| `origin` column + agent-scoped delete | `library_store.rs`, `context_manager.rs` |
| citations filtered to what was cited | `context_manager::retain_cited` |
| progress line, references block, collapsed panel, `agent` tags | `ReaderInspector.svelte` |

Two things resolved differently from the plan above, both simpler:

- **Handles never enter the loop**, so there is nothing to freeze (see above).
- **Phase 1 sees the last four turns.** Without them the prompt's claim that
  follow-ups need no lookup is one the model cannot act on — "why?" arrives with
  no antecedent. A tail, not the thread: history is resent every iteration.

## Open decisions

Say the word on any of these:

1. **Full loop, cap 3** — not the single-round alternative. Your ask needs more
   than one look.
2. **Agent adds are persistent and labelled `agent`**, not ephemeral. This is
   the literal reading of "adds it to state"; the alternative is that agent
   finds stay ephemeral until you press `keep:`.
3. **The agent may delete only its own additions.**
4. **The deciding round runs on the cheap `annotation_model`**, not the answer
   model — reversed after measuring the wait. Picking a query is light; the
   reader is staring at a spinner while it happens.
5. **Search scope stays the thread's paper.** Cross-paper agentic search is a
   later RFC — the scope plumbing already supports it.
6. **`search_context` returns previews, not full text.** Full text arrives once,
   in the final assembly.
7. **Handles are never exposed during the loop.** The model works in chunk ids
   while deciding and markers while writing, so there is nothing to freeze.
8. **No agent-initiated compaction.** Compaction is lossy and stays yours.
9. **Only cited passages become references** — and this is applied to RFC 0077's
   shipped behaviour too, not held back for the agentic loop. It is a small
   change (filter the map against the answer text before persisting) and the
   current behaviour is wrong today.
10. **The context panel is collapsed by default**, and opens itself only when
    something is unresolved or was dropped.
