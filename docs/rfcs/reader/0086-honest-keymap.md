# RFC 0086: Every key the app advertises should do something

Status: Proposed
Date: 2026-08-13
Product: i0i
Target: Tauri v2 + SvelteKit (Svelte 5), macOS first
Milestone: Release 0.0.1
Builds on: RFC 0063 (reader toolbar), RFC 0071 (Zotero-style layout),
RFC 0079 §4 / RFC 0081 / RFC 0082 (focus mode and `Esc`).

## Summary

The app prints a keyboard legend in three places — two footers and the activity
rail — and honours almost none of it.

`ReaderFooter.svelte` is a static bar with no script block at all. It says
`< p.2 / 15 >`, `mode: PDF`, `scroll / up down / j k`, and then advertises
`h` highlight, `n` note, `q` question, `cmd+enter` ask. Not one of those is
wired — and the page count is a literal `2 / 15` for every document in the
library. `StatusBar.svelte:27-29` advertises a different set: `[j k] navigate`,
`[o] open`, `[cmd+k] palette`.

The activity rail prints a letter under each of its six modes as though it were
a shortcut; none of the six is bound, and two of the modes (**GRAPH**, **ASK**)
do nothing when clicked either (`+page.svelte:837` handles only `V`, `R`, `F`).

Of the eleven keys the two bars promise, **exactly one works**: `cmd+k` focuses
the title-bar search (`TitleBar.svelte:117`). The reader's only real key handler
is `Esc` (`ReaderView.svelte:124`, disarm tool then leave focus mode) and the
window's only global is the same `Esc` (`+page.svelte:333`).

A legend for a dead key is worse than no legend: it teaches a gesture, the
gesture does nothing, and the user concludes the app is broken rather than
unfinished.

## Principle

**A key is either bound or unadvertised.** This RFC binds the keys that have an
obvious meaning, and *removes the legend* for the ones that do not. The release
criterion is not "eleven keys work" — it is "the app makes no promise it does
not keep."

---

## 1. Keys to bind

All reader keys are scoped to the reader workspace and suppressed while typing
(`target` is `INPUT`/`TEXTAREA`/`contentEditable`), the guard `+page.svelte:331`
already uses.

| Key | Action | Notes |
|---|---|---|
| `h` | Highlight the current selection | `highlightSelection()` (`ReaderView.svelte:283`) already exists behind the toolbar button. No-op with no selection. |
| `n` | Note the current selection | `revealSection("notes")` (`:277`), same as the toolbar's Note. |
| `q` | Ask about the current selection | `revealSection("chat")`. `q` for "question" is what the footer already claims. |
| `cmd+enter` | Send the composer / ask | Only when a composer has focus, which is the one case where the typing guard must *not* suppress it. |
| `cmd+=` / `cmd+-` | Zoom the PDF in / out | `zoomIn` / `zoomOut` (`ReaderView.svelte:1199,1203`) exist and are bound only to toolbar buttons. `cmd+0` resets to the 1.15 default. |
| `←` / `→` | Previous / next page | PDF only. Needs the page-scroll seam PdfPage already has for citation flash. |
| `Esc` | Unchanged | Disarm tool, then leave focus mode. |

`cmd+=` is spelled `cmd+plus` in the legend and matched as both `=` and `+`,
since the unshifted key reports `=`.

## 2. Keys to stop advertising

`j`, `k`, `o` are vim-style list navigation for a list that does not have
keyboard focus semantics yet — the vault paper list is a stack of `<button>`s
with no roving tabindex, and giving it one is its own piece of work. The report
says as much: *"i dont what they do honestly … If we have no good idea, what
they should do, keep them dead for now."*

R2.1 Remove `[j k] navigate` and `[o] open` from `StatusBar.svelte`. Keep
`[cmd+k] palette`, which works — though it opens a **search**, not a palette, so
the legend should say `[cmd+k] search`.

R2.2 Remove `scroll / up down / j k` from `ReaderFooter.svelte`.

R2.3 Record `j`/`k`/`o` in this RFC's Open Decisions rather than in the UI, so
the intent survives without the app claiming the keys are live.

## 3. The activity rail has two modes that do nothing

### Diagnosis

`ActivityRail.svelte:16` lists six modes — `V` vault, `F` find, `R` read,
`G` graph, `A` ask, `S` study. `handleModeSelect` (`+page.svelte:837`) handles
three: `V`, `R`, `F`. Clicking **GRAPH**, **ASK** or **STUDY** does nothing at
all, silently, and each already prints its key letter under the label as though
that letter were a shortcut. None of the six letters is bound to a key.

### Change

R3.1 **Remove GRAPH and ASK.** Graph is a feature nobody has specified; Ask is
already what the reader's Chat section and RFC 0087's "Ask this vault" do, from
the surfaces where the asking makes sense. A mode is a place you go, and neither
has a place to go to.

R3.2 **Keep STUDY**, which RFC 0093 gives a destination. Until that lands it is
the one mode allowed to be disabled-with-a-reason rather than removed, because
its RFC exists.

R3.3 The rail becomes **V / F / R / S**, and each is bound to its own first
letter as a global shortcut — the letters the rail has been printing all along.
Subject to the same `isEditing` guard as §1.

R3.4 A mode with no destination is `disabled`, not inert. The pattern is already
in the toolbar: RFC 0086 §1's keys and the toolbar's AI button both disable with
a title explaining why.

## 4. The footer is lying about the document, too

`ReaderFooter.svelte` has no props. `p.2 / 15` and `mode: PDF` are hardcoded for
every paper, including HTML articles.

R4.1 The footer takes `contentKind` and, for PDFs, the current and total page.
`PdfPage` knows both; `ReaderView` already threads `contentKind` to the toolbar.

R4.2 For an HTML article there are no pages: the footer shows `mode: HTML` and
omits the page counter rather than inventing one.

R4.3 The `<` `>` page arrows become real buttons bound to the same action as
`←` / `→`.

---

## Task list

| # | Task | Ships alone | Size |
|---|---|---|---|
| 1 | R1 reader keymap (`h`, `n`, `q`, `cmd+enter`, zoom, arrows) | yes | M |
| 2 | R2 remove dead legends | yes | XS |
| 3 | R3 rail modes: drop GRAPH/ASK, bind V/F/R/S | yes | S |
| 4 | R4 footer reflects the real document | yes | S |

## Risks

- **A single-letter global is a trap next to a text field.** Mitigated by the
  existing `isEditing` guard, which must wrap every binding in R1 except
  `cmd+enter`.
- **`cmd+-` and `cmd+=` are browser zoom in a webview.** They must
  `preventDefault`, or the whole app scales instead of the page.

## Open Decisions

- **A. What should `j`/`k`/`o` do?** The natural answer is paper-list navigation
  in the vault, which needs a focus model for `PaperList` first (roving
  tabindex, a selected row that survives re-render). Recommendation: defer, and
  do it as part of a list-navigation RFC rather than bolting three keys onto a
  list that cannot hold focus.
- **B. Is `cmd+k` a search or a palette?** It is a search today. A real command
  palette is a separate feature; until it exists the legend should describe what
  happens.

## Success criteria

1. Every key printed in the reader footer, the status bar and the activity rail
   performs the action it names.
2. No key is printed that does nothing, and no mode button is inert.
3. The footer's page count and mode describe the document actually open.
