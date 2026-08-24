# RFC 0058: Highlight Primitive + Author-Agnostic Capability Layer (Phase 1)

Status: Stale
Date: 2026-07-26
Product: i0i
Target: Tauri v2 + Svelte, macOS first
Builds on: RFC 0033/0034 (anchored chat threads, lazy threads), RFC 0048
(reader focus mode + threads rail), RFC 0056 (HTML reader annotation)
Paired with: RFC 0059 (Agent-authored highlights — Phase 2)

## Summary

Today a "highlight" is a side effect: you can only mark a passage by *asking*
or *noting* on it, which lazily creates a chat thread whose embedded anchor is
the highlight. You cannot highlight without starting a conversation, marks have
no color, and one request can't produce several marks.

This RFC inverts the model. The **highlight becomes the core primitive** — a
standalone, colored mark on a passage — and **note** and **ask** become optional
layers that attach to it. Three distinct verbs:

- **Highlight** — select text, pick a color, done. No note, no conversation.
- **Note** — attach written text to a highlight (now or later).
- **Ask** — open a chat thread on a highlight (now or later).

A highlight can carry nothing, a note, a thread, or both.

Crucially, every operation is defined once in an **author-agnostic capability
layer** (a `HighlightService`) with **two front doors**: Tauri commands for the
UI, and — in Phase 2 — an agent tool registry. Anything the user can do, the
agent can do, because both call the same service. Phase 1 builds the primitive,
the service, and a fluent manual UX; Phase 2 wires the agent to the same service.

## Problem

- **Highlighting is coupled to conversation.** `ReaderInspector` opens a *virtual
  thread* on selection (RFC 0034); the mark only exists because a thread does.
  You can't just highlight.
- **No color.** Marks render in a single amber (`::highlight(i0i-annotation)`,
  and PDF rect fills). The user wants color to carry meaning ("important" =
  yellow, "experimental result" = red).
- **Visibility is inconsistent and pinning-coupled.** PDF marks show only for
  **pinned** threads (`PdfPage.svelte`: `thread.pinnedCount > 0`); HTML draws
  **all** anchored threads (`HtmlReader.svelte`, no pin filter). Same data, two
  behaviors.
- **No shared capability surface.** Chat verbs live as ad-hoc commands
  (`add_note_at_anchor`, `ask_at_anchor_streamed`) that embed the anchor. There
  is no author-agnostic operation an agent could later call, which is exactly
  what Phase 2 needs.

## The model

### Highlight is the primitive

```
Highlight {
  id
  paper_id            // scope (a paper)
  source_id           // which rendered source (PDF source / HTML source)
  locator             // WHERE on that source (see Locator)
  excerpt             // the quoted passage text (for the rail + resolvers)
  color               // HighlightColor (named palette)
  label?              // optional category, e.g. "experimental result"
  author              // User | Agent { model }
  created_at, updated_at
}
```

`Locator` is the old anchor payload, lifted out of the thread and owned by the
highlight:

```
Locator (serde tag = "kind")
  TextOffset { source_id, start_offset, end_offset }   // HTML, browser-offset space
  PdfRect    { source_id, page_index, rects_json }     // PDF, page + rectangles
```

### Note and ask attach to a highlight (shared thread substrate)

Notes and asks continue to use the **existing thread/entry machinery**, unchanged
— but the thread now hangs off a highlight instead of embedding the anchor:

- `chat_threads` gains a nullable `highlight_id`. A thread references a highlight
  (a passage) or, when `highlight_id` is `NULL`, the whole document (today's
  `Document` anchor).
- **Note** = a `note` entry; **Ask** = a `question` + streamed `answer` entry —
  both in the highlight's thread. The thread is still created **lazily** on the
  first note/ask. A highlight with no note/ask simply has no thread yet.

"Distinct operations" is a UX/verb distinction, **not** distinct storage: a
note-only highlight lazily materializes a thread under the hood. This keeps
multi-note/multi-turn, pinning, titles, and the rail exactly as they are.

### Pinning: keeper role only, decoupled from visibility

Visibility now means **"a highlight exists"** (in both readers), so pinning is
freed from gating PDF marks. `chat_entries.pinned` keeps its current
keeper/Pins-list role unchanged; it no longer controls whether a mark is drawn.
This also removes the PDF/HTML inconsistency.

## Migration (fidelity-critical — please confirm at review)

Every existing **anchored** thread (anchor_kind `text_offset` or `pdf_rect`)
becomes a `Highlight` (author = `User`, color = a neutral default; `excerpt` =
`selected_text`), and that thread's new `highlight_id` points at it.
`Document`-anchored threads keep `highlight_id = NULL`. The legacy anchor columns
on `chat_threads` are read once during backfill, then ignored.

**The one behavior change to call out:** unpinned **PDF** anchored threads were
*invisible* before (PDF drew pinned only) and will now render as visible marks —
matching how HTML already behaves. This is deliberate: those anchors were real
passages the user selected to note/ask, and the unified model treats an existing
mark as always-visible.

**Recommendation:** migrate all anchored threads to visible highlights (option
A). The alternative — keep a hidden flag to preserve exact prior PDF visibility —
reintroduces the visibility/pinning coupling this RFC removes, so it's not
recommended. Flagging because this touches the user's real research data; happy
to switch to "migrate only pinned PDF anchors to visible" if preferred.

## User flow (must be fluent)

**Choosing a color — on selection.** Selecting text pops a compact **selection
toolbar** anchored to the selection:

```
[🟡 🟢 🔵 🔴 🟣 🟠]   [ Note ]   [ Ask ]
```

- Clicking a **swatch** creates the highlight in that color immediately — one
  click, no dialog. The chosen color becomes the **sticky default** (next plain
  highlight reuses it). Number keys `1–6` mirror the swatches.
- The toolbar's leftmost swatch is pre-highlighted as the current sticky color,
  so a user who just wants "a highlight" clicks once.

**Ask/Note in one gesture (no two-step).** The **Note** and **Ask** buttons in
the same toolbar each do *both* things atomically:

- **Note** → creates the highlight (sticky color) **and** opens the inspector's
  note composer focused, ready to type. One action from selection to noting.
- **Ask** → creates the highlight **and** opens its thread with the question box
  focused. One action from selection to asking.

So the two-step ("highlight, then separately note") is never *required* — but it
is *available*: you can highlight-only now and add a note/ask later.

**Adding note/ask after the fact.** Clicking an existing highlight opens a small
**popover**: shows its note/thread if any, with **Add note · Ask · Recolor ·
Remove**. This is the "later" path — the highlight is the durable object; notes
and conversations accrete onto it whenever.

**The rail.** The threads rail becomes a **highlights rail**: one row per
highlight (color chip + excerpt + `📝`/`💬` badges for note/thread presence),
plus document-scoped threads. Clicking a row scrolls to the mark and opens its
popover. Pure-yellow marking no longer floods the rail with empty threads.

## Rendering (multi-color, both readers)

- **HTML** already uses the CSS Custom Highlight API. Replace the single
  `i0i-annotation` highlight with one registered highlight **per color**
  (`i0i-hl-yellow`, …), each with its own `::highlight()` rule; ranges are
  grouped by the highlight's color. Draw from the `highlights` list, not threads.
- **PDF** already draws rect fills for pinned threads; switch the source to the
  `highlights` list and color each fill by `highlight.color`. Drop the
  `pinnedCount > 0` gate.
- Colors are **theme-aware** CSS variables (light/dark), one translucent fill +
  readable text per palette entry.

**Palette (fixed, named):** `yellow` (default), `green`, `blue`, `red`,
`purple`, `orange`. Stored as the name (not hex) so the UI, storage, and the
Phase 2 agent tool all speak the same small vocabulary.

## Architecture: the author-agnostic capability layer

This is the part built **now** so Phase 2 is a wiring job, not a rewrite.

### One service, author-agnostic operations

`HighlightService` (Rust) owns every verb, each taking an explicit `author`:

```
create_highlight(paper, Locator, excerpt, color, label?, author) -> highlight_id
recolor_highlight(highlight_id, color, author)
set_label(highlight_id, label?, author)
remove_highlight(highlight_id, author)
add_note(target, body, author)                 // target = Highlight(id) | Document(paper)
ask(target, question, author) -> stream         // lazily threads on the target
list_highlights(paper) -> [Highlight]
```

`target` unifies "on a highlight" and "on the whole document". `Note`/`ask`
delegate to the existing chat service, now keyed by `highlight_id`.

### Two front doors to the same service

1. **Tauri commands** (Phase 1): thin wrappers — `create_highlight`,
   `recolor_highlight`, `remove_highlight`, `set_highlight_label`, plus the
   existing note/ask commands re-pointed at `highlight_id`. The Svelte UI calls
   these.
2. **Agent tool registry** (Phase 2): each tool = name + JSON schema + a handler
   that calls the *same* `HighlightService` method. Because both doors converge
   on one service, "anything the user can do, the agent can do" is structural.

### The single real user/agent difference: locator provenance

The user supplies a `Locator` **directly** (their selection → offset/rects). The
agent supplies a **quote**, which a resolver turns into a `Locator` *before*
calling the service. Phase 1 therefore fixes two seams so Phase 2 can't break
them:

- The service always takes an **already-resolved `Locator`**. It never resolves
  quotes and never assumes an operation completed synchronously in the backend.
- **PDF resolution is client-side and async** (only the PDF.js text layer knows
  quote→rects), while HTML resolution is a backend string search. So the Phase 1
  contract must allow an operation whose locator is produced on the client and
  posted back. (Phase 2 builds the resolvers; Phase 1 just refuses to assume they
  are synchronous or backend-only.)

## Commands / API (Phase 1)

New Tauri commands: `create_highlight`, `recolor_highlight`, `set_highlight_label`,
`remove_highlight`, `list_highlights`. Re-pointed: `add_note_at_anchor` /
`ask_at_anchor_streamed` accept a `target` (`highlight_id` or document) instead
of an embedded anchor. `HighlightColor` and `Locator` are shared serde types.

## Testing

- Storage: create/list/recolor/remove highlight; note/ask lazily thread on a
  highlight; `highlight_id = NULL` for document threads.
- Migration: an anchored (text_offset) thread → highlight + linked thread with
  entries intact; a pdf_rect thread likewise becomes a *visible* highlight; a
  document thread stays `highlight_id = NULL`; pinned entries still drive the
  Pins list.
- Service is author-agnostic: the same `create_highlight(..., author=Agent{…})`
  path produces an identical row with author tagged (proves the Phase 2 seam
  before Phase 2 exists).
- Frontend: selection swatch → one-click highlight; Note/Ask from selection is a
  single gesture; click-highlight popover adds note/recolors/removes.

## Risks

- **Migration fidelity** (see above) — the visible-PDF-marks change is the thing
  the user will notice first; gated behind review confirmation.
- **Refactor blast radius**: anchor moves out of the thread. Mitigated by keeping
  the thread/entry/pinning machinery intact and only re-keying it by
  `highlight_id`.
- **Two highlight sources during rollout**: render from `highlights` only once
  backfill has run; do the schema + backfill in one migration step.

## Rollout (slices)

1. Schema: `highlights` table + `chat_threads.highlight_id`; backfill migration.
2. `HighlightService` + Tauri commands + shared `HighlightColor`/`Locator` types.
3. Rendering: multi-color from `highlights` in HTML and PDF; drop pin-gate.
4. UX: selection toolbar (swatches + one-gesture Note/Ask), click-highlight
   popover, highlights rail badges.

## Non-goals / deferred to Phase 2

- Agent authoring, tool-calling, and the quote→`Locator` resolvers (RFC 0059).
- Multiple notes-as-first-class (kept as thread entries, unchanged).
- Cross-highlight grouping/tags beyond the single `label`.
