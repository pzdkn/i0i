# RFC 0081: Focus mode hides all chrome, and gives it all back

Status: Implemented (pending manual verification)
Date: 2026-08-13
Product: i0i
Target: Tauri v2 + SvelteKit (Svelte 5), macOS first
Builds on: RFC 0048 (focus mode with threads rail), RFC 0071 (Zotero-style
layout), RFC 0072 (reader header sizing), RFC 0079 §4 (focus starts collapsed,
hover-revealed toolbar, `Esc` exits).

## Summary

RFC 0079 R4.1–R4.4 got focus mode most of the way: the activity rail, the vault
explorer and the workspace tabs disappear, the inspector starts collapsed to a
rail, the toolbar hides above the fold and comes back on hover, and `Esc` exits.
What is still on screen when you enter focus mode on a paper:

| Element | Where | In focus mode today |
|---|---|---|
| App title bar (search) | `AppShell.svelte:39` | **visible** |
| Status bar | `AppShell.svelte:52` | **visible** |
| Reader header (title, authors, meta, threads button) | `ReaderView.svelte:1400` | **visible**, in a 132px-min resizable pane |
| Reader toolbar | `ReaderView.svelte:1371` | hidden, hover-revealed ✓ |
| Threads rail | `ReaderView.svelte:1527` | 42px rail, always painted |
| Activity rail / explorer / tabs | `AppShell`, `+page.svelte` | hidden ✓ |

So roughly 150 vertical pixels of chrome and a 42px column survive a mode whose
whole promise is the paper and nothing else.

This RFC finishes the job with one mechanism, the one RFC 0079 R4.2 already
introduced for the toolbar: **a collapsed hover zone at the edge**. Everything
that hides gets a strip you can point at to bring it back. Nothing becomes
unreachable, and nothing new needs to be learned — the toolbar already behaves
this way.

**Out of scope:** an OS-level fullscreen (`window.setFullscreen`) — this is
about the app's own chrome; a typewriter/centered-column reading width;
per-element "keep this one visible" preferences.

---

## 1. The reader header is inside a resizable pane, so it cannot just be hidden

### Diagnosis

`ReaderView.svelte:1348` renders the header and the content as a vertical
`ResizableSplit` with `storageKey="i0i.reader-header-split"` and
`{ id: "header", min: 132, max: 320, default: 132 }`. The RFC 0072 comment in
that block records that raising `min` *lifts already-persisted panes back above
the floor* via `ResizableSplit`'s `clampSizes` — the split actively enforces its
minimum against stored state.

That rules out the obvious implementation. Collapsing the header pane to zero in
focus mode fights the clamp, and sharing the storage key means a focus-mode
collapse would follow you back into normal mode.

### Change

R1.1 In focus mode, do not render the header split at all. The reader content
becomes the whole pane, and `ReaderHeader` moves into the top hover zone
underneath the toolbar. Normal mode keeps the split and its storage key
untouched, so the persisted header height survives.

R1.2 The top hover zone therefore reveals **toolbar + header together**, in that
order — the toolbar is the thing you reach for, the title is the thing you
confirm. One zone, one gesture, one reveal.

R1.3 `ReaderHeader`'s focus-mode branch (`ReaderHeader.svelte:35-42`, the action
line with the threads toggle) stays: while the zone is revealed, the threads
panel is one click away, which is the same affordance the rail gives.

---

## 2. The app title bar and status bar survive focus mode

### Diagnosis

`AppShell.svelte` hides `ActivityRail` behind `{#if !readerFocusMode}` and
leaves `TitleBar` and `StatusBar` unconditional. The title bar is not the OS
window chrome — `tauri.conf.json` declares no `decorations: false` and nothing in
`src/` carries `data-tauri-drag-region`, so the native macOS title bar is doing
the window dragging and closing. `TitleBar.svelte` is the app's own bar: path,
vault status, and the library search box. `StatusBar` is a thin strip of vault
status and bridge errors.

Both are safe to hide. Neither is safe to *remove*: the search box is the only
way to search the library, and a bridge error is the app telling you why nothing
works.

### Change

R2.1 In focus mode, `AppShell` wraps `TitleBar` in a collapsed hover zone at the
top of the window and `StatusBar` in one at the bottom, each leaving a 6px
always-live strip.

The bars **slide off the window edge** rather than collapsing inside a clipped
`max-height` box the way `.focus-toolbar-zone` does. `TitleBar`'s search results
are an absolutely positioned dropdown (`.results`, `max-height: 60vh`) that is
far taller than the 28px bar, and `overflow: hidden` on the zone would cut them
off — hiding a function instead of hiding chrome, which §4 forbids. So the zone
is a 6px `position: relative` strip holding a `position: absolute` panel at
`translateY(±100%)`, brought to `translateY(0)` on reveal. The page below never
reflows, and the revealed bar overlays it.

R2.2 The status bar reveals itself unconditionally whenever `bridgeError` is
non-empty. An error you have to hunt for is not a report.

R2.3 The hover zones live in `AppShell` with the elements they reveal, not in
`+page.svelte`. `readerFocusMode` is already a prop there and the markup is
already conditional on it.

---

## 3. The threads rail is a permanent 42px column

### Diagnosis

`.focus-collapsed-layout` (`ReaderView.svelte:1626`) is
`grid-template-columns: minmax(0, 1fr) 42px` — the collapsed inspector is a
vertical "Threads" rail that is always painted. RFC 0048 designed it as the
re-expand affordance and RFC 0079 R4.1 made it the default state. It is the
smallest of the offenders, and it is still a column of chrome in a mode that
promises none.

### Change

R3.1 In focus mode the rail collapses to a hover zone on the right edge: `6px`
at rest, the full 42px rail on pointer-enter, using the same transition as the
toolbar zone. Clicking the revealed rail opens the inspector as it does today.

R3.2 The rail keeps its counts (`threads.length`, `pins.length`) when revealed.
Once the inspector is open, focus mode behaves exactly as it does today — the
split at `i0i.reader-focus-split` is unchanged.

---

## 4. Nothing may become unreachable

The constraint the bug report states twice ("all functionalities should be able
to be reexpandable") is worth writing down as a rule rather than a per-element
promise. After this RFC, in focus mode:

| Hidden | Comes back by |
|---|---|
| Toolbar | pointing at the top strip |
| Reader header / title | same top strip (revealed together) |
| App title bar + search | pointing at the very top of the window |
| Status bar | pointing at the bottom; automatically on a bridge error |
| Inspector / threads | pointing at the right edge, then clicking the rail |
| Explorer, tabs, activity rail | `Esc` (leaves focus mode) |

R4.1 Every reveal is a pointer-enter on a strip that is always live, never a
keyboard-only or menu-only path. R4.2 A revealed zone stays revealed while the
pointer is inside it, and while a tool is armed (the existing
`toolbarRevealed = toolbarHovered || activeTool !== null` rule, unchanged).

---

## Task list

| # | Task | Ships alone | Size |
|---|---|---|---|
| 1 | R1.1–R1.3 header out of the split, into the top zone | yes | S |
| 2 | R2.1–R2.3 title bar and status bar hover zones | yes | S |
| 3 | R3.1–R3.2 threads rail hover zone | yes | XS |

## Risks

- **A screen made entirely of hover zones is a screen of hidden state.** Four
  reveal strips is the practical ceiling; this RFC spends all four and adds no
  more. `Esc` remains the escape hatch that brings the whole app back at once.
- **Pointer-enter on a 6px strip is a small target.** It is the target RFC 0079
  R4.2 already shipped for the toolbar and it has not been reported as a
  problem; the three new zones inherit the dimension rather than inventing one.
- **Hiding the status bar hides bridge errors.** R2.2 is the mitigation.
- **The reader header renders in two different places** depending on mode. The
  alternative — a second storage key for a collapsed split — leaves a resizable
  pane you can drag open to a state focus mode says it removed. Two call sites
  of one component is the cheaper of the two.

## Verification

No Svelte component test harness exists in this repo (the `features/reader/`
tests all cover pure-logic modules), so verification is manual:

1. Enter focus mode on a paper: the window shows the page and nothing else — no
   app title bar, no status bar, no paper title, no toolbar, no threads rail.
2. Point at the top of the window → the app title bar appears; point a little
   lower → the toolbar and the paper title appear; move away → both collapse.
3. Point at the bottom → the status bar appears.
4. Point at the right edge → the threads rail appears; click it → the inspector
   opens and focus mode behaves as before.
5. Arm the highlight tool: the toolbar zone stays revealed with the pointer
   elsewhere. `Esc` disarms the tool; a second `Esc` exits focus mode.
6. Disconnect the bridge (or force `bridgeError`): the status bar is visible
   without hovering.
6a. Reveal the title bar and type a library search: the results dropdown is
   fully visible, not clipped to the bar's height.
7. Leave focus mode: the header split still has the height it had before, and
   normal mode is pixel-identical to today.

## Success criteria

1. On entering focus mode, no app chrome is painted — the reading surface is the
   window minus the OS title bar.
2. Every hidden element is reachable with one pointer gesture, and the mode is
   exited with `Esc`.
3. Normal mode is unchanged, including the persisted reader-header pane height.
