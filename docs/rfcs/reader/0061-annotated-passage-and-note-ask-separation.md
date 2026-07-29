# RFC 0061: Annotated-Passage Primitive + Note/Ask Separation

Status: Proposed
Date: 2026-07-28
Product: i0i
Target: Tauri v2 + SvelteKit (Svelte 5), macOS first
Builds on: PDF Reader UX north star (`docs/design/pdf-reader-ux.md`); RFC 0058
(highlight primitive + capability layer). Follows RFC 0060 (icons).

## Summary

Make the **annotated passage** the primitive, with three **independent, optional
attachments** — a color highlight, a note, and a conversation — any of which can
exist alone. In particular, split **Note** from **Ask**:

- The **Note** field's **Enter saves a note** (your text; never asks the AI).
- **Ask** is a separate, deliberate action that starts an AI conversation and
  **leaves no mark on the page**.
- A **color** highlight is its own one-click action, addable to any passage
  anytime — it is not a prerequisite for noting or asking.

This is the highest-value quality-of-life fix in the roadmap: it removes the
"Enter surprises me by asking the AI" trap and the note/answer entanglement.

## Problem

Today highlighting is bolted onto chat (RFC 0058/0059 sequence), producing three
coupled smells (`ReaderInspector.svelte`):

1. **Enter asks the AI.** The selection composer's placeholder is literally
   *"Note this passage, or ask… (Enter asks, Shift+Enter newline)"* — **Enter =
   Ask**, Note is a button. A quick margin jot becomes an expensive, surprising
   AI question.
2. **Notes and answers share one thread.** Notes are stored as thread entries
   (`append_chat_entry(thread_id, ChatEntryDraft::note(body))`), interleaved with
   questions and answers. Your private annotations and AI chatter mix.
3. **Color is forced.** `highlights.color` is `NOT NULL`; every note/ask on a
   selection creates a colored mark. You cannot note or ask *without* marking.

## Model change

**Today (Phase 1/2):** a `highlights` row *is* the annotated passage (it owns the
`locator` + `excerpt`), but `color` is required; a note is a thread entry; a
conversation is a thread keyed by `highlight_id`.

**Target:** the `highlights` row remains the annotated-passage primitive, with
color and note as **optional** attributes and the conversation holding **only**
Q&A:

```
highlights (= annotated passages)
  id, paper_id, source_id, locator, excerpt, author, created_at, updated_at
  color: nullable          ← no color ⇒ no color mark
  note:  nullable (NEW)     ← your text on the passage (single, editable)

chat_threads (= conversations)
  … highlight_id            ← the passage it discusses
  entries: question | answer ONLY   (notes no longer live here)
```

**Rendering by state:**

| Passage has… | On the page |
|---|---|
| a color | its color highlight (existing rendering) |
| a note, no color | a **subtle neutral marker** (thin low-opacity dotted underline) so it's findable |
| a conversation only (no color, no note) | **nothing** — the anchor is stored (Annotations list + jump-back), but the page is unmarked |
| color + note | the color highlight (the note shows in the popover/panel) |

> Naming: the storage table/domain type stays `Highlight` to bound the diff, even
> though a row may now have no color. A rename to `Annotation` is a deferred
> cleanup (non-goal here); RFC 0062 already renames the *panel* to "Annotations".

## Interaction spec

Selecting text shows the popup: `🟡🟢🔵🔴🟣🟠  Note  Ask` (icons per RFC 0060).

| Action | Effect | Enter |
|---|---|---|
| **color swatch** | Create/update the passage's `color`. One click, done. | — |
| **Note** | Create the passage (no color required) + open an inline **note field**. | **Enter SAVES the note** (`set_highlight_note`). Shift+Enter = newline. Never asks. |
| **Ask** | Create the passage (no color) + open its **conversation**. | Enter sends the question to the AI (in the ask input only). |

Rules:
- The **note field and the ask input are separate controls** with separate Enter
  behavior. They never share a composer again.
- **Ask sets no color** — the passage stays unmarked on the page unless you add a
  color. (This is the "ask without a highlight" the design calls for.)
- **Independent:** add a color to a noted/asked passage by clicking a swatch, any
  time; add a note to a colored passage via the popover; etc.

## Storage & migration

1. `highlights.color` → **nullable** (`Option<HighlightColor>`). Existing rows keep
   their color.
2. Add `highlights.note TEXT` (nullable).
3. **Backfill notes into the field:** for each highlight whose linked thread has
   `note`-kind entries, set `highlights.note` to those entries' text (joined oldest
   → newest with blank lines). The thread keeps its question/answer entries; the
   migrated note entries are removed from the thread so notes and Q&A no longer
   mix. Idempotent (only runs where `note IS NULL` and note entries exist).
4. Going forward, the note path writes `highlights.note`; `ChatEntryDraft::note`
   is retired from the anchored-note flow (threads become Q&A-only).

## Commands / API

- `create_highlight` / the annotation-create path: `color` becomes
  `Option<HighlightColor>` (a null color creates an un-colored annotation).
- **New:** `set_highlight_note(id, note: Option<String>)` — save/edit/clear the
  passage's note.
- The note UI calls `set_highlight_note`, not `add_note_at_anchor`.
- The ask path is unchanged (thread Q&A stream) except it no longer forces a
  color and no longer accepts note entries.
- `Highlight`/TS types gain `note: string | null` and `color: HighlightColor |
  null`.

## Frontend

- **Selection view (`ReaderInspector`):** replace the single composer with a
  **Note field** (its own submit; Enter saves) and a distinct **Ask** entry;
  update the placeholder/help text (no more "Enter asks"). Wire Note →
  `setHighlightNote`, Ask → the existing stream.
- **Rendering:** add the neutral-marker style for note-only passages (both
  readers); a color-null + note-null passage draws nothing.
- **Popover (`HighlightPopover`):** show the note inline and **editable**; keep
  Ask/recolor/remove. Recolor also *sets* a color on a previously un-colored
  passage.

## Testing

- Note field: Enter **saves a note** and does **not** create a thread/answer;
  Shift+Enter inserts a newline.
- Ask: creates a conversation and **no color mark** (color stays null).
- Independence: a passage can have color-only, note-only, conversation-only, or
  any combination — each added without the others.
- Rendering: color→color; note-only→neutral marker; conversation-only→no page
  mark (unit-check the state→style mapping).
- Migration: existing note entries land in `highlights.note`; questions/answers
  remain in the thread; colored highlights keep their color; second run is a
  no-op.

## Risks

- **Note-entry migration fidelity** — the delicate part. Mitigate: join rather
  than drop, gate on `note IS NULL`, keep it idempotent, and cover with a test.
- **Two "note" concepts during transition** (thread-entry vs field). Mitigate:
  migrate in the same release and retire the thread-note write path.
- **`Highlight` naming** now covers color-less rows. Accepted; rename deferred.

## Rollout (slices)

1. **Storage:** `color` nullable + `note` column + the note backfill migration.
2. **Backend:** optional color through service/commands; `set_highlight_note`;
   note path writes the field; threads become Q&A-only.
3. **Bridge/types:** `color`/`note` nullable; `setHighlightNote`.
4. **Selection UX:** separate Note field (Enter saves) from Ask.
5. **Rendering:** neutral marker for note-only; ask leaves no mark.
6. **Popover:** inline editable note.

## Non-goals

- The **Annotations panel** restructure (list ⇄ detail, filters) — RFC 0062.
- **AI auto-highlight** and structured outputs — RFC 0064.
- Renaming `Highlight` → `Annotation` in code (deferred cleanup).
- Multiple notes per passage (one editable note; richer discussion is Ask).
