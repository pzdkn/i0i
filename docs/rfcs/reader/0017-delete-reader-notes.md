# RFC 0017: Delete Reader Notes

Status: Implemented  
Date: 2026-05-28  
Product: i0i  
Target: Tauri v2 + Svelte, macOS first

## Summary

Add the smallest missing note lifecycle action: deleting a saved Reader note.

We already support creating notes anchored to selected Reader text. This RFC makes saved notes removable from the `Notes` Inspector tab.

## Goals

- Allow deleting one saved note.
- Remove the note from SQLite.
- Refresh the visible saved notes list.
- Keep notes ordered newest first.
- Keep UI simple and low ceremony.
- Add Rust store coverage for deletion.

## Non-Goals

- No note editing.
- No confirmation dialog.
- No undo.
- No persistent text highlight behavior.
- No changes to note anchoring.
- No Discover-paper notes.

## Proposed Interaction

```text
Reader -> Notes tab -> saved note -> `-` button
```

Clicking the `-` icon deletes that note immediately.

Suggested UI:

```text
[quote]                        -
user note body
date
```

The visual control should be a small `-` icon button, not a text button. It should still have an accessible label such as `remove note`.

## Architecture

```text
Svelte ReaderInspector
  -> library bridge deletePaperNote({ paperId, noteId })
  -> Tauri command delete_paper_note(note_id, paper_id)
  -> LibraryStore::delete_paper_note(note_id)
  -> SQLite DELETE FROM paper_notes
  -> return updated notes for the paper
  -> frontend refreshes Notes tab
```

## Files Likely Affected

Frontend:

- `src/lib/features/reader/ReaderInspector.svelte`
- `src/lib/features/reader/ReaderView.svelte`
- `src/lib/bridge/library.ts`
- `src/lib/domain/library.ts`

Rust/Tauri:

- `src-tauri/src/commands/library.rs`
- `src-tauri/src/storage/library_store.rs`

Tests:

- `src-tauri/src/storage/library_store.rs`

## Data Model

No schema change.

Existing table:

```text
paper_notes
```

Deletion uses `note.id`.

Important behavior:

- If a note id does not exist, the operation should be harmless.
- After deletion, the frontend should show the current notes for that paper.

## Rust API Shape

Store method:

```rust
delete_paper_note(note_id: &str) -> Result<()>
```

Command shape:

```rust
delete_paper_note(note_id: String, paper_id: String) -> Result<Vec<PaperNote>, String>
```

The store deletes by `note_id`.

The Tauri command also receives `paper_id` only so it can return the refreshed note list for the current Reader view. This keeps the frontend to one round trip:

```text
delete note -> receive current notes -> replace notes in UI
```

Without `paper_id`, the frontend would need a second call after deletion:

```text
delete note -> fetch notes again
```

## Frontend API Shape

Bridge function:

```ts
deletePaperNote(input: { paperId: string; noteId: string }): Promise<PaperNote[]>
```

Reader state:

```text
on delete:
  call deletePaperNote(...)
  replace notes with returned notes
```

## Risks

- The store should only need `note_id`; the command receives `paper_id` as UI orchestration data for refreshing the current Reader notes.
- If the UI allows repeated clicks, a second delete should not crash.
- We should avoid optimistic UI for now; let the backend response be the source of truth.

## Validation Plan

```bash
cargo test
pnpm check
pnpm build
```

## Teaching Notes

This is a good example of command design.

Mental model:

```text
Store methods mutate durable data.
Tauri commands shape that mutation for frontend use.
Bridge functions keep raw invoke calls out of components.
Svelte components stay focused on interaction and rendering.
```

What can go wrong:

- If components call `invoke` directly everywhere, the app becomes harder to browse.
- If the command returns only `Ok(())`, the frontend has to guess or make a second request.
- If deletion errors on missing notes, repeated user clicks can create noisy failures.

## Recommendation

Implement the harmless-delete version and return the refreshed notes list.
