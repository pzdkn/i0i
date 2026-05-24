# RFC 0014: Store CRUD Tests

Status: Implemented  
Date: 2026-05-24  
Product: i0i  
Target: Tauri v2 + Svelte, macOS first

## Summary

Add Rust tests for the persisted library CRUD rules in `LibraryStore`.

The app now has real SQLite-backed behavior for:

- creating Vaults
- renaming Vaults
- removing Vaults
- adding papers to Vaults
- removing papers from one Vault
- removing papers from the library

These rules should be protected by tests before we add more features on top.

## Context

`cargo test` currently passes, but it runs zero meaningful tests.

The most important product rules now live in the Rust Store, not in Svelte:

```text
Svelte UI -> Tauri command -> LibraryStore -> SQLite -> LibrarySnapshot
```

That means we can test the core behavior without starting the Tauri desktop app.

## Goals

- Add focused Rust tests for `LibraryStore`.
- Test SQLite-backed behavior directly.
- Avoid launching Tauri in tests.
- Avoid testing Svelte UI behavior in this RFC.
- Keep tests readable for a Rust beginner.
- Lock in the no-orphan-paper lifecycle rule.
- Make `cargo test` meaningful.

## Non-Goals

- No Playwright tests.
- No UI interaction tests.
- No Tauri window/app integration tests.
- No schema migrations.
- No production data reset behavior.
- No refactor of the frontend cache.
- No new user-facing features.

## First Increment

Build a small test harness for `LibraryStore` that creates a temporary SQLite database and initializes the schema.

Then add tests for the current CRUD rules.

## Proposed Test Setup

Today `LibraryStore::new(app)` depends on a Tauri `AppHandle` because production needs the platform app-data directory.

For tests, add a test-only constructor:

```rust
#[cfg(test)]
fn for_test(db_path: PathBuf) -> Self
```

This keeps production construction unchanged while allowing tests to point the Store at a temporary database path.

Test database path:

```text
std::env::temp_dir()/i0i-test-<unique-id>/library.sqlite
```

We can avoid adding a dependency at first by using a unique directory name based on process id and timestamp.

## Proposed Tests

### Seed

```text
init seeds the default library when the database is empty
```

Assert:

- default Vaults exist
- default Papers exist
- default memberships exist

### Create Vault

```text
create_vault normalizes a path and persists it
```

Assert:

- `/new/topic` appears in `snapshot.vaults`
- title is `topic`
- generated id is stable and slug-like

### Duplicate Vault

```text
create_vault rejects duplicate paths
```

Assert:

- second create returns an error
- existing Vault count does not increase

### Rename Vault

```text
rename_vault changes path and title but keeps id stable
```

Assert:

- renamed Vault has the same id
- path changes
- title changes
- memberships still point to the same Vault id

### Add Paper To Multiple Vaults

```text
add_paper_to_vaults stores one paper and multiple memberships
```

Assert:

- paper exists once
- both Vault memberships exist
- duplicate add does not duplicate memberships

### Remove Paper From One Vault

```text
remove_paper_from_vault keeps paper if another Vault still references it
```

Assert:

- selected membership disappears
- other membership remains
- paper remains

### Remove Paper From Final Vault

```text
remove_paper_from_vault deletes paper when final membership is removed
```

Assert:

- final membership disappears
- paper row disappears

### Remove Paper From Library

```text
delete_paper_globally removes paper and all memberships
```

Assert:

- paper disappears
- all `vault_papers` rows for that paper disappear

### Remove Vault With Shared Paper

```text
delete_vault preserves papers that still belong to another Vault
```

Assert:

- removed Vault disappears
- memberships for removed Vault disappear
- shared paper remains

### Remove Vault With Unique Paper

```text
delete_vault deletes papers that lose their final Vault membership
```

Assert:

- removed Vault disappears
- unique paper disappears
- unrelated papers remain

## Proposed File Location

Keep tests near the Store:

```text
src-tauri/src/storage/library_store.rs
```

Use an internal `#[cfg(test)] mod tests` at the bottom of the file for now.

Reason:

- tests can access private helpers if needed
- easy for a beginner to browse
- no need to expose production-only APIs just for tests

If the test module grows too large later, move it to:

```text
src-tauri/src/storage/library_store_tests.rs
```

## Teaching Notes

This RFC introduces store-level testing.

Mental model:

```text
UI tests ask: did the user flow work?
Store tests ask: did the data rule hold?
```

For this app, the Store is where the critical data rules live. That makes it the best first testing layer.

What can go wrong:

- Tests that use the real app data directory can corrupt local development data.
- Tests that require launching Tauri become slow and brittle.
- Tests that inspect frontend state miss SQLite lifecycle bugs.
- Tests that share one database can affect each other.

The safest pattern is:

```text
one test
  -> one temporary SQLite file
  -> initialize Store
  -> perform operation
  -> assert returned LibrarySnapshot
```

## Open Questions

- Should we add a `tempfile` dev-dependency, or use `std::env::temp_dir()` manually?
- Should the Store test constructor be `pub(crate)` or private under `#[cfg(test)]`?
- Should we split the Store file once tests are added, or wait until it feels too long?

## Acceptance Criteria

- `cargo test` runs meaningful Store tests.
- Tests do not touch the real app data SQLite database.
- Tests do not require launching Tauri.
- Tests cover create, rename, Vault deletion, paper removal, and global paper removal.
- Tests cover shared-paper preservation.
- Tests cover orphan-paper cleanup.
- Tests are readable and focused.
- `cargo fmt --check` passes.
- `cargo check` passes.
- `cargo test` passes.
