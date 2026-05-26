# RFC 0016: Reader Inspector Tabs

Status: Implemented  
Date: 2026-05-26  
Product: i0i  
Target: Tauri v2 + Svelte, macOS first

## Summary

Convert the Reader Inspector from one vertical stack of panels into a tabbed Inspector.

The current Inspector now contains:

- Notes
- Lineage
- Ask this paper
- Metadata

After RFC 0015, Notes became a real persisted feature. Keeping every tool visible at once makes the Inspector feel crowded and harder to scan.

## Context

RFC 0015 explicitly recorded this risk:

```text
The Inspector can become crowded if it tries to show every Reader tool at once.
```

It also proposed a possible mode-based Inspector:

```text
[Notes] [Graph] [Ask] [Meta]
```

Now that selection-anchored notes exist, this refinement should become real.

## Product Decision

Use tabs in the Reader Inspector:

```text
[Notes] [Lineage] [Ask] [Meta]
```

Only the active tab content is visible.

When the user creates a note from selected Reader text, switch the Inspector to `Notes`.

## Goals

- Reduce Inspector crowding.
- Keep Notes as the primary active Reader workflow.
- Preserve the existing mock Lineage and Ask content.
- Preserve Metadata content.
- Keep this as a frontend-only layout change.
- Make tab labels short and readable.
- Keep keyboard save behavior for notes: `Cmd/Ctrl + Enter`.

## Non-Goals

- No backend changes.
- No schema changes.
- No new note behavior.
- No note editing/deletion.
- No persistent highlights or margin markers.
- No real graph implementation.
- No real Ask/AI implementation.
- No routing or persisted Inspector tab preference.

## First Increment

Build this:

- Add local Inspector tab state.
- Render a compact tab strip below the Inspector header.
- Tabs:
  - `Notes`
  - `Lineage`
  - `Ask`
  - `Meta`
- Move the existing Notes UI into the Notes tab.
- Move the existing Lineage mock into the Lineage tab.
- Move the existing Ask mock into the Ask tab.
- Move the existing Metadata grid into the Meta tab.
- Show only active tab content.
- When a note draft appears, activate `Notes`.

## Proposed Interaction

Default:

```text
Reader opens
  -> Inspector shows Notes tab
```

Create note:

```text
select text
  -> click comment icon
  -> Inspector switches to Notes
  -> note draft is visible
```

Manual tab switch:

```text
click Lineage
  -> Lineage content appears

click Ask
  -> Ask content appears

click Meta
  -> Metadata content appears
```

## Proposed Frontend Changes

`ReaderInspector.svelte`

- Add local tab state:

```ts
type InspectorTab = "notes" | "lineage" | "ask" | "meta";
```

- Add a tab strip below the header.
- Set active tab to `notes` when `noteDraft` changes from empty to present.
- Render tab content conditionally.

`ReaderView.svelte`

- No required backend changes.
- If needed, pass a lightweight signal that a note draft was created. Prefer keeping the tab-switch behavior inside `ReaderInspector` by reacting to `noteDraft`.

## UI Notes

The tab strip should feel like part of an IDE/tool panel, not like a marketing navigation bar.

Suggested style:

- compact height
- subtle borders
- active tab uses amber/cyan accent
- no large cards
- no nested card layout

The Inspector should remain narrow and utility-focused.

## Teaching Notes

This RFC introduces local UI mode state.

Mental model:

```text
Reader Inspector = one side panel
Inspector tabs = which tool is currently active
Notes / Lineage / Ask / Meta = sibling tools, not stacked sections
```

What can go wrong:

- Leaving all content mounted can still feel crowded if CSS hides it poorly.
- Auto-switching tabs too aggressively can surprise the user.
- Putting backend state into tab selection would overcomplicate a local UI concern.
- Making tabs too large would waste the limited Inspector width.

## Open Questions

- Should the Inspector remember the last selected tab per Reader session later?
- Should `Ask` eventually become the default tab when the user invokes an AI action?
- Should the tab names become icon-only once the Inspector gets denser?

## Acceptance Criteria

- Reader Inspector has visible tabs: `Notes`, `Lineage`, `Ask`, `Meta`.
- Only one tab's content is visible at a time.
- Notes tab contains the existing note draft and saved notes UI.
- Lineage tab contains the existing lineage mock.
- Ask tab contains the existing Ask mock.
- Meta tab contains the existing metadata grid.
- Creating a note draft switches the Inspector to Notes.
- `Cmd/Ctrl + Enter` still saves a note from the textarea.
- `pnpm check` passes.
- `pnpm build` passes.
