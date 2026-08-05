# RFC 0067: Marks vs Chats + Ask leaves a marker

Status: Implemented
Date: 2026-07-29
Product: i0i
Target: Tauri v2 + SvelteKit (Svelte 5), macOS first
Builds on: `docs/design/pdf-reader-ux-refinement.md` (R1, R2). Amends the north
star's "Ask leaves no page mark" rule. Follows RFC 0062/0066.

## Summary

Two changes that go together:

- **R2 — Ask leaves a light marker.** A passage you ask about (a conversation,
  no color, no note) now renders the **neutral marker** on the page — the same
  subtle marker a note gets — so it stays findable. (Amends the north star; note
  vs. conversation are told apart by the **panel badge**, not two page styles —
  v1 keeps one "annotated, no color" marker.)
- **R1 — Marks vs Chats in the panel.** The Annotations tab shows a **Marks**
  list (passages with a **color or a note** — "everything I highlighted/noted")
  and, below it, a **collapsible Chats** section listing every **conversation**
  (the whole-paper "Ask about this paper" + each passage conversation). A
  conversation-only passage lives under **Chats** (and is marked on the page),
  not in Marks; a colored/noted passage that also has a chat appears in Marks
  with a chat badge.

Net: "everything I marked" stays clean; conversations get their own home; and you
never lose an asked passage on the page.

## Problem

From first use: **every conversation was dumped into the Annotations list** (#6),
and **asking a passage left no page mark** so you lost it (#4). The primitive is
still the annotated passage, but the *panel* conflated marks and chats, and the
*page* hid conversation passages entirely.

## Model / rendering

`Highlight` is unchanged. Whether a passage "has a conversation" is derived from
the threads (a `ChatThreadSummary` whose `anchor` matches the highlight's
`locator`, with `entryCount > 0`) — the same linkage the panel already uses.

**Page rendering (both readers)** — a passage draws a mark when it has a color, a
note, **or a conversation**:

| Passage has… | On the page |
|---|---|
| a color | that color |
| a note (no color) | the neutral marker |
| a conversation (no color, no note) | the **neutral marker** (was: nothing) |

`ReaderView` computes `conversationIds` (the set of highlight ids with a
conversation) and passes it to `HtmlReader` and `PdfPage`→`PdfRenderedPage`; each
includes those ids in the neutral-marker group.

## Panel (ReaderInspector)

- **Marks list:** annotation rows filtered to **color or note**. Each row keeps
  its badges — `Sparkles` (AI), `StickyNote` (note), `MessageSquare` (also has a
  chat → jump to it), `Star`.
- **Chats section (collapsible, below Marks):** a header **Chats (N)**; rows are
  the **whole-paper** conversation ("Ask about this paper") plus each **passage
  conversation** (excerpt + entry count). Clicking opens that conversation. The
  standalone "Ask about this paper" row moves **into** this section.
- The **Filter** control (RFC 0066) applies to **Marks**. "Has: Chat" narrows
  Marks to color/noted passages that also have a conversation.

## Testing

Presentational; `pnpm check` + `pnpm build` + manual:

- Ask a passage (no color/note) → neutral marker appears on the page (PDF + HTML)
  and the passage is under **Chats**, not **Marks**.
- Color/note a passage → it's in **Marks**; add a chat → a chat badge appears and
  it's also listed under **Chats**.
- Chats collapses/expands; "Ask about this paper" lives at the top of Chats.
- Existing color/note rendering unchanged.

## Risks

- **One marker for note and conversation.** A note-only and a chat-only passage
  look identical on the page (both neutral). Accepted for v1 — the panel badge
  distinguishes them; two distinct page markers are a later polish (doc open
  question).
- **`conversationIds` recompute** on every threads/highlights change — O(threads
  × highlights) via `samePassage`. Fine at reader scale; memoized by `$derived`.

## Non-goals

- A distinct conversation page-marker style (deferred).
- Note/Chat detail switch — RFC 0068.
- PDF quote resolution — RFC 0069.

## Rollout (slices)

1. **R2 rendering:** `conversationIds` in `ReaderView` → `HtmlReader` +
   `PdfPage`/`PdfRenderedPage`; conversation passages join the neutral marker.
2. **R1 panel:** Marks list = color/note; collapsible **Chats** section; move
   "Ask about this paper" into it.
