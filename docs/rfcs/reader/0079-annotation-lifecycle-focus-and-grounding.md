# RFC 0079: Annotation lifecycle, real focus mode, and honest grounding

Status: Proposed
Date: 2026-08-12
Product: i0i
Target: Tauri v2 + SvelteKit (Svelte 5), macOS first
Builds on: RFC 0058 (highlight primitive), RFC 0061 (annotated passage /
note-ask separation), RFC 0062 (annotations panel), RFC 0067 (marks vs chats),
RFC 0074 (sticky notes), RFC 0048 (focus mode), RFC 0077 (ContextManager),
RFC 0078 (agent decides what it needs), RFC 0055 (settings).
Amends: RFC 0078 — §5 revisits phase 1's authority to skip retrieval.

## Summary

Seven issues from a real reading session. They are not one feature, but they
share a shape: **the app can do the thing and does not offer it, or does the
thing and does not say so.**

- Deleting an annotation exists in one place only, and the one place is the
  page — not the list.
- Focus mode hides the app chrome and then opens a rail on top of the paper.
- A note is one text field pretending to be a stack of notes.
- An ask can retrieve nothing, fall back to the first 32k characters of the
  paper, answer confidently about §6.2 which is not in those 32k, and report
  only `Context: 32,000 chars · truncated`. **Retrieval is local and costs
  milliseconds; the round trip spent deciding whether to run it costs hundreds.
  So it should always run** — §5 is where this RFC revisits RFC 0078.
- Paper deletion cascades through six tables that are not indexed for it.
- The answer model is Sonnet 4.5 where a model at ~1/10 the price would do.

Each section below states the diagnosis with the code that causes it, then the
change. **Sections 4, 5 and 6 ship independently** — they touch no shared
surface. Sections 1–3 are one annotation-editing model and land in order.

**Out of scope:** multi-note-per-passage schema (Open Decision A — decided
against), annotation export, undo/redo history, changing the retrieval algorithm
itself.

---

## 1. You cannot delete an annotation from the list

### Diagnosis

Deletion is fully implemented in the backend and reachable from exactly one
gesture. `remove_highlight` (`src-tauri/src/commands/highlight.rs:47`) →
`HighlightService::remove` (`services/highlight/mod.rs:62`) works. The frontend
binds it in two places, both in `ReaderView.svelte`: `removePopoverHighlight`
(:884) behind the click-a-mark popover, and `undoTurnHighlights` (:850) for
undoing an AI batch.

The popover is the *only* affordance. `HighlightPopover.svelte:129` renders
`Remove`; the marks list does not. `ReaderInspector.svelte:1336` renders each
annotation as a single whole-row `<button>` whose only action is
`onOpenHighlight` — no trash icon, no context menu, no keyboard handler. The
panel already has trash icons for other things (thread at :926, context item at
:1027), which is what makes the omission read as "you can't delete highlights"
rather than "look elsewhere."

Two further gaps found while confirming this:

- **Orphaned conversations.** `filteredAnnotations` derives every row from
  `highlights` (`ReaderInspector.svelte:258`). Removing a highlight that has a
  thread leaves the `chat_threads` row alive with no row that renders it — the
  conversation becomes unreachable rather than deleted. `remove()` deletes the
  highlight and nothing else.
- **No bulk, no keyboard.** `Delete` on a focused row does nothing;
  multi-select does not exist.

### Change

R1.1 **On the page (PDF and HTML):** right-click a mark or a sticky opens a
context menu with Delete, alongside the recolor and note actions the popover
already carries. Hovering a mark reveals the same affordance without the
right-click, so the gesture is discoverable by pointing rather than by guessing.
The existing left-click popover stays as it is.

R1.2 **In the right panel:** each row in Marks and Chats gets a persistent `−`
button at the row's trailing edge — visible, not hover-gated, so the list reads
as a list of removable things. Right-click on a row opens the same context menu
as the page, so one mental model covers both surfaces. A focused row accepts
`Delete`/`Backspace`. Styling follows the `note-icon remove` treatment already
at `ReaderInspector.svelte:926` so the panel stays one visual language. New prop
`onRemoveHighlight(id)`, wired to the existing `removeHighlight` bridge call in
`ReaderView`.

R1.3 Deletion is **transactional across the passage**: removing an annotation
removes its highlight row, its note, and its thread. Without it, R1.1 makes the
orphan problem more reachable, not less.

The cost is in "its thread." A highlight and a thread are linked only by
comparing anchors, and that comparison lives in TypeScript —
`samePassage` (`src/lib/features/reader/highlight-thread-match.ts`). The backend
has no anchor matching at all. A service-level `remove_annotation` therefore
means **porting anchor comparison into Rust and keeping two implementations in
agreement forever**, which is why Task 6 is sized L, not M. See Open Decision C
for the alternative: give `chat_threads` a nullable `highlight_id` and make the
link explicit rather than re-derived.

R1.4 Confirm before destroying a conversation, not before destroying a mark. A
colored-only highlight deletes silently (it is one gesture to redo); a passage
with a note or a thread asks once. Nothing here needs an undo stack.

R1.5 Same treatment for sticky notes, which are the same row with a point
locator (`isStickyNote`, `src/lib/domain/highlight.ts:14`) — no separate path.

---

## 2. Note vs. sticky note is a distinction without a difference

### Diagnosis

The user is right, and the code agrees with them. There is no `kind` column.
A sticky note is a highlight whose locator is a *point* (`pdfPoint` /
`textPoint`) instead of a *range* (`pdfRect` / `textOffset`) — that is the
entire difference (`src/lib/domain/highlight.ts:12-16`). Both carry the same
single `note` field. Both live in the same Marks list.

Where they diverge is only presentation: `ReaderInspector.svelte:1339-1343`
renders a `StickyGlyph` for one and a `color-chip` for the other, and RFC 0074
gave stickies a rule exempting them from the "must have color or note" filter
(:283-288).

So the honest answer to "should they be visually the same?" is: **they are the
same object, and the only thing worth distinguishing is where it is anchored.**

### Change

R2.1 Keep one visual language for the row body — same typography, same badges,
same actions. The leading glyph stays different because it encodes the real
difference: a chip means "attached to this text", a sticky glyph means "dropped
at this spot". That is information, not decoration.

R2.2 **Keep** RFC 0074's filter exemption for stickies. Dropping it would make a
freshly-placed sticky vanish from the list until typed into, which is worse than
the inconsistency it removes. Instead, delete an empty sticky on editor blur —
the annotation that never got a note was a misfire, not a record.

R2.3 Document the model in one comment at the `Locator` definition, since three
files currently re-derive it.

---

## 3. Saving a note should close the editor

### Diagnosis

`ReaderInspector.svelte:607` `saveNote()` writes through `setHighlightNote` and
leaves everything as it was: the textarea (:984) stays open, still bound to
`noteDraft`, and the Save button vanishes only because its `{#if}` compares the
draft against the just-saved value (:972). Nothing moves. There is no visible
"the note is now saved, and it is over there" moment — which is exactly what
the report describes wanting.

The deeper mismatch: the user says "push **one note** to the stack/list below",
which assumes a passage can hold several notes. Today `highlights.note` is a
single `text` column (`library_store.rs:2715` block) — one note per passage,
overwritten on each save. See Open Decision A.

### Change (single-note reading)

R3.1 On successful save, blur and collapse the editor, and render the saved
note as a read-only block with an edit affordance. Re-opening the editor is one
click.

R3.2 The saved note is what the passage's row shows in the list below — already
true (`ReaderInspector.svelte:1344` prefers `note` over `excerpt`), so the
"pushed to the list" feeling comes free once the editor collapses.

R3.3 `Cmd/Ctrl+Enter` saves; `Escape` cancels back to the stored value. The
existing keydown path at :598 already calls `saveNote()` — this makes the
outcome visible.

R3.4 Identical behavior for sticky notes, per §2 — one editor component, two
anchors.

---

## 4. Focus mode is not focused

### Diagnosis

Focus mode hides the app chrome (`ReaderView.svelte:1219` `{#if !isFocusMode}`)
but then explicitly opens the threads rail: `focusThreadsMode` initializes to
`"open"` (:197) **and** is reset to `"open"` every time focus is entered
(:1107-1108). The reader toolbar renders in focus mode too (:1179). So entering
focus trades the window chrome for a rail — the paper does not get materially
more room, and nothing about the mode reads as "just the paper."

### Change

R4.1 Entering focus mode collapses the rail by default: initialize and reset
`focusThreadsMode` to `"collapsed"`. This is a two-line change and delivers most
of the reported want.

R4.2 The toolbar auto-hides and reveals on pointer-near-top (or a keypress),
rather than being removed — the tools must stay one gesture away.

R4.3 Remember the last focus-mode rail state per session, so a reader who opens
the rail and pages on does not have it collapse under them. Default on first
entry is collapsed.

R4.4 `Esc` exits focus mode. Today the only exit is the toolbar button (:1230).

---

## 5. Answers are not grounded, and the UI does not admit it

This is the most consequential item. The reported symptom is one line of UI, but
it is covering two different problems.

### Diagnosis A — nothing was retrieved, so nothing can be cited

`ContextManager::get_context` assembles the prompt in five rows
(`context_manager.rs:235-345`). Rows 3 and 4 are the citable passages —
persistent context items and this turn's retrieval. If both are empty,
`assembly.passages_block()` returns `None` and the code takes this branch:

```rust
None => {
    eprintln!("[chat] assembled 0 passages — this answer cannot cite");
    bundle.system_prompt
}
```

That is your case exactly: **zero passages, therefore zero references in the UI,
therefore no `[C1]` handles for the answer to cite with.** Truncation is not why
there are no passages.

Why zero? Three candidate causes, and which one it was decides which half of
Task 3 does the work:

| Cause | Discriminator |
|---|---|
| The decide model emitted no tool call. Retrieval is the agent's call now (RFC 0078): one iteration (`MAX_ITERATIONS = 1`, `agent_loop.rs:34`) on the cheap `annotation_model`, and no tool call means no lookup. | No `ChatProgress::Searching` line appeared in the panel during the ask. |
| It searched and the paper has no chunks. `search_context` can only return `document_chunks` rows; a paper whose structural extraction never ran has none (RFC 0075). | `select count(*) from document_chunks where paper_id = ?` |
| Phase 1 hit a transport or parse error. The loop swallows its own errors by design ("never fails the turn") and returns an empty outcome. | stderr around the `[chat] assembled 0 passages` line. |

R5.1 below removes the first and third causes outright — a local search that
always runs cannot be skipped by a model's judgment or lost to a transport
error. The second is different in kind: **if the paper has no chunks, no amount
of retrieval policy helps**, and R5.4 (say the paper is not indexed) is the only
honest answer. Run the `document_chunks` count against the paper in the report
first — it decides whether this section is a retrieval fix or an indexing fix.

### Diagnosis B — the fallback keeps the *head* of the paper

Row 5 hands whatever budget is left to `build_context`, which calls
`truncate_to_chars` — it keeps the **first** `max_context_chars` characters and
drops the rest (`services/chat/context.rs:99-110`). The cap is 32,000
(`config.rs:14`), clamped down from the 60,000 in `app.conf.json`.

No inference is needed about whether the paper fit: the reported line says
`32,000 chars · truncated`, and `truncated: true` at exactly the cap *proves*
the source text is longer than what was sent. Everything past character 32,000
— including §6.2 — was dropped. So the model answered about §6.2 **from prior
knowledge, not from the paper** — while the system prompt told it to ground
every claim in the paper text. The answer may well be right; the system has no way to know, and
neither does the reader.

Worth stating plainly: *the app can currently emit a confident, ungrounded
answer and signal nothing.* The one line the reader sees —
`Context: 32,000 chars · truncated` — is about the paper body, not about
evidence.

### Diagnosis C — the summary already knows, and the UI throws it away

`ChatContextSummary` carries `context_items`, `dropped_items`,
`unresolved_items`, `retrieval_capped`, `compacted`, and the full `citations`
list. `ReaderInspector.svelte:886` renders one of them:

```js
return `Context: ${chars} chars${summary.truncated ? " · truncated" : ""}`;
```

### The economics RFC 0078 got backwards

The first draft of this RFC treated unconditional retrieval as a *fallback*, on
the assumption that searching costs something. Check what it actually costs:

| Step | Where it runs | Cost |
|---|---|---|
| lexical half | `store.lexical_chunk_ranking` — BM25 in SQLite (`services/search/mod.rs:169`) | sub-millisecond |
| semantic half | `semantic_ranking` — embed the query in-process, KNN over `document_chunk_embeddings` via sqlite-vec (`services/search/mod.rs:176`, `:190`) | one short embed — milliseconds |
| the *decide* round trip | OpenRouter, over the network | hundreds of ms, and it happens **whether or not the model then searches** |

The search is local end to end. `SearchService` holds an
`Option<Arc<dyn TextEmbedder>>` resolved at startup (`:120`) — the
`bge-small-en-v1.5` ONNX model is already resident — and the vector index is a
table in the same SQLite file. Nothing in the retrieval path opens a socket.

`SearchService` also already reports its own holes: `SemanticStatus`
(`:104-111`) distinguishes `Ran`, `Unavailable`, `NoEmbeddings` and
`NotRequested`. R5.4 and the risks below need exactly that signal, and it exists.

So RFC 0078's optionality trades away the cheap thing to keep paying for the
expensive one: **the round trip is spent deciding whether to run a search that
would have cost less than the decision.** And when the decision comes back "no
search needed," the turn falls through to the head of the paper — which is how a
question with an obvious lexical hint in it (`trigger`, `SEP code`, `report the
topic`) reached the model with §6.2 nowhere in the prompt.

Retrieval should not be a fallback. It should be the floor.

### Change

R5.1 **Always retrieve. Ground every answer.** Before assembly, run one hybrid
search on the question (plus the anchor passage, when there is one) through
`SearchService` and seed row 4 with the top chunks. Unconditional, local, no
round trip, no model in the loop. An ask can no longer reach the model with zero
citable passages unless the paper genuinely has none. This fires on **both**
call sites of `retrieve_for_turn` (`chat/service.rs:451` and `:505`) — the
anchor path has `thread_id: None` and no persistent items, so it is the one that
most needs a floor.

R5.1a **Who owns the bounds.** `MAX_CHUNKS_PER_TURN = 6` and `SEARCH_LIMIT = 3`
are `agent_loop` constants, and the baseline search now runs outside that loop.
Move both to a single per-turn budget owned by the retrieval step: the baseline
takes up to 4, refinement may add up to the same total of 6, and the loop is
told what is left rather than counting on its own. Dedup must widen with it —
row 4 currently checks the chunk against *persistent* items only
(`context_manager.rs:277`), so a chunk found by both the baseline and a
refinement query is pushed twice today.

R5.2 **The decide loop becomes refinement, not gatekeeping.** `agent_loop` keeps
its tools and its one iteration, but it now runs *on top of* a populated context
rather than in front of an empty one: it sees the baseline hits and can search
again with better wording, or keep one with `add_context`. It can no longer
decide the turn gets no evidence — that is not a decision worth a round trip.

R5.3 **Then measure whether the decide round trip still earns its place.** With
a baseline that is always present, phase 1's remaining value is query refinement
and the `add_context`/`drop_context` memory writes (RFC 0078). If refinement
rarely changes the assembled set, gate the loop — skip it when the baseline
scores well, keep it for follow-ups and anchored asks. That removes hundreds of
milliseconds from the critical path *and* strengthens grounding, which is the
opposite of the tradeoff RFC 0078 assumed it faced. Do not gate it on a guess;
log baseline-vs-refined for a week of real asks first.

R5.4 **Head-of-paper is the last resort, and it says so.** When retrieval
genuinely returns nothing — the paper has no `document_chunks` rows, or the
`embeddings` feature is off and lexical found nothing — the turn still answers
from the paper head, but the context line reads `Paper not indexed · answered
from first 32,000 chars` and offers to run extraction. The silent degradation is
the bug, not the fallback.

R5.5 **Say what grounded the answer.** The context line reports passages, not
just characters — `4 passages · 12,400 chars` — and an answer assembled from
zero passages is marked as such. `ChatContextSummary` already carries
`context_items`, `dropped_items`, `retrieval_capped` and the full citation list;
the UI renders one field of it (`ReaderInspector.svelte:886`). Keep
`RetrievalOutcome.queries` on the stored summary too, so a past turn can be
audited and not just a live one.

---

## 6. Paper deletion is slow

### Diagnosis

`delete_paper_globally` (`library_store.rs:1546`) is one `delete from papers`,
one explicit `delete from chat_threads`, and a `get_library()` snapshot.

The cost is in the cascade. `pragma foreign_keys = on` (:2402), and SQLite
enforces `on delete cascade` by finding the child rows — which requires an index
on the child's FK column, or it scans the whole child table. Seven tables
declare `foreign key (paper_id) references papers(id) on delete cascade`, and
their indexes are on `extraction_id`, not `paper_id`:

| Table | index on `paper_id`? | rows per paper (order) |
|---|---|---|
| `document_sources` | yes (:2461) | 1–2 |
| `document_extractions` | yes (:2479) | 1–2 |
| `document_pages` | **no** — `extraction_id` only (:2498) | tens |
| `document_blocks` | **no** — `extraction_id` only (:2520) | hundreds |
| `document_spans` | **no** — `extraction_id` only (:2540) | **thousands–tens of thousands** |
| `document_assets` | **no** — `extraction_id` only (:2560) | tens |
| `document_chunks` | yes (:2588) | tens |

Deleting one paper therefore full-scans `document_spans`, `document_blocks`,
`document_pages` and `document_assets` **across the entire library**. That cost
grows with the library, not with the paper — which matches "takes a long time"
getting worse over time.

Second contributor, magnitude unmeasured from here: the same call returns a
full `get_library()` snapshot before the UI unblocks.

Third, and a correctness bug rather than a performance one: **`highlights` has
no foreign key to `papers` at all** (:2715). It is cleaned up neither by cascade
nor explicitly. Delete a paper and its highlight rows survive forever.

### Change

R6.1 Add `create index if not exists idx_document_pages_paper_id on
document_pages(paper_id)` and the same for `document_blocks`, `document_spans`,
`document_assets`. Four lines in the schema batch, no migration risk (the batch
is `if not exists` and runs at open). This is certainly correct regardless of
which contributor dominates the clock.

R6.2 Delete `highlights` rows for the paper inside the same transaction, next to
the existing `chat_threads` delete, with the same comment explaining why it is
explicit. Adding the FK properly is a table rebuild; the explicit delete is the
small diff and can be revisited.

R6.3 Measure before and after with a real library: `explain query plan` plus a
wall-clock number on a paper with a large `document_spans` count. Put the two
numbers in the RFC's "What landed".

R6.4 If deletion is still slow after R6.1, return the snapshot asynchronously —
optimistically remove the row in the UI and reconcile. Do not do this first: an
index fix that makes the operation fast is better than a spinner that hides it.

---

## 7. Use DeepSeek instead of Anthropic

### Diagnosis

Three model slots, one config file, and one of them is not user-settable:

| Slot | Set by | Today |
|---|---|---|
| `model.chat` (answers) | Settings → Models, else `app.conf.json` | `anthropic/claude-sonnet-4.5` |
| `model.annotation` (phase-1 decide + AI highlight) | `preference("model.annotation")` only — **not in ModelsTab** | `meta-llama/llama-3.3-70b-instruct` |
| `title_model` | `app.conf.json` only — **no preference key** | `anthropic/claude-sonnet-4.5` |
| `model.planner`, `model.expansion` | Settings → Models | discovery-side |

`ModelsTab.svelte` exposes `model.chat`, `model.planner`, `model.expansion`. The
model on the retrieval critical path is not reachable from the UI, and the title
model is hardcoded to Sonnet 4.5 for a task with a 24-token ceiling.

Two slots have hard requirements: `agent_loop` passes `tools` to the annotation
model, and RFC 0064 auto-highlight passes `response_format`. Verified against
the OpenRouter model catalog on 2026-08-12 — every current DeepSeek model except
`deepseek-r1-distill-llama-70b` supports both:

| Model | tools | structured outputs | $/M in | $/M out | ctx |
|---|---|---|---|---|---|
| `anthropic/claude-sonnet-4.5` (today) | yes | yes | 3.00 | 15.00 | — |
| `deepseek/deepseek-v4-pro` | yes | yes | 0.63 | 1.26 | 1M |
| `deepseek/deepseek-v3.2` | yes | yes | 0.27 | 0.40 | 164k |
| `deepseek/deepseek-v4-flash` | yes | yes | 0.14 | 0.28 | 1M |
| `deepseek/deepseek-v4-flash-0731` | yes | yes | 0.08 | 0.18 | 1M |
| `meta-llama/llama-3.3-70b-instruct` (annotation today) | yes | yes | 0.10 | 0.32 | — |

### Change

R7.1 `model.chat` → `deepseek/deepseek-v4-pro` (decided 2026-08-12). ~5x cheaper in, ~12x cheaper out
than Sonnet 4.5, with a 1M context window that makes §5's truncation cap a
policy choice rather than a constraint. Change the default in `app.conf.json`;
the Settings override already wins over it.

R7.2 `title_model` → `deepseek/deepseek-v4-flash-0731`. A 24-token title does
not need a frontier model. Give it a `model.title` preference key while there.

R7.3 Leave `model.annotation` where it is *or* move it to
`deepseek-v4-flash-0731` — note that today's Llama 3.3 70B is already cheaper
than mid-tier DeepSeek, so this slot is not where the money is. Decide on
decision quality (does it emit a tool call when one is needed — §5's failure),
not on price.

R7.4 Expose `model.annotation` and `model.title` in `ModelsTab.svelte`. Four
slots read preferences; two are visible.

R7.5 While in `services/llm.rs`: `describe_error_status` (:255) maps every 402
to "OpenRouter account is out of credits" and discards the provider's actual
message. OpenRouter also returns 402 when a request's *maximum* cost exceeds the
remaining balance and when a per-key credit cap is hit — both of which look like
"but I have credits." Include `body_snippet(body)` for 402 and 401, as the
fallback arm already does, and update the two assertions at :594.

---

## Task list

| # | Task | Ships alone | Size |
|---|---|---|---|
| 1 | R6.1 cascade indexes + R6.2 highlight cleanup + measurement | yes | S |
| 2 | R7.1–R7.2 model defaults; R7.4 expose slots; R7.5 402 message | yes | S |
| 3 | R5.1 always retrieve (unconditional hybrid search seeds row 4) + R5.4 not-indexed path | yes | M |
| 4 | R5.5 context line reports passages; queries kept on the stored summary | after 3 | S |
| 5 | R4.1 collapsed rail + R4.4 Esc; then R4.2 auto-hiding toolbar | yes | S/M |
| 6 | R1.3 `remove_annotation` transactional delete (service + command) | yes | L — see Open Decision C |
| 7 | R1.1–R1.2, R1.4–R1.5 delete gestures: page context menu + panel `−` | after 6 | M |
| 8 | R3.1–R3.4 note editor collapse-on-save + R2.1–R2.3 | after 7 | M |
| 9 | R5.2–R5.3 decide loop becomes refinement; measure whether it earns its round trip | after 3 | M |

Tasks 1–5 are a day and a half and cover four of the seven reports. Task 3 is
the one that changes an answer's correctness rather than its presentation —
prefer it over Task 4 if only one lands.

## Open decisions

**A. One note per passage, or many?** ~~Open~~ **Decided 2026-08-12: one.** §3
stands as written — saving collapses the editor and the saved note appears in
the list below, on today's single `highlights.note` column. The alternative (a
`highlight_notes` table, a migration, and changes to every `hasNote` /
`filterHasNote` derivation) is not being built.

**B. Does the decide round trip survive?** R5.1 makes retrieval unconditional,
which removes phase 1's gatekeeping job and leaves it two: refining the query,
and the `add_context` / `drop_context` memory writes. Both are real, neither is
obviously worth hundreds of milliseconds in front of every first token. The
options are keep it always (today), gate it on a weak baseline, or restrict it
to follow-ups and anchored asks. **Decide from logged data, not from taste** —
R5.3 says what to log. This RFC does not remove it.

This decision partly reopens RFC 0078, which is fine: 0078 optimized the right
variable against a cost model that assumed search was expensive. It isn't — it
is local, and that changes the answer.

**C. How a thread knows its highlight.** Today: not at all — the two are matched
by comparing anchors in TypeScript (`samePassage`). Porting that into Rust for
R1.3 leaves two implementations of a fuzzy comparison that must never disagree.
The alternative is a nullable `chat_threads.highlight_id`, set when the thread
is created from a passage and backfilled once by the existing anchor match. Then
deletion is a foreign key, not an algorithm, and `filteredAnnotations` stops
re-deriving the join on every render. It is a migration, and it changes both
Task 6 and Task 7. **Recommended** — the re-derivation is already load-bearing
in three files.

**D. Raising `CONTEXT_CHARS_CAP`.** With a 1M-context model at 1/12 the output
price (R7.1), 32,000 characters is no longer the cost-driven necessity the
comment at `config.rs:56-63` describes. Raising it does not fix grounding —
an answer with the whole paper in front of it still cites nothing if no passage
was retrieved, which is why R5.1 comes first.

But it is **not** independent of R5.1. Row 5 gets whatever rows 3–4 did not
spend (`let paper_chars = chars_for_tokens(assembly.remaining())`), so making
retrieval unconditional shrinks the paper text on every ask. The cap is the
*only* reason passages and paper body compete: at 1M context (R7.1) there is
room for both. Raise it in the same change as R5.1, or the relevance floor in
Risks is mitigating a problem that R7.1 was about to delete.

## Risks

- **R1.3 deletes conversations.** Removing a mark now also removes its thread.
  That is the correct model (the alternative leaves unreachable rows), but it is
  destructive and there is no undo. R1.4's confirmation is the whole mitigation
   — get the copy right.
- **R4.2 auto-hiding chrome** is the classic way to make tools undiscoverable.
  Reveal must be generous.
- **R5.1 spends context budget on every turn**, including follow-ups that needed
  none ("why?", "say that shorter"). The passages compete with the paper body
  for the same assembly allowance, so a bad baseline is not free — it displaces
  paper text. Mitigation is a relevance floor: seed row 4 only with hits above a
  score threshold, and let a weak query contribute nothing rather than noise.
- **A local search on every ask is milliseconds, not zero.** The embedder is
  in-process but the `embeddings` feature is optional (`Cargo.toml:25`); with it
  off, retrieval is lexical-only and R5.1's grounding guarantee is weaker than
  it reads. Say which half ran.
- **R7.1 changes answer quality**, not just price. Sonnet 4.5 → DeepSeek V4 Pro
  is a real swap; run a handful of asks against a known paper before making it
  the default rather than a setting.

## Success criteria

1. A highlight, a note-only passage, and a sticky note can each be deleted in
   one gesture from **both** surfaces — right-click or hover on the page, `−` or
   right-click in the panel — and the deletion takes its conversation with it.
   No `chat_threads` row survives without a highlight.
2. Entering focus mode shows the paper and nothing else; the rail and toolbar
   are one gesture away; `Esc` exits.
3. Saving a note closes the editor and the note is visible in the list below
   without a further click.
4. **Every answer about an indexed paper cites at least one passage.** Zero
   passages is reachable only when the paper has no chunks, and that case says
   so in the context line and offers to index it.
5. Asking a question about a section in the tail of a long paper retrieves that
   section's chunks and cites them. Re-running the reported §6.2 question
   produces a cited answer.
6. The context line reports passages, not only characters, and the queries that
   produced them survive on the stored turn.
7. Deleting a paper is no longer perceptibly slow, and does not get slower as
   the library grows — measured against R6.3's before/after numbers, since no
   baseline exists yet. No `highlights` rows survive the delete.
8. The default answer model costs under $1/M input tokens, and every model slot
   the app uses is visible in Settings → Models.

## What landed

_(filled in on implementation)_
