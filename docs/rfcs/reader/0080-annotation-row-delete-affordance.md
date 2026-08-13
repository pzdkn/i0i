# RFC 0080: The annotation row's delete affordance

Status: Implemented (pending manual verification)
Date: 2026-08-13
Product: i0i
Target: Tauri v2 + SvelteKit (Svelte 5), macOS first
Builds on: RFC 0062 (annotations panel), RFC 0074 (sticky notes),
RFC 0079 (annotation lifecycle — §1 added deletion to the list).
Amends: RFC 0079 R1.2 — the two gestures it shipped are both wrong, in
opposite directions.

## Summary

RFC 0079 R1.2 gave every annotation row three ways to delete: a trailing `−`
button, `Delete`/`Backspace` on the focused row, and right-click. Two of the
three are wrong as shipped:

- **Right-click deletes on contact.** It reads as "the right-click menu doesn't
  work" because no menu appears — the row is simply gone. Everywhere else in
  this app right-click opens a menu (`PaperList.svelte:120`,
  `VaultExplorer.svelte:190`); here it is an unlabeled, unconfirmed destructive
  action.
- **The `−` button is a 24px-wide full-height bar** welded to the right edge of
  every row. It is the second-heaviest thing in the list after the title, and
  the list is not primarily a delete surface.

Both live in the same block of `ReaderInspector.svelte` and ship as one change.

**Out of scope:** confirmation dialogs for deletion (the menu *is* the
confirmation step — a second click on a labeled item), multi-select, undo.

---

## 1. Right-click deletes instead of offering a menu

### Diagnosis

`ReaderInspector.svelte:1405`:

```svelte
oncontextmenu={(event) => {
  event.preventDefault();
  void onRemoveHighlight?.(row.highlight.id);
}}
```

The handler is live — `onRemoveHighlight` is bound from
`ReaderView.svelte:1493` (`deleteAnnotation`), which is also why the `−` button
renders at all (it is behind `{#if onRemoveHighlight}`). So the bug report
"right click menu for deletion does not work" is precise about the symptom and
misleading about the cause: nothing is broken, there is just no menu. Right-click
destroys the annotation *and* its conversation (RFC 0079 R1.3) with no label, no
confirmation, and no undo.

The repo already has the pattern this wants. `PaperList.svelte:36-64` holds
`contextMenu = $state<{x, y, paperId} | null>`, opens it from `oncontextmenu`,
closes it on `svelte:window` click and on `Escape`, and renders a
`position: fixed` `.context-menu` with `role="menu"` and `.danger` styling for
the destructive item.

### Change

R1.1 Right-click on an annotation row opens a context menu at the pointer
instead of deleting. The menu carries the actions that already exist for a row:

| Item | Action |
|---|---|
| Open passage | `onOpenHighlight(id)` — same as clicking the row |
| Delete annotation (`.danger`) | `onRemoveHighlight(id)` |

R1.2 The menu follows `PaperList`'s mechanics exactly — `$state` holding
`{ x, y, highlightId }`, dismissed by a window click or `Escape`, `role="menu"`
with `role="menuitem"` children, `event.stopPropagation()` on the menu itself so
the opening click does not immediately close it. Do not extract a shared
component: two call sites with ~30 lines each is not an abstraction worth the
indirection (AGENTS.md, *Introduce Abstractions Sparingly*).

R1.3 `Delete`/`Backspace` on a keyboard-focused row keeps deleting directly.
That gesture is explicit, keyboard-only, and requires the row to already be
focused — it is not the accident right-click is.

---

## 2. The delete control is the loudest thing in the row

### Diagnosis

`.row-remove` (`ReaderInspector.svelte:1822`) is a `24px`-wide,
`align-items: stretch` bar with its own `1px solid var(--border)` — a full-height
button on every row, always visible, in a panel whose rows are `11px` text with
`7px 8px` padding. Next to it the color chip is `9px` and the badges are `12px`
icons. The heaviest element in a list of annotations is the one that removes
them.

### Change

R2.1 Replace the full-height bar with a small icon button positioned in the
row's top-right corner: `position: absolute; top: 3px; right: 3px`, a 16px hit
box, `Minus` at `size={11}`, no border, `color: var(--fg-3)`. The row wrapper
gains `position: relative`; the row itself gains right padding so a long title
cannot slide under the button.

R2.2 The button is revealed on row hover and on keyboard focus
(`.thread-row-wrap:hover .row-remove`, `.row-remove:focus-visible`), not painted
on every row at rest. The list at rest is a list of annotations; the delete
control appears where the pointer already is. It keeps its red hover state so
the destructive action still announces itself.

R2.3 `aria-label="delete annotation"` and the `title` stay as they are — the
control gets quieter, not less reachable.

---

## Task list

| # | Task | Ships alone | Size |
|---|---|---|---|
| 1 | R1.1–R1.3 context menu replaces destroy-on-right-click | yes | S |
| 2 | R2.1–R2.3 corner icon replaces the full-height bar | yes | XS |

Both touch the same ~40 lines of `ReaderInspector.svelte`; they land in one
commit.

## Risks

- **Right-click becomes two clicks.** Deliberate. The one-click path stays for
  the deliberate gestures (the corner icon, `Delete` on a focused row).
- **A hover-revealed control is invisible on touch and to a scanning eye.** The
  panel is a pointer surface on macOS today, and the context menu plus keyboard
  `Delete` cover the other two paths, so no gesture is lost.

## Verification

There is no Svelte component test harness in this repo — the `*.test.ts` files
under `features/reader/` all cover pure-logic modules. Verification is manual:

1. Right-click an annotation row → a menu appears at the pointer with **Open
   passage** and **Delete annotation**; the annotation is still there.
2. `Escape`, or a click elsewhere, dismisses the menu with nothing deleted.
3. **Delete annotation** removes the row and its conversation (RFC 0079 R1.3).
4. At rest, no row shows a delete control; hovering a row reveals a small `−` in
   its top-right corner; clicking it deletes that row.
5. Tabbing to a row and pressing `Delete` still deletes it.

## Success criteria

1. No gesture in the annotations panel deletes an annotation without either a
   labeled menu item or a deliberate press on a delete control.
2. The delete control is no longer the largest element in the row, and is absent
   from rows the pointer is not on.
