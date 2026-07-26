# RFC 0059: Agent-Authored Highlights (Phase 2)

Status: Proposed
Date: 2026-07-26
Product: i0i
Target: Tauri v2 + Svelte, macOS first
Builds on: RFC 0058 (Highlight primitive + author-agnostic capability layer)
Requires: RFC 0058 shipped first.

## Summary

Let the chat agent mark passages in the paper while it answers — the original
ask:

- *"Mark the most important passages in yellow."*
- *"Mark the most important experimental results in red."*
- *"What are the distinctions between sharding strategies? Answer and mark the
  relevant passages."*

RFC 0058 already made the highlight an author-agnostic primitive created through
a shared `HighlightService`. Phase 2 adds the **second front door**: give the
chat model **tools** that call that same service, and build the one thing users
don't need — a **quote → `Locator` resolver** — since the agent points at
passages by quoting text, not by selecting pixels.

No new highlight model, no new storage. This RFC is: tool-calling + resolvers +
an accept/undo affordance.

## Problem

Chat is **prose-only** today (`ask_at_anchor_streamed` streams text). To mark
passages the agent needs to (1) emit structured mark *intents* mid-answer, and
(2) have those intents land on exact spans in the rendered paper. The hard half
is (2): the agent knows the *text* of a passage, but a highlight needs a
`Locator` — a char offset (HTML) or page + rectangles (PDF) — and **only the
rendered client knows PDF geometry** (the PDF.js text layer). RFC 0058 fixed the
seam for this: the service takes an already-resolved `Locator`, and resolution
may be client-side and async.

## Design

### 1. Tools on the chat turn (the second front door)

The chat request gains a tool registry. Each tool is `name + JSON schema + a
handler that calls a Phase 1 `HighlightService` method`. The v1 tool set is a
**deliberate subset** of user operations:

```
highlight(quote: string, color: HighlightColor, label?: string)
note(quote: string, body: string)          // highlight + attach a note
```

- `color` is the RFC 0058 named palette (`yellow|green|blue|red|purple|orange`) —
  the model picks from the same vocabulary the UI swatches use. This is how color
  is exposed to the agent: a closed enum in the tool schema, so "in yellow" /
  "in red" map to `color: "yellow" | "red"` directly.
- `label` carries the category the user named ("experimental results"),
  surfaced on the mark's popover.
- **Excluded in v1:** `recolor`/`remove` of the *user's own* highlights (the
  agent may only recolor/remove marks it authored), and any op on the whole
  library. Author-tagging (`author = Agent { model }`, from RFC 0058) enforces
  this boundary.

The model may interleave tool calls with prose, so *"answer and mark"* is one
turn: it streams the written answer into the thread **and** emits `highlight`
calls for the passages, which link back to that answer (clicking a mark opens the
explanation).

### 2. Quote → `Locator` resolution (uniformly client-side)

Rather than split resolution (backend for HTML, client for PDF), resolve
**both** on the client, because the client already renders both surfaces and RFC
0058's seam allows a client-produced locator. The agent's tool calls are
collected as **highlight intents** `{ quote, color, label, source_id, link_thread_id }`
and streamed to the reader, which resolves each and calls the Phase 1
`create_highlight` (author = Agent):

- **HTML** — search the rendered article text for `quote`; map the match to a
  `TextOffset` in the same browser-offset space RFC 0056 uses. Fuzzy-tolerant
  (normalize whitespace) to survive minor model paraphrase.
- **PDF** — match `quote` against the **PDF.js text layer** per page: locate the
  run of text items covering the quote, union their client rects into a
  `PdfRect { page_index, rects_json }`. Handles multi-line and (best-effort)
  two-column spans.
- **Unresolved quotes** (not found on any rendered page) are reported back in the
  turn as "couldn't locate N passages" rather than silently dropped.

Because resolution renders against open content, agent marks are created while
the reader is open on that source — which is exactly when the user is reading.

### 3. Accept / undo (agent marks are suggestions until kept)

Marking twenty passages must be reversible in one move. Agent-authored highlights
render with an **agent style** (a subtle author badge on the mark + rail row) and
the turn shows a batch affordance: **Keep all · Undo all** (and per-mark remove
via the normal popover). Undo removes the agent-authored highlights from that
turn; Keep leaves them as ordinary highlights. Author-tagging makes both
one-query operations.

## How the three examples resolve

| Request | Turn behavior |
|---|---|
| "Mark the important passages in yellow" | N `highlight(quote, "yellow")` calls, no prose. Marks appear; Keep/Undo offered. |
| "Mark experimental results in red" | `highlight(quote, "red", "experimental result")` per result. |
| "Distinctions between sharding strategies — answer and mark" | Streams the prose answer into the thread **and** `highlight(quote, color, label)` per passage, linked to that answer. |

## Testing

- Tool dispatch: a fake model emitting `highlight`/`note` tool calls creates
  highlights via `HighlightService` with `author = Agent` (reuses Phase 1's
  author-agnostic tests).
- HTML resolver: quote (with whitespace variance) → correct `TextOffset`;
  not-found → reported unresolved.
- PDF resolver: quote spanning two text-layer items → union rects on the right
  page; multi-line quote covered.
- Undo: removing an agent turn's marks deletes only `author = Agent` rows from
  that turn and leaves user highlights and the linked answer's thread intact
  (unless the thread is empty).

## Risks

- **Resolver precision** is the core risk — paraphrase or OCR'd PDF text can
  miss. Mitigations: exact-then-fuzzy match, per-page scan, explicit
  "unresolved" reporting instead of wrong-span marks.
- **Prompt cost / loop**: tool-calling adds round-trips. Keep the tool set tiny
  (two tools) and cap marks per turn.
- **Two-column / rotated PDFs**: best-effort in v1; unresolved passages degrade
  to a reported miss, never a misplaced mark.

## Rollout (slices)

1. Tool registry + `highlight`/`note` tools mapped to `HighlightService`;
   author-tagging enforced.
2. Chat turn emits highlight intents (streamed alongside prose).
3. Client resolvers: HTML offset match, then PDF text-layer match.
4. Accept/undo affordance + agent mark styling.

## Non-goals

- Agent editing/removing the **user's** highlights (agent touches only its own).
- Library-wide or cross-paper agent marking.
- Resolver perfection on adversarial PDFs (columns/rotation) — reported misses
  are acceptable in v1.
