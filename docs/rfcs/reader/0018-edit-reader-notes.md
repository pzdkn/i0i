# RFC 0018: Edit Reader Notes

Status: Implemented  
Date: 2026-05-28  
Product: i0i  
Target: Tauri v2 + Svelte, macOS first

## Summary

Add editing for saved Reader notes.

Reader notes currently support create, list, and delete. Editing completes the basic note lifecycle without changing note anchors, paper text offsets, or the Reader layout.

## Goals

- Allow editing the body of one saved note.
- Persist the updated note body in SQLite.
- Update `paper_notes.updated_at`.
- Return the refreshed notes list for the current paper.
- Keep notes ordered newest first.
- Keep the UI compact inside the Notes Inspector tab.
- Add Rust store coverage for note updates.

## Non-Goals

- No rich text editor.
- No edit history.
- No undo.
- No confirmation dialog.
- No note title.
- No anchor or selected quote editing.
- No persistent text highlight changes.
- No shared icon component yet.

## Proposed Interaction

Saved note:

```text
[selected quote]                 pencil  -
note body
date
```

Clicking the pencil enters edit mode for that note.

Edit mode:

```text
[selected quote]
textarea with current note body
Save
```

Keyboard behavior should match note creation:

- `Enter` saves.
- `Shift + Enter` inserts a line break.
- `Esc` cancels editing.

Only one note should be edited at a time. Starting edit on another note replaces the active edit draft.

## Icon Decision

Use a small pencil edit control for now.

The visual label can be the pencil character:

```text
✎
```

The button must still have an accessible label:

```text
aria-label="edit note"
```

This keeps the saved note actions visually compact:

```text
✎  -
```

Later, if we introduce a shared icon-button component or lucide icons, this can become a real pencil icon without changing the note editing model.

## Architecture

```text
Svelte ReaderInspector
  -> local edit draft state
  -> library bridge updatePaperNote({ paperId, noteId, body })
  -> Tauri command update_paper_note(paper_id, note_id, body)
  -> LibraryStore::update_paper_note(note_id, body)
  -> SQLite UPDATE paper_notes
  -> return updated notes for the paper
  -> frontend replaces Notes tab state
```

## Files Likely Affected

Frontend:

- `src/lib/features/reader/ReaderInspector.svelte`
- `src/lib/features/reader/ReaderView.svelte`
- `src/lib/bridge/library.ts`

Rust/Tauri:

- `src-tauri/src/commands/library.rs`
- `src-tauri/src/storage/library_store.rs`
- `src-tauri/src/lib.rs`

Tests:

- `src-tauri/src/storage/library_store.rs`

## Data Model

No schema change.

Existing table:

```text
paper_notes
```

Update fields:

```text
body
updated_at
```

Do not update:

```text
paper_id
source_id
start_offset
end_offset
selected_text
created_at
```

## Rust API Shape

Store method:

```rust
update_paper_note(note_id: &str, body: &str) -> Result<()>
```

Command shape:

```rust
update_paper_note(
    paper_id: String,
    note_id: String,
    body: String,
) -> Result<Vec<PaperNote>, String>
```

The store updates by `note_id`.

The command also receives `paper_id` only so it can return the refreshed note list for the current Reader view, matching RFC 0017's delete command shape.

## Frontend API Shape

Bridge function:

```ts
updatePaperNote(input: {
  paperId: string;
  noteId: string;
  body: string;
}): Promise<PaperNote[]>
```

Reader state:

```text
on save edit:
  call updatePaperNote(...)
  replace notes with returned notes
  leave edit mode
```

Inspector local state:

```text
editingNoteId
editBody
isUpdating
```

## Validation Rules

- Empty note bodies are rejected.
- Whitespace-only note bodies are rejected.
- Saved body is trimmed.
- Missing note id should return a clear error.

Unlike deletion, update should not be harmless for missing ids. If the user is editing a note that no longer exists, something is stale and the UI should know.

## Risks

- Inline edit state can get confusing if the user switches tabs mid-edit.
- The pencil character may render slightly differently across systems.
- Updating `updated_at` will move the edited note to the top because notes are ordered newest first.
- If we later add rich notes, this simple body-only update may need to evolve.

## Validation Plan

```bash
cargo fmt --check
cargo test
pnpm check
pnpm build
```

## Teaching Notes

This RFC completes basic note CRUD:

```text
create -> read -> update -> delete
```

Mental model:

```text
Store update = durable mutation
Tauri command = frontend-shaped mutation plus refreshed read
Bridge function = typed JavaScript wrapper
Inspector edit state = temporary UI draft
```

What can go wrong:

- If the component mutates the note object directly before saving, the UI can show unsaved data as if it were persisted.
- If the command only returns `Ok(())`, the frontend needs a second fetch.
- If missing notes are silently ignored, real stale-state bugs become harder to notice.

## Recommendation

Implement body-only inline editing with the pencil control, one active edit draft, and refreshed notes returned from the backend command.
