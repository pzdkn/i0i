# RFC 0010: Create Vault

Status: Draft  
Date: 2026-05-24  
Product: i0i  
Target: Tauri v2 + Svelte, macOS first

## Summary

Add the first real Vault CRUD operation: create a Vault.

The user should be able to create a new Vault from the Explorer. The Vault is persisted in SQLite through Rust and appears immediately in the Explorer and Discover target autocomplete.

## Context

RFC 0007 made Rust/SQLite the source of truth for saved library data.

RFC 0008 made the Vault Explorer render persisted Vaults and real paper counts.

RFC 0009 cleaned up the frontend boundary so saved library data no longer looks mock-owned.

Now that Vaults are real persisted entities, the user needs the first minimal way to shape their library structure.

## Product Decision

Creating a Vault is a backend-backed operation.

The frontend should collect the user's desired Vault path/title, call a Tauri command, then hydrate from the returned `LibrarySnapshot`.

No frontend-only Vault creation.

## Goals

- Add a create-Vault action to the Explorer.
- Let the user enter a Vault path/name.
- Persist the new Vault in SQLite.
- Return an updated `LibrarySnapshot`.
- Refresh the Explorer and Discover target autocomplete from the returned snapshot.
- Reject empty Vault names/paths.
- Reject duplicate Vault paths.
- Keep the interaction small and inline.

## Non-Goals

- No rename Vault.
- No delete Vault.
- No nested folder management.
- No drag-and-drop.
- No command palette.
- No large modal.
- No paper movement.
- No migrations beyond existing schema.
- No cloud sync.

## First Increment

Build this:

- Explorer `+` opens a compact inline create input.
- User enters a Vault path/name.
- Confirm calls Rust `create_vault`.
- Rust inserts the Vault into SQLite.
- Rust returns an updated `LibrarySnapshot`.
- Frontend hydrates the library cache from the snapshot.
- New Vault appears in Explorer with count `0`.
- New Vault appears in Discover target autocomplete.
- Duplicate path returns an error and does not mutate frontend cache.
- Empty input is rejected before calling Rust.

## Proposed User Flow

```text
User clicks + in Explorer
  -> inline input appears

User types "diffusion"
  -> preview path is /diffusion

User confirms
  -> Rust creates Vault
  -> Explorer shows /diffusion 0
  -> Discover target autocomplete can select /diffusion
```

Duplicate:

```text
User enters "/self-supervised"
  -> Rust rejects duplicate path
  -> bridge error is shown
  -> Explorer stays unchanged
```

## Proposed Data Rules

Input normalization:

- Trim whitespace.
- If input does not start with `/`, prefix `/`.
- Collapse repeated `/`.
- Remove trailing `/` unless the path is `/`.
- Derive `title` from the last path segment.

Examples:

```text
"diffusion"          -> path "/diffusion", title "diffusion"
"/vision/frontier"  -> path "/vision/frontier", title "frontier"
"  /notes/  "       -> path "/notes", title "notes"
```

Invalid:

```text
""      -> empty
"/"     -> no usable Vault name
"   "   -> empty
```

Vault ID:

- For this increment, derive a stable slug from the normalized path.
- Example: `/vision/frontier` -> `vision-frontier`.
- If an ID collision occurs, return an error for now.

## Proposed SQLite Operation

Existing table:

```sql
vaults (
  id text primary key,
  title text not null,
  path text not null unique,
  created_at text not null,
  updated_at text not null
)
```

Insert:

```sql
insert into vaults (id, title, path, created_at, updated_at)
values (?, ?, ?, datetime('now'), datetime('now'));
```

Duplicate path should return a user-readable error.

## Proposed Rust Domain Type

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultDraft {
    pub path: String,
}
```

The Rust store should normalize path/title/id so the frontend does not become the authority for persistence rules.

## Proposed Rust Store Method

```rust
pub fn create_vault(&self, draft: &VaultDraft) -> Result<LibrarySnapshot, String>
```

Responsibilities:

- Normalize and validate path.
- Derive title.
- Derive ID.
- Insert into `vaults`.
- Return updated `LibrarySnapshot`.

## Proposed Tauri Command

```rust
#[tauri::command]
pub fn create_vault(
    store: tauri::State<'_, LibraryStore>,
    draft: VaultDraft,
) -> Result<LibrarySnapshot, String>
```

## Proposed Frontend Bridge

```ts
export async function createVault(path: string): Promise<LibrarySnapshot>;
```

## Proposed Frontend Interaction

`VaultExplorer.svelte`:

- Keep existing header.
- `+` opens inline create row below the filter.
- Input accepts typing.
- `Enter` confirms.
- `Escape` cancels.
- Confirm button can be a compact `Create`.
- On success, close input.
- On error, let existing bridge error display handle it.

No modal.

## Data Boundary

```text
Explorer UI
  -> createVault(path)
  -> Tauri create_vault
  -> LibraryStore::create_vault
  -> SQLite
  -> LibrarySnapshot
  -> hydrateLibrary(snapshot)
```

## Teaching Notes

This RFC is the first real create operation for a persisted top-level domain object.

Mental model:

```text
Frontend collects intent.
Rust validates persistence rules.
SQLite stores the result.
Frontend re-renders from a fresh snapshot.
```

What can go wrong:

- Letting frontend generate final IDs can create inconsistent persistence rules.
- Creating Vaults only in frontend cache would disappear on reload.
- Not normalizing paths makes duplicates like `diffusion` and `/diffusion`.
- Not handling duplicate path errors creates confusing UI state.

## Acceptance Criteria

- User can open a create-Vault input from Explorer.
- Empty input is rejected.
- New Vault is persisted in SQLite.
- New Vault appears in Explorer with count `0`.
- New Vault appears in Discover target autocomplete.
- Duplicate path is rejected.
- Reloading the app preserves the new Vault.
- `pnpm check` passes.
- `pnpm build` passes.
- `cargo check` passes.
