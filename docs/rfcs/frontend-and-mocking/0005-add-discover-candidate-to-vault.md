# RFC 0005: Add Discover Candidate To Vault

Status: Draft  
Date: 2026-05-24  
Product: i0i  
Target: Tauri v2 + Svelte, macOS first

## Summary

Make Discover feed into Vault for the first time.

The smallest useful behavior is:

```text
Any Discover candidate
  -> Add
  -> appears in the target Vault folder
  -> can be opened from Vault in Reader
```

This RFC introduces one product decision: Vault is the editable saved-paper library, while Discover is a source of unsaved paper candidates.

This RFC uses the product-wide architecture defined in [`docs/overview/architecture.md`](../overview/architecture.md).

## Context

RFC 0001 created the app shell and Vault view.

RFC 0002 added the Reader skeleton.

RFC 0003 introduced workspace tabs.

RFC 0004 added the Discover skeleton with mock candidates and visible candidate actions.

Right now, Discover candidates can be double-clicked into Reader, but the `Add` action is still decorative. That means Discover does not yet affect the user's library. To make i0i feel like a knowledge-curation IDE, Discover needs to create saved Vault entries.

## Product Decision

Vault owns saved papers.

Discover owns candidate papers.

Adding any candidate converts a `DiscoverCandidate` into a `Paper` and inserts it into a selected Vault workspace.

This implies Vault will eventually need full CRUD:

- Create: add/import papers
- Read: browse/open papers
- Update: edit metadata, tags, status, notes, folder membership
- Delete: remove papers from a vault

This RFC only implements the first small create path.

## Goals

- Make `Add` on every Discover candidate row do one real thing.
- Add any Discover candidate to a mock Vault workspace.
- Mark each added candidate as already in Vault after adding.
- Let the added paper appear in the Vault paper list.
- Let the added paper open in Reader from Vault.
- Preserve browsing/viewing all existing mock Vault workspaces.
- Keep the implementation frontend-only and mock-driven.
- Keep the code understandable and modular.
- Avoid turning `+page.svelte` into a long-term state junk drawer.

## Non-Goals

- No full CRUD implementation.
- No edit-paper form.
- No delete action.
- No folder management UI.
- No create/rename/delete Vault UI.
- No move-paper-between-vaults UI.
- No duplicate-resolution modal.
- No undo.
- No persistence across app reloads.
- No Rust command changes.
- No database.
- No real import pipeline.
- No real metadata lookup.

## First Increment

Build this:

- Clicking `Add` on any Discover candidate inserts it into the target Vault.
- Target Vault is the currently active/open Vault when available.
- If no Vault is active/open, fallback target Vault is `/self-supervised`.
- Each added candidate row updates from `Add` to an in-vault state.
- The target Vault paper count/list updates immediately.
- All existing mock Vaults remain browsable from the left Explorer.
- Opening the target Vault shows the newly added paper alongside its existing papers.
- Double-clicking that new Vault paper opens Reader.
- Adding the same candidate twice is a no-op.

Keep all data in frontend memory.

## Proposed User Flow

```text
User opens Discover
  -> sees candidate rows
  -> clicks Add on any row
  -> row shows candidate is in Vault

User opens the target Vault
  -> added candidate appears in the paper list
  -> user double-clicks paper
  -> Reader opens the paper

User returns to Discover
  -> same candidate still shows in-vault state

User opens another Vault from the Explorer
  -> that Vault remains browsable
  -> its own papers are shown
```

## Proposed State Direction

RFC 0001-0004 use static mock arrays imported by views. That is fine for read-only screens, but adding a paper requires mutable app state.

For this RFC, introduce a small frontend state module:

```text
src/lib/state/
  library-state.svelte.ts
```

This module should own the temporary in-memory library state:

```ts
type LibraryState = {
  vaults: VaultWorkspace[];
  discoverWorkspaces: DiscoverWorkspace[];
};
```

It should expose small operations:

```ts
function getVaultWorkspace(vaultId: string): VaultWorkspace;
function getVaultWorkspaces(): VaultWorkspace[];
function getDiscoverWorkspace(discoverId: string): DiscoverWorkspace;
function addCandidateToVault(candidateId: string, vaultId: string): Paper;
function isCandidateInVault(candidateId: string): boolean;
```

The exact Svelte implementation can stay simple. The important rule is that components should not directly mutate imported mock arrays.

## Proposed Frontend Structure

```text
src/lib/
  state/
    library-state.svelte.ts

  mock/
    papers.ts
    vault-workspaces.ts
    discover.ts

  features/
    discover/
      DiscoverFeed.svelte
      DiscoverView.svelte

    vault/
      VaultHome.svelte
      PaperList.svelte
```

Mock files remain the seed data. The state module creates mutable working state from those seeds.

## Component Responsibilities

`+page.svelte`
: Coordinates workspaces, tracks the active Vault, and passes action handlers down. It should call library-state operations instead of manually editing paper arrays.

`library-state.svelte.ts`
: Owns temporary frontend library state and basic operations such as adding a candidate to a vault.

`DiscoverFeed.svelte`
: Emits `onAddCandidate(candidateId)` when the user clicks `Add`. It should not know how Vault storage works.

`DiscoverView.svelte`
: Passes candidate actions between the feed and page/state layer.

`VaultHome.svelte`
: Renders the current Vault workspace from state.

`VaultExplorer.svelte`
: Continues to let the user browse/open all existing mock Vault workspaces.

## Data Boundary

For this increment:

- Mock data seeds initial state.
- Svelte owns editable in-memory library state.
- Adding is immediate and local.
- Rust remains unchanged.
- Reloading the app resets the data.

Later:

- Rust should own persistent Vault state.
- `addCandidateToVault` can move behind a typed Tauri command.
- Frontend state can become a cache over Rust-owned data.
- Duplicate handling can use stable paper identifiers such as DOI, arXiv ID, Semantic Scholar ID, or normalized title.

## Duplicate Rule

For now, use candidate ID as the duplicate key.

If a candidate with the same ID is already present in any Vault workspace:

- Do not add it again.
- Keep the UI in an in-vault state.
- Return the existing paper.

This is intentionally simple and will not catch real-world duplicate papers yet.

## Vault Browsing Rule

All existing mock Vault workspaces remain viewable from the left Explorer.

For this RFC, every Vault is editable in the narrow sense that a paper can be added to it in memory. However, RFC 0005 does not add full Vault CRUD or folder-management controls.

Target selection rule:

```text
if a Vault workspace is currently active/open:
  add to that Vault
else:
  add to /self-supervised
```

This avoids a target-picker modal while still making the app behave predictably.

## Design Notes

Preserve the current design language:

- Dense rows
- Amber-on-black
- Hairline borders
- Visible affordances for future controls
- No modal for this increment

The `Add` action should become visually distinct after adding, for example:

```text
Add      -> Vault
```

or:

```text
+ Add    -> In Vault
```

The exact label can follow what looks best in the current row layout.

## Teaching Notes

This RFC introduces a key frontend architecture concept: state ownership.

Mental model:

```text
mock data = seed data
state module = current editable app memory
components = render state and emit intentions
```

What can go wrong:

- Mutating imported mock arrays directly makes behavior hard to reason about.
- Letting every component own its own copy creates inconsistent UI.
- Putting all mutations in `+page.svelte` makes the page too important.
- Designing full CRUD before the first create path will slow the product down.

## Open Questions

- Should `Add` open the target Vault automatically, or should it keep the user in Discover?
- Should an already-owned candidate offer `Open` into Reader, `Reveal` in Vault, or both?
- Should the first state module be named `library-state.svelte.ts`, `vault-state.svelte.ts`, or `workspace-state.svelte.ts`?

## Acceptance Criteria

- Discover `Add` button works for every candidate row that is not already in Vault.
- Added candidate appears in the target Vault paper list.
- All existing mock Vaults can still be browsed/opened from the Explorer.
- Added candidate can be opened in Reader from Vault.
- Duplicate add does not create a second row.
- `pnpm check` passes.
- `pnpm build` passes.
- `cargo check` still passes.
