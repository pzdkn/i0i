# RFC 0023: Reader Document Model

Status: Draft
Date: 2026-05-30
Product: i0i
Target: Tauri v2 + Svelte, macOS first

## Summary

Define the canonical document model the Reader renders and annotates.

A `Paper` is metadata. A `DocumentSource` is a saved readable source attached to a paper, such as a remote or cached PDF. A `DocumentExtraction` is one extraction run against a source. An `ExtractedDocument` is i0i's normalized, structured representation after extraction.

The Reader must not render arbitrary extractor HTML directly. It should render i0i-owned structured data so annotations, PDF mode, text/structured mode, and future extractor swaps can agree on the same anchors.

## Core Decision

Yes, a paper needs to link to document records. Split source files from extraction results:

```text
Paper 1 -> many DocumentSources
DocumentSource 1 -> many DocumentExtractions
```

The source link should live on `document_sources.paper_id`, not as a single `paper.document_id`, because one paper may have multiple sources:

- remote PDF source
- cached local PDF
- future supplemental files
- future replacement PDFs

The extraction link should live on `document_extractions.source_id`, because one source may be extracted multiple times:

- Docling v1 extraction
- MinerU extraction
- Pdfium baseline extraction
- future improved extraction of the same PDF

Add optional active pointers on `papers`:

```text
papers.active_source_id
papers.active_extraction_id
```

Rules:

- On first successful PDF cache, set `active_source_id`.
- On first successful extraction, set `active_extraction_id`.
- Reader opens `active_extraction_id` by default when present.
- If active extraction is missing, Reader may use the latest ready extraction.
- Opening a note should open the extraction referenced by that note's `source_id`, even if it is not the current active extraction.

## Paper vs Document

```text
Paper
  id
  title
  authors
  venue
  year
  citations
  identifiers
  abstract metadata
  active_source_id
  active_extraction_id

DocumentSource
  id
  paper_id
  source_kind
  source_url
  local_path
  status
  error

DocumentExtraction
  id
  paper_id
  source_id
  extractor
  extractor_version
  status
  error

ExtractedDocument
  pages
  blocks
  spans
  assets
  source_text
```

The abstract can remain paper metadata. It is not a satisfying Reader document fallback.

## Internal Reader States

The Reader should be driven by a real internal document state. These state names are implementation concepts, not UI copy. The UI should present calm, task-oriented surfaces and actions based on the state.

```text
ready
  -> render PDF mode and/or structured mode

downloading
  -> progress/status surface

extracting
  -> progress/status surface

failed
  -> error plus retry/open-source actions

missing_document
  -> add/download/import actions
```

Do not use these as reader-body fallbacks:

- saved abstract as the main reading surface
- metadata-only empty state as if it were a readable document

The abstract may appear in the inspector or metadata panel.

## Structured Format

i0i should normalize extractor output into an internal `ExtractedDocument`.

Each extraction must include one canonical global `sourceText` string. Notes anchor to offsets in this string. Blocks and spans are render/recovery structures around the same text, not a replacement for the global text layer.

```ts
type ExtractedDocument = {
  paperId: string;
  sourceId: string;
  extractionId: string;
  annotationSourceId: string;
  extractor: "docling" | "mineru" | "marker" | "grobid" | "pdfium_basic";
  extractorVersion: string;
  sourceText: string;
  pages: ReaderPage[];
  blocks: ReaderBlock[];
  spans: ReaderSpan[];
  assets: ReaderAsset[];
};
```

Pages preserve rough layout:

```ts
type ReaderPage = {
  pageIndex: number;
  width: number;
  height: number;
};
```

Blocks are the renderable document flow:

```ts
type ReaderBlock = {
  id: string;
  pageIndex: number;
  blockIndex: number;
  readingOrder: number;
  kind: "title" | "authors" | "heading" | "paragraph" | "caption" | "figure" | "table" | "equation";
  text?: string;
  assetId?: string;
  sourceStart?: number;
  sourceEnd?: number;
  bbox?: [number, number, number, number];
};
```

Spans are for precise text anchoring:

```ts
type ReaderSpan = {
  id: string;
  blockId: string;
  pageIndex: number;
  text: string;
  sourceStart: number;
  sourceEnd: number;
  bbox: [number, number, number, number];
};
```

The backend must persist and return spans explicitly. The frontend should not derive spans from block text, because derived spans lose extractor-provided geometry needed for highlights, PDF/text mapping, and precise annotation recovery.

Assets are renderable visual material:

```ts
type ReaderAsset = {
  id: string;
  paperId: string;
  sourceId: string;
  extractionId: string;
  kind: "page_image" | "embedded_image" | "figure" | "table" | "equation";
  pageIndex: number;
  bbox?: [number, number, number, number];
  localPath: string;
  caption?: string;
};
```

Assets should also participate in reading order. For every renderable asset block, include a placeholder text range in `sourceText`:

```text
[Figure fig-003: Attention head patterns across layers.]
[Table tbl-002: Ablation results.]
[Equation eq-004]
```

The asset is the visual truth. The placeholder text preserves reading order, searchability, and a stable offset region for comments near the asset.

Example:

```ts
{
  kind: "figure",
  assetId: "fig-003",
  text: "[Figure fig-003: Attention head patterns across layers.]",
  sourceStart: 4820,
  sourceEnd: 4878
}
```

The Reader should consume a frontend DTO shaped like:

```ts
type ReaderDocument = {
  paperId: string;
  sourceId: string;
  extractionId: string;
  annotationSourceId: string;
  title: string;
  authors: string[];
  venue: string;
  year: number;
  pdfLocalPath?: string;
  pdfSourceUrl?: string;
  sourceText: string;
  pages: ReaderPage[];
  blocks: ReaderBlock[];
  spans: ReaderSpan[];
  assets: ReaderAsset[];
};
```

## Storage Shape

Store files on disk:

```text
Application Support/i0i/documents/{paper_id}/source.pdf
Application Support/i0i/documents/{paper_id}/assets/{asset_id}.png
Application Support/i0i/documents/{paper_id}/raw/{extractor}.json
```

Store normalized structure in SQLite:

```sql
papers(..., active_source_id, active_extraction_id)
document_sources(id, paper_id, source_kind, source_url, local_path, status, error, created_at, updated_at)
document_extractions(id, paper_id, source_id, extractor, extractor_version, annotation_source_id, status, error, created_at, updated_at)
document_pages(id, paper_id, source_id, extraction_id, page_index, width, height)
document_blocks(id, paper_id, source_id, extraction_id, page_index, block_index, reading_order, kind, text, source_start, source_end, bbox_json)
document_spans(id, paper_id, source_id, extraction_id, block_id, page_index, text, source_start, source_end, bbox_json)
document_assets(id, paper_id, source_id, extraction_id, asset_kind, page_index, bbox_json, local_path, caption)
```

The raw extractor output may be kept for debugging, but the Reader must render the normalized model.

## Notes and Anchors

Notes should keep the current core model, but `source_id` should store the extraction's `annotationSourceId`, not the PDF `DocumentSource.id`.

```text
paper_id
source_id = DocumentExtraction.annotation_source_id
start_offset
end_offset
selected_text
body
```

The canonical text anchor is:

```text
annotationSourceId + start_offset + end_offset
```

Blocks, spans, and page rectangles exist to render highlights and recover from imperfect extraction changes. They do not replace `sourceText` offsets.

Why:

- offsets are produced by extraction, not by the PDF file itself
- the same PDF can be extracted multiple ways
- different extractors may produce different `sourceText`
- old notes should remain stable if a better extraction is added later
- full-document search, quote recovery, and note anchoring are simpler with one global text layer

Example:

```text
DocumentSource.id: pdf:paper123
DocumentExtraction.id: extraction:paper123:docling:v1
DocumentExtraction.annotation_source_id: reader-text:paper123:docling:v1
PaperNote.source_id: reader-text:paper123:docling:v1
```

Extend anchors over time:

```text
prefix
suffix
block_id
span_id
page_index
rects_json
asset_id
```

Anchor priority:

```text
1. source_id + start_offset/end_offset
2. selected_text + prefix/suffix within the same source
3. span_id or block_id
4. page rects
5. asset_id for figure/table/equation comments
```

Once notes exist for a `sourceId`, do not silently rewrite that source's text. If extraction changes enough to move offsets, create a new extraction version and handle migration explicitly later.

## Non-Goals

- Do not choose the extractor in this RFC.
- Do not implement PDF download in this RFC.
- Do not render extractor-native HTML as the canonical Reader surface.
- Do not treat abstracts as a real Reader-body fallback.
