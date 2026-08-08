# RFC 0073: Mark navigation, explicit Note/Ask intent, and PDF render cost

Status: Implemented — R1, R2, R3 Phase 0 + Phase 1 + Phase 1.5 landed
(`pnpm check` 0 errors, `pnpm build` green, `cargo test --lib` 269 passed).
**R3 Phase 2 was measured and then dropped** — see "The measurement, taken" at
the end of this RFC. Scheduling far pages' text layers onto idle time delivers
the whole win without virtualization's behavioral risk. The original gate read:
**R3 Phase 2 is not implemented and should not be** until the measurement below
says it is needed. The behavioral checks in the Verification table — including
the RFC 0069 quote-resolution regression check — need a live app run.
Deviations found during implementation are recorded under "Implementation notes".
Date: 2026-08-06
Product: i0i
Target: Tauri v2 + SvelteKit (Svelte 5), macOS first
Builds on: RFC 0058 (highlight primitive), RFC 0061 (note/ask separation),
RFC 0062 (annotations panel), RFC 0067 (marks vs chats), RFC 0069 (PDF quote
resolution readiness), RFC 0071 (Zotero-style reader layout), RFC 0072 (reader
defect triage).
Covers: `docs/bug-doc/bug-logs.md` items 4–6.

## Summary

Three reader items — one is a genuine bug with a one-line cause, two are
capability work:

- **R1 (bug) — "Ask" opens the Note editor.** The PDF selection popover's
  **Note** and **Ask** buttons are wired to the *same* handler, which carries no
  intent. Which pane you land in is decided by whichever inspector section you
  last used, persisted in `localStorage`. Label and behavior are simply
  disconnected.
- **R2 (feature) — the marks list should sit below the note input.** Opening a
  passage currently *replaces* the marks list with the passage detail, because
  both live in one `{#if}/{:else if}` chain. The Notes section should compose
  them: detail on top, list underneath, so marking and browsing marks stop being
  modal alternatives.
- **R3 (feature/perf) — PDF scrolling is slow, and faster when zoomed out.**
  Measured on the reported paper: **every one of its 43 pages renders eagerly**,
  producing **~421 MB of canvas backing store** and **21,829 absolutely
  positioned text spans**, all resident at once. Zooming out shrinks every
  canvas quadratically, which is exactly why it feels faster.

R1 and R2 are small and independent. R3 is phased, and its first step is a
measurement, not a rewrite.

---

## R1 — "Ask" leads to the Note pane

### Root cause (evidence)

`PdfRenderedPage.svelte:409-424` renders the selection popover:

```svelte
<button class="popover-action" type="button" onclick={startThread}>
  <StickyNote … /> Note
</button>
<span class="popover-divider" aria-hidden="true"></span>
<button class="popover-action" type="button" onclick={startThread}>
  <MessageSquare … /> Ask
</button>
```

Both call `startThread` (`:329-345`), which calls `onSelectPassage(…)` and
nothing else. The comment above it states the original design outright:

> Both popover actions open this passage's thread; Note vs. Ask is chosen in the
> thread composer (RFC 0034).

That was true under RFC 0034's single composer. RFC 0061 split note from ask, and
RFC 0071 turned them into separate rail **sections** — but the popover was never
updated. So the landing pane is decided by `activeSection`, which the inspector
restores from `localStorage` on mount (`ReaderInspector.svelte:120-132`) and
defaults to `"notes"`. Ask → Note editor, every time, until you happen to leave
the rail on Chat.

The machinery to fix it already exists and is used by the toolbar:
`revealSection("notes" | "chat")` → `requestedSection` (`ReaderView.svelte:156-165`)
→ an effect in the inspector sets `activeSection` and clears the request
(`ReaderInspector.svelte:301-313`). Only the popovers bypass it.

Two more entry points have the same gap:

- `HighlightPopover`'s **Ask / Open thread** → `openHighlightThread`
  (`ReaderView.svelte:806-833`) — opens the passage, requests no section.
- `HtmlReader` has **no popover at all** — selecting text calls `onSelectPassage`
  directly (`HtmlReader.svelte:183-190`). There are no mislabeled buttons there,
  but the same sticky-section ambiguity applies. Out of scope for R1 beyond
  keeping the plumbing symmetric.

### Change

**R1.1 — carry the intent with the selection.** Extend the passage-selection
callback with an optional intent:

```ts
onSelectPassage: (selection: ReaderTextSelection, intent?: "note" | "chat") => void;
```

`PdfRenderedPage` gets `startThread(intent)`; the Note button passes `"note"`,
the Ask button `"chat"`. `ReaderView.selectPassage` calls
`revealSection(intent)` when one is given, and otherwise keeps today's sticky
behavior (so `HtmlReader`, which passes nothing, is unaffected).

Thread it through the whole forwarding path — `PdfRenderedPage` →
`PdfPage` (which passes `onSelectPassage` down verbatim, `PdfPage.svelte:196`) →
`ReaderView.selectPassage`. An optional second parameter is **source-compatible**,
so a forwarding site that drops the argument still type-checks; the failure mode
is "Ask works from some pages and not others," which `pnpm check` cannot catch.
Verify by clicking Ask on a page other than the first.

**R1.2 — `openHighlightThread` requests `"chat"`**, since every one of its call
sites is an Ask/Open-thread action.

**R1.3 — decide the stickiness, don't inherit it.** `activeSection` is persisted
by an effect (`ReaderInspector.svelte:128-132`), so an intent-driven switch also
becomes the default for the *next* selection. Recommended: **keep that**
(clicking Ask twice in a row shouldn't fight you), and note it as deliberate. The
alternative — restore the previous section when the passage closes — adds state
for a case users are unlikely to notice.

### Alternatives considered

- **Drop the two-button popover, keep one "Annotate" action.** Fewer moving
  parts, but the popover's whole value is choosing the intent at the point of
  selection. Rejected.
- **Make the section non-sticky and always default to Notes.** Predictable, but
  it makes Ask-heavy reading a two-click-per-passage loop. Rejected.

---

## R2 — Marks list below the note input

### Root cause (evidence)

The inspector's panel body is a single branch chain
(`ReaderInspector.svelte:653-921`):

```
{#if !chatEnabled}                                     → "add to a Vault" notice
{:else if openThread && (activeSection === "chat" || openPassage)}
                                                       → passage detail
                                                         (swatches, Note field,
                                                          or the chat thread)
{:else if activeSection === "notes"}                   → "Marks" header,
                                                         filter control, list
{:else}                                                → chat thread list
{/if}
```

The detail branch is entered *before* the list branch and serves **both** Notes
and Chat, so opening a passage in Notes takes the marks list off screen
entirely. The two are alternatives today purely because of that chain order —
there is no layout reason for it.

### Change

**R2.1 — compose, don't replace, in the Notes section.** Restructure so Notes
renders both, in this order:

1. the passage detail when one is open — quote, color swatches, Note field
   (`:703-747`),
2. then the "Marks" header + filter control + list (`:809-902`).

Chat keeps replace-semantics: a conversation needs the full panel height, and
nothing in the bug log asks otherwise.

**R2.2 — make the list navigable.** With the list always visible while a passage
is open, clicking a row should move the reader, not just swap the detail. PDF
pages are all mounted today (see R3), so the target page's element is always
reachable.

Do **not** use `scrollIntoView`: it scrolls every scrollable ancestor, and once
R2.1 puts the marks list inside the scrolling `.tab-panel`, clicking a row would
also scroll the inspector out from under the pointer. Follow what
`HtmlReader.focusMatch` already does (`:141-147`) — compute a delta against one
named scroller and call `scrollBy` on it:

```ts
// PdfPage.svelte
export function scrollToPage(pageIndex: number) {
  const scroller = /* the .pdf-scroll element */;
  const target = /* that page's article element */;
  const delta = target.getBoundingClientRect().top
    - scroller.getBoundingClientRect().top
    - scroller.clientHeight / 2
    + target.clientHeight / 2;
  scroller.scrollBy({ top: delta, behavior: "smooth" });
}
```

**R2.4 — Chat composes too, but collapsed.** The same complaint applies to
conversations: with a thread open, every *other* conversation disappears, so
moving between them is a round trip through the back button. Chat therefore
renders the open thread (with its composer) **and** the chat list in one panel —
but behind a disclosure toggle, because a conversation genuinely needs the
height that a marks list does not.

- Collapsed by default: opening a thread means you want to read it.
- The choice is **remembered** (`localStorage`, alongside the section key).
  Without that, the list re-collapses on every thread you open, which reads as
  the panel fighting you.
- The toggle only appears when a thread is open. With no thread, the list *is*
  the section and a collapse control there would just let you empty the panel.
- The list markup is shared between both states via a snippet, so the two paths
  cannot drift.

This supersedes the draft's "Chat keeps replace-semantics" — that was the right
call for the *layout* (the conversation still owns the panel by default) but the
wrong call for *navigation*.

**R2.3 — two sub-decisions to settle before implementing** (both are judgment,
not discovery):

- *Where the "Marks" header and filter control go.* Recommended: they travel
  **with the list**, so the reading order is `detail → Marks (n) → filters →
  rows`. The alternative (header pinned at the top of the section) puts controls
  between the note field and the thing they filter.
- *Scroll position when a passage opens.* The panel scrolls as one
  (`.tab-panel`). Recommended: keep the scroll position anchored to the detail
  (i.e. scroll to top of panel), so the note field is never pushed out of view by
  a long list.

### Alternatives considered

- **Split the Notes section into two independently scrolling panes** (detail
  above, list below, with a resizer). More Zotero-like and it survives long
  notes — but it adds a second `ResizableSplit` to a 320px-wide panel, and
  RFC 0072 has just shown how a fixed-height pane in that column behaves with
  variable content. Rejected for v1; revisit if the composed panel feels cramped.
- **Keep replace-semantics and add a "back to marks" affordance.** That already
  exists (`‹ Marks`, `:662`) and is precisely what the request is pushing back
  on. Rejected.

---

## R3 — PDF rendering cost

### Symptom

> Too Slow when marked and you navigate · Scrolling is too slow · It's faster
> when you zoom out

### Root cause (evidence — measured)

**Every page is rendered eagerly.** `PdfPage.svelte:184-200` mounts one
`PdfRenderedPage` per page with no windowing, and each mounts its own canvas plus
a full pdf.js text layer (`PdfRenderedPage.svelte:81-147`). There is no
`IntersectionObserver`, no scroll listener, and no virtualization anywhere in
`src/lib/features/reader/`. RFC 0069 even relies on it — "work already in flight,
since every page renders eagerly" (`PdfPage.svelte:124`).

Measured on the reported document (`local:eb15b46f6a8f`, 29.7 MB) with the
project's own pdf.js build:

| quantity | value |
| --- | --- |
| pages | 43 |
| page viewport @ `scale: 1.15` | 704 × 911 CSS px |
| canvas backing store @ DPR 2 | 1408 × 1822 = 2,565,376 px = **9.8 MB/page** |
| **total canvas memory** | **~421 MB, all resident** |
| text items (spans) across all pages | **21,829** |

The canvas is allocated at `viewport.width × devicePixelRatio`
(`PdfRenderedPage.svelte:155-159`). DPR 2 is **inferred**, not measured — the
reported screenshots are 2732 px wide for a ~1366 pt window — so read the figure
as "≥421 MB on a Retina display"; on a 1× display it is ~105 MB.

This explains the "faster when zoomed out" clue precisely: cost scales with the
*square* of `scale`. At the zoom floor of 0.65 (`ReaderView.svelte:897-899`),
each canvas is (0.65/1.15)² ≈ **32%** of its size at 1.15 — ~134 MB instead of
~421 MB, and every page re-renders smaller.

**What is *not* the cause.** "Too slow when marked" reads as correlation rather
than a mark-rendering defect: each mark contributes a handful of absolutely
positioned `<button>`s to its page's annotation layer
(`PdfRenderedPage.svelte:380-393`) — on the order of 300 elements for 60 marks,
against 21,829 text spans. The one real mark-linked cost is that every page
re-filters the whole marks array whenever any highlight changes (`:69-78`,
43 × N comparisons) — measurable in principle, negligible in practice. No
mark-specific rendering bug was found; the document is heavy whether or not it
is marked, and marking is simply when you start scrolling back and forth.

Two secondary costs worth fixing while in here:

- **43 window listeners.** Each page registers `onmouseup`/`onkeyup` on
  `<svelte:window>` (`:370`), so every click runs `updateSelectionAffordance`
  43 times, each doing a `contains()` walk and clearing `pendingNote`. It affects
  *selection* responsiveness, not scrolling.
- **43 blurred shadows.** `box-shadow: 0 8px 32px rgba(0,0,0,0.48)` on every
  `.pdf-rendered-page` (`:433`) is real compositing work in WebKit for large
  layers.

**"When you navigate" is ambiguous** and probably covers two different things:
scrolling within a document, and switching papers. The phases below target
**scrolling only**. Switching papers re-runs the entire load — `getReaderPdfBytes`
re-fetches all 29.7 MB over IPC (`PdfPage.svelte:66`), `pageNumbers` resets, and
43 components remount — and that latency is bounded by the byte fetch, which no
phase here changes. If paper-switch feels slow after Phase 1, the fix is a
different one (cache the `PDFDocumentProxy` per source, or stream the bytes), and
it belongs in its own RFC. Measure the two separately before assuming Phase 1
addressed either.

### Change — phased, measurement first

**Phase 0 — cheap wins, no architecture change.** Both are single-line and
independently revertable:

- Cap the canvas backing store: `outputScale = Math.min(window.devicePixelRatio || 1, CAP)`
  (`PdfRenderedPage.svelte:155`). A cap of 1.5 cuts memory ~1.8×; a cap of 1.0
  cuts it 4×, at a visible sharpness cost on Retina. Recommended: cap 1.5, and
  consider lowering it further for documents past ~30 pages.
- Replace the 32 px page shadow with a 1 px border or a much tighter shadow
  (`:433`).

**Phase 1 — virtualize the canvas bitmap, not the component.** Keep every
`PdfRenderedPage` mounted; release only the *pixels* of pages far from the
viewport (clear the bitmap with `canvas.width = 0`, re-render on approach), with
the page's CSS box preserving its size.

This is deliberately the conservative cut, and it is why it goes first:

- `pageRefs`, `whenTextReady()`, and every text layer stay live, so **RFC 0069's
  quote resolution is untouched** — no new geometry code, no re-derived rects.
- **Cross-page text selection and copy/paste keep working**, because the spans
  are still in the DOM.
Two requirements that are easy to miss:

- **Page dimensions before render.** `pageWidth` / `pageHeight` are set after
  `getPage()` resolves (`:110-112`), so a page that never renders would be 0 × 0
  and collapse the scroll height. Fix it in `PdfPage`: walk
  `getPage(n).getViewport({scale})` once per page up front (cheap — it parses the
  page dictionary, not the content stream) and pass width/height down as props.
  Correcting per page as it renders also works, but produces scroll jumps on
  mixed-orientation PDFs.
- **The bitmap release/restore must be its own effect, separate from the
  text-layer render.** The current render effect sets `textReady = false`
  unconditionally at its top (`:93`) and flips it true in its `finally`
  (`:133-139`). If Phase 1 re-runs *that* effect each time a page comes back into
  view, every scroll-approach invalidates readiness — and a `resolveQuote` in
  flight can end up awaiting a page whose text layer was already built and never
  changed. Split it: one effect owns the text layer (runs once per
  document/scale), a second owns the canvas bitmap (runs on visibility). Only the
  first touches `textReady`.

**Phase 2 — only if Phase 1's measurement says the spans still matter.**
Virtualizing text layers is a *behavioral* change, not just a perf one, and it
breaks three things that Phase 1 preserves:

1. cross-page selection and copy/paste,
2. `updateSelectionAffordance`, which requires the range's ancestor to be inside
   a live `textLayerElement` (`:204`),
3. **two** places in `resolveQuote`: `whenTextReady()` on every ref
   (`PdfPage.svelte:145`) *and* the mount-wait loop, whose
   `pageRefs.filter(Boolean).length >= pageNumbers.length` condition
   (`PdfPage.svelte:133`) can never be satisfied when pages are unmounted.

If it proves necessary, the recommended shape is a **hybrid** that keeps
RFC 0069's proven rect math: scan `page.getTextContent()` — no DOM, no geometry —
to find *which* page contains the quote, then mount that one page's text layer
and resolve rects through the existing DOM path. RFC 0069 explicitly rejected
re-deriving rects from text-item transforms; this avoids doing so.

**Phase 1.5 (small, enables R2.2)** — a `scrollToPage(pageIndex)` export on
`PdfPage`, used by the marks list. Trivial while every page is mounted; it also
becomes the primitive Phase 2 would need for jumping into unrendered pages.

### Measurement plan (the first implementation step)

Before Phase 1, capture a baseline on the 43-page document with the reader open
at `scale: 1.15`:

- Safari/WebKit Web Inspector **Timelines** → scroll the full document, record
  frame rate and the layer/compositing breakdown.
- `performance.memory` (or the Inspector's memory panel) after full load.
- Repeat at `scale: 0.65` — the user's own "faster when zoomed out" observation
  is the control condition. If the frame-rate delta tracks canvas memory rather
  than span count, Phase 1 alone is likely sufficient and Phase 2 can be dropped.

Record the numbers in this RFC when they land. Everything above is arithmetic
from real document measurements, but **no profile has been taken** — the phases
are ordered so that the cheapest change comes first and the riskiest one is
gated behind evidence that it is needed.

### The measurement, taken — and what it changed

Taken by mounting the real `ReaderView` in headless Chrome against a stubbed
Tauri bridge, timing `publish → first painted pixel` over 3-4 warmed runs per
configuration, on a 320KB/19-page and a 28MB/43-page document.

| configuration | publish → first ink |
| --- | --- |
| eager text layers, 19 pages | ~130-190ms |
| eager text layers, 43 pages | ~270-420ms |
| scheduled, either document | ~50-110ms |

**The cost scales with page count, so Phase 2's premise was right — but its
prescription was wrong.** The problem is not that far pages *have* text layers,
it is that they build them on the critical path to first paint. Scheduling fixes
that; virtualization is not needed, and none of the three behaviors Phase 2 would
have broken are touched: every page still gets a text layer, so cross-page
selection, `updateSelectionAffordance`, and both `resolveQuote` waits keep
working unchanged.

> **Correction.** The paragraph above was written from *open-latency* data and
> then over-generalized into a verdict on Phase 2 as a whole. It is right about
> opening and wrong about scrolling — see "Scroll" below. The text layers still
> all exist; scheduling only changes *when* they are built, not what the engine
> must lay out on every subsequent frame.

**Implemented instead of Phase 2:** pages beyond the first
`EAGER_TEXT_LAYER_PAGES` (2) build their text layer inside
`requestIdleCallback(…, { timeout: 2000 })`. Two details are load-bearing:

- **Keyed on `pageNumber`, not `isNear`.** Reading `isNear` in that effect would
  make it re-run on every scroll — reintroducing precisely the teardown thrash
  the Phase 1 effect split exists to prevent.
- **The first two pages stay synchronous.** A page that is visible and
  canvas-painted but has no text layer is one you cannot select text on. Idle
  scheduling every page would open that window on the page the user is actually
  looking at, for up to the 2s timeout under load.

The `timeout` is not optional: a starved idle callback would mean a text layer
that never renders, and therefore a quote that can never be resolved.

Phase 2 as drafted (virtualization + the `getTextContent()` hybrid) is
**dropped** — but for the reason in "Scroll" below, not because the text layers
stopped mattering once opening was fast.

### Scroll — a second, separate cost with the same root

Scrolling stayed laggy after the change above, because scheduling does not
reduce how much text layer exists — only when it is built. Every page's spans
remain in the scroll container:

| document | pages | live spans | worst page |
| --- | --- | --- | --- |
| 15pp paper | 15 | 2,869 | 304 |
| 19pp paper | 19 | 4,071 | 685 |
| DeepSeek-V3 | 53 | 9,071 | 647 |
| 28MB paper | 43 | **21,829** | 965 |

Frame intervals while driving `.pdf-scroll` from `requestAnimationFrame`, canvas
rendering held constant across arms (2 live canvases in every case):

| arm | spans | worst frame | frames > 32ms |
| --- | --- | --- | --- |
| all 43 text layers | 20,343 | 140-190ms | 2-3 / 198 |
| near pages only | 475 | 21-46ms | 0-1 / 198 |
| no text layers | 0 | 17-21ms | 0 / 198 |

Jank scales with span count with canvas work fixed, so the spans are causal.
The same spikes appear in a production build (184-197ms), so this is not a
dev-mode artifact.

**Fix: `content-visibility: auto` on `.pdf-rendered-page`**, with
`contain-intrinsic-size` set inline from the measured page box. The engine skips
layout and paint for off-screen pages while every span stays in the DOM — which
is why this is preferable to Phase 2's virtualization: nothing is unmounted, so
cross-page selection, `updateSelectionAffordance`, and both `resolveQuote` waits
keep working with no `getTextContent()` fallback path to build.

The correctness gate was whether geometry survives skipping, since RFC 0069
derives rects from `getBoundingClientRect()`. It does — measured on page 30 of
the 43-page document, a span's rect and its normalized position within the page
are **identical to four decimal places** with and without the property.

Result: worst frame 140-190ms → **30-45ms**, zero janky frames on slow scroll,
scroll height unchanged. Remaining fast-scroll hitches (~40ms) are canvas
renders as pages cross the render margin — inherent to painting PDF pages, and
an order of magnitude smaller than what they replaced.

### The larger cost this measurement uncovered

Profiling first paint surfaced a bigger cost than anything in R3: `get_reader_pdf_bytes`
returned `Vec<u8>`, which serde serializes as a **JSON array of integers**. A
28MB PDF crossed the IPC as a 101MB string.

Fixed by returning `tauri::ipc::Response`, which sends the bytes raw
(`application/octet-stream`) and arrives in JS as an `ArrayBuffer`. This is
independent of R3 and is by far the dominant win.

**Measured in the running app**, timing the `invoke` round-trip with both
commands registered side by side and the page cache warm:

| PDF | `Vec<u8>` (before) | `Response` (after) |
| --- | --- | --- |
| 0.5MB | 78ms | 2ms |
| 3.7MB | 550-569ms | 3-5ms |
| 8.5MB | 1281-1291ms | 6-8ms |
| 28.3MB | 4285ms | 19ms |

Component benchmarks badly understated this. Isolated `serde_json::to_string`
(237ms at 28MB) plus isolated `JSON.parse` (540ms) predicted ~800ms; the real
round-trip was **4285ms**. The gap is the IPC transport itself moving a 101MB
string plus the webview materializing 30M boxed numbers, and it grows
superlinearly with file size. The lesson for future perf work here: **measure
the round-trip in the app**, not the stages in isolation — a harness that stubs
the bridge cannot see the cost that dominates.

### Alternatives considered

- **Render pages at a fixed low resolution and upscale on zoom.** Simple, but
  makes text soft at exactly the moment users zoom in to read it. The DPR cap in
  Phase 0 is the same idea with a much smaller quality hit.
- **Swap in pdf.js's own viewer (`PDFViewer`/`PDFPageView`)**, which already
  virtualizes. It brings the whole viewer's DOM, CSS, and event model, and i0i's
  marks/selection/quote-resolution layers all sit on the current custom
  structure. Rejected — the migration cost dwarfs the fix.
- **Extract page images backend-side (pdfium) and render `<img>` tiles.** Removes
  pdf.js rendering entirely, but loses the live text layer that selection,
  marking, and RFC 0069 all depend on. Rejected.
- **Do nothing until a profile exists.** Phase 0 is two lines with an obvious
  mechanism and a 1.8–4× memory effect; making it wait on instrumentation costs
  more than it saves.

---

## Verification

| # | Change | How it is verified |
| --- | --- | --- |
| R1.1 | popover intent | select text in a PDF with the rail left on **Notes**; click **Ask** → the Chat section opens with the passage. Click **Note** → the Note field. Repeat with the rail left on Chat |
| R1.2 | popover Ask | click an existing mark → **Ask**/**Open thread** → Chat section, not Notes |
| R1.3 | stickiness | after an intent switch, the next fresh selection opens in the section the intent chose (documented behavior, not a regression) |
| R2.1 | composed Notes | open a passage in Notes: the note field *and* the marks list are both visible, list below |
| R2.2 | list navigation | click a mark on page 30 of the 43-page paper: the reader scrolls to it, in both PDF and HTML |
| R2.4 | collapsible chat list | open a conversation: the composer is visible with a collapsed `› Chat · n` header below it. Expanding it lists the other conversations and switching between them takes one click; the expanded/collapsed choice survives a reload |
| R3 P0 | DPR cap + shadow | memory after full load drops ~1.8× at cap 1.5; text still legible at `scale: 1.15` on Retina |
| R3 P1 | canvas virtualization | memory is bounded by the window size, not the page count; scrolling the full document holds frame rate; **auto-highlight still resolves quotes** (RFC 0069 regression check) and cross-page selection still works |
| R3 P1.5 | scrollToPage | as R2.2 |

## Implementation notes

Five deviations from the draft above, all found while building it:

1. **Intent vocabulary is `"notes" | "chat"`, not `"note" | "chat"`.** It now
   matches `InspectorSection` exactly, so `selectPassage` can hand the value
   straight to `revealSection` with no mapping layer — one vocabulary instead of
   two that differ by one character.
2. **`openHighlightThread` takes the intent as a parameter; it does not hard-code
   `"chat"`.** The Marks list reaches the same function through
   `openHighlightById`, so forcing chat inside it would make clicking a row in
   the Notes list jump to Chat. Only the mark popover's Ask passes an intent.
3. **The detail markup is a `{#snippet passageDetail()}`**, rendered from both
   the Notes branch and the Chat branch. Restructuring the chain in place would
   have duplicated ~130 lines; the snippet keeps one copy. It carries its own
   `{#if openThread}` guard, since call-site narrowing does not reach inside a
   snippet.
4. **Page measurement must not block first paint.** Measuring all pages before
   publishing `pdfDocument` serializes N page-dictionary parses ahead of the
   first render — worse open latency than the eager rendering this replaces, on
   the very axis this RFC says it is not addressing. Implemented instead as:
   measure page 1, publish immediately with it as the provisional box for every
   page, then refine all sizes in parallel (which corrects mixed-orientation
   documents a beat later).
5. **`scrollToPage` uses instant scrolling for long jumps.** A smooth scroll to
   page 30 drags the viewport across 29 intermediate pages, each crossing the
   render margin and queueing a canvas render that is cancelled a frame later.
   Over two screens of travel, it lands instantly instead. `HtmlReader` keeps
   smooth scrolling — there is no per-element render cost on that path.

Two things to watch on the first live run, both consequences of splitting the
render effects (neither is visible to `pnpm check`):

- **Quote resolution (the RFC 0069 regression check).** Text layers stay eager,
  so `pageRefs` should still fill and every `whenTextReady()` should still
  resolve — but that is an argument, not a test. Run **Highlight with AI** on the
  43-page paper. If it reports "couldn't locate", the mount-wait loop at
  `PdfPage.svelte:133` is the first place to look.
- **First-page latency.** Text layers used to be serialized behind each page's
  canvas render; they now all start at mount, so 43 `streamTextContent` requests
  compete with the near pages' canvas renders against one worker. If the first
  usable page arrives later than before, throttle the text layer to a wider
  margin than the canvas — a smaller change than reverting the split.

Cosmetic and expected: a released page is a plain white rectangle until its
bitmap is repainted (no "Rendering…" badge, since `isRendering` is only true
during an active paint). That is ordinary viewer behavior on a fast scroll.

## Out of scope

- The remaining `bug-logs.md` item — a collapsible index/outline sidebar — is a
  separate feature with its own extraction dependency (it needs heading-level
  structure, which `pdfium_basic` does not currently produce).
- HTML-reader performance. The HTML path renders one document with CSS Custom
  Highlights and has not been reported as slow.
- Zoom-step behavior and fit-to-width, beyond the DPR cap.
