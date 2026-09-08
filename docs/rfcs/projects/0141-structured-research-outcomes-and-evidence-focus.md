# RFC 0141: Structured Research Outcomes and Evidence Focus

- Status: Implemented and verified
- Date: 2026-09-08
- Depends on: RFCs 0135 and 0138

## Problem

The Project Research inspector renders the latest Run's summary as one plain
paragraph. A result such as four separate research gaps becomes a dense
semicolon-separated sentence. Research Entry evidence similarly mixes identity,
quotation, and interpretation into one large button.

Activating evidence opens the correct paper and page, but does not focus the
exact passage. The researcher must visually find text that i0i already knows by
`sourceId`, `extractionId`, `chunkId`, offsets, pages, and excerpt.

## Outcome

The latest Run is a short, scannable research report. Each material result is a
separate list item. Evidence is easy to inspect, and opening a source visibly
focuses the exact cited passage in the Reader.

## Design

### Latest Run

Keep `LAST RUN · STATUS` and the timestamp. Render the outcome beneath it as:

1. A one-sentence overview, when useful.
2. A **What changed** list with one item per finding, gap, hypothesis, or question.
3. **Still open** and **Next** lists when populated.
4. A compact factual footer for papers read, papers retained, and State revision.

Do not obtain this structure by splitting prose on punctuation. Extend the
validated `ResearchRunOutcome` with structured display items while retaining
`summary` for history and old Runs. The agent's final response schema supplies
the list; the backend validates its bounds. Existing outcomes without items
continue to render their summary as restricted Markdown.

Render Markdown with i0i's existing safe Markdown renderer. Allow paragraphs,
emphasis, links, and lists; do not allow raw HTML.

### Evidence Rows

Each evidence row separates:

- paper title and page range;
- relationship (`supports`, `contradicts`, `context`, or `unspecified`);
- the quoted passage;
- the support note explaining why it bears on the State entry.

Show a bounded quotation initially and let the user expand it. The entire row is
not one oversized button: use one clear source-navigation action and ordinary
selectable text.

### Passage Focus

Opening evidence passes a typed navigation target rather than only `pageIndex`:

```text
EvidenceNavigationTarget
  paperId
  sourceId
  extractionId
  chunkId
  sourceStart
  sourceEnd
  pageStart
  pageEnd
  excerpt
  requestId
```

The Reader opens the saved paper, scrolls to `pageStart`, resolves the cited
chunk/offsets against the loaded Reader document, and paints the existing
temporary citation highlight over its PDF rectangles. The highlight is a focus
effect, not a persisted user annotation, and fades after several seconds.

For HTML, focus the matching text offsets. If geometry is unavailable, still
jump to the page and show the quoted evidence in the Reader inspector with an
honest “exact location unavailable” state. Never silently highlight a fuzzy
string match elsewhere in the document.

## Scope

Reuse the existing Research State evidence fields and Reader citation-flash
primitive. Do not add a new annotation type, duplicate the PDF text layer, or
redesign the entire Research workspace.

## Acceptance

- A Run with four result items renders four distinct list items, not one dense
  paragraph.
- Legacy plain summaries remain readable as restricted Markdown.
- Evidence rows clearly separate source, quotation, relationship, and support
  explanation; quotation text remains selectable.
- Activating PDF evidence opens the paper, reaches the cited page, and
  temporarily highlights the exact available rectangles.
- Activating HTML evidence focuses the exact available offsets.
- Missing geometry produces a page jump and an explicit fallback, not an
  unrelated fuzzy highlight.
- Focused tests cover structured and legacy outcomes, evidence layout data, PDF
  focus, HTML focus, repeated excerpts, and unavailable geometry.
- Native acceptance opens evidence from Project Research and observes the focus
  in the Reader.
