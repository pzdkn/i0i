# RFC 0012: Remove Paper From Vault

Status: Implemented  
Date: 2026-05-24  
Product: i0i  
Target: Tauri v2 + Svelte, macOS first

## Summary

Add the first persisted paper-removal operations.

The user should be able to:

- Remove a paper from one specific Vault.
- Remove a paper from the entire library.

This RFC keeps the lifecycle rule:

```text
A paper is part of the library only if it belongs to at least one Vault.
```

## Context

i0i now has persisted Vaults, persisted Papers, many-to-many Vault/Paper membership, adding Discover candidates to one or more Vaults, creating Vaults, and renaming Vaults.

The next natural curation operation is removing an incorrect membership:

```text
This paper does not belong in this Vault.
```

There is also a separate user intent:

```text
This paper does not belong anywhere in my library.
```

These should be two separate actions.

## Product Decision

`Remove from vault` removes one Vault membership and then performs reference-count cleanup.

```text
Remove from vault
  -> delete one vault_papers row
  -> count remaining memberships for that paper
  -> if count == 0, delete the paper
```

`Remove from library` deletes the paper entity directly.

```text
Remove from library
  -> delete paper row
  -> let vault_papers cascade-delete all memberships
```

This avoids invisible orphan papers while still giving the user a convenience action for removing a paper from the whole library.

## Goals

- Add a paper-row context menu in Vault paper lists.
- Add `Remove from vault`.
- Add `Remove from library`.
- Persist both operations through Rust/SQLite.
- Keep `Remove from Vault` transactional:
  - delete selected membership
  - count remaining memberships
  - delete paper if count is zero
- Let global delete remove all Vault memberships through existing FK cascade.
- Return updated `LibrarySnapshot`.
- Update Vault counts after hydration.
- Update Discover `In Vault` chips after removal/deletion.

## Non-Goals

- No trash.
- No undo.
- No soft delete.
- No custom confirmation modal in the first increment.
- No bulk remove.
- No deleting PDFs from disk.
- No notes/annotations cascade design yet.
- No schema changes unless strictly necessary.

## First Increment

Build this:

- Right-click a paper row in `PaperList`.
- Show context menu with:
  - `Remove from vault`
  - `Remove from library`
- `Remove from Vault` calls `removePaperFromVault(vaultId, paperId)`.
- `Remove from library` calls `removePaperFromLibrary(paperId)`.
- Rust returns updated `LibrarySnapshot`.
- Frontend hydrates cache.

## Proposed User Flow

Remove from one Vault:

```text
User opens /self-supervised
  -> right-clicks a paper
  -> clicks Remove from vault
  -> paper disappears from /self-supervised
  -> count decreases by 1
```

If the paper is still in another Vault:

```text
paper remains visible in other Vaults
Discover row still shows In Vault with remaining Vault chips
```

If that was the paper's final Vault membership:

```text
paper is deleted from papers
Discover row no longer shows In Vault
Reader can no longer open it from Vault
```

Delete globally:

```text
User right-clicks a paper
  -> clicks Remove from library
  -> paper disappears from every Vault
  -> Discover row no longer shows In Vault
```

## Proposed SQLite Operations

### Remove From Vault

Transaction:

```sql
delete from vault_papers
where vault_id = ? and paper_id = ?;

select count(*)
from vault_papers
where paper_id = ?;

-- if count == 0
delete from papers
where id = ?;
```

The operation should commit only after both the membership removal and possible paper deletion succeed.

### Remove From Library

```sql
delete from papers
where id = ?;
```

Existing foreign keys should cascade-delete related memberships:

```sql
foreign key (paper_id) references papers(id) on delete cascade
```

## Proposed Rust Store Methods

```rust
pub fn remove_paper_from_vault(
    &self,
    vault_id: &str,
    paper_id: &str,
) -> Result<LibrarySnapshot, String>
```

Responsibilities:

- Validate inputs are non-empty.
- Delete the selected membership.
- If no membership was deleted, return a user-readable error.
- Count remaining memberships.
- Delete paper if count is zero.
- Return updated `LibrarySnapshot`.

```rust
pub fn delete_paper_globally(
    &self,
    paper_id: &str,
) -> Result<LibrarySnapshot, String>
```

Responsibilities:

- Validate input is non-empty.
- Delete the paper row.
- Let FK cascade remove Vault memberships.
- If no paper row was deleted, return a user-readable error.
- Return updated `LibrarySnapshot`.

## Proposed Tauri Commands

```rust
#[tauri::command]
pub fn remove_paper_from_vault(
    store: tauri::State<'_, LibraryStore>,
    vault_id: String,
    paper_id: String,
) -> Result<LibrarySnapshot, String>
```

```rust
#[tauri::command]
pub fn delete_paper_globally(
    store: tauri::State<'_, LibraryStore>,
    paper_id: String,
) -> Result<LibrarySnapshot, String>
```

## Proposed Frontend Bridge

```ts
export async function removePaperFromVault(
  vaultId: string,
  paperId: string,
): Promise<LibrarySnapshot>;

export async function removePaperFromLibrary(
  paperId: string,
): Promise<LibrarySnapshot>;
```

## Proposed Frontend Interaction

`PaperList.svelte`

- Add row context menu state.
- Right-click paper row opens compact menu.
- Menu items:
  - `Remove from vault`
  - `Remove from library`
- Emits `onRemoveFromVault(paperId)`.
- Emits `onRemoveFromLibrary(paperId)`.

`VaultHome.svelte`

- Receives current `workspace.id`.
- Passes remove action to parent as `(vaultId, paperId)`.
- Passes global delete action to parent as `paperId`.

`+page.svelte`

- Calls bridge.
- Hydrates returned snapshot.
- If active Reader is showing a paper that was deleted, leave Reader tab open for now.

Reason for leaving Reader open:

- It avoids surprise tab closure.
- Reader already holds a paper object in frontend state.
- A future RFC can define stale/deleted Reader behavior.

## Data Lifecycle Rule

For now:

```text
paper exists iff paper has at least one Vault membership
```

This is like reference counting:

```text
vault_papers rows = references
papers row = object
delete object when reference count reaches zero
```

`Remove from Vault` follows the reference-count rule.

`Remove from library` bypasses reference counting because the user explicitly chose to remove the paper entity from the whole library.

## Teaching Notes

This RFC introduces relationship deletion and entity deletion.

Mental model:

```text
Removing from one Vault removes a relationship.
Deleting globally removes the entity.
The entity is also deleted when no relationships remain.
```

What can go wrong:

- Deleting from `papers` for `Remove from Vault` would remove the paper from all Vaults.
- Keeping orphans would create invisible saved papers.
- Doing membership removal and paper deletion outside one transaction can leave inconsistent state.
- Global delete should be visually distinct because it removes every membership.
- Future notes/annotations will need an explicit cascade or soft-delete policy.

## Open Questions

- Should `Remove from Vault` need confirmation later when it deletes the final membership?
- Should removing from the library need confirmation once the app holds real user data?
- Should future notes/annotations block deletion, cascade deletion, or move to trash?
- Should Reader show a stale/deleted state if open while the paper is deleted?

## Acceptance Criteria

- Right-clicking a paper row opens a context menu.
- Context menu includes `Remove from vault`.
- Context menu includes `Remove from library`.
- `Remove from Vault` deletes only the selected Vault membership first.
- If remaining membership count is zero, `Remove from Vault` deletes the paper row.
- `Remove from library` deletes the paper row.
- Global delete removes all Vault memberships through FK cascade.
- Operations are transactional or single backend writes.
- Vault counts update after removal/deletion.
- Discover `In Vault` chips update after removal/deletion.
- Removing a paper from one Vault does not remove it from other Vaults unless it was the final membership.
- Duplicate/nonexistent membership removal returns an error.
- `pnpm check` passes.
- `pnpm build` passes.
- `cargo check` passes.
- `cargo test` passes.
