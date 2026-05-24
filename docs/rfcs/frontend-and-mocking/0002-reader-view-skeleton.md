# RFC 0002: Reader View Skeleton

Status: Draft  
Date: 2026-05-24  
Product: i0i  
Target: Tauri v2 + Svelte, macOS first

## Summary

Add the first Reader view skeleton to i0i.

The Reader should reuse the app shell from RFC 0001, follow the existing Layout A design language, and use mock data only. The goal is to prove that i0i can move from a Vault/Home view into an active reading workspace without introducing PDF rendering, persistence, or AI features yet.

This RFC uses the product-wide architecture defined in [`docs/overview/architecture.md`](../overview/architecture.md).

## Context

RFC 0001 implemented the first product-shaped foundation:

- App shell
- Activity rail
- Vault explorer
- Vault/Home paper list
- Inspector
- Mock data
- One Rust status command

The next smallest useful increment is the Reader. Reading is central to i0i: papers should become active learning objects with text, annotations, questions, lineage, and metadata around them.

The existing design mockups already define a Reader direction in `design/ui/src/layout-a.jsx`:

- Reader header with paper metadata
- TEXT / PDF / SPLIT view toggle
- Paper-like central reading surface
- Margin notes column
- Right inspector with lineage, ask-this-paper, and metadata
- Bottom reading controls

## Goals

- Add a Reader view skeleton that visually matches the Layout A design direction.
- Reuse the existing app shell and theme.
- Keep all Reader content mock/static for this increment.
- Introduce Reader-specific frontend domain types.
- Add mock extracted text for one paper.
- Add mock margin notes/questions/highlights.
- Add a visual TEXT / PDF / SPLIT toggle, with only TEXT implemented.
- Add a right inspector with static lineage, metadata, and ask-this-paper blocks.
- Keep the implementation modular and easy to browse.

## Non-Goals

- No real PDF rendering.
- No `pdf.js` integration yet.
- No real text extraction.
- No annotation persistence.
- No database changes.
- No real Q&A.
- No LLM integration.
- No graph rendering.
- No reader routing system beyond the simplest view switch needed for this skeleton.
- No new Rust command unless implementation reveals a tiny, useful status command.

## First Increment

Build this:

- A Reader feature folder under `src/lib/features/reader`.
- Mock Reader domain and data under `src/lib/domain` and `src/lib/mock`.
- A Reader workspace that fits inside the existing `AppShell`.
- A Reader header showing:
  - Paper title
  - Authors
  - Venue/year/arXiv-like identifier
  - Tags
  - TEXT / PDF / SPLIT toggle
  - Basic action buttons: Notes, Annotate, Graph, Ask
- A central paper-like text page:
  - Title block
  - Abstract
  - Several paragraph blocks
  - Paragraph numbers
  - Highlighted paragraph states
- A margin column:
  - Mock note attached to a paragraph
  - Mock question attached to a paragraph
  - Mock highlight summary
- A right inspector:
  - Selected paper metadata
  - Static lineage teaser
  - Static ask-this-paper card
  - Citation key / DOI-like metadata
- A bottom reading status/control strip:
  - Page position
  - Text mode indicator
  - Hotkey hints

## User Flow

The minimum user flow should be:

```text
Vault/Home
  -> single-click selects a paper
  -> double-click opens "Attention Is All You Need"
  -> Reader view appears
  -> mock text is visible
  -> margin notes and inspector provide context
  -> user can return to Vault/Home
```

For this skeleton, the transition can be a simple local Svelte state switch. We do not need a full router or tab model yet.

Only `vaswani2017` / "Attention Is All You Need" needs full mocked Reader content in this increment. If another paper is opened, the Reader can show an honest placeholder state instead of pretending all papers have Reader data.

## Architecture Reference

Use [`docs/overview/architecture.md`](../overview/architecture.md) for the product-wide architecture, folder structure, layer responsibilities, ASCII diagram, data boundary, and planned feature fit.

For this RFC, the relevant slice is:

- `src/lib/app` for the persistent shell.
- `src/lib/design` for the i0i theme.
- `src/lib/domain/reader.ts` for Reader domain types.
- `src/lib/mock/reader.ts` for mock Reader document data.
- `src/lib/features/reader` for Reader-owned UI.
- `src/lib/features/vault` only for the minimal "open selected paper" entry point.

## Proposed Frontend Structure

```text
src/lib/
  domain/
    reader.ts

  mock/
    reader.ts

  features/
    reader/
      ReaderView.svelte
      ReaderHeader.svelte
      TextPage.svelte
      ReaderMargin.svelte
      ReaderInspector.svelte
      ReaderFooter.svelte
```

## Proposed Domain Types

```ts
export type ReaderMode = "TEXT" | "PDF" | "SPLIT";

export type ReaderParagraph = {
  id: string;
  kind: "heading" | "paragraph";
  text: string;
  highlight?: "soft" | "strong";
};

export type ReaderMark = {
  id: string;
  paragraphId: string;
  kind: "note" | "question" | "highlight";
  body: string;
  createdLabel: string;
};

export type ReaderDocument = {
  paperId: string;
  title: string;
  authors: string[];
  venue: string;
  year: number;
  identifier: string;
  citationKey: string;
  tags: string[];
  paragraphs: ReaderParagraph[];
  marks: ReaderMark[];
};
```

These are frontend mock/domain types only. They do not define final persistence.

## State Model

Use local Svelte state for the skeleton:

- `activeView`: `"vault"` or `"reader"`
- `selectedPaperId`
- `readerMode`: `"TEXT"` initially

Do not introduce app-wide stores yet. A store can come later when multiple unrelated components need to coordinate shared state.

## Data Boundary

For this increment:

- Frontend mock data owns the Reader document.
- Rust is not responsible for Reader content yet.
- Existing `get_vault_status` can remain the only Rust command.

This keeps the boundary simple:

```text
Svelte mock Reader data -> Reader components -> rendered reading workspace
```

Later, Reader content can move behind Rust services:

```text
Reader UI -> bridge/tauri.ts -> reader command -> pdf/text service -> storage/filesystem
```

## Design Notes

The Reader should preserve the fixed i0i design language:

- Lowercase `i0i` in app chrome.
- Amber-on-black shell.
- Paper-like reading surface for extracted text.
- IBM Plex Mono.
- Sharp rectangles.
- Hairline borders.
- Dense but readable pane layout.
- Keep the Vault explorer visible in Reader, like an IDE sidebar.
- No rounded card-heavy dashboard style.

The PDF and SPLIT modes should be visible as disabled future affordances in this skeleton.

Long-term view modes:

- `TEXT`: clean extracted text view.
- `PDF`: rendered original paper PDF.
- `SPLIT`: extracted text and PDF side by side.

The product mental model is that i0i is an IDE for knowledge curation: the shell remains stable while the center workspace changes.

## Teaching Notes

This increment should teach:

- How feature views share shell chrome but own their workspace.
- Why mock data is useful before persistence.
- How local Svelte state can drive a view switch.
- Why Reader domain types should not be coupled to a specific PDF renderer.
- Why PDF rendering and annotation persistence should be delayed until the UI shape is stable.

## Risks

- The Reader design is dense and can become a large component quickly.
- A fake PDF mode could imply functionality that does not exist yet.
- Adding a routing/tab system too early would overcomplicate this slice.
- Annotation types can become too PDF-specific if modeled around page coordinates too soon.

## Risk Mitigations

- Keep Reader components small and named by pane.
- Implement only TEXT mode while clearly keeping PDF/SPLIT as visual affordances.
- Use local state before stores.
- Model mock marks by paragraph ID, not PDF coordinates.
- Avoid backend changes unless needed for compilation or bridge teaching.

## Validation Plan

- `pnpm check`
- `pnpm build`
- `cargo check`
- `cargo fmt --check`
- `pnpm tauri dev`
- Manual visual check against Layout A Reader mockups.
- Manual flow check:
  - Vault/Home loads.
  - User can open Reader.
  - Reader text page renders.
  - User can return to Vault/Home.

## Open Questions

Resolved:

- Single-click selects a paper; double-click opens Reader.
- Only `vaswani2017` / "Attention Is All You Need" has full mocked Reader content in this increment.
- `TEXT` is active; `PDF` and `SPLIT` are visible but disabled.
- Keep the Vault explorer visible in Reader, matching the IDE-like Layout A direction.

## Recommendation

Implement the Reader skeleton next, using mock data and local state only.

This is the smallest useful follow-up because it proves the shell can host a second product feature while keeping backend complexity out of the path.
