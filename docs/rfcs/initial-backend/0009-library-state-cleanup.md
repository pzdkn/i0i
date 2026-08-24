# RFC 0009: Library State Cleanup

Status: Stale
Date: 2026-05-24  
Product: i0i  
Target: Tauri v2 + Svelte, macOS first

## Summary

Clean up the codebase after RFC 0007 and RFC 0008 moved saved library data to SQLite.

This RFC does not add a new product feature. It makes the data ownership boundary easier to understand:

```text
Rust + SQLite = saved library source of truth
Svelte state = frontend cache and interaction state
Discover mocks = transient candidate feed
Reader mocks = temporary document text
```

## Context

Earlier RFCs used frontend mock data to discover the product shape. That was useful for App Shell, Vault, Reader, Discover, workspace tabs, and the multi-Vault add flow.

RFC 0007 introduced SQLite persistence for Vaults, Papers, and Vault/Paper membership.

RFC 0008 made the Explorer render persisted Vaults directly and removed the old mock Vault tree.

Some old mock-era code remains, especially around `mock/vault-workspaces.ts` and the naming of `library-state.svelte.ts`. That can confuse future implementation work because it is no longer obvious which data is real persisted state and which data is just seed/transient mock data.

## Goals

- Make saved library ownership clear in code.
- Ensure Vault and Paper saved data flows from Rust/SQLite.
- Remove or rename remaining frontend mock Vault/Paper code that is no longer live app data.
- Keep Discover mock data because Discover candidates are still transient.
- Keep Reader mock body text because real document extraction/PDF rendering is not implemented.
- Keep frontend state as a cache over `LibrarySnapshot`, not as an independent source of truth.
- Reduce accidental imports from old mock modules.
- Add small orientation comments where they prevent confusion.

## Non-Goals

- No new UI behavior.
- No schema changes.
- No new Rust commands.
- No migration framework.
- No notes/annotations.
- No import/export.
- No create/rename/delete Vault behavior.
- No removal of Discover mock candidates.
- No removal of Reader mock document text.

## First Increment

Build this:

- Rename or replace `mock/vault-workspaces.ts` so it is clearly seed/bootstrap-only.
- Remove unused live-app helpers from mock Vault modules.
- Ensure app components import `VaultWorkspace` from `domain/library.ts`, not mock modules.
- Ensure `VaultExplorer`, `VaultHome`, Discover target editor, and workspace page do not read saved Vault/Paper data directly from mocks.
- Rename or clarify `library-state.svelte.ts` as a frontend cache over Rust snapshots.
- Add a short comment near the frontend cache explaining:

```text
Saved library data comes from Rust/SQLite.
This module caches the latest LibrarySnapshot for Svelte views.
```

- Add a short comment near Discover mock data explaining:

```text
Discover candidates are transient proposals until added to Vault.
```

## Proposed Structure Direction

Current shape:

```text
src/lib/
  bridge/
    library.ts

  domain/
    library.ts
    paper.ts

  mock/
    discover.ts
    reader.ts
    vault-workspaces.ts

  state/
    library-state.svelte.ts
```

Target shape after cleanup:

```text
src/lib/
  bridge/
    library.ts

  domain/
    library.ts
    paper.ts

  mock/
    discover.ts        # transient Discover feed
    reader.ts          # temporary document text
    library-seed.ts    # optional frontend fallback/bootstrap only, not live app state

  state/
    library-cache.svelte.ts
```

The exact file name can be decided during implementation. The main rule is that names should not imply frontend mocks own the saved library.

## Frontend Data Boundary

Saved library:

```text
Rust SQLite -> get_library -> LibrarySnapshot -> frontend cache -> views
```

Discover candidates:

```text
mock/discover.ts -> transient candidates -> add converts to PaperDraft -> Rust SQLite
```

Reader body text:

```text
mock/reader.ts -> temporary reading content
```

## Rust Data Boundary

Rust seed/default data currently lives inside `LibraryStore`.

For this RFC, it is acceptable to keep it there, but the code should make clear that seeds are used only when the SQLite library is empty.

Optional cleanup:

```text
src-tauri/src/storage/
  library_store.rs
  library_seed.rs
```

This can make `LibraryStore` easier to read, but it is not required if it makes the diff larger than the value.

## What Should Stay Mocked

Keep:

- `mock/discover.ts`
- `mock/reader.ts`

Reason:

- Discover search/ranking is not implemented yet.
- Reader PDF/text extraction is not implemented yet.

Remove or rename:

- frontend mock Vault/Paper data that looks like live saved app state.

## Teaching Notes

This RFC is about architectural hygiene.

Mental model:

```text
mock = fake product input
seed = first-run default data
cache = latest backend snapshot in frontend memory
source of truth = SQLite
```

What can go wrong:

- Keeping old mock names makes future work use the wrong data path.
- Deleting all mocks too aggressively would remove useful transient product scaffolding.
- Moving too much code at once can make a cleanup harder to review than the feature code.

## Acceptance Criteria

- No Vault UI component imports saved Vault/Paper data from mock modules.
- Remaining mock Vault/Paper seed code is clearly named as seed/bootstrap-only or removed.
- Frontend library state/cache naming and comments make the SQLite ownership boundary clear.
- Discover mock data remains available.
- Reader mock text remains available.
- `pnpm check` passes.
- `pnpm build` passes.
- `cargo check` passes.
