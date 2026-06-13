# RFC 0028: Note Draft Cancellation

Status: Draft
Date: 2026-06-03
Product: i0i
Target: Tauri v2 + Svelte, macOS first

## Summary

Make note creation forgiving by allowing empty note drafts to be dismissed casually, while protecting drafts that already contain user-written text.

This RFC applies to Reader note drafts opened from PDF text selection, as defined in RFC 0027. The same behavior can later apply to other note creation paths.

## Problem

After selecting PDF text and clicking the floating add-note affordance, the Inspector opens a note draft.

If the user changes their mind, there should be a clear way to cancel. Without cancellation, accidental drafts feel sticky. But silently discarding typed content is risky because a note body may contain an unfinished thought.

## Decision

Use two cancellation behaviors:

```text
empty draft
  -> click outside, Escape, or Cancel dismisses quietly

non-empty draft
  -> click outside does not discard
  -> Cancel is the explicit discard action
```

The draft is considered non-empty once the user has typed non-whitespace note body content.

## Why

Selecting text is a lightweight reading action. Note creation should not punish accidental starts.

Typing a note is different: it creates user-authored content. Once there is typed content, the app should avoid silent data loss.

## Goals

- Let users quickly abandon accidental empty note drafts.
- Provide an explicit Cancel action for deliberate discard.
- Preserve non-empty drafts when the user clicks back into the PDF.
- Keep Save as the primary action.
- Support Escape as a keyboard cancellation path for empty drafts.

## Non-Goals

- No modal confirmation for empty drafts.
- No autosave of unfinished drafts in this slice.
- No draft recovery after app restart.
- No multi-draft queue.
- No changes to saved-note editing behavior unless required by shared UI.

## Interaction Behavior

Starting a draft:

```text
select PDF text
  -> floating add-note button appears
  -> click button
  -> Inspector opens note draft
```

Empty draft cancellation:

```text
draft body is empty
  -> click outside Inspector
  -> dismiss draft
```

```text
draft body is empty
  -> press Escape
  -> dismiss draft
```

```text
draft body is empty
  -> click Cancel
  -> dismiss draft
```

Non-empty draft behavior:

```text
draft body has non-whitespace text
  -> click outside Inspector
  -> keep draft open
```

```text
draft body has non-whitespace text
  -> click Cancel
  -> discard draft intentionally
```

```text
draft body has non-whitespace text
  -> click Save
  -> persist note
```

## UI

The Inspector draft editor should show:

- Save as the primary action.
- Cancel as a secondary action near Save.

Cancel should not be visually dominant. It is a recovery action, not the main path.

Recommended labels:

```text
Save
Cancel
```

## Data Safety

Click-away dismissal must only discard drafts where the note body is empty after trimming whitespace.

The selected PDF quote and anchor do not make the draft "non-empty" for cancellation purposes. A draft with selected quote but no user-written note body can be dismissed quietly.

## Validation Plan

- Start a note draft from PDF text selection.
- Click outside before typing and verify the draft disappears.
- Start another draft, type whitespace only, click outside, and verify the draft disappears.
- Start another draft, type note body text, click outside, and verify the draft remains.
- Click Cancel on an empty draft and verify it disappears.
- Click Cancel on a non-empty draft and verify it is discarded.
- Press Escape on an empty draft and verify it disappears.
- Save a non-empty draft and verify the note persists.
- Run `pnpm check`.
- Run `pnpm build`.

## Open Questions

- Should Escape on a non-empty draft do nothing, or focus the Cancel button?
- Should non-empty Cancel require a confirmation later, or is explicit Cancel enough?
