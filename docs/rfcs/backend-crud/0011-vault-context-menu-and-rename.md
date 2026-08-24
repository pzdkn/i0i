# RFC 0011: Vault Context Menu And Rename

Status: Stale
Date: 2026-05-24  
Product: i0i  
Target: Tauri v2 + Svelte, macOS first

## Summary

Add a small Vault Explorer context menu and implement Vault rename.

Right-click should expose contextual Vault actions:

```text
Right-click Vault row
  -> Rename
  -> Create Vault

Right-click Vaults section / empty Explorer list
  -> Create Vault
```

This RFC adds the first update operation for Vaults. Delete remains out of scope.

## Context

RFC 0010 added persisted Vault creation through Rust/SQLite.

The visible `+` button next to `Vaults` is a good discoverable create action. However, desktop users also expect right-click context menus in explorer-like sidebars.

Rename is the natural next Vault CRUD slice after create:

```text
Create Vault = C
Rename Vault = U
```

Delete is intentionally deferred because it is destructive and requires decisions about paper membership cleanup.

## Goals

- Add a custom context menu to the Vault Explorer.
- Right-clicking a Vault row offers `Rename` and `Create Vault`.
- Right-clicking the Vaults section or empty Vault list offers `Create Vault`.
- Rename opens an inline row editor.
- Persist rename through Rust/SQLite.
- Normalize and validate the new Vault path in Rust.
- Reject empty paths.
- Reject duplicate paths.
- Refresh frontend state from the returned `LibrarySnapshot`.
- Update visible Explorer row and any open Vault tab title after rename.

## Non-Goals

- No delete Vault.
- No drag-and-drop.
- No nested folder editor.
- No native OS context menu.
- No global command palette.
- No multi-select Vault operations.
- No schema changes.
- No paper membership changes.

## First Increment

Build this:

- Custom context menu component or local Explorer menu state.
- `contextmenu` on Vault row:
  - prevent browser default menu
  - show menu at pointer location
  - menu items: `Rename`, `Create Vault`
- `contextmenu` on Vaults section/list background:
  - show menu item: `Create Vault`
- `Rename` turns that Vault row into an inline path input.
- `Enter` confirms rename.
- `Escape` cancels rename.
- Rust command `rename_vault`.
- Rust updates `vaults.path`, `vaults.title`, `updated_at`.
- Rust returns updated `LibrarySnapshot`.
- Frontend hydrates cache from snapshot.
- If the renamed Vault tab is open, its title updates to the new path.

## Proposed User Flow

Rename:

```text
User right-clicks /diffusion
  -> menu opens
  -> user clicks Rename
  -> row becomes inline input

User types /vision/diffusion
  -> presses Enter
  -> Rust renames Vault
  -> Explorer shows /vision/diffusion
  -> open tab title updates
```

Create from context menu:

```text
User right-clicks Vaults section
  -> menu opens
  -> user clicks Create Vault
  -> same inline create input from RFC 0010 appears
```

Duplicate:

```text
User renames /diffusion to /self-supervised
  -> Rust rejects duplicate path
  -> frontend cache is not mutated
  -> existing bridge error display shows the error
```

## Proposed Data Rules

Use the same normalization rules as RFC 0010:

- Trim whitespace.
- If input does not start with `/`, prefix `/`.
- Collapse repeated `/`.
- Remove trailing `/` unless the path is `/`.
- Derive `title` from the last path segment.
- Derive `id` only on create, not rename.

Rename should not change Vault ID.

Reason:

```text
vault.id = stable identity
vault.path/title = editable display and organization fields
```

This keeps `vault_papers` memberships stable.

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

Update:

```sql
update vaults
set title = ?, path = ?, updated_at = datetime('now')
where id = ?;
```

If no row is updated, return a user-readable "Vault not found" error.

If the path conflicts with another Vault, return a duplicate-path error.

## Proposed Rust Domain Type

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultRenameDraft {
    pub id: String,
    pub path: String,
}
```

## Proposed Rust Store Method

```rust
pub fn rename_vault(&self, draft: &VaultRenameDraft) -> Result<LibrarySnapshot, String>
```

Responsibilities:

- Normalize path.
- Validate path is usable.
- Derive title from path.
- Update existing Vault by stable ID.
- Preserve Vault ID.
- Return updated `LibrarySnapshot`.

## Proposed Tauri Command

```rust
#[tauri::command]
pub fn rename_vault(
    store: tauri::State<'_, LibraryStore>,
    draft: VaultRenameDraft,
) -> Result<LibrarySnapshot, String>
```

## Proposed Frontend Bridge

```ts
export async function renameVault(id: string, path: string): Promise<LibrarySnapshot>;
```

## Proposed Frontend Interaction

`VaultExplorer.svelte`:

- Tracks context menu state:
  - x/y position
  - selected Vault ID, if any
- Closes menu when:
  - clicking elsewhere
  - choosing an action
  - pressing Escape
- Tracks rename state:
  - active Vault ID
  - draft path text
- Reuses the current create input behavior for `Create Vault`.

`+page.svelte`:

- Calls `renameVault`.
- Hydrates returned snapshot.
- Updates any open Vault tab title/path for the renamed Vault.

## Tab Update Rule

If a Vault tab is open while that Vault is renamed:

```text
tab.vaultId == renamedVault.id
  -> tab.title = renamedVault.path
```

If the renamed Vault is active:

```text
currentPath updates from tab.title
```

## Data Boundary

```text
Explorer context menu
  -> renameVault(id, path)
  -> Tauri rename_vault
  -> LibraryStore::rename_vault
  -> SQLite update
  -> LibrarySnapshot
  -> hydrateLibrary(snapshot)
  -> update open tab title
```

## Design Notes

Keep the context menu small and sharp:

```text
+ Create Vault
Rename
```

Suggested behavior:

- Menu is dark, compact, and bordered.
- It appears near the pointer.
- It should not be a modal.
- It should close on action.
- It should not use browser default context menu.

## Teaching Notes

This RFC introduces update operations and stable identity.

Mental model:

```text
id = stable identity used by relationships
path/title = editable user-facing metadata
```

What can go wrong:

- Changing Vault IDs on rename would break `vault_papers`.
- Mutating frontend tab titles without backend success can create inconsistent UI.
- Browser context menus need `preventDefault` if using a custom menu.
- Duplicate paths must be rejected by Rust/SQLite, not only by frontend checks.

## Acceptance Criteria

- Right-clicking a Vault row opens a context menu.
- Vault row context menu includes `Rename` and `Create Vault`.
- Right-clicking Vaults section/list background offers `Create Vault`.
- Rename opens an inline editor.
- Empty rename input is rejected.
- Rename persists in SQLite.
- Duplicate path rename is rejected.
- Vault ID remains stable after rename.
- Open Vault tab title updates after successful rename.
- Explorer still shows accurate paper counts.
- `pnpm check` passes.
- `pnpm build` passes.
- `cargo check` passes.
