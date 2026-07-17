# RFC 0048: Reader Focus Mode With Threads Rail

Status: Implemented
Date: 2026-07-17
Product: i0i
Target: Tauri v2 + Svelte, macOS first
Builds on: RFC 0026 (PDF-First Reader Notes), RFC 0027 (PDF Text Selection And Note Creation), RFC 0034 (Unify Notes Into Threads)

## Summary

Add a Reader Focus Mode that gives the PDF most of the workspace while keeping
anchored threads available.

This is not a pure fullscreen PDF viewer. i0i's Reader is PDF plus notes,
questions, and anchored threads. Focus Mode should hide broader navigation, but
it should not remove the thinking tools that make the Reader useful.

## Problem

The current workspace keeps the PDF inside the normal app shell. That is useful
for browsing, but cramped for serious reading. Users need a way to make the PDF
larger without losing access to:

- highlights,
- anchored notes,
- paper chat,
- pinned thread summaries.

Pure fullscreen would solve space but break the research workflow. A side-by-side
focus layout preserves both.

## Goals

- Make the PDF the dominant surface.
- Keep threads accessible in the same mode.
- Allow the threads panel to collapse into a narrow rail.
- Preserve current Reader state: same paper, same loaded PDF, same selected note
  state where possible.
- Avoid a separate fullscreen route or duplicate Reader implementation.

## Non-Goals

- No OS-level fullscreen requirement in this RFC.
- No redesign of note/thread storage.
- No new PDF renderer.
- No mobile-specific Reader layout yet.
- No command palette integration yet.

## Product Shape

Normal Reader:

```text
[ Explorer ] [ PDF Reader ] [ Threads / Inspector ]
```

Focus Reader:

```text
[              PDF Reader              ][ Threads ]
```

Collapsed Threads:

```text
[                    PDF Reader                    ][ rail ]
```

## UI Design

Reader toolbar gains a focus action:

```text
[ Focus ]
```

Icon-only is acceptable once tooltips exist:

```text
[ expand icon ]
```

Tooltip:

```text
Focus reader
```

Inside Focus Mode, the toolbar should expose:

```text
Paper title     PDF controls     Threads toggle     Exit Focus
```

The Reader should keep the app's IDE/terminal flavor: compact controls, no hero
layout, no modal takeover.

## Layout Rules

Focus Mode hides or suppresses broad workspace chrome:

- left Vault/Discover browser,
- normal multi-pane workspace proportions,
- any non-reader sidebars that are not needed for reading.

Focus Mode keeps:

- PDF toolbar,
- PDF page surface,
- threads panel or collapsed threads rail,
- a clear `Exit Focus` affordance.

The threads panel has two states:

```text
open
collapsed
```

Open:

- fixed or resizable width, initially around `340px`,
- best for writing notes, reading existing threads, and chatting.

Collapsed:

- narrow rail, around `40px`,
- shows a thread/comment icon and possibly counts,
- clicking the rail reopens the panel.

The PDF should resize beside the panel. The threads panel should not overlay the
PDF by default, because overlays can cover exactly the text the note refers to.

## Interaction Behavior

Entering Focus Mode:

1. User clicks `Focus reader`.
2. Current Reader tab remains active.
3. Layout switches to Focus Mode.
4. PDF keeps its current document and reloads as little as possible.

Exiting Focus Mode:

1. User clicks `Exit Focus`, or later presses `Esc`.
2. Normal workspace layout returns.
3. Current Reader tab remains active.

Threads behavior:

- If the user clicks a highlight or starts note creation while the rail is
  collapsed, open the threads panel.
- If the user manually collapses the threads panel, keep it collapsed until a
  thread-relevant action needs attention.
- The threads panel state can be per-session UI state for now.

## Implementation Shape

Prefer layout state over a new route.

Possible state:

```ts
type ReaderLayoutMode = "normal" | "focus";
type ReaderThreadsMode = "open" | "collapsed";
```

Ownership options:

- `+page.svelte` owns `readerLayoutMode`, because focus mode changes the outer
  workspace shell.
- `ReaderView.svelte` owns `readerThreadsMode`, because the threads rail is a
  Reader-local concern.

The same `ReaderView` should render in both modes. It may receive a prop:

```ts
layoutMode: "normal" | "focus"
```

This keeps PDF loading, note anchors, chat state, and Reader commands in one
place.

## Accessibility

- The focus button needs an accessible label: `Focus reader`.
- The exit button needs an accessible label: `Exit reader focus`.
- Collapsing the threads panel must not trap focus.
- When the collapsed rail is opened, focus should move to the threads panel only
  if the user explicitly opened it from the rail or a note action requires it.
- `Esc` can exit Focus Mode later, but should not conflict with note editing.

## Risks

- Hiding too much chrome may make users feel lost. Mitigation: keep paper title
  and `Exit Focus` visible.
- Keeping threads visible may make Focus Mode feel not focused enough.
  Mitigation: collapsed rail gives a near-full-width PDF.
- If implemented as a second route, Reader state may fork. Mitigation: use layout
  state and the existing Reader components.

## Validation

- Opening Focus Mode enlarges the PDF area.
- The current paper remains open.
- The threads panel remains available.
- Collapsing the threads panel gives the PDF more horizontal space.
- Clicking the threads rail reopens the panel.
- Exiting Focus Mode returns to the previous workspace shell.
- `pnpm check` passes.

## Implementation Notes

Implemented in the first slice:

- `ReaderView` accepts a `layoutMode` prop and renders the same Reader state in
  normal or focus layout.
- `+page.svelte` owns whether the outer workspace is in Reader Focus Mode.
- Focus Mode hides the Explorer, workspace tab strip, and activity rail.
- The Reader header exposes `Focus`, `Hide Threads`, `Threads`, and
  `Exit Focus` controls.
- The threads panel can collapse to a narrow right rail and reopen from that rail.
- Selecting a passage or opening an existing mark reopens the threads panel.
- `Esc` exits Focus Mode unless the user is editing text.
- Loading/error states still show an `Exit Focus` escape hatch.

## Future Work

- Keyboard shortcut, likely `Cmd+Shift+F`.
- Persist the last focus/threads mode per user.
- Optional OS-level fullscreen integration.
- Command palette action: `Reader: Toggle Focus Mode`.
- Better thread counts or active-anchor indicators in the collapsed rail.
