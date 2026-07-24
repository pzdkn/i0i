# RFC 0056: HTML Reader with Annotation and Chat

Status: Draft
Date: 2026-07-24
Product: i0i
Target: Tauri v2 + Svelte, macOS first
Builds on: RFC 0033 (anchored chat threads), RFC 0048 (reader focus mode),
RFC 0051 (bounded discovery PDF open / source acquisition), RFC 0053 (provider
expansion — Europe PMC/CORE HTML full-text links)

## Summary

Let the reader open, annotate, and chat over **HTML articles**, not just PDFs —
with full parity: the same threads, highlights, pins, and vault curation.

The decision is:

- Render HTML via **reader-mode extraction**: fetch the page, extract the
  article body, sanitize it into clean semantic HTML we own, and render that in
  a styled reading column. No iframe, no scripts, no external loads — safe and
  offline. A **"View original"** button opens the source URL in the system
  browser for fidelity.
- **Preserve math.** Scholarly HTML is math-heavy; the sanitizer allowlist keeps
  **MathML**, which the system WebView renders natively (no JS). Math is a
  first-class requirement, not an afterthought.
- **Reuse the existing anchor model.** Annotations/chat already support
  `ThreadAnchor::TextOffset { source_id, start_offset, end_offset,
  selected_text }` — a flow-document, character-offset selection. HTML is a flow
  document; its selections *are* `TextOffset` anchors over the extracted
  `source_text`. No new anchoring system, no new storage.
- A new `source_kind = "html"` stored beside PDFs; an HTML-only OA result (e.g.
  Europe PMC's HTML link with no PDF) now opens in-app instead of falling to
  "open in browser."

Plain English: a lot of good open-access reading is HTML-native. Today that
means "open in browser" and losing every i0i affordance. This makes HTML a
first-class reader document.

## Problem

1. **The reader is PDF-only.** `ReaderDocument` serves a `pdf_local_path` and
   renders through pdfium. An article that exists only as HTML (many OA
   journals, arXiv's HTML/ar5iv, biomedical full text, preprint and blog-style
   sources) cannot be read, annotated, or chatted over in i0i — it falls to the
   external browser, losing threads, highlights, and curation.
2. **Discovery already surfaces HTML we can't use.** RFC 0053's Europe PMC
   provider returns `fullTextUrl` entries with **HTML** links (often alongside
   or instead of a PDF). RFC 0051's viewability tiers can only mark those
   "browser_required." The content is right there; the reader can't hold it.
3. **The plumbing is closer than it looks.** `acquire_web_page(url)` already
   fetches a page and reports `content_type: text/html`; `DocumentSource`
   already carries a `source_kind` discriminator; and — critically — the
   annotation model already has a flow-document anchor (`TextOffset`). The gap
   is extraction + rendering + wiring, not a new subsystem.

## Goals

- Open an HTML source in the reader with clean, readable rendering.
- Select text to create threads / ask chat / highlight — persisted and repainted
  on reopen, exactly like PDF selections.
- Preserve math (MathML) and figures so scholarly articles are actually usable.
- Acquire HTML sources through the existing acquisition path, and let HTML-only
  OA results open in-app (a new "Viewable (HTML)" case).
- Keep everything local and offline: no script execution, no external network at
  read time.

## Non-Goals

- **No original-fidelity layout.** We render extracted, restyled content, not the
  publisher's exact page. "View original" covers the rare need for fidelity.
- **No in-app JS / live web browsing.** Sanitized static HTML only.
- **No sandboxed-iframe annotation** (considered and rejected — fragile anchors,
  external-load/CSP problems, fights local-first).
- **No LaTeX-source rendering engine** in v1. Math already delivered as MathML or
  images renders; raw LaTeX-in-text is left as-is (rare in rendered HTML).
- No re-crawling or multi-page article stitching.

## Design

### D1. Ingestion & storage

A new `source_kind = "html"`, stored in the same on-disk layout as PDFs
(`documents/<paper_id>/sources/<source_id>/`), with two artifacts:

- `source.html` — the sanitized, self-contained article HTML we render.
- the derived **`source_text`** — the plain-text offset space annotations anchor
  into (already a `ReaderDocument` field for PDFs).

Pipeline (Rust, in a new `html_ingestion` module, mirroring how PDF ingestion
produces text):

```text
fetched HTML
  -> parse (html5ever)
  -> extract article body (Readability-style)
  -> sanitize: tag/attr allowlist, drop scripts/handlers/tracking,
     KEEP MathML + basic tables/figures; rewrite img src -> inlined data: URI
  -> emit clean source.html
  -> derive source_text = in-order concatenation of the clean DOM's text nodes
```

**Dependencies** (committed per direction — validated during implementation, no
separate spike): `html5ever`/`scraper` for parsing, `dom_smoothie` (a Rust
Readability port) for extraction, `ammonia` for allowlist sanitization. Ammonia's
allowlist is extended to keep MathML elements/attributes and `data:` image URIs.

### D2. Offset space (correctness detail)

`source_text` and the rendered DOM **must agree on offsets**, or highlights land
in the wrong place. Rule: `source_text` is the concatenation of the clean DOM's
text-node values in document order, **with no normalization**. The frontend maps
a selection to offsets by walking the *same* rendered DOM's text nodes in order,
so backend-derived offsets and frontend-computed offsets are identical by
construction. `selected_text` on the anchor is a stored fallback for display and
sanity-checking; because `source.html` is immutable, offsets never drift.

The cost of "no normalization" is that block boundaries carry no separator, so a
selection spanning two paragraphs quotes them run together. Acceptable for v1
(selections rarely cross blocks). Inserting a defined block separator (e.g. one
`\n` per block, mirrored on both sides) is a clean later refinement, deferred to
keep the offset contract as simple — and as obviously correct — as possible.

### D3. Reader document & serving

- `ReaderDocument` gains a content discriminator (`content_kind: "pdf" | "html"`,
  additive/`#[serde(default)]`). For HTML it carries `source_text` (as today)
  and the source URL for "View original".
- A command serves the sanitized HTML bytes, alongside `get_reader_pdf_bytes`
  (e.g. `get_reader_html`). The `.part`-file / cache patterns from RFC 0051 are
  reused.

### D4. Rendering (frontend)

`ReaderView` branches on `content_kind`:

- **pdf** → today's pdfium canvas path, unchanged.
- **html** → the sanitized HTML rendered into a styled reading column via
  `{@html cleanHtml}`. This is safe **only** because the backend sanitized it;
  defense-in-depth re-sanitizes client-side, and the app's strict CSP already
  blocks external loads. MathML renders natively in the WebView. A **View
  original** button opens the source URL via the opener plugin.

Reading-column typography matches the app aesthetic; images (inlined) and tables
render inline; math renders as MathML.

### D5. Annotation & chat (reuses existing model)

No new anchor kind. An HTML selection becomes
`ThreadAnchor::TextOffset { source_id, start_offset, end_offset, selected_text }`
over `source_text` — the same variant text selections already use — so threads,
`ask_at_anchor`, pins, and highlight storage flow **unchanged**. Reader-side work
is only:

- **selection → offsets**: walk the rendered DOM text nodes (D2) to compute
  start/end offsets for the current selection.
- **offsets → highlight**: given stored offsets, resolve a DOM `Range` over the
  same walk and paint the highlight overlay (the reader already paints PDF-rect
  highlights; this adds the flow-text painter).

`PdfRect` stays PDF-only; HTML never uses it.

### D6. Acquisition & discovery tie-in

- Acquisition gains an HTML path: given an HTML full-text URL (or an arbitrary
  URL the user imports), run D1 and store an `html` source. This slots beneath
  the RFC 0051 service next to PDF acquisition.
- RFC 0051 viewability gains **Viewable (HTML)**: a candidate whose best
  obtainable location is HTML (no PDF) resolves to an in-app HTML open instead
  of "browser_required." The Discover chip reflects it.

### D7. Errors

- Extraction yields too little / fails → fall back to the sanitized full
  `<body>` (still readable and annotatable).
- Fetch fails → the existing missing-source panel + "Open Source".
- Non-article pages still render sanitized content (import-any-URL works).

## What stays the same

- `ThreadAnchor`, chat/threads/pins storage, vault curation, the PDF reader
  path, and the Obscura runtime.
- The acquisition service shape; HTML is an added source kind, not a rewrite.

## Validation

Backend tests:

```text
extraction: known article fixture -> expected block text + source_text
sanitize: <script>/on*-handlers/trackers stripped; MathML preserved;
          img src rewritten to data: URI (or dropped past size/count cap)
source_text: equals in-order clean-DOM text concatenation (offset contract)
storage: html source persists with source_kind="html" and serves back
acquisition: html full-text URL -> stored html source
viewability: html-only candidate classifies as Viewable (HTML)
fallback: unextractable page -> sanitized body still stored
```

Frontend checks:

```text
html content_kind renders in the reading column; MathML shows equations
selection -> TextOffset offsets round-trip against source_text
stored offsets repaint the correct highlight on reopen
ask/chat/thread on an HTML selection behaves like a PDF selection
"View original" opens the source URL
PDF reader path unchanged
```

Commands:

```bash
cargo fmt --check
cargo test
cargo check
pnpm check
git diff --check
```

Manual smoke:

```text
1. Open a Europe PMC HTML-only OA article from Discover; confirm it reads
   in-app with equations rendered.
2. Select a passage; start a thread and ask chat; reopen; highlight persists.
3. Import an arbitrary article URL; confirm clean render + annotate.
4. "View original" opens the browser.
```

## Implementation Order

1. Add RFC 0056.
2. `html_ingestion`: parse + extract + sanitize (MathML-aware) + image inlining
   + `source_text` derivation, with fixture tests. (Dependency wiring lands
   here; validated by the fixtures.)
3. Storage: `source_kind="html"` persistence + serve command.
4. `ReaderDocument.content_kind` + reader branch to a sanitized-HTML renderer.
5. Flow-text selection→offset and offset→highlight mapping; wire `TextOffset`
   anchors through the existing chat/thread/pin path.
6. Acquisition HTML path + RFC 0051 "Viewable (HTML)" tier + Discover chip.
7. "View original" + import-any-URL entry point.
8. Validation.

## Risks

- **Extraction quality varies.** Some pages extract poorly. Mitigation:
  sanitized-body fallback keeps them readable; "View original" is always there.
- **Math delivered as images or non-MathML.** MathML renders; image math renders
  (inlined); LaTeX-in-text does not (v1 non-goal) — flagged so it isn't a
  surprise. Given math is common here, the fixtures explicitly include a MathML
  article.
- **Offset contract drift.** The single normalization rule (D2) plus an
  immutable `source.html` keeps backend/derived and frontend/selection offsets
  identical; a round-trip test guards it.
- **Sanitizer over/under-stripping.** Too aggressive kills equations/figures;
  too loose risks unsafe content. Mitigation: explicit allowlist with MathML +
  figure/table tags, tested both ways; client-side re-sanitize as defense.
- **Image inlining bloat.** Data-URI inlining can balloon `source.html`.
  Mitigation: per-image size cap and per-document count cap; oversized images
  dropped (alt text kept).

## Future Work

- LaTeX-source math rendering (KaTeX-style) for sources that ship raw LaTeX.
- Better table/figure fidelity and figure captions as first-class blocks.
- Side-by-side original view instead of only an external-browser escape hatch.
- Re-extraction / "reader settings" (width, font) shared with the PDF reader.
- Full-text HTML extraction feeding the same search/index path as PDF text.
