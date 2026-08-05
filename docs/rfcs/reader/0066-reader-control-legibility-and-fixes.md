# RFC 0066: Reader control legibility & bug fixes

Status: Implemented
Date: 2026-07-29
Product: i0i
Target: Tauri v2 + SvelteKit (Svelte 5), macOS first
Builds on: `docs/design/pdf-reader-ux-refinement.md` (R4, R5, R6a). Follows RFC
0060–0064.

## Summary

The first refinement slice: make the reader's controls legible and fix two
correctness bugs found in first real use. Specifically —

- **R6a:** stop offering **Read as HTML** for local PDFs (it errors on a
  `local://…` URL).
- **R5:** fix the **✨ Highlight with AI** menu layout (it renders malformed /
  clipped).
- **R4:** make the annotation **filters legible** — put them behind a labeled
  **Filter** control with grouped, labeled sections, and rename **You → Me**.

Small, isolated, low-risk; no model or storage change.

## Problem

From the refinement doc's walk-through:

- The toolbar shows **Read as HTML** whenever any source URL exists. A local
  PDF's URL is `local://sha256/…`, which `open_html_document` can't fetch —
  clicking it errors (`builder error for url`). It should only appear for real
  `http(s)` web sources.
- The **✨ menu** is malformed — it's an `absolute`-positioned popover inside the
  toolbar, clipped by an ancestor's overflow.
- The filter row (`All · You · AI`, color dots, `Note · Chat · Starred`) reads
  like actions, is unlabeled, and always occupies space. "You" is inconsistent
  with the rest of the UI.

## Changes

### R6a — Read as HTML only for web sources

- In `ReaderView`, compute `canReadAsHtml` from the **scheme**: true only when the
  fallback source URL starts with `http://` or `https://` (a saved web page or a
  discovery candidate), never `local://`. Pass that to the toolbar.
- (Defense in depth) `readAsHtml()` early-returns unless the URL is `http(s)`.

### R5 — ✨ menu renders correctly

- Anchor `AiHighlightMenu` with **`position: fixed`** at coordinates computed
  from the trigger button's bounding rect (the pattern `HighlightPopover`
  already uses), so no ancestor `overflow` clips it. Clamp to the viewport.
- The toolbar passes the button element (or its rect) to the menu on open; the
  menu positions itself top-right under the button.

### R4 — Legible filters

- Replace the always-on filter row in `ReaderInspector` with a single **Filter**
  button (Lucide `SlidersHorizontal`/`Filter` + the word "Filter" + a count of
  active filters). Clicking it expands a labeled panel:
  - **Author:** All · Me · AI
  - **Color:** (the swatches + "Any")
  - **Has:** Note · Chat · Starred
- Collapsed by default, so the Marks list has room. A **Clear** action appears
  when any filter is active. Rename **You → Me** throughout.
- The expanded panel is visually a *filter* (labeled groups, inset), distinct
  from action buttons.

## Testing

Presentational Svelte; verify with `pnpm check` + `pnpm build` + manual:

- Local PDF: no **Read as HTML** button; a saved web page: it appears and works.
- The ✨ menu opens fully visible under the button, not clipped, and closes on
  outside-click / Escape.
- Filters collapsed by default; expanding shows labeled groups; each filter
  still narrows the list; active-count + Clear behave; "Me" replaces "You".

## Risks

- **Fixed-position menu drift on scroll/resize.** The toolbar doesn't scroll, but
  a window resize while open could misplace it. Mitigation: recompute on open;
  close on outside interaction (already implemented) — acceptable for a transient
  menu.

## Non-goals

- Marks vs Chats split, Ask-marker (RFC 0067); Note/Chat switch (RFC 0068); PDF
  quote resolution (RFC 0069).

## Rollout (slices)

1. ✅ R6a — scheme-gated Read as HTML (`isWebUrl` in `ReaderView` + `readAsHtml`
   guard); the button is hidden for `local://` PDFs.
2. ✅ R5 — `AiHighlightMenu` is `position: fixed` at button-anchored coordinates
   (viewport-clamped), so no ancestor overflow clips it.
3. ✅ R4 — filters collapsed behind a labeled **Filter** control (funnel + active
   count) with **Author / Color / Has** groups; **You → Me**; filters reset on
   paper change (consistent with the RFC 0063 search reset); the empty-state
   **Clear** (gated on `anyFilterActive`) remains reachable when collapsed.

**Note:** R5 is a best-guess fix — "malformed" was diagnosed as ancestor-overflow
clipping (the menu was `absolute` inside the toolbar). The `fixed` reposition
addresses that cause; if the menu still looks wrong, it's a different symptom and
needs a screenshot.
