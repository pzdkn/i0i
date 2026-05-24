# RFC 0006: Inline Vault Target Autocomplete

Status: Draft  
Date: 2026-05-24  
Product: i0i  
Target: Tauri v2 + Svelte, macOS first

## Summary

Replace the implicit "add to current Vault" behavior from RFC 0005 with an explicit inline target-selection flow.

When the user clicks `Add` on a Discover candidate, the row should reveal a small target input where the user can search existing Vaults and select one or more target Vaults. Unknown Vault names cannot be selected or created.

This RFC keeps everything frontend-only and mock-driven.

## Context

RFC 0005 made Discover candidates addable to Vault. Its initial target rule was:

```text
if a Vault workspace is currently active/open:
  add to that Vault
else:
  add to /self-supervised
```

That works technically, but it is awkward as a product interaction. The user often thinks about the paper first, then decides where it belongs. Requiring the user to preselect a Vault elsewhere in the app hides an important curation decision.

The better model is:

```text
Add candidate
  -> choose one or more Vault targets inline
  -> confirm
  -> paper appears in those Vaults
```

## Product Decision

Adding from Discover should be explicit and local to the candidate row.

Papers may belong to multiple Vaults.

Only existing Vaults can be selected in this increment. Typing unknown text filters suggestions but does not create a new Vault.

## Goals

- Replace the implicit current-Vault target rule with explicit inline target selection.
- Let the user add one candidate to multiple existing Vaults.
- Keep the interaction close to the candidate row.
- Let typing filter existing Vault suggestions.
- Show selected Vaults as chips/tokens.
- Prevent unknown Vault names from being selected.
- Keep all state frontend-only and in memory.
- Preserve the dense i0i design language.

## Non-Goals

- No create-new-Vault behavior.
- No rename/delete Vault behavior.
- No drag-and-drop into the Explorer.
- No modal target picker.
- No persistence.
- No Rust command changes.
- No full keyboard-command-palette implementation.
- No perfect accessibility pass in this increment.
- No fuzzy search library.

## First Increment

Build this:

- Candidate row initially shows `Add`.
- Clicking `Add` opens an inline target editor in that row.
- The editor contains:
  - selected Vault chips
  - a text input
  - filtered suggestions from existing mock Vaults
  - confirm/cancel actions
- Typing filters suggestions by Vault title or path.
- Clicking a suggestion selects that Vault.
- Comma can separate typed search segments, but unknown typed text is not accepted as a target.
- Confirm is enabled only when at least one valid Vault is selected.
- Confirm adds the candidate to every selected Vault.
- Candidate row changes to an in-vault state.
- The row shows the selected target Vault chips after adding.
- Duplicate adds remain no-ops per Vault.

## Proposed User Flow

```text
User opens Discover
  -> sees candidate row
  -> clicks Add
  -> inline target input appears

User types "self"
  -> suggestions show /self-supervised
  -> user selects /self-supervised
  -> chip appears

User types "vision"
  -> suggestions show /vision-transformers
  -> user selects /vision-transformers
  -> second chip appears

User confirms
  -> paper is added to both Vaults
  -> row shows In Vault: /self-supervised, /vision-transformers
```

Unknown input:

```text
User types "random new vault"
  -> no matching Vault
  -> cannot confirm that text as a target
```

## Proposed Interaction Details

This is best described as an inline multi-select target input.

Technically, it is similar to a combobox/autocomplete, but the product language should stay simple:

```text
Vault target autocomplete
```

Expected behavior:

- `Add` opens the editor.
- `Escape` or `Cancel` closes it without adding.
- Clicking a suggestion creates a selected Vault chip.
- Clicking a selected chip's `x` removes that target.
- `Enter` confirms when at least one target is selected.
- Comma clears the current query after selecting an exact suggested Vault, if one is available.
- Unknown query text stays only as query text and is never persisted.

Keyboard navigation can be minimal in the first pass. Mouse selection is acceptable for this mock increment.

## Proposed State Changes

RFC 0005 introduced `library-state.svelte.ts`.

Extend it with target-aware operations:

```ts
function getVaultWorkspaces(): VaultWorkspace[];
function addCandidateToVaults(candidateId: string, vaultIds: string[]): Paper;
function getCandidateVaultTargets(candidateId: string): VaultWorkspace[];
```

Notes:

- `addCandidateToVaults` should add to each selected Vault.
- If a candidate already exists in one target Vault, do not duplicate it there.
- The candidate is considered in-vault if it exists in at least one Vault.
- `getCandidateVaultTargets` lets Discover display where the candidate already lives.

## Proposed Frontend Structure

```text
src/lib/features/discover/
  DiscoverFeed.svelte
  DiscoverTargetEditor.svelte
  DiscoverView.svelte

src/lib/state/
  library-state.svelte.ts
```

`DiscoverTargetEditor.svelte` should remain small and local to Discover. It should not become a generic design-system component yet.

## Component Responsibilities

`DiscoverFeed.svelte`
: Owns which candidate row currently has the target editor open.

`DiscoverTargetEditor.svelte`
: Renders selected target chips, input, suggestions, confirm, and cancel.

`DiscoverView.svelte`
: Passes available Vaults and add handlers into the feed.

`+page.svelte`
: Calls state operations and keeps workspace coordination.

`library-state.svelte.ts`
: Owns in-memory mutation and duplicate prevention.

## Data Boundary

For this increment:

- Existing mock Vaults are the only valid target suggestions.
- Selected target Vaults are held in local component state until confirm.
- Confirm mutates frontend memory through `library-state.svelte.ts`.
- Rust remains unchanged.
- Reloading the app resets the data.

Later:

- Vault creation can become an explicit command.
- Target selection can support recent Vaults, favorites, or learned suggestions.
- Persistence can move behind Rust commands.
- Duplicate detection can use DOI, arXiv ID, Semantic Scholar ID, or normalized metadata.

## Design Notes

Preserve the current i0i design language:

- Dense row interaction
- Inline editing instead of modal
- Sharp rectangles
- Hairline borders
- Amber-on-black
- IBM Plex Mono

The row can temporarily expand while the editor is open. Keep the expanded state compact enough that the feed still feels scannable.

Example visual shape:

```text
[ /self-supervised x ] [ /vision-transformers x ] [ type vault...        ]

suggestions:
  /self-supervised        41 papers
  /vision-transformers    33 papers

[ Cancel ] [ Add to 2 Vaults ]
```

After adding:

```text
In Vault: /self-supervised /vision-transformers
```

## Teaching Notes

This RFC introduces two important product-engineering ideas.

First, target selection should be explicit when the action has meaningful consequences.

Second, multi-membership is different from moving a file into one folder. A paper can belong to more than one knowledge context.

Mental model:

```text
Discover candidate = unsaved proposal
Vault target autocomplete = choose knowledge contexts
Confirm = persist candidate into selected contexts
```

What can go wrong:

- Creating unknown Vaults from free text would make accidental typos persistent.
- A modal would slow down repeated curation.
- Hiding the target behind current selection makes Add feel unpredictable.
- Making this a generic component too early adds abstraction before the interaction is proven.

## Open Questions

- Should `Enter` select the first suggestion or confirm selected chips?
- Should already-owned candidates allow adding to additional Vaults?
- Should suggestions rank by recently used Vaults later?
- Should the row show all target Vaults or only the first few when many are selected?

## Acceptance Criteria

- Clicking `Add` opens an inline target editor.
- Existing Vaults appear as filtered suggestions while typing.
- The user can select multiple existing Vault targets.
- Unknown text cannot be selected as a Vault.
- Confirm adds the candidate to all selected Vaults.
- Added papers appear in each selected Vault.
- Duplicate add does not create duplicate rows inside a Vault.
- Candidate row shows an in-vault state with target Vault chips.
- `pnpm check` passes.
- `pnpm build` passes.
- `cargo check` still passes.
