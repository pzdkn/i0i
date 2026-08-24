# RFC 0025: Extractor Evaluation and Structured Rendering

Status: Stale
Date: 2026-05-30
Product: i0i
Target: Tauri v2 + Svelte, macOS first

## Summary

PDF reading is not just text extraction. Papers need reading order, rough layout, figures, captions, tables, equations, and stable annotation anchors.

This RFC defines the extractor evaluation spike and the extraction/enrichment layer that can be run on demand after a PDF is already readable.

The product decision is:

- PDF mode is the fast default Reader path.
- MinerU is optional on-demand enrichment, not a prerequisite for opening a paper.
- A full text/structured mode can be deferred until we know it earns its keep.

This RFC builds on RFC 0023 and RFC 0024:

```text
DocumentSource(status = cached)
  -> DocumentExtraction(status lifecycle)
  -> ExtractedDocument
  -> Reader structured mode
```

RFC 0025 should only extract from cached local PDFs. Downloading and PDF source caching belong to RFC 0024.

## Core Requirement

The Reader should preserve the cached PDF as the instant-access view, and enrich it with an i0i `ExtractedDocument` only when extraction is explicitly requested or otherwise run in the background.

```text
PDF
  -> cached DocumentSource
  -> extractor adapter
  -> DocumentExtraction
  -> ExtractedDocument
  -> persisted pages/blocks/spans/assets
  -> Reader structured mode
```

The structured/enrichment layer should preserve enough layout to make the paper useful after extraction:

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

Before implementing production extraction, run a small benchmark. This is the first implementation step for RFC 0025. Do not implement a production `PdfiumBasicAdapter` or any other production adapter until the scorecard selects the first adapter.

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

Suggested workspace:

```text
scripts/extractor_spike/
  corpus/
  outputs/
    docling/
    mineru/
    marker/
    grobid/
    pdfium_basic/
  normalized/
  previews/
  report.md
```

Use a fixed spike corpus, not PDFs from the user's app database. The corpus should be manifest-driven so the spike is reproducible without committing large PDFs:

```toml
[[paper]]
id = "attention-is-all-you-need"
url = "https://arxiv.org/pdf/1706.03762"
features = ["figures", "tables", "equations"]

[[paper]]
id = "two-column-vision-paper"
url = "..."
features = ["two_column", "figures", "captions"]
```

A fetch script can download PDFs into:

```text
scripts/extractor_spike/corpus/
```

Use a simple scorecard so the decision is not vibes-only:

```text
0 = unusable
1 = poor
2 = acceptable with manual cleanup
3 = good enough for first production adapter
4 = strong
5 = excellent
```

Minimum gates for the first production adapter:

- can run locally on macOS in development
- produces page-level geometry
- produces block-level reading order
- produces enough text spans or geometry to create highlights
- preserves or exports figures/page-region assets
- has acceptable license/redistribution constraints

## Adapter Boundary

All extractors must normalize into the RFC 0023 model:

```text
ExtractorAdapter
  -> cached DocumentSource
  -> raw extractor output
  -> DocumentExtraction
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

Raw extractor output may be stored for debugging under:

```text
documents/{paper_id}/extractions/{extraction_id}/raw/{extractor}.json
```

Reader assets should be stored under:

```text
documents/{paper_id}/extractions/{extraction_id}/assets/{asset_id}.png
```

The persisted normalized rows remain the Reader's source of truth.

## Extraction Lifecycle

These are internal `DocumentExtraction.status` values, not UI copy:

```text
queued
  -> extraction requested but not yet running

extracting
  -> extractor is running

ready
  -> normalized ExtractedDocument is persisted

failed
  -> extraction failed; error stored
```

On first successful extraction for a paper, set:

```text
papers.active_extraction_id = document_extractions.id
```

Do not overwrite an existing active extraction automatically unless the user explicitly chooses a different extraction later.

## Commands and Events

Commands:

```rust
extract_paper_document(paper_id, source_id?, extractor?) -> DocumentExtraction
get_reader_document(paper_id, extraction_id?) -> ReaderDocument
get_document_extractions(paper_id) -> Vec<DocumentExtraction>
```

If `source_id` is omitted, use `papers.active_source_id` or the first cached PDF source.

Extraction should follow the event style from RFC 0024:

```text
document_extraction_updated
```

Payload:

```ts
type DocumentExtractionUpdated = {
  paperId: string;
  sourceId: string;
  extractionId: string;
  annotationSourceId?: string;
  extractor: string;
  status: "queued" | "extracting" | "ready" | "failed";
  error?: string;
};
```

## Rendering Behavior

PDF mode renders the cached PDF, likely via PDF.js if inline PDF reading and text-layer selection are needed.

Optional structured mode renders:

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

PDF mode is the primary fast path. Structured mode is an enrichment view and should not block reading the PDF.

Structured mode should use the RFC 0023 asset placeholder rule: figures, tables, and equations are visual assets in the render tree, and they also occupy placeholder text ranges inside `sourceText`.

## Notes

Text comments anchor to spans/offsets from `ExtractedDocument` when extraction exists.

Figure/table/equation comments anchor to:

```text
asset_id
page_index
rects_json
caption/block context
```

PDF-mode note creation is harder than structured-mode note creation. The first implementation may support notes in structured mode first, then map PDF.js text-layer selections back to spans later.

If the user writes notes before extraction exists, those notes may remain PDF-anchored until a later MinerU pass can resolve them to text spans or assets.

## Safety and Limits

PDF extraction can be expensive and PDFs are untrusted input. The production extractor runner should enforce:

- maximum input PDF size from RFC 0024 config
- extraction timeout
- temporary working directory under app data
- cleanup of partial raw/assets output on failure
- no extractor network access unless a future adapter explicitly requires it

If an extractor process crashes or times out, mark the `DocumentExtraction` as `failed` and preserve the cached PDF source.

## Validation Plan

- Run the extractor spike.
- Choose a first adapter.
- Extract a cached PDF into `ExtractedDocument`.
- Persist `DocumentExtraction`, pages, blocks, spans, and assets.
- Verify `papers.active_extraction_id` is set on first successful extraction.
- Verify `document_extraction_updated` events fire for extraction status changes.
- Render optional structured mode from normalized blocks/assets.
- Verify figures and captions appear in the enrichment flow.
- Create a text note and verify it reopens at the same text when extraction exists.
- Create a figure/table note if included in the slice.
- Open PDF mode and verify the cached PDF displays.
