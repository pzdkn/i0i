# RFC 0063: Reader toolbar & in-document search

Status: Implemented (HTML search; PDF search deferred — see Scope)
Date: 2026-07-29
Product: i0i
Target: Tauri v2 + SvelteKit (Svelte 5), macOS first
Builds on: PDF Reader UX north star (`docs/design/pdf-reader-ux.md`, "Layout" +
"Reader toolbar"); RFC 0060 (Lucide icons). Independent of 0061/0062 (can land in
parallel). Precedes RFC 0064 (which hosts its AI button here).

## Summary

Introduce a **persistent, document-level reader toolbar** — one strip above the
reading surface that owns document tools, so contextual annotation stays at the
selection and document tools stay put (north-star principle #4). v1 consolidates
what's scattered today (zoom, view/read-as-HTML, focus) into the toolbar with
Lucide icons, adds **in-document search** for the HTML reader (match count +
next/prev, via the Custom Highlight API we already use), and leaves a **reserved
slot** for RFC 0064's ✨ *Highlight with AI* action.

## Problem

The north star calls for a persistent toolbar hosting page nav, zoom, in-document
search, a PDF/HTML toggle, **✨ Highlight with AI**, and **🖍 highlighter-mode**.
Today there is no toolbar: zoom and focus live in `ReaderHeader`'s action-line,
"Read as HTML" lives inside a PDF-failure fallback in `ReaderView`, and there is
**no in-document search at all**. RFC 0064 needs a toolbar to host its button, so
the shell must exist first.

## Scope (v1) — and explicit deferrals

**In:**

1. A new **`ReaderToolbar.svelte`** strip, rendered persistently at the top of the
   reading surface for any loaded document.
2. **Zoom** (`ZoomIn`/`ZoomOut` + a `%` readout) for the PDF reader — moved out of
   `ReaderHeader` so there is one zoom control.
3. **View indicator** (`PDF` / `HTML`) and, when a source URL is available, a
   **Read as HTML** action surfaced here (reusing `ReaderView.readAsHtml`) instead
   of only appearing in the PDF-failure fallback.
4. **In-document search** for the **HTML reader**: a `Search`-icon input; typing
   finds matches in the rendered article, shows `n/total`, and `Enter` /
   next-prev cycle through them, scrolling the current match into view. Matches
   render via a dedicated `i0i-search` custom highlight (distinct from annotation
   highlights).
5. A **reserved, clearly-labeled slot** where RFC 0064 mounts ✨ *Highlight with
   AI* (empty in this RFC).

**Deferred (with reasons):**

- **PDF in-document search.** PDF.js renders pages lazily, so a correct search
  must index text across not-yet-rendered pages and coordinate scroll/paint — a
  substantial, separate effort. v1 ships HTML search (tractable, DOM-based) and
  shows the search box **disabled with a tooltip** on PDF. Tracked as a
  follow-up.
- **Highlighter-mode** (click-drag continuous marking). The north star lists this
  as an open question ("v1 or later?"). Deferred to avoid shipping a half-mode;
  no dead toggle in v1.
- **Page navigation (◀ 3/24 ▶).** The PDF reader is a continuous scroll without a
  current-page signal wired up; a real page indicator needs scroll-position
  tracking. Deferred with search; the toolbar reserves space for it.

## Design

- **`ReaderToolbar.svelte`** (presentational): props for `contentKind`,
  `zoomScale` + `onZoomIn`/`onZoomOut` (shown only for PDF), `canReadAsHtml` +
  `onReadAsHtml`, and search props (`searchQuery`, `matchCount`, `activeMatch`,
  `onSearch`, `onNextMatch`, `onPrevMatch`, `searchEnabled`). It renders nothing
  document-specific itself — all behavior is delegated up to `ReaderView`.
- **`HtmlReader.svelte`** gains an exported search API used via `bind:this`:
  `search(query): number` (returns match count, paints the `i0i-search`
  highlight), `focusMatch(index)` (scroll into view + a distinct "active" style),
  and `clearSearch()`. It reuses the Custom Highlight API already present for
  annotations — a separate highlight name, so search and annotations don't
  collide.
- **`ReaderView.svelte`** owns search state (`searchQuery`, `matchCount`,
  `activeMatch`), renders `ReaderToolbar` above the reading surface, and wires
  zoom (existing `zoomIn`/`zoomOut`), `readAsHtml`, and search → `htmlReaderRef`.
  `ReaderHeader` loses its zoom group (moved to the toolbar).

## Testing

Presentational Svelte components have no unit harness here, so verification is
`pnpm check` + `pnpm build` + manual (matching 0060–0062). The search matcher's
pure logic (case-insensitive substring ranges over the article text) is small and
covered by a node unit test alongside the existing `resolve-quote-html` tests.
Manual: search highlights and counts matches; next/prev cycles and scrolls;
clearing removes the search highlight without disturbing annotation highlights;
zoom works from the toolbar; the PDF search box is disabled with a tooltip.

## Risks

- **Two Custom Highlight consumers.** Search adds an `i0i-search` highlight
  alongside annotation highlights; they must use distinct names and repaint
  independently. Mitigated by a dedicated effect and name.
- **HTML-only search** is a partial feature until PDF search lands; disclosed in
  the toolbar (disabled state + tooltip) so it isn't silently missing.

## Non-goals

- AI auto-highlight — RFC 0064 (this RFC only reserves its slot).
- PDF search, highlighter-mode, page navigation (deferred as above).
- Any change to the annotation model or panel (0061/0062).

## Rollout (slices)

1. ✅ `ReaderToolbar.svelte` shell + zoom/view/read-as-HTML moved into it; zoom
   removed from `ReaderHeader`.
2. ✅ `HtmlReader` search API (`search`/`focusMatch`/`clearSearch`, `i0i-search` +
   `i0i-search-active` highlights) + the pure matcher `find-matches.ts` + its
   node test (`find-matches.test.ts`).
3. ✅ `ReaderView` search state + wiring; PDF search box disabled with a tooltip;
   reserved AI slot in the toolbar.

**Implementation notes (from review):**
- Search runs at a **2-character minimum** (a 1-char query matches thousands of
  ranges per keystroke on a real article).
- `focusMatch` scrolls the **match's own geometry** into the middle of the
  `.html-reader` column (not the containing paragraph), so it works even before
  the highlight paints.
- Paper-change resets search state but deliberately does **not** call
  `htmlReaderRef.clearSearch()` — reading the ref inside that effect would
  subscribe it and cause a redundant reload on every reader mount; stale ranges
  point at removed nodes and the browser ignores them.

> **Primary-format caveat:** in-document search works for **HTML documents only**
> in this RFC. PDFs — the reader's most common format — have **no search yet**
> (the box is disabled with a tooltip). If PDF search is wanted before RFC 0064,
> it should be its own slice ahead of it.
