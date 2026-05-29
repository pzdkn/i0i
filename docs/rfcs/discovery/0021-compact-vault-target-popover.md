# RFC 0021: Compact Inline Vault Target Editor

Status: Draft  
Date: 2026-05-29  
Product: i0i  
Target: Tauri v2 + Svelte, macOS first

## Summary

Replace the current bulky Discover target editor with a compact inline vault suggestion editor.

The current Add/Add target flow expands inside the candidate row. Because the action column sits near the right Inspector, the editor can cross the Discover/Inspector boundary and make the row feel too heavy. Drawer and popover variants avoid one issue while creating another. This RFC keeps the original inline row placement, but makes the editor small enough to fit comfortably inside the row.

## Goals

- Keep the candidate row compact.
- Reduce the chance of the Add target UI spilling into the Inspector.
- Remove explicit Add and Cancel buttons from the target editor.
- Let typing filter Vault suggestions.
- Add a candidate immediately when the user clicks a Vault suggestion.
- Support `Escape` to close.
- Support `Enter` to add the first visible suggestion.
- Use the same `Add to Vault` button for saved and unsaved candidates.
- Slightly narrow the Discover Inspector to give the feed more horizontal room.
- Keep existing add-to-vault persistence behavior.

## Non-Goals

- No modal.
- No Inspector-based add flow.
- No save reason field.
- No PDF download/import options.
- No create-new-Vault flow.
- No persisted Scout changes.
- No change to the Rust library store.

## Proposed Interaction

Candidate row:

```text
#1  Paper title and authors                  Meta   [Add]
```

Click `Add to Vault`:

```text
#1  Paper title and authors      Meta   [Add to Vault]
                                      +----------------+
                                      | type vault...   |
                                      | /attention      |
                                      | /self-supervised|
                                      +----------------+
```

Typing filters suggestions:

```text
type "self"
  -> show matching Vault paths/titles/ids
```

Click a suggestion:

```text
candidate + clicked Vault
  -> existing addCandidate(candidateId, [vaultId])
  -> popover closes
  -> row updates to In Vault
```

Already-owned candidate:

```text
Add to Vault
  -> same compact editor
  -> suggestions exclude Vaults where the candidate already lives
```

Use the same button label for unsaved and saved candidates:

```text
Add to Vault
```

## Layout Decision

Render the editor inline inside the row actions area, like the first version, but compact:

```text
width: 190-220px
no selected chip area
no footer buttons
input + suggestion list only
```

Also narrow the Inspector:

```text
Discover Inspector: 320px -> about 280px
```

The editor remains attached to the row action that opened it, and the candidate row stays the user's locus of action.

## Component Direction

Update `DiscoverTargetEditor.svelte` into a compact inline suggestion editor:

- one input
- vertical suggestion list
- no selected-chip area
- no Add button
- no Cancel button
- autofocus input
- `Escape` closes
- `Enter` adds first suggestion
- click suggestion confirms immediately

`DiscoverFeed.svelte` should:

- render `DiscoverTargetEditor` inside the active row's action area
- pass candidate-specific target Vaults into it
- close it after selection
- use `Add to Vault` for both unsaved and saved candidates

## Files Likely Affected

Frontend:

- `src/lib/features/discover/DiscoverFeed.svelte`
- `src/lib/features/discover/DiscoverTargetEditor.svelte`
- `src/lib/features/discover/DiscoverInspector.svelte`

No Rust or SQLite changes expected.

## Risks

- The inline editor can still feel cramped on narrow windows.
- A very long Vault path may need truncation.
- Keyboard behavior should remain simple and predictable.

## Validation Plan

```bash
pnpm check
pnpm build
```

Manual validation:

- click `Add` on a candidate
- confirm compact editor appears inside the row action area
- confirm it is smaller than the original editor
- confirm Inspector is slightly narrower
- type text and see Vault suggestions filter
- click a suggestion and confirm the candidate is added
- click outside and confirm the popover closes
- press `Escape` and confirm the popover closes
- press `Enter` and confirm the first suggestion is added
- click `Add to Vault` on an owned candidate and confirm existing Vaults are excluded
