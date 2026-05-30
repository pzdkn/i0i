# RFC 0025: Extractor Evaluation and Structured Rendering

Status: Draft
Date: 2026-05-30
Product: i0i
Target: Tauri v2 + Svelte, macOS first

## Summary

PDF reading is not just text extraction. Papers need reading order, rough layout, figures, captions, tables, equations, and stable annotation anchors.

This RFC defines the extractor evaluation spike and the target structured Reader behavior.

## Core Requirement

The Reader should render an i0i `ExtractedDocument`, not raw extractor HTML/Markdown/TEI.

```text
PDF
  -> extractor adapter
  -> ExtractedDocument
  -> persisted pages/blocks/spans/assets
  -> Reader structured mode
```

The structured mode should preserve enough layout to read papers naturally:

- page order
- section/paragraph order
- rough block geometry
- figure/caption adjacency
- tables near surrounding text
- equations as text or visual assets
- page-image fallback for complex regions

It does not need pixel-perfect PDF reconstruction.

## Candidate Extractors

Extraction does not need to be pure Rust.

Candidates:

- Docling: strong structured-document candidate with pages, text, tables, pictures, bounding boxes, and figure/table image export.
- MinerU: strong scientific-PDF candidate with layout-aware output, images, tables, formulas, and JSON/Markdown-style representations.
- Marker: promising Markdown/JSON/HTML extractor with figure/table/picture/caption block concepts.
- GROBID: mature scientific TEI parser for structure, references, figures, tables, and captions; heavier and service-like.
- Pdfium via `pdfium-render`: useful Rust-accessible baseline for page rendering, text extraction, image extraction, and geometry. It is mature PDF infrastructure, but not a full scientific-paper understanding tool.
- PDF.js: useful frontend PDF renderer/text layer, not the persisted extraction source of truth.

`pdfium-render` requires access to native Pdfium:

```text
macOS -> libpdfium.dylib
Windows -> pdfium.dll
Linux -> libpdfium.so
```

That does not make i0i macOS-only. It means packaging has per-platform native binaries. Since i0i is macOS-first, the first concern is bundling macOS Pdfium inside the `.app` if Pdfium is selected.

## Evaluation Spike

Before implementing production extraction, run a small benchmark.

Test corpus:

- recent arXiv ML paper with multiple figures
- two-column NeurIPS/CVPR-style paper
- paper with complex tables
- paper with equations/formulas
- paper with vector-heavy figures
- one ugly or older PDF if robustness matters

Scoring criteria:

- reading order quality
- rough layout preservation
- text block and span bounding boxes
- figure detection
- figure image export or page-region crop support
- caption linkage
- table extraction/rendering quality
- equation/formula handling
- output format quality and normalization cost
- local install and macOS packaging burden
- speed and memory use
- license and redistribution constraints

Spike output:

```text
extractor-report.md
sample outputs per tool
normalized ExtractedDocument per tool
Reader preview screenshots
recommendation for first production adapter
```

## Adapter Boundary

All extractors must normalize into the RFC 0023 model:

```text
ExtractorAdapter
  -> raw extractor output
  -> ExtractedDocument
```

Potential adapters:

```text
DoclingAdapter
MinerUAdapter
MarkerAdapter
GrobidAdapter
PdfiumBasicAdapter
```

The Reader must not depend directly on Docling JSON, Marker HTML, GROBID TEI, or Pdfium internals.

## Rendering Behavior

Structured mode renders:

```text
title
authors
headings
paragraphs
figures
captions
tables
equations
```

PDF mode renders the cached PDF, likely via PDF.js if inline PDF reading and text-layer selection are needed.

Both modes should map selections back to the same persisted `sourceId`/offset model whenever possible.

## Notes

Text comments anchor to spans/offsets from `ExtractedDocument`.

Figure/table/equation comments anchor to:

```text
asset_id
page_index
rects_json
caption/block context
```

PDF-mode note creation is harder than structured-mode note creation. The first implementation may support notes in structured mode first, then map PDF.js text-layer selections back to spans later.

## Validation Plan

- Run the extractor spike.
- Choose a first adapter.
- Extract a cached PDF into `ExtractedDocument`.
- Render structured mode from normalized blocks/assets.
- Verify figures and captions appear in the reading flow.
- Create a text note and verify it reopens at the same text.
- Create a figure/table note if included in the slice.
- Open PDF mode and verify the cached PDF displays.
