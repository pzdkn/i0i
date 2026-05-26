# RFC 0015: Selection-Anchored Reader Notes

Status: Implemented  
Date: 2026-05-24  
Product: i0i  
Target: Tauri v2 + Svelte, macOS first

## Summary

Add the first persisted Reader note workflow.

The user should be able to:

- select text in the Reader
- click a small comment icon near the selection
- create a note in the Inspector
- save that note to SQLite
- reopen the paper and see the saved note

For this RFC, the Reader margin area is deactivated as an interaction surface. Notes are anchored to selected text, but edited in the Inspector.

## Context

i0i now has a persisted library backend for Vaults and Papers. The Reader still uses mock text and mock marks.

The next step is to make the Reader useful without overbuilding the annotation system.

The important product idea is:

```text
notes should attach to evidence in the source text
```

That means a note should not only belong to a paper. It should belong to a selected quote inside a paper.

## Product Decision

Use this interaction:

```text
select text in Reader
  -> floating comment icon appears near selection
  -> click icon
  -> Inspector opens a frontend note draft
  -> note is anchored to selected text
  -> user writes note
  -> Save creates the persisted note
```

Remove the margin area from the Reader layout for now:

```text
Reader margin = not rendered in this RFC
```

The margin may come back later as a discovery/navigation layer for note markers, but it should not be part of this first persisted note workflow.

## Goals

- Add selection-based note creation in the Reader.
- Show a small comment/dialogue icon near a valid text selection.
- Open/focus the note editor in the Inspector after the icon is clicked.
- Persist notes in SQLite.
- Store notes by `paper_id`, `source_id`, full-text offsets, selected text, and note body.
- Load saved notes when opening a paper.
- Keep editing in the Inspector.
- Do not render the margin area in this RFC.
- Add Store tests for note creation/loading.

## Non-Goals

- No margin note markers.
- No Reader margin column.
- No inline note editor.
- No rich text editor.
- No markdown rendering.
- No autosave.
- No note threading.
- No editing saved notes.
- No deleting saved notes.
- No persistent Reader highlights for saved notes.
- No temporary Reader highlight after creating a note draft.
- No Cancel button for note drafts.
- No PDF coordinate anchoring.
- No full annotation/highlight model.
- No Reader search.
- No notes on Discover-only papers in this RFC.
- No paper lifecycle change for "noted but not vaulted" papers.
- No separate `paper_text_sources` table yet.

## First Increment

Build this:

- User selects text inside the Reader text.
- A small comment icon appears near the selection.
- Clicking the icon creates an unsaved frontend draft note in the Inspector.
- Inspector shows:
  - selected quote
  - note textarea
  - Save button
  - existing saved notes for the paper
- Save writes the note to SQLite.
- Saving increments the paper's `note_count`.
- Reopening the same paper reloads saved notes.

Notes are enabled only for persisted library papers in this RFC. If a Discover-only candidate is opened in Reader, the Notes UI should explain that notes require adding the paper to a Vault first.

No SQLite row is created until the user presses Save.

After Save succeeds, clear the frontend draft. The saved note should then appear in the saved notes list.

If the user creates another note draft before saving the current draft, replace the current draft immediately. The unsaved draft is frontend-only and is allowed to be discarded by this action.

There is no Cancel button in this RFC.

Multiple notes may be saved for the same selected quote. Each saved note is a separate entity with its own `id`.

Saved notes are read-only in this RFC. Editing and deleting notes should be handled in a later RFC.

Saving a note does not create a persistent visual highlight in the Reader text. Saved notes appear in the Inspector list only.

Saved notes are display-only in this RFC. Clicking a saved note does not jump, focus, or highlight the Reader text yet.

After the user clicks the comment icon, clear the browser text selection. The selected quote shown in the Inspector is the confirmation of what will be saved.

## Interaction Details

Valid selection:

```text
selection maps to a range inside the paper text source
```

Invalid selection:

```text
selection is empty
selection is outside the Reader text page
```

For invalid selections, do not show the comment icon.

When a valid selection exists:

```text
Reader captures:
  paper_id
  source_id
  start_offset
  end_offset
  selected_text
```

`source_id` identifies the paper text source that offsets refer to.

For RFC 15, this source is just the Reader text we currently have for the paper. It is not HTML, DOM structure, PDF coordinates, or a paragraph id.

Use a source id such as:

```text
reader-text-v1:<paper_id>
```

Later, when real extraction exists, `source_id` should identify the text version for that paper.

Do not add a separate `paper_text_sources` table in this RFC. `source_id` is plain note metadata for now.

Offsets are measured from the first character of the paper text source:

```text
first character = offset 0
```

For now, the source text is simply the Reader text string available for the paper.

## Proposed UI

Reader main area:

```text
paper text
  user selects phrase
  small comment icon appears near the end of the selected phrase
```

Position the icon from the browser selection range rectangle:

```text
selection range -> getBoundingClientRect()
icon position = near rect.right / rect.top
```

Clamp the icon inside the Reader viewport if needed. If the selection rectangle is unavailable or invalid, hide the icon.

The floating affordance should be icon-only:

```text
comment / dialogue icon
no visible text label
```

Use `title="Add note"` and `aria-label="Add note"` so the icon remains accessible.

Inspector:

```text
Notes

Select text in the Reader to add a note.
```

After clicking the comment icon:

```text
Notes

Selected quote
"A paragraph-level model is useful..."

[textarea]

[Save]
```

After save:

```text
Saved notes
  quote snippet
  note body
  updated timestamp
```

The note editor lives in the Inspector because it needs enough space for multiline writing, save state, and future metadata. The selected quote remains anchored to the Reader text.

Do not show an empty note editor before a selected quote has been captured. Before that, show an empty state such as `Select text in the Reader to add a note.`

## Inspector Crowding Risk

The Inspector can become crowded if it tries to show every Reader tool at once:

```text
Lineage
Ask
Notes
Metadata
```

This RFC does not require a full Inspector redesign, but it should keep the crowding risk visible.

One possible option later is to make the Inspector mode-based:

```text
Inspector
[Notes] [Graph] [Ask] [Meta]
```

In that model:

- selecting text and clicking the comment icon could switch the Inspector to `Notes`
- the Notes tab would own the selected quote, note editor, and saved notes list
- other tools would remain available without all competing for vertical space

This is not a definite requirement for the first implementation. It is a design option if the first Notes section makes the Inspector feel too dense.

## Proposed Data Model

Add a table:

```sql
create table if not exists paper_notes (
  id text primary key,
  paper_id text not null,
  source_id text not null,
  start_offset integer not null,
  end_offset integer not null,
  selected_text text not null,
  body text not null,
  created_at text not null,
  updated_at text not null,
  foreign key (paper_id) references papers(id) on delete cascade
);
```

`paper_id` connects the note to the Paper.

`source_id` connects the note to the paper text source.

`start_offset` and `end_offset` locate the selected quote inside that text.

Offsets are source-text-relative, not DOM-relative. `start_offset` is inclusive and `end_offset` is exclusive.

`selected_text` stores the quote the user selected.

`body` stores the user's note.

`id` identifies the individual note. It is not derived from `paper_id`, offsets, or `selected_text`, because the same quote may have multiple notes.

Rust generates note IDs when saving. Frontend drafts do not have persisted IDs.

`selected_text` is stored even though offsets exist. It is the quote snapshot shown in the UI and a future drift check if the paper text changes.

When displaying a saved note, show `selected_text`. Do not recompute the display quote from offsets in this RFC.

## Proposed Rust Domain Types

```rust
pub struct PaperNote {
    pub id: String,
    pub paper_id: String,
    pub source_id: String,
    pub start_offset: i64,
    pub end_offset: i64,
    pub selected_text: String,
    pub body: String,
    pub created_at: String,
    pub updated_at: String,
}
```

```rust
pub struct PaperNoteDraft {
    pub paper_id: String,
    pub source_id: String,
    pub start_offset: i64,
    pub end_offset: i64,
    pub selected_text: String,
    pub body: String,
}
```

## Proposed Store Methods

```rust
pub fn get_paper_notes(&self, paper_id: &str) -> Result<Vec<PaperNote>, String>
```

Responsibilities:

- Validate `paper_id`.
- Read notes ordered by `updated_at desc`.

The Inspector shows newest notes first, so the note just saved appears at the top.

```rust
pub fn create_paper_note(&self, draft: &PaperNoteDraft) -> Result<Vec<PaperNote>, String>
```

Responsibilities:

- Validate non-empty `paper_id`, `source_id`, `selected_text`, and `body`.
- Validate `start_offset >= 0` and `end_offset > start_offset`.
- Generate the note ID in Rust.
- Insert a note.
- Increment the paper's `note_count`.
- Return notes for that paper.

Returning all notes keeps the frontend simple: after Save, replace local note state with the backend result instead of appending and sorting manually.

Draft note bodies may be empty in frontend state. Saved note bodies must be non-empty after trimming whitespace.

Use the existing SQLite foreign key to reject notes for missing papers. Do not add a separate paper-existence preflight in this RFC.

## Proposed Tauri Commands

```rust
#[tauri::command]
pub fn get_paper_notes(
    store: tauri::State<'_, LibraryStore>,
    paper_id: String,
) -> Result<Vec<PaperNote>, String>
```

```rust
#[tauri::command]
pub fn create_paper_note(
    store: tauri::State<'_, LibraryStore>,
    draft: PaperNoteDraft,
) -> Result<Vec<PaperNote>, String>
```

## Proposed Frontend Bridge

```ts
export async function getPaperNotes(paperId: string): Promise<PaperNote[]>;
export async function createPaperNote(draft: PaperNoteDraft): Promise<PaperNote[]>;
```

## Proposed Frontend Changes

`ReaderView.svelte`

- Stop rendering `ReaderMargin` for this RFC.
- Hold selected quote state.
- Load notes for the current paper directly from Tauri on paper change.
- Pass notes and selected quote state to `ReaderInspector`.
- Do not add a global frontend notes cache yet.

`TextPage.svelte`

- Add Reader text selection handling.
- Emit selected quote details when selection is valid.
- Show a floating comment icon near the selection.
- Emit `onCreateNoteFromSelection` when icon is clicked.
- Clear the browser selection after the icon is clicked.

`ReaderInspector.svelte`

- Add a Notes section.
- Show selected quote when a note draft is active.
- Show textarea.
- Save calls `createPaperNote`.
- Do not create a database row before Save.
- Disable Save until the note body is non-empty after trimming whitespace.
- Show saved notes list.

## Teaching Notes

This RFC introduces anchored data.

Mental model:

```text
Paper = source entity
Extracted text source = offset coordinate system
Selected text = quote/evidence
Note = user-authored thought attached to that evidence
```

The Inspector is not where the note "belongs." The note belongs to the selected Reader text. The Inspector is only where the note is edited.

For now, notes can be local Reader state loaded directly from Tauri. Later, i0i may need a shared notes/query layer when notes become part of AI-agent context, search, note counts, graph edges, or cross-view previews.

What can go wrong:

- Selection APIs can be fiddly across nested DOM nodes.
- Offsets are only meaningful for the extracted text source they were created from.
- Text offsets can become stale if extracted text changes.
- Autosave can hide persistence errors.
- Margin markers can add layout complexity before the data model is stable.

## Open Questions

- Future RFC: should i0i support notes on Discover papers by changing paper lifecycle from `exists iff vaulted` to `exists iff vaulted or has user artifacts`?
- Future RFC: when should saved note editing, deletion, Reader highlights, or margin markers be added?

## Acceptance Criteria

- Reader margin is not rendered for this RFC.
- Selecting text in the Reader text shows a comment icon.
- Clicking the icon opens a note draft in the Inspector.
- The Inspector shows the selected quote.
- Saving persists the note in SQLite.
- Reopening the paper loads saved notes.
- Notes are deleted automatically when their Paper is deleted.
- Store tests cover note creation and loading.
- Store tests cover `note_count` incrementing after note creation.
- `pnpm check` passes.
- `pnpm build` passes.
- `cargo fmt --check` passes.
- `cargo check` passes.
- `cargo test` passes.
