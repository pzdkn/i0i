# RFC 0019: Reader Note Anchor Highlights

Status: Implemented  
Date: 2026-05-28  
Product: i0i  
Target: Tauri v2 + Svelte, macOS first

## Summary

Make saved Reader notes visibly connected to their source text.

Reader notes are now full CRUD objects, but their anchors are only visible inside the Inspector quote. This RFC uses the existing `sourceId`, `startOffset`, and `endOffset` fields to highlight saved note spans in the Reader text and let the user jump from a saved note back to its source location.

Implementation note: Reader highlights are passive `<mark>` elements, not buttons or editable controls. The paper text should remain source text with an annotation layer over it.

## Goals

- Highlight saved note spans in the Reader text.
- Clicking a saved note in the Inspector scrolls the Reader to its anchored quote.
- The clicked note becomes the active note.
- The active note highlight is visually stronger than inactive note highlights.
- Keep this frontend-only.
- Keep using the current mock Reader text and current offset model.
- Preserve note create/edit/delete behavior.

## Non-Goals

- No PDF coordinate model.
- No HTML document anchors.
- No margin markers.
- No overlapping-highlight sophistication.
- No editing note anchors.
- No backend schema changes.
- No note search.
- No multi-source paper text model.

## Product Decision

Use the existing text-offset anchor model for now:

```text
sourceId + startOffset + endOffset
```

For the current Reader, `sourceId` points to the mock extracted text source:

```text
reader-text-v1:<paper-id>
```

The offsets are interpreted against the plain Reader text we render today.

## Proposed Interaction

Saved notes:

```text
Reader text shows subtle highlights for all saved notes.
```

Click saved note:

```text
Inspector saved note -> click note card
  -> Reader scrolls to highlighted quote
  -> that note becomes active
  -> active highlight gets stronger styling
```

Create note:

```text
select text -> comment icon -> save note
  -> new saved note appears
  -> new note highlight appears in Reader text
```

Delete note:

```text
click -
  -> note removed
  -> highlight disappears
```

Edit note:

```text
click pencil
  -> edit body only
  -> anchor/highlight unchanged
```

## Architecture

```text
ReaderView
  owns notes and activeNoteId
  passes notes + activeNoteId to TextPage
  passes activeNoteId + onActivateNote to ReaderInspector

ReaderInspector
  saved note click calls onActivateNote(note.id)

TextPage
  renders Reader text segmented by note offsets
  marks note spans with data-note-id
  scrolls active note span into view
  does not own active note selection
```

No Rust or SQLite change is needed. The backend already stores the anchor fields.

## Files Likely Affected

Frontend:

- `src/lib/features/reader/ReaderView.svelte`
- `src/lib/features/reader/ReaderInspector.svelte`
- `src/lib/features/reader/TextPage.svelte`

Optional:

- `src/lib/domain/reader.ts` if a small render helper type is useful.

## Rendering Model

TextPage currently renders plain Reader sections. For this RFC, it should derive render segments from:

```text
plain text + notes for same sourceId
```

Conceptual segment shape:

```ts
type TextSegment = {
  text: string;
  noteId?: string;
  active?: boolean;
};
```

Then render:

```text
normal text
highlighted note mark
normal text
```

## Offset Rules

- Ignore notes whose `sourceId` does not match the rendered source.
- Ignore notes with invalid offsets.
- Clamp nothing silently if it would hide a real bug; invalid anchors should simply not render.
- If highlights overlap, use the simplest deterministic behavior for v1.

Suggested v1 overlap behavior:

```text
sort by startOffset asc, endOffset asc
render the first valid non-overlapping spans
skip later spans that overlap an already rendered span
```

This is acceptable because overlap handling is not the goal of this RFC.

## Active Note Behavior

`ReaderView` owns:

```ts
activeNoteId: string | null
```

When `activeNoteId` changes:

- `TextPage` scrolls the matching span into view.
- `TextPage` gives the matching span stronger active styling.
- `ReaderInspector` can add subtle active styling to the selected note card.

If the active note is deleted:

```text
activeNoteId = null
```

## Styling

Inactive note highlight:

```text
subtle amber background
thin underline or border-bottom
```

Active note highlight:

```text
stronger amber background
clearer outline/border-bottom
```

The highlight should feel like an annotation layer, not text selection.

It should not turn source text into an inline button or any editable-looking control.

## Risks

- Text segmentation can make the Reader component harder to read if done inline.
- Offsets are only meaningful for the current extracted text. Future PDF/import work will need a more durable source model.
- Overlapping notes are intentionally underpowered in v1.
- Auto-scrolling can feel jumpy if triggered too often.

## Validation Plan

```bash
pnpm check
pnpm build
```

Manual check:

```text
create note -> highlight appears
click saved note -> Reader scrolls to highlight
edit note -> highlight remains
delete note -> highlight disappears
```

## Teaching Notes

This RFC separates note content from note anchors:

```text
note body = what the user thinks
note anchor = where the thought points
Reader highlight = visual rendering of the anchor
```

Mental model:

```text
SQLite stores the anchor.
ReaderView owns active UI state.
TextPage renders anchors into visible spans.
Inspector controls which anchor is active.
```

What can go wrong:

- If we treat browser selection as the saved highlight, the highlight disappears as soon as selection changes.
- If we put active state into SQLite, we persist temporary UI focus by accident.
- If TextPage owns note state, Inspector and Reader can drift apart.

## Recommendation

Implement this as a frontend-only Reader feature using existing note anchors. Keep the segmentation helper small and local unless it becomes hard to read.
