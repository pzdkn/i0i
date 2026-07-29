# RFC 0064: Explicit AI auto-highlight + structured outputs

Status: Implemented
Date: 2026-07-29
Product: i0i
Target: Tauri v2 + SvelteKit (Svelte 5), macOS first
Builds on: PDF Reader UX north star (`docs/design/pdf-reader-ux.md`, "AI
features"); RFC 0061 (AI-authored annotations without a conversation); RFC 0063
(toolbar to host the button); the shipped annotation plumbing (RFC 0059:
`create_agent_highlight`, the quote→locator resolvers, the batch Keep/Undo turn).

## Summary

Retire chat-driven marking. Make AI highlighting an **explicit toolbar command**:
✨ *Highlight with AI* opens a **discrete multi-select lens menu** (categories,
each with a default color, plus a Custom row), and the annotation call switches
from streamed **tool-calls** to a **`json_schema` structured output** — one
parseable `{ "highlights": [ { "quote", "color", "label" } ] }` object. No prose,
no keyword-guessing, no conversation. Results resolve to locators and become
AI-authored highlights with the existing **Keep · Undo** batch affordance.

## Problem

Today AI marking is a hidden chat trick (north-star smell #2): you type
"highlight the results" into the ask box, a keyword gate (`shouldAnnotate`)
guesses intent, and `annotate_streamed` fires a **streamed tool-call** pass whose
fragments must be reassembled. It is undiscoverable, fragile, and coupled to a
conversation that also emits prose. The north star calls for a *command* — a
named action with concrete inputs and a visible result — and for **structured
outputs** instead of streamed tool-call assembly.

## Design

### 1. The command surface (frontend)

- The toolbar's reserved slot (RFC 0063) gets a **✨ Highlight with AI** button
  that opens a small popover **lens menu**:
  ```
  ☐ Key contributions 🟡   ☐ Limitations 🔴
  ☐ Methods           🔵   ☐ Definitions 🟣
  ☐ Results           🟢   ☐ Custom…     (free text + color)
                       [ Highlight ]
  ```
  - **Multi-select**, each category carrying a default color, so one action can
    mark several categories at once (contributions=yellow, results=green, …).
  - **Custom…** is a single row (free text + a color) for off-menu requests.
  - v1 ships a fixed category set; *editing the category set* is deferred.
- On **Highlight**: send the selected categories to the backend, then resolve
  each returned passage and create AI-authored highlights, showing a **"Marking…
  N"** progress indicator and, when done, a **Keep · Undo all** bar — reusing the
  existing per-turn affordance (`turnHighlightIds`, `undoTurnHighlights`) now
  driven from the toolbar rather than a chat turn.

### 2. Structured outputs (backend)

- Extend `ResponseFormat` (`services/llm.rs`) to support **`json_schema`**
  (OpenAI/OpenRouter shape: `{ type: "json_schema", json_schema: { name, strict,
  schema } }`), alongside the existing `json_object`.
- New service method `auto_highlight(scope, categories) -> Vec<HighlightIntentData>`:
  assembles the **document** context (reusing `prepare_ask_at_anchor` with the
  `Document` anchor), builds one instruction enumerating the chosen categories +
  colors, and calls the **non-streaming** `complete()` with the fast
  **annotation model** and the strict `highlights` schema. Parse the single JSON
  object → the passage list. No tools, no streaming, no prose.
- New command `auto_highlight(scope, categories) -> Vec<HighlightIntent>` and a
  `autoHighlight` bridge. It returns the passages; the frontend resolves + creates
  them via the **existing** `handleHighlightIntent` path (quote→locator matcher +
  `create_agent_highlight`), so all the shipped resolution/geometry is reused.

### 3. Retire the chat trigger

- Remove the `shouldAnnotate` keyword gate and the `runAnnotationPass` trigger
  from the ask flow (`ReaderInspector.ask`). Asking is now purely a conversation;
  it never marks. The streamed `annotate_streamed` tool-call path is retired from
  the UI (the command/service may remain until a later cleanup, but nothing calls
  it).

## Data & schema

`json_schema` (strict):

```json
{ "type": "object",
  "properties": {
    "highlights": { "type": "array", "items": {
      "type": "object",
      "properties": {
        "quote": { "type": "string" },
        "color": { "type": "string", "enum": ["yellow","green","blue","red","purple","orange"] },
        "label": { "type": "string" } },
      "required": ["quote","color"] } } },
  "required": ["highlights"] }
```

`HighlightIntentData { quote, color, label }` already exists and already maps to
the wire `HighlightIntent`; this RFC produces the same values from a parsed JSON
object instead of assembled tool-call fragments.

## Testing

- **Backend (unit):** `ResponseFormat::json_schema(...)` serializes to the
  expected wire shape (a serde test beside the existing `response_format` tests);
  a `parse_auto_highlights(json) -> Vec<HighlightIntentData>` pure function is
  unit-tested (well-formed list, missing optional `label`, empty list, malformed
  JSON → error). The networked `complete()` call is covered by `cargo build` +
  manual (matching RFC 0065's acquire path).
- **Frontend:** `pnpm check` + `pnpm build`; manual — the lens menu marks the
  selected categories, "Marking… N" shows, Keep/Undo works, and asking in chat no
  longer produces marks.

## Risks

- **Model support for `json_schema`.** Not every model honors strict structured
  outputs. Mitigation: the annotation model is configurable
  (`DEFAULT_ANNOTATION_MODEL`); on a parse failure we surface a clean "couldn't
  read the AI's response" error and mark nothing (no partial garbage).
- **Whole-document context size.** Auto-highlight sends document context; the
  existing `CONTEXT_CHARS_CAP` bounds it (same as ask).
- **Quote resolution misses** remain possible (page text vs. model quote); the
  existing unresolved-count reporting is reused.

## Non-goals

- Editing/persisting a custom category set (v1 is a fixed set + one Custom row).
- Highlighter-mode, PDF search (RFC 0063 deferrals).
- Removing the `annotate_streamed` command/service code (retired from the UI here;
  code deletion is a later cleanup).

## Rollout (slices)

1. ✅ **Backend structured outputs:** `ResponseFormat::json_schema` +
   `JsonSchemaFormat`; `auto_highlight` service method + `parse_auto_highlights`
   (4 unit tests); the `auto_highlight` command (`HighlightIntentPayload`); bridge
   `autoHighlight`.
2. ✅ **Toolbar action:** the ✨ *Highlight with AI* button + `AiHighlightMenu`
   lens popover (5 preset lenses with colors + a Custom row).
3. ✅ **Wiring:** `ReaderView.runAutoHighlight` → resolve each via the existing
   `handleHighlightIntent` → an AI bar under the toolbar showing **Marking… N**,
   then **Keep · Undo all**. The bar **always** gives feedback — including a
   "Couldn't locate N passages" branch for the common PDF case where no quote
   resolves (so the command is never silent).
4. ✅ **Retire the chat trigger:** removed `shouldAnnotate`/`runAnnotationPass`
   and the marking progress/turn-affordance UI from `ReaderInspector.ask`; asking
   is now purely a conversation.

**Implementation notes:**
- `runAutoHighlight` resolves passages **sequentially** (progress ticks up, no
  reload storm), capped at 25 by the backend.
- The `json_schema` strict schema omits `additionalProperties:false` (some
  providers reject extra-strict schemas); parsing tolerates missing `label`.

**Deferred cleanup (non-breaking):** the `annotate_streamed` service/command/
bridge is now unused by the UI (kept until a later deletion), and
`ReaderInspector` still declares `onHighlightIntent`, `onAskTurnStart`,
`onAskTurnComplete`, `showTurnAffordance`, `turnHighlightCount`,
`onKeepTurnHighlights`, `onUndoTurnHighlights` (still passed by `ReaderView`) —
all now dead there; remove them when trimming the chat-marking plumbing.
