# RFC 0069: PDF quote resolution waits for the text layer

Status: Implemented (unverified — success criterion is behavioral; needs a live PDF)
Date: 2026-07-31
Product: i0i
Target: Tauri v2 + SvelteKit (Svelte 5), macOS first
Builds on: `docs/design/pdf-reader-ux-refinement.md` (R6). Follows RFC 0064
(AI auto-highlight) and RFC 0059 (PDF quote → rect resolution).

## Summary

Auto-highlight (RFC 0064) parses passages from the model but marks **nothing**
on PDFs — the bar shows "Couldn't locate N passages." The root cause is
**timing**, not the matcher: `PdfPage.resolveQuote` reads each page's *rendered
DOM text layer*, but that layer is populated by an async render that has usually
**not finished** at the moment auto-highlight fires. The matcher is handed an
empty (or partial) string and reports "quote not found."

This RFC makes PDF resolution **await text-layer readiness** before matching,
reusing the existing, proven rect geometry (`rectsFromClientRects` on a DOM
Range — the same path manual highlighting uses). No rect math is rewritten.

## Root cause (evidence)

- `ReaderView` already does `await pdfPageRef.resolveQuote(intent.quote)`
  (`ReaderView.svelte:537`) — it *expects* an async resolver.
- But `PdfPage.resolveQuote` (and each `PdfRenderedPage.resolveQuote`) is
  **synchronous**; the `await` is a no-op. It iterates the page refs and reads
  `layerFullText(textLayerElement)` immediately.
- Each `PdfRenderedPage` renders its canvas **and** text layer in an async
  `$effect` (`await textLayerTask.render()`). Auto-highlight runs right after the
  model returns, while most pages' text layers are still empty → the fuzzy
  matcher (NFKD alphanumeric, already tolerant of extraction differences) has
  nothing to match against.

## Change

Each rendered page exposes when its text layer is ready; the parent awaits all of
them before matching.

- **`PdfRenderedPage`** gains `whenTextReady(): Promise<void>`. It tracks a
  `ready` flag that is **reset to false whenever a (re)render starts** (scale
  change, source change) and set **true in the render `finally`** — on success
  *or* error, so a page that fails to render never hangs the await (its text is
  just empty and it contributes no match). Awaiters registered before readiness
  are flushed when it flips true.
- **`PdfPage.resolveQuote`** becomes genuinely async: it `await`s
  `Promise.all(pageRefs.map((r) => r.whenTextReady()))`, then runs the existing
  per-page `resolveQuote` scan. First page that matches wins (unchanged).

Everything downstream (rect geometry, locator shape, `ReaderView` batching,
Keep/Undo, the "Couldn't locate N" fallback) is unchanged.

## Why not reimplement rects from `getTextContent`?

Matching against `page.getTextContent()` directly (independent of the DOM) would
also decouple from render timing, but it requires re-deriving rects from text-item
transforms/viewport math — new, geometry-sensitive code that can't be validated
without live PDF rendering. The readiness gate reuses the rect path that already
produces correct, tight marks for manual highlights, so it's strictly lower risk.
If a *specific* passage still fails after this (a genuine text-extraction
mismatch, not timing), that's a follow-up on the matcher/text source — but the
"marks nothing at all" failure is timing.

## Testing

`pnpm check` + `pnpm build` + **manual (required — this is the slice that must be
seen working)**:

- Open a multi-page PDF, run **Highlight with AI** on a preset immediately after
  the page loads → passages that exist in the text now land as marks (previously
  0). The "Couldn't locate" count reflects only genuinely-absent quotes.
- A single agent-asked passage draws its rect on the correct page.
- Manual highlighting, scale changes, and page switching still work (readiness
  resets on re-render; no hang on a page that errors).

## Risks

- **All pages must be rendered for full coverage.** `PdfPage` already renders
  every page eagerly (`{#each pageNumbers}`), so awaiting readiness only waits for
  work already in flight. A very large PDF means a longer wait before marks
  appear — acceptable; the alternative is missing them.
- **`whenTextReady` must never deadlock.** Mitigated by flipping `ready` in the
  render `finally` (success or error) and resolving pending awaiters there.

## Non-goals

- Rewriting the matcher or switching to a `getTextContent` text source (follow-up
  only if a genuine mismatch survives this fix).
- Progress UI for large-PDF resolution waits.

## Rollout

Single slice — `PdfRenderedPage.svelte` (readiness) + `PdfPage.svelte`
(async scan).
