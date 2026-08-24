# RFC 0074: Sticky notes and Zotero-style annotation tools

Status: Stale
Date: 2026-08-06
Product: i0i
Target: Tauri v2 + SvelteKit (Svelte 5), macOS first
Builds on: RFC 0058 (highlight primitive), RFC 0061 (annotated passage,
note/ask separation), RFC 0062 (annotations panel), RFC 0067 (marks vs chats),
RFC 0071 (reader layout), RFC 0073 (mark navigation, note/ask intent).

## Summary

Two changes that answer the same complaint — *"if we allow highlighting without
notes, visually they'll get confused with each other"*:

- **A standalone note annotation.** A note becomes a thing you can place on the
  page, anchored at a **point**, drawn as a small in-theme sticky glyph. It is
  its own object with its own anchor, not an attribute of a passage.
- **Persistent annotation tools.** Highlight and Note become sticky toolbar
  toggles with an active-color dropdown. With Highlight active, dragging over
  text marks it immediately. With Note active, clicking places a sticky. With no
  tool active, the reader behaves exactly as it does today (selection popover).

Two decisions were taken before drafting, and they shape everything below:

1. **Highlights keep their optional comment** (Zotero's actual model — its
   sidebar comment on a highlight is not the same object as its note
   annotation). **Nothing migrates**: all 75 marks in the reference vault stay
   as they are, including the 19 that carry both a color and a note. (75 is the
   post-RFC-0072 count — that migration healed 4 orphaned threads into marks,
   which is why it exceeds the 58 quoted there.)
2. Because comments stay, a commented highlight must become **visually distinct
   from a bare one** — otherwise the original complaint survives the change.
   That is R3 below, and it is not optional.

## The visual vocabulary this produces

```
▓▓▓▓▓▓▓▓▓▓            colored band            highlight, nothing attached
▓▓▓▓▓▓▓▓▓▓·           band + trailing glyph   highlight WITH a comment
▤                     sticky glyph            standalone note (own anchor)
░░░░░░░░░░            neutral band            conversation-only passage (RFC 0067)
```

Four states, four appearances. Today the first two are identical, which is the
bug in the current design.

## R1 — Point anchors, with no schema change

A sticky note needs an anchor that is a *position*, not a range. Two new
`Locator` variants:

```rust
Locator::PdfPoint  { source_id, page_index, x, y }   // x/y normalized 0..1
Locator::TextPoint { source_id, offset }             // HTML char offset
```

**These need no migration and no `ALTER TABLE`** — the existing columns already
carry them, which matters after RFC 0072:

| variant | `locator_kind` | columns used |
| --- | --- | --- |
| `PdfPoint` | `pdf_point` | `page_index`, `rects_json` = `[{"x":…,"y":…,"width":0,"height":0}]` |
| `TextPoint` | `text_point` | `start_offset` = `end_offset` = offset |

A sticky note is then simply *an annotation row whose locator is a point*. No
`kind` column, no second table, and every capability the passage row already has
— the Marks list, filters, starring, export, agent authorship, the chat linkage
— applies to it for free. `excerpt` stays non-null (empty string for a note
placed on blank space; the nearby text when we have it).

Touch points: `Locator` (`domain/highlight.rs`, `domain/highlight.ts`),
`insert_highlight` / `highlight_from_row` (`library_store.rs`), and
`samePassage` (`highlight-thread-match.ts:24-29`), which must compare the new
kinds rather than falling through to `false` — a bug that would silently create
duplicate rows at one point.

`create_highlight` and `set_highlight_note` need no signature change: a sticky
note is `create_highlight({locator: point, color, excerpt: ""})` followed by
`set_highlight_note`.

## R2 — Tools: Highlight, Note, and an active color

`ReaderView` gains `activeTool: "highlight" | "note" | null` and `activeColor`,
both persisted (`localStorage`, next to the reader's other preferences). The
toolbar renders them as toggles plus a color dropdown, replacing today's
selection-gated Highlight button — the one that is greyed out ~95% of the time
because it is the only toolbar control bound to a transient selection
(`ReaderToolbar.svelte:137`).

| tool | gesture | result |
| --- | --- | --- |
| Highlight | drag over text | mark created immediately in the active color, no popover; **the selection is then cleared** |
| Highlight | click | nothing |
| Note | click on a page | sticky placed at that point, its editor opens |
| Note | drag over text | text selects normally; **no popover, no sticky** |
| none | drag over text | today's popover: Note / Ask / **Highlight** |

Rules that keep modes from becoming a trap:

- **Escape clears the active tool**, and the tool is per-session-per-reader, not
  global — a mode you cannot see is a mode you cannot leave.
- The active color is shared by both tools and by the popover's Highlight
  action, replacing `stickyColor` (`ReaderView.svelte:79`), which already does
  exactly this job under a different name.
- **Clearing the selection after a tool-highlight is required, not cosmetic.**
  `updateSelectionAffordance` runs on every window mouseup; a still-selected
  range would be re-marked on the next click anywhere in the document.
  `pickColor` already calls `clearSelection()` for this reason
  (`ReaderView.svelte:508`) — the tool path reuses it.
- **A drag with the Note tool active places nothing.** Selecting text stays
  possible (so copy still works) but the popover is suppressed — that
  suppression is what makes the mode feel like a mode. The alternative
  considered was "drag places a sticky at the selection start, quoting the
  selection"; rejected because every stray drag would then litter the page with
  stickies, and copying text would require leaving the mode.

The selection popover keeps Note and Ask and **gains Highlight** — so the "just
highlight this" gesture exists without entering a mode at all. This is the piece
RFC 0073 R1 left undone.

## R3 — Make a commented highlight look different

Required, per decision 2. On the page:

- a highlight with a non-empty `note` draws a small glyph at the **end of its
  last rect**, **on PDF only** — HTML marks are painted with CSS Custom
  Highlights (`HtmlReader.svelte:208-236` sets ranges into `CSS.highlights`),
  so there is no per-mark DOM element to hang a glyph on. The HTML half needs
  the same absolute-positioning design that Phase 4 defers, and rides along with
  it,
- the glyph is the same sticky silhouette as a standalone note, at ~10px, in the
  mark's own color,
- a bare highlight draws nothing extra.

The glyph is **our own**, not Zotero's: a filled square with a clipped
bottom-left corner, drawn as inline SVG so it inherits the mark color and the
amber-on-dark chrome. One shared component (`StickyGlyph.svelte`) used by both
the standalone note and the comment marker, so they cannot drift.

## R4 — Editing a sticky

A sticky is a **`<button>` in the page's annotation layer**, positioned at its
normalized x/y and wired to the same `onHighlightClick(mark.id, event.clientX,
event.clientY)` the rect anchors already use (`PdfRenderedPage.svelte:382-391`).
It must not go through `findHighlightForOffset` or any rect-overlap matching —
a ~10px glyph needs its own hit target, and offset matching has nothing to match
on a point.

That click opens the existing `HighlightPopover`, which already has a note field
with Save, a color row, and Remove (`HighlightPopover.svelte:103-124`). It needs
one change: for a point annotation, hide the "Ask" action — a conversation
anchored to a zero-width point has nothing to quote. Everything else is reused.

In the inspector, stickies appear in the Marks list like any other annotation
(they already satisfy `markRows`, which keeps any row with a note —
`ReaderInspector.svelte:244-248`), with the sticky glyph in place of the color
chip so the list mirrors the page.

## Phasing

1. **R1 + R4 (PDF)** — point locators end to end, sticky rendering on PDF pages,
   editing through the existing popover, Marks-list integration.
2. **R2** — tools and the color dropdown; the popover gains Highlight.
3. **R3** — the comment glyph.
4. **HTML stickies — deferred.** A point anchor in reflowing HTML has to survive
   re-layout, and the honest options (absolute glyph vs. a margin gutter) need
   their own design pass. Until then the Note *tool* is PDF-only and the HTML
   reader keeps note-on-passage. `ReaderMargin.svelte` exists but is **dead
   code** — not imported anywhere — so treat it as a rewrite, not a reuse.

## Alternatives considered

- **Separate `annotations` table for notes.** Cleaner on paper; in practice it
  duplicates every capability the passage row already has (list, filters,
  starring, agent authorship, export) and doubles the reconciliation work in the
  inspector. Rejected — the locator already carries the distinction.
- **Detaching notes from highlights entirely** (the "clean split"). Maximum
  visual clarity, but it breaks 19 existing marks apart, drops the one-gesture
  note flow, and contradicts Zotero's own model, where a highlight comment and a
  note annotation coexist. Rejected by decision.
- **A `kind` column instead of point locators.** Adds a schema migration —
  precisely what RFC 0072 has just finished paying for — to express something
  the locator already says.
- **Zero-size rect in `rects_json` without a new locator kind.** No Rust change
  at all, but then every consumer has to guess whether a degenerate rect means
  "point" or "bad data". Rejected: explicit variants, same storage.

## Implementation notes

Landed: R1, R2, R3, R4 for PDF. `pnpm check` 0 errors, `pnpm build` green,
`cargo test --lib` 270 passed (including a new
`point_locators_round_trip_through_storage`, which is the guard that the
zero-size-rect encoding stays symmetric — a broken round-trip would silently
move every sticky to the page corner).

Five deviations from the draft:

1. **The tool is not persisted; only the color is.** R2 above says "both
   persisted" — wrong. A mode restored on open is a mode you did not choose, and
   the reader gives no persistent indication of it beyond the toolbar. The color
   is a preference and does persist (`i0i.reader-active-color`).
2. **The tools are hidden on HTML papers** (`toolsEnabled={chatEnabled && !isHtml}`).
   This is the Phase 4 deferral showing through as a visible UI difference the
   draft did not mention: with no HTML sticky rendering, a Note tool there would
   place invisible annotations. HTML keeps the selection popover and
   note-on-passage.
3. **`pdfMarks` in `PdfPage` had to admit `pdfPoint`.** It filtered
   `kind === "pdfRect"` before pages ever saw the list, so a page's own sticky
   filter could never match anything — the feature would have looked completely
   dead while being fully wired.
4. **A newly placed sticky is seeded into `highlights` locally before its editor
   opens.** The popover resolves its target by id out of `highlights`; relying on
   the `reloadChat()` round-trip meant a superseded reload (paper switch,
   concurrent refresh) would open an editor over nothing.
5. **`markRows` gained an explicit sticky clause.** A freshly placed sticky has
   no note yet, and the existing predicate only kept it by accident (because
   placement sets a color). Stickies now list unconditionally.

`hasExistingHighlight` needed no change — it delegates to `samePassage`, so the
point arms added there cover it.

### Post-landing defect: `TOOL_COLOR_KEY` temporal dead zone

The first landing of R2 broke **every reader open**, PDF and HTML alike: clicking
a paper in the vault created its tab and rendered nothing. `ReaderView` declared

```js
let stickyColor = $state(loadActiveColor());   // calls it here
const TOOL_COLOR_KEY = "i0i.reader-active-color";   // …declared after
```

`loadActiveColor` is a function *declaration*, so it hoists and is callable — but
its body reads `TOOL_COLOR_KEY`, and a `const` stays in the temporal dead zone
until its own declaration executes. Every mount threw `Cannot access
'TOOL_COLOR_KEY' before initialization` during component init, which in Svelte 5
takes the whole subtree down silently. Fixed by hoisting the constant above its
first use.

Worth recording because **no static check in this repo could have caught it**:
`pnpm check`, `pnpm build`, and `cargo test --lib` were all green while the app
was unusable. TypeScript deliberately permits use-before-declaration across a
function boundary (the function might only be called later), so this is a lint
concern (`no-use-before-define`), not a type one. It was found by mounting the
real `ReaderView` in headless Chrome against a stubbed Tauri bridge and reading
`pageerror` — the reader has no automated mount coverage, and that gap is what
let a one-line ordering mistake ship as a total outage.

## Verification

| # | Change | How it is verified |
| --- | --- | --- |
| R1 | point locators | place a sticky, reload the reader: it returns at the same spot on the same page; no duplicate row when placing twice in the same place |
| R2 | tools | Highlight active → dragging text marks instantly in the active color; Note active → click places a sticky, drag still selects; Escape exits the tool |
| R3 | comment glyph | a highlight with a note is distinguishable from one without at a glance, at 100% zoom |
| R4 | editing | click a sticky → popover with its note; Save persists; Remove deletes; no Ask action on a point |
| — | regression | the 75 existing marks render exactly as before; auto-highlight (RFC 0069) still resolves quotes |
