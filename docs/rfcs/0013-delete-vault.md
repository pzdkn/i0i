# RFC 0013: Delete Vault

Status: Implemented  
Date: 2026-05-24  
Product: i0i  
Target: Tauri v2 + Svelte, macOS first

## Summary

Finish the first persisted Vault CRUD loop by allowing users to remove a Vault from the Explorer.

The action should remove:

- the selected Vault
- all memberships between that Vault and its papers
- any papers that no longer belong to any Vault after the Vault is removed

This preserves the current lifecycle rule:

```text
A paper is part of the library only if it belongs to at least one Vault.
```

## Context

i0i already supports:

- creating Vaults
- renaming Vaults
- browsing Vaults
- adding papers to one or more Vaults
- removing papers from a Vault
- removing papers from the library

The missing Vault CRUD operation is deletion.

Deleting a Vault is not just deleting one `vaults` row. It also removes relationships in `vault_papers`, and those removed relationships may make some papers orphaned. Orphaned papers should not remain hidden in SQLite.

## Product Decision

Add a Vault context-menu action:

```text
remove vault
```

Use lowercase action labels in the Explorer context menu for consistency with the paper context menu.

First increment behavior:

```text
remove vault
  -> delete the selected vault
  -> cascade-delete that vault's vault_papers rows
  -> delete papers that now have zero vault_papers rows
  -> return updated LibrarySnapshot
```

## Goals

- Add `remove vault` to the Vault row context menu.
- Persist Vault deletion through Rust/SQLite.
- Keep deletion transactional.
- Preserve papers that still belong to other Vaults.
- Delete papers that belonged only to the removed Vault.
- Return an updated `LibrarySnapshot`.
- Update Vault list, Vault counts, Discover `In Vault` chips, and open tabs after hydration.
- If the active Vault is removed, move to another available Vault.
- If no Vaults remain, show the existing empty workspace state.

## Non-Goals

- No trash.
- No undo.
- No soft delete.
- No bulk Vault deletion.
- No recursive folder tree model.
- No custom confirmation modal in this increment.
- No schema changes.
- No deleting local PDF files from disk.
- No Reader stale/deleted-paper behavior change.

## First Increment

Build this:

- Right-click a Vault row in `VaultExplorer`.
- Show context menu with:
  - `create vault`
  - `rename vault`
  - `remove vault`
- `remove vault` calls `deleteVault(vaultId)`.
- Rust deletes the Vault inside a transaction.
- Rust cleans up orphaned papers inside the same transaction.
- Frontend hydrates the returned snapshot.
- Frontend closes tabs for the removed Vault.
- Frontend selects another Vault tab if available.

## Proposed User Flow

Delete a Vault that shares papers with another Vault:

```text
User right-clicks /transformers/attention
  -> clicks remove vault
  -> /transformers/attention disappears
  -> shared papers remain in other Vaults
```

Delete a Vault containing unique papers:

```text
User right-clicks /vision-transformers
  -> clicks remove vault
  -> /vision-transformers disappears
  -> papers that only lived there are removed from the library
  -> Discover no longer shows those papers as In Vault
```

Delete the active Vault:

```text
User removes the currently open Vault
  -> active Vault tab closes
  -> app activates another open tab if present
  -> otherwise app opens another remaining Vault
  -> otherwise center workspace becomes empty
```

## Proposed SQLite Operation

Use one transaction:

```sql
delete from vaults
where id = ?;

delete from papers
where id not in (
  select paper_id from vault_papers
);
```

The first delete relies on the existing foreign key:

```sql
foreign key (vault_id) references vaults(id) on delete cascade
```

The second delete enforces the lifecycle rule by cleaning up papers that now have no memberships.

## Proposed Rust Store Method

```rust
pub fn delete_vault(&self, vault_id: &str) -> Result<LibrarySnapshot, String>
```

Responsibilities:

- Validate `vault_id` is non-empty.
- Start a transaction.
- Delete the Vault row.
- If no Vault row was deleted, return a user-readable error.
- Delete papers with zero remaining memberships.
- Commit.
- Return updated `LibrarySnapshot`.

## Proposed Tauri Command

```rust
#[tauri::command]
pub fn delete_vault(
    store: tauri::State<'_, LibraryStore>,
    vault_id: String,
) -> Result<LibrarySnapshot, String>
```

## Proposed Frontend Bridge

```ts
export async function deleteVault(vaultId: string): Promise<LibrarySnapshot>;
```

## Proposed Frontend Interaction

`VaultExplorer.svelte`

- Add `onDeleteVault`.
- Add `remove vault` to Vault row context menu.
- Use lower-case labels:
  - `create vault`
  - `rename vault`
  - `remove vault`
- Make `remove vault` visually dangerous but not visually loud.

`+page.svelte`

- Calls bridge.
- Hydrates returned snapshot.
- Removes tabs whose `vaultId` is the deleted Vault.
- If active tab was removed:
  - activate the first remaining tab
  - else open the first remaining Vault
  - else clear active workspace

## Teaching Notes

This introduces parent deletion with cascading relationships.

Mental model:

```text
vaults row = container
vault_papers rows = links from container to papers
papers rows = entities that may be linked from multiple containers
```

Deleting a Vault removes the container. SQLite can automatically remove its links because of `on delete cascade`.

But SQLite should not automatically remove papers, because a paper might still belong to another Vault. That is why the Store explicitly removes only papers with zero remaining links after the Vault is gone.

What can go wrong:

- Deleting papers before deleting Vault memberships can remove papers that are still needed elsewhere.
- Forgetting orphan cleanup leaves invisible saved papers.
- Doing the work outside a transaction can leave half-applied state.
- Leaving a tab open for a removed Vault creates a frontend state mismatch.

## Open Questions

- Should `remove vault` ask for confirmation once real user data exists?
- Should the UI show how many papers will be affected before removal?
- Should Vault deletion eventually move papers to a trash area instead of deleting orphaned papers?

## Acceptance Criteria

- Right-clicking a Vault row opens a context menu.
- Context menu includes `remove vault`.
- `remove vault` deletes the selected Vault.
- Deleting a Vault preserves papers that still belong to another Vault.
- Deleting a Vault removes papers that no longer belong to any Vault.
- The operation is transactional.
- Vault list updates after deletion.
- Vault counts update after deletion.
- Discover `In Vault` chips update after deletion.
- Open tabs for the removed Vault close.
- If no Vault remains, the app shows an empty workspace instead of crashing.
- `pnpm check` passes.
- `pnpm build` passes.
- `cargo fmt --check` passes.
- `cargo check` passes.
- `cargo test` passes.
