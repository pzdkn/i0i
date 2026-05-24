# RFC 0007: SQLite Library Persistence

Status: Draft  
Date: 2026-05-24  
Product: i0i  
Target: Tauri v2 + Svelte, macOS first

## Summary

Move i0i's saved library data from frontend-only mock state into a local SQLite database owned by Rust.

This RFC persists the core library model:

```text
Vaults
Papers
Paper membership in one or more Vaults
```

Discover candidates remain transient/mock data for now. A candidate becomes persisted only when the user adds it to one or more Vaults.

## Context

RFC 0001-0004 established the app shell, Vault, Reader, Discover, and workspace tabs.

RFC 0005 made Discover candidates addable to Vault through frontend memory.

RFC 0006 made the add target explicit with an inline multi-Vault target autocomplete.

Those RFCs proved the product loop:

```text
Discover candidate
  -> choose Vault targets
  -> add to Vault(s)
  -> open from Vault in Reader
```

The next important step is persistence. Continuing to mock saved library data would make CRUD, annotations, and real import/export harder to reason about.

## Product Decision

Rust owns persisted library data.

Svelte owns presentation and short-lived interaction state.

SQLite is the local persistence layer for the first real library backend.

Discover candidates are not persisted until added. They are still proposals, not saved papers.

## Goals

- Add a local SQLite database for saved library data.
- Add Rust domain models for `Vault`, `Paper`, `PaperDraft`, and `LibrarySnapshot`.
- Add a Rust `LibraryStore` responsible for SQLite access.
- Seed default Vaults and Papers when the database is empty.
- Expose Tauri commands:
  - `get_library`
  - `add_paper_to_vaults`
- Update Svelte bridge code to call those commands.
- Hydrate Vault views and Discover target autocomplete from persisted data.
- Persist added papers across app reloads.
- Keep Discover feed itself mock/transient.

## Non-Goals

- No real Discover search backend.
- No notes or annotations persistence yet.
- No PDF storage.
- No import/export pipeline.
- No migrations framework beyond simple initial schema setup.
- No cloud sync.
- No user accounts.
- No deleting or editing papers yet.
- No create/rename/delete Vault UI yet.

## First Increment

Build this:

- SQLite database stored in the app data directory.
- Tables:
  - `vaults`
  - `papers`
  - `vault_papers`
- Rust `LibraryStore` initializes schema on startup.
- Rust `LibraryStore` seeds initial Vaults/Papers if the library is empty.
- Frontend calls `get_library` on startup.
- Vault list and paper lists render from the returned library snapshot.
- Discover target autocomplete uses persisted Vaults.
- Confirming Discover Add sends a `PaperDraft` and selected `vault_ids` to Rust.
- Rust upserts the paper and inserts `vault_papers` rows.
- Duplicate membership inserts are no-ops.
- Rust returns an updated `LibrarySnapshot`.
- Frontend refreshes its library state from that snapshot.

## Proposed Data Model

### `vaults`

```sql
create table if not exists vaults (
  id text primary key,
  title text not null,
  path text not null unique,
  created_at text not null,
  updated_at text not null
);
```

### `papers`

```sql
create table if not exists papers (
  id text primary key,
  title text not null,
  authors_json text not null,
  venue text not null,
  year integer not null,
  citations integer not null default 0,
  tags_json text not null,
  note_count integer not null default 0,
  annotation_count integer not null default 0,
  status text not null,
  abstract text,
  created_at text not null,
  updated_at text not null
);
```

### `vault_papers`

```sql
create table if not exists vault_papers (
  vault_id text not null,
  paper_id text not null,
  added_at text not null,
  primary key (vault_id, paper_id),
  foreign key (vault_id) references vaults(id) on delete cascade,
  foreign key (paper_id) references papers(id) on delete cascade
);
```

## Proposed Rust Structure

```text
src-tauri/src/
  commands/
    library.rs
    mod.rs

  domain/
    library.rs
    mod.rs

  storage/
    library_store.rs
    mod.rs
```

## Proposed Rust Domain Types

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Vault {
    pub id: String,
    pub title: String,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Paper {
    pub id: String,
    pub title: String,
    pub authors: Vec<String>,
    pub venue: String,
    pub year: i32,
    pub citations: i32,
    pub tags: Vec<String>,
    pub note_count: i32,
    pub annotation_count: i32,
    pub status: String,
    pub abstract_text: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaperDraft {
    pub id: String,
    pub title: String,
    pub authors: Vec<String>,
    pub venue: String,
    pub year: i32,
    pub citations: i32,
    pub tags: Vec<String>,
    pub status: String,
    pub abstract_text: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VaultPaper {
    pub vault_id: String,
    pub paper_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibrarySnapshot {
    pub vaults: Vec<Vault>,
    pub papers: Vec<Paper>,
    pub vault_papers: Vec<VaultPaper>,
}
```

## What `LibraryStore` Is

`LibraryStore` is the Rust-side persistence boundary for saved library data.

Mental model:

```text
Svelte UI
  -> typed bridge function
  -> Tauri command
  -> LibraryStore
  -> SQLite
```

The UI should not know SQL. Tauri command handlers should stay thin. `LibraryStore` owns database setup and library operations.

## Proposed `LibraryStore` Responsibilities

`LibraryStore::new`
: Finds the app data directory, opens `library.sqlite`, and creates a store instance.

`LibraryStore::init`
: Creates tables if needed and seeds default data if the library is empty.

`LibraryStore::get_library`
: Reads Vaults, Papers, and Vault/Paper memberships and returns a `LibrarySnapshot`.

`LibraryStore::add_paper_to_vaults`
: Upserts one paper, inserts membership rows for selected Vaults, prevents duplicate membership, and returns an updated `LibrarySnapshot`.

## Startup Sequence

Seeding happens in `LibraryStore::init`, not in `LibraryStore::new`.

The startup flow should be:

```text
LibraryStore::new
  -> resolve app data directory
  -> open library.sqlite
  -> return store

LibraryStore::init
  -> create schema if missing
  -> check whether vaults table is empty
  -> seed default Vaults/Papers/memberships only if empty
  -> return ready store
```

Reasoning:

- `new` constructs the store.
- `init` performs setup side effects.
- Seeding must happen after the schema exists.
- Seeding must be guarded so it does not overwrite local user data.

## Proposed Tauri Commands

```rust
#[tauri::command]
pub fn get_library(
    store: tauri::State<'_, LibraryStore>,
) -> Result<LibrarySnapshot, String>
```

```rust
#[tauri::command]
pub fn add_paper_to_vaults(
    store: tauri::State<'_, LibraryStore>,
    paper: PaperDraft,
    vault_ids: Vec<String>,
) -> Result<LibrarySnapshot, String>
```

The commands should mostly delegate to `LibraryStore`.

## Proposed Frontend Bridge

```text
src/lib/bridge/library.ts
```

```ts
export async function getLibrary(): Promise<LibrarySnapshot>;

export async function addPaperToVaults(
  paper: PaperDraft,
  vaultIds: string[],
): Promise<LibrarySnapshot>;
```

## Proposed Frontend State Direction

`library-state.svelte.ts` should stop being the source of truth for saved data.

Instead:

```text
SQLite/Rust = source of truth
library-state.svelte.ts = frontend cache of latest LibrarySnapshot
mock/discover.ts = transient Discover candidates
```

The state module should expose frontend-friendly selectors:

```ts
hydrateLibrary(snapshot: LibrarySnapshot): void;
getVaultWorkspaces(): VaultWorkspace[];
getVaultWorkspace(vaultId: string): VaultWorkspace;
getCandidateVaultTargets(candidateId: string): VaultWorkspace[];
```

And actions should call the bridge:

```ts
addDiscoverCandidateToVaults(candidateId: string, vaultIds: string[]): Promise<void>;
```

## Action Mapping

### Browse Vaults

```text
Frontend:
  getVaultWorkspaces()

Backend:
  select * from vaults
```

### Open Vault

```text
Frontend:
  getVaultWorkspace(vaultId)

Backend:
  papers joined through vault_papers
```

### Discover Add

```text
Frontend:
  candidate -> PaperDraft
  addPaperToVaults(paperDraft, vaultIds)

Backend:
  upsert papers
  insert vault_papers
  return LibrarySnapshot
```

### In-Vault Chips

```text
Frontend:
  getCandidateVaultTargets(candidateId)

Backend data:
  vault_papers joined to vaults
```

### Reader Open

```text
Frontend:
  getPaperById(paperId)

Backend data:
  papers
```

## Seeding Rule

On first launch, if `vaults` is empty:

- Insert the current mock/default Vaults.
- Insert the current mock/default Papers.
- Insert default `vault_papers` memberships.

After that, startup should not overwrite local data.

This gives us a working initial library without making mock arrays the long-term source of truth.

## Error Handling

For this increment:

- Rust commands can return `Result<T, String>`.
- The frontend can show existing bridge error text if loading fails.
- Failed add should not mutate frontend cache.

Later, introduce structured Rust error types and better UI feedback.

## Teaching Notes

This RFC introduces the real Tauri persistence bridge.

Mental model:

```text
Svelte component = user intent
bridge function = typed frontend boundary
Tauri command = command glue
LibraryStore = persistence API
SQLite = durable local state
```

What can go wrong:

- Putting SQL directly in command handlers makes commands hard to test and grow.
- Letting Svelte remain the source of truth creates reload bugs.
- Forgetting transactions can cause partial writes.
- Seeding every startup can overwrite user data.
- Storing arrays as JSON is acceptable for authors/tags now, but may need normalization later.

## Open Questions

- Should we use `rusqlite` or `sqlx`?
- Should schema setup be manual `create table if not exists` or migration-based from the start?
- Where exactly should the database live on macOS during development?
- Should paper IDs remain mock/source IDs for now, or should we introduce stable generated IDs later?

## Acceptance Criteria

- App creates/opens a local SQLite database.
- App initializes `vaults`, `papers`, and `vault_papers`.
- First launch seeds the initial library.
- Frontend Vault views render from `get_library`.
- Discover target autocomplete renders persisted Vaults.
- Discover Add calls Rust and persists the paper/Vault memberships.
- Reloading the app preserves added papers.
- Duplicate add to the same Vault does not duplicate rows.
- `pnpm check` passes.
- `pnpm build` passes.
- `cargo check` passes.
