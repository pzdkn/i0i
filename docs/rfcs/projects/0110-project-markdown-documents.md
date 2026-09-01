# RFC 0110: Project Markdown Documents

- Status: Implemented and verified
- Date: 2026-09-02
- Area: Projects / Documents
- Parent: RFC 0108
- Depends on: RFC 0109

## Summary

Add ordinary Markdown documents owned by a Project. Researchers can create,
open, edit, rename, mark Harness-writable, and delete documents from the
Project workspace. Surveys, meeting notes, experiment plans, and later
Harness-generated outputs all use this same document type.

This RFC provides authoring and persistence only. It does not generate content,
run the Harness, or treat document text as source evidence.

## Data Model

`ProjectDocument` contains:

- stable id and owning `project_id`;
- title;
- `markdown` format;
- Markdown content;
- `harness_writable`, defaulting to false;
- optional originating Research Run and Research State revision ids; and
- created and updated timestamps.

Deleting a Project deletes its documents. A document cannot move between
Projects in this RFC. Origin fields are read-only provenance reserved for the
later “Create from research” RFC.

Document summaries are included in the library snapshot for Project navigation;
full content is fetched only when a document is opened.

## Interaction

The selected Project expands to its Vault and Documents destinations. Documents
shows a list and a plain Markdown editor in the centre workspace. Creating a
document selects it immediately. Changes are persisted with an explicit Save
action; switching away with unsaved edits requires a local confirmation.

The editor exposes a **Harness may update this document** checkbox. It grants
only future Harness write eligibility and has no effect until Harness execution
is implemented.

## Epistemic Boundary

Project documents are authored working material, not source evidence. Their
content may later guide a Run as researcher context, but only canonical Paper
evidence can support a source-supported Research Entry.

## Acceptance Criteria

1. Markdown documents can be created, listed, loaded, updated, and deleted
   within one Project.
2. Document titles and content survive application restart.
3. Document ownership is enforced; unknown Projects and documents fail clearly.
4. Deleting a Project cascades its documents.
5. New documents are not Harness-writable unless the user opts in.
6. Full document content is not embedded in `LibrarySnapshot` summaries.
7. The Project Explorer shows Documents and its count; the Project workspace
   exposes a usable document list/editor without creating a special Survey
   entity.
8. Rust tests, frontend type checking, and the production frontend build pass.

## Approval

Approved on 2026-09-02 under the user's instruction to author, approve,
implement, verify, and commit each focused RFC from RFC 0108 without a separate
approval round.

## Verification

Implemented and verified on 2026-09-02.

- `cargo test --manifest-path src-tauri/Cargo.toml --no-default-features`
  passed: 489 tests, 5 ignored live/manual tests.
- `pnpm check` passed with no errors or warnings.
- `pnpm build` completed successfully.
