# RFC 0062: Annotations panel (list ⇄ detail + filters)

Status: Implemented
Date: 2026-07-29
Product: i0i
Target: Tauri v2 + SvelteKit (Svelte 5), macOS first
Builds on: PDF Reader UX north star (`docs/design/pdf-reader-ux.md`, "The
Annotations panel"); RFC 0061 (passage primitive + Note/Ask separation); RFC 0060
(Lucide icons).

## Summary

Restructure the reader's right panel from **Threads / Pins / Meta** into an
**annotation-first** panel: **Annotations + Meta**. The Annotations tab is a
**list ⇄ detail** view whose list is the index of *annotated passages* (the
`highlights` rows RFC 0061 made the primitive), filterable by color, author,
has-note, has-conversation, and starred. Clicking a row jumps to the passage and
opens its **detail** — the excerpt, its **note** (inline, editable), and its
**conversation** (the passage-scoped AI thread). A persistent **"Ask about this
paper"** entry opens the document-scoped conversation.

Pins and Threads stop being top-level silos: **Pins → the ⭐ starred filter**,
**Threads → the 💬 has-conversation filter**.

## Problem

The panel is organized around **conversations**, not annotations (north star
"Why this document exists" #3). Today `ReaderInspector` has three tabs:

- **Threads** — a thread-centric list (whole-paper + anchored threads + orphan
  highlights) ⇄ an open-thread detail.
- **Pins** — pinned chat entries, a parallel list.
- **Meta** — paper metadata.

After RFC 0061 the primitive is the **annotated passage**, and a passage can have
a color, a note, and/or a conversation independently. The thread-first list can't
express that (a note-only or color-only passage isn't a "thread"), and there is
no single place to see *everything you marked* or to filter it.

## What changes (and what doesn't)

**Reused as-is (already built in RFC 0061):** the **detail view** — the passage
excerpt, the inline **note field** (Enter saves), the color swatch row, the
conversation Q&A stream, and the Ask composer. This RFC does **not** touch the
detail; it changes the **index** in front of it.

**New/changed:**

1. **Tabs:** `Threads | Pins | Meta` → **`Annotations | Meta`**. The Pins tab is
   removed (its content becomes the ⭐ filter).
2. **List is passage-driven.** Iterate `highlights` (each is an annotated
   passage), not `threads`. Each row: a **color chip** (or the neutral note
   marker when color-less), the **excerpt**, and badges — `StickyNote` (has a
   note), `MessageSquare` (has a conversation), `Sparkles` (AI-authored), `Star`
   (starred). Clicking a row → `onOpenHighlight(id)` (jump to the passage +
   open its detail — the existing path).
3. **"Ask about this paper" entry.** A persistent top row (`MessageSquare`) that
   opens the document-scoped conversation (the existing `openWholePaper`).
4. **Filters.** A compact filter bar over the list:
   - **Author:** All · You · AI.
   - **Color:** All · the six swatches (single-select).
   - **Toggles:** Has note · Conversation · Starred.
   Filters combine with AND; an empty result shows a friendly empty note.

## Data model

No schema change. Every field a row needs already exists:

- `Highlight` — `color`, `note`, `excerpt`, `author.kind` (`user | agent`),
  `locator`.
- Its linked conversation is the `ChatThreadSummary` whose `anchor` matches the
  highlight's `locator` (`samePassage`), carrying `entryCount` (→ has-conversation)
  and `pinnedCount` (→ **starred**, reusing existing pin data — no highlight-level
  "starred" column in v1).

An **annotation row** view-model is derived in the component:

```
row = { highlight, thread?, hasNote, hasConversation, isAgent, starred, color }
```

## Frontend

All changes are in `ReaderInspector.svelte` (plus its parent wiring, unchanged —
`onOpenHighlight`, `onReloadChat`, `openWholePaper` already exist):

- Replace the `tabs` array and the Threads/Pins branches with a single
  **Annotations** branch: when a passage/selection is open (`openThread`), show
  the **detail** (unchanged); otherwise show **"Ask about this paper" + filter bar
  + the filtered annotation list**.
- Derive `annotationRows` from `highlights` × `threads`; apply the active filters.
- Rename the detail's back button and section labels from "Threads" to
  "Annotations".
- Icons via `@lucide/svelte` (`StickyNote`, `MessageSquare`, `Sparkles`, `Star`),
  per RFC 0060.

## Testing

`ReaderInspector` is a Svelte component with no unit harness in this repo, so
verification is `pnpm check` (types) + `pnpm build` + manual, matching how RFCs
0060/0061 were validated. Manual checklist:

- A color-only, a note-only, and a conversation-only passage each appear as one
  row with the correct chip/marker and badges.
- Each filter narrows the list correctly; combined filters AND together; the
  "starred" filter matches passages whose thread has a pinned entry.
- Clicking a row jumps to the passage and opens its detail (note + conversation);
  Back returns to the filtered list.
- "Ask about this paper" opens the document conversation.
- Meta tab unchanged.

## Risks

- **Starred semantics.** v1 derives "starred" from a linked thread's
  `pinnedCount`; a passage with no thread can't be starred yet. Accepted — a
  highlight-level star is a later, additive change.
- **List size.** Papers with many annotations render many rows; the list is a
  simple scroll (same as today's thread list). Virtualization is out of scope.
- **Document-thread pins lose their top-level row.** The old Pins tab listed
  pinned entries across *all* threads, including the whole-paper ("Ask about this
  paper") conversation. The new list is passage-driven, and the document thread
  has no passage/highlight, so a pinned answer in the document conversation is now
  reachable only by opening that conversation (one click from the "Ask about this
  paper" row) rather than from a top-level pin row. Accepted for v1; a
  document-scoped starred view can be added later.
- **Opening a thread-less passage from the list shows the swatch (recolor) row.**
  Passages with no conversation open via the selection path (`selectPassage`),
  which surfaces the color swatches — intended, since that's how you recolor/add
  a color from the detail. Passages that already have a conversation open via the
  thread path (no swatch row); recolor for those remains available from the
  on-page popover. Accepted.

## Non-goals

- Reader toolbar / search / highlighter-mode — RFC 0063.
- AI auto-highlight — RFC 0064.
- A highlight-level "starred" column (v1 reuses thread pins).
- Renaming `Highlight` → `Annotation` in code (deferred cleanup).

## Rollout (slices)

1. ✅ Tabs → `Annotations | Meta`; Pins tab removed.
2. ✅ Passage-driven list (`annotationRows` from `highlights` × `threads`) +
   badges (`Sparkles`/`StickyNote`/`MessageSquare`/`Star`) + "Ask about this
   paper" entry.
3. ✅ Filter bar (author · color · note · conversation · starred), AND-combined,
   with Clear.
4. ✅ Detail back-label/section renames; Lucide icon pass.

Deferred cleanup (non-breaking): the `pins` prop is now unused in
`ReaderInspector` (the starred filter reads `thread.pinnedCount`); it and its
`ReaderView` plumbing can be removed in a later tidy-up.
