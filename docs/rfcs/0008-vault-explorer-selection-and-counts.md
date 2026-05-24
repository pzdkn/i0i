# RFC 0008: Vault Explorer Selection And Counts

Status: Draft  
Date: 2026-05-24  
Product: i0i  
Target: Tauri v2 + Svelte, macOS first

## Summary

Make the Vault Explorer feel reliable now that Vaults are backed by SQLite.

The Explorer should show a small, selectable list of real persisted Vaults. The count next to each Vault should reflect the current number of papers in that Vault.

## Context

RFC 0007 moved saved library data to SQLite. Vaults and paper memberships are now persisted in Rust instead of being frontend-only mock state.

The current Explorer still behaves like an old mock tree. Some rows map awkwardly to Vault IDs, and counts are not guaranteed to reflect persisted membership counts. That was acceptable while everything was mock UI, but it is confusing now that Vaults are real persisted entities.

## Goals

- Make every visible Vault row selectable.
- Render Vault rows from persisted library state instead of the old mock tree.
- Reduce visual noise by showing only the current persisted Vaults.
- Highlight the active Vault reliably.
- Show paper counts from each Vault's actual paper list.
- Keep the Explorer simple and easy to understand.
- Remove frontend mock Vault code that has been replaced by SQLite-backed library state.
- Make `VaultExplorer.svelte` prop-driven and independent from mock imports.

## Non-Goals

- No create/rename/delete Vault UI.
- No drag-and-drop.
- No nested tree editing.
- No custom sorting UI.
- No search/filter behavior beyond accepting typed text.
- No changes to SQLite schema.
- No Rust command changes unless existing snapshot data is insufficient.
- No removal of Discover mock data; Discover candidates are still intentionally transient.

## First Increment

Build this:

- Pass persisted `VaultWorkspace[]` into `VaultExplorer`.
- Replace the old hardcoded `vaultTree` rendering for Vault folders.
- Show one selectable row per persisted Vault.
- Use `workspace.path` as the visible label.
- Use `workspace.papers.length` as the count.
- Keep the filter input editable, but filtering can remain minimal or visual-only.
- Clicking a Vault row opens that Vault tab.
- The active Vault row is highlighted.
- Remove unused `vault-tree` imports/code if the Explorer no longer needs them.
- Reduce `mock/vault-workspaces.ts` to seed/fallback helpers only, or remove dead exports if they are no longer used.
- Do not use mock Vault fallback data in `VaultExplorer`.
- If no persisted Vaults exist, render the normal Explorer shell with an empty Vault selection, optionally with a muted `No Vaults` row.

## Proposed User Flow

```text
User looks at Explorer
  -> sees persisted Vaults
  -> each row has a real paper count

User clicks /self-supervised
  -> /self-supervised opens in the workspace
  -> /self-supervised row is highlighted

User adds a Discover candidate to /self-supervised
  -> /self-supervised count updates
```

## Proposed Component Changes

`+page.svelte`
: Passes `vaultWorkspaces` to `VaultExplorer`.

`VaultExplorer.svelte`
: Renders Vault rows from `vaultWorkspaces` instead of mapping a mock tree node to a Vault ID. It should receive `activeVaultId`, `vaults`, and `onOpenVault` as props and should not import mock Vault data directly.

`library-state.svelte.ts`
: Already computes each `VaultWorkspace.papers` list from the persisted `LibrarySnapshot`.

## Mock Cleanup

After RFC 0007, saved library data should come from SQLite through `LibrarySnapshot`.

The frontend should keep only mock data that still represents intentionally transient product surfaces:

- Keep `mock/discover.ts` for Discover candidates.
- Keep Reader mock body text until real document text/PDF extraction exists.
- Retire `mock/vault-tree.ts` if the Explorer renders persisted Vaults directly.
- Avoid using `mock/vault-workspaces.ts` as the source of truth for saved Vaults/Papers.
- Do not add a mock fallback path inside `VaultExplorer`.

If some mock Vault helpers are still useful before Rust hydration completes, keep them outside `VaultExplorer` and name them as seed/bootstrap data rather than live app data.

## Empty Vault Rule

If the persisted library contains zero Vaults, the Explorer should render the same chrome with an empty Vault list.

It may show a single muted row:

```text
No Vaults
```

It should not switch to mock data, create fake Vaults, or show a large onboarding panel.

## Design Notes

The Explorer should stay dense and IDE-like:

- Keep the header and filter field.
- Render a compact list.
- Show path and count.
- Use the existing active-row styling.

Example:

```text
Explorer
  /interpretability              5
  /self-supervised               7
  /transformers/attention       12
  /transformers/scaling-laws     5
  /vision-transformers           5
```

This is less visually rich than the old mock tree, but more truthful and easier to reason about.

## Acceptance Criteria

- Explorer shows persisted Vaults.
- Every shown Vault row is clickable.
- Active Vault row is highlighted.
- Counts match each Vault's actual paper count.
- Counts update after adding a paper from Discover.
- Explorer does not import mock Vault data directly.
- Empty persisted Vault list renders as an empty selection or muted `No Vaults` row.
- Stale mock Vault tree code is removed or no longer imported.
- SQLite-backed library state remains the saved-data source of truth.
- `pnpm check` passes.
- `pnpm build` passes.
- `cargo check` passes.
