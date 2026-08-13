# RFC 0082: Reveal handles you can see, aim at, and pin

Status: Implemented (pending manual verification)
Date: 2026-08-13
Product: i0i
Target: Tauri v2 + SvelteKit (Svelte 5), macOS first
Builds on: RFC 0079 §4 (focus mode), RFC 0081 (focus mode hides all chrome).
Amends: RFC 0081 R2.1/R3.1 and RFC 0079 R4.2 — the hover strip is the right
mechanism at the wrong size, with the wrong exit rule, and with no way to say
"stay".

## Summary

RFC 0081 hid every bar and panel behind a 6px hover strip. The hiding works.
The getting-back does not: *"the options to reexpand toolbars and left/right
panels are a bit limited/fragile."* Three separate faults, all in the same
mechanism:

- **Invisible targets.** A 6px strip with no marking is a target you have to
  already know about. Four of them, all unmarked, all looking like the gap
  between the window edge and the page.
- **No tolerance.** `pointerleave` collapses the zone instantly. Overshoot the
  toolbar by two pixels on the way to a button and it shuts under your cursor;
  the reveal has to be re-earned mid-gesture.
- **No way to say "stay".** Everything is hover-only. There is no state between
  "hidden" and "hidden again the moment I move".

And one gap: in focus mode the **left** side has no affordance at all. The
activity rail comes back only by leaving focus mode entirely (`Esc`), so
switching modes costs you the mode you were in.

## 1. The strips are invisible and too thin

R1.1 Every reveal strip grows from 6px to 12px.

R1.2 Each strip paints a **handle** at rest: a short (28px), 2px, rounded bar in
`var(--border-2)`, centered on the edge — horizontal for the top and bottom
zones, vertical for the left and right. It brightens to `var(--amber)` on hover.
It is the smallest mark that reads as "there is something here," and it is what
turns a discovered gesture into a visible one.

R1.3 The handle is a `<button>`, not a decoration — see R3.

## 2. Leaving by two pixels should not close it

R2.1 Reveal stays immediate on `pointerenter`. Collapse waits **300ms** after
`pointerleave`, and re-entering within that window cancels the collapse.

R2.2 The delay is per zone, cleared on unmount. RFC 0081's existing reset rules
(the stale-hover fix) stay — a timer that fires after the zone is gone must not
write to state.

R2.3 The armed-tool exception is unchanged: `toolbarRevealed = toolbarHovered ||
activeTool !== null` (RFC 0079 R4.2).

## 3. Pinning

R3.1 Clicking a zone's handle **pins** the zone open. A pinned zone ignores
pointer events entirely — it behaves like ordinary chrome until unpinned.
Clicking the handle again unpins it, and it collapses.

R3.2 A pinned zone marks its handle in `var(--amber)` so the state is visible
without hovering.

R3.3 Pins are per focus-mode session. Entering focus mode starts everything
collapsed and unpinned, matching RFC 0079 R4.1's rule that every *entry* into
focus mode is a fresh request for the paper alone.

## 4. The left edge

R4.1 In focus mode, a left-edge zone reveals the `ActivityRail` — same 12px
strip, same handle, same pin. Mode switching stops costing you focus mode.

R4.2 The vault explorer and the workspace tabs stay out. They are navigation
between documents, and focus mode is a mode for one document; `Esc` is the way
back to them. This RFC adds one zone, not three.

## Task list

| # | Task | Ships alone | Size |
|---|---|---|---|
| 1 | R1.1–R1.3 12px strips with visible handles | yes | S |
| 2 | R2.1–R2.3 close delay with cancel-on-re-entry | yes | S |
| 3 | R3.1–R3.3 pinning | yes | S |
| 4 | R4.1 left-edge activity rail zone | yes | XS |

## Risks

- **Five zones is more hidden state, not less.** Mitigated by making all five
  visible at rest (R1.2) and pinnable (R3), which is the opposite trade from
  RFC 0081's — that one bought space at the cost of discoverability, this one
  buys some of the discoverability back at a cost of 6px per edge.
- **A pinned zone in focus mode is just normal mode with extra steps.** True and
  intended: the user decides which chrome earns its space, per paper, per
  session.

## Verification

Manual (no Svelte component harness in this repo):

1. Enter focus mode: four (five with the left) handles are visible at the
   window edges; the bars are not.
2. Point at a handle → the bar appears. Move the pointer off it and back within
   ~300ms → it never collapsed.
3. Move the pointer well away → it collapses after the delay.
4. Click a handle → the bar stays open with the pointer elsewhere, handle amber.
   Click again → it collapses.
5. Click the left handle → the activity rail appears; switching mode from it
   does not exit focus mode.
6. `Esc` still exits focus mode; a pin does not survive re-entering it.

## Success criteria

1. Every hidden element has a visible handle at rest.
2. A reveal survives a small pointer overshoot.
3. Any revealed element can be made to stay without leaving focus mode.
