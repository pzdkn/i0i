# RFC 0085: A list should hold one kind of thing, and the page should offer Delete

Status: Implemented (pending manual verification)
Date: 2026-08-13
Product: i0i
Target: Tauri v2 + SvelteKit (Svelte 5), macOS first
Builds on: RFC 0062 (annotations panel), RFC 0067 (marks vs chats),
RFC 0079 §1 (annotation deletion), RFC 0080 (the list's delete affordance).
Amends: RFC 0067 R1 — the invariant it states is not the one the code enforces.

## Summary

Two faults, both about a gesture or a list not matching what the rail promises.

- **The Notes list contains conversations.** The rail lets you pick Notes or
  Chat; the Notes list then shows rows of both kinds. *"You should be able to
  see in the listing only the type of item you selected."*
- **The page has no Delete.** RFC 0080 gave the *list* a context menu. Right-
  clicking a mark on the PDF page or in the HTML article still just opens the
  same popover a left-click opens.

---

## 1. Notes lists chats

### Diagnosis

RFC 0067 R1 states the invariant plainly in a comment at
`ReaderInspector.svelte:277`: *"every highlight appears in Marks ∪ Chats exactly
once."* The predicate below it does not implement that:

```ts
const markRows = $derived(
  annotationRows.filter(
    (row) =>
      isStickyNote(row.highlight.locator) ||
      row.highlight.color !== null ||
      row.hasNote ||
      !row.hasConversation,
  ),
);
```

A passage you highlighted **and then asked about** has a color, so it is in
Marks; it has a conversation with entries, so `passageChats` (:302) puts it in
Chats too. It is in both lists — the union is not disjoint, and the Notes list
advertises the overlap with a speech-bubble badge on the row.

Only a *pure* ask — no color, no note — is excluded from Marks today, which is
why the separation looks like it works until you highlight something and ask
about it.

### Change

R1.1 `markRows` becomes: **rows without a conversation.**

```ts
const markRows = $derived(annotationRows.filter((row) => !row.hasConversation));
```

Notes holds highlights, notes and sticky notes. Chat holds conversations. Every
highlight is in exactly one list, which is what RFC 0067 said and what the rail
implies.

R1.2 Asking about a mark **moves** it from Notes to Chat. Its color stays
painted on the page, and the conversation row carries the passage text, so
nothing becomes unreachable — but this is a visible consequence and it is the
point of the change, not a side effect of it.

R1.3 The row's conversation badge goes: no row in Notes can have one. So does
the **Has: Chat** filter chip, which after R1.1 can only ever return nothing.

---

## 2. The page cannot delete a mark

### Diagnosis

`PdfRenderedPage.svelte:613` and `:650` bind `oncontextmenu` to the same
`onHighlightClick` a left-click calls, and `HtmlReader.svelte:197` does the same
via `handleContextMenu`. So right-click opens the highlight popover — which does
carry Remove (`HighlightPopover.svelte:129`), but only after you have found it
among the popover's other controls, and only for a mark whose popover opens
where you can see it.

RFC 0080 fixed exactly this shape in the annotations list: right-click should
name its actions. The page never got the same treatment.

### Change

R2.1 Right-clicking a mark, a sticky note, or a highlighted range in the article
opens a context menu at the pointer with the actions that apply to it:

| Item | Action |
|---|---|
| Open | the popover, i.e. what right-click does today |
| Delete (`.danger`) | remove the mark and its conversation (RFC 0079 R1.3) |

R2.2 The menu is `ReaderView`'s, not each viewer's. `PdfRenderedPage` and
`HtmlReader` already resolve a pointer position to a highlight id; they report
`(highlightId, x, y)` upward through a new `onHighlightContextMenu` callback and
render nothing themselves. One menu, one place, both viewers — the alternative
is the same thirty lines in three components.

R2.3 Left-click behaviour is unchanged: it opens the popover, as it always has.

---

## Task list

| # | Task | Ships alone | Size |
|---|---|---|---|
| 1 | R1.1–R1.3 strict Notes/Chat partition | yes | XS |
| 2 | R2.1–R2.3 page context menu in both viewers | yes | S |

## Risks

- **A mark leaving the Notes list on its first question is surprising once.**
  The alternative is a list that shows two kinds of thing, which is surprising
  every time. R1.2 makes it explicit.
- **Right-click no longer opens the popover directly** — it takes a second click
  via *Open*. Left-click is the one-gesture path and is untouched.

## Verification

Manual (no Svelte component harness):

1. Highlight a passage: it appears under Notes. Ask about it: it leaves Notes
   and appears under Chat, still coloured on the page.
2. A sticky note and a plain highlight stay in Notes; the Notes list shows no
   speech-bubble badges and no *Has: Chat* filter.
3. Right-click a PDF highlight → menu with Open / Delete. Delete removes the
   mark from the page and the list.
4. Right-click a PDF sticky note → same menu, same result.
5. Right-click a highlighted range in an HTML article → same menu.
6. Left-click any of the three → the popover, exactly as before.

## Success criteria

1. The Notes list and the Chat list never contain the same passage.
2. Every mark can be deleted from the page it lives on, from a menu that says so.
