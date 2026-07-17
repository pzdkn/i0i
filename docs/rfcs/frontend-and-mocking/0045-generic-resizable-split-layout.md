# RFC 0045: Generic Resizable Split Layout

Status: Implemented
Date: 2026-07-17
Product: i0i
Target: Svelte + Tauri v2, macOS first
Builds on: RFC 0001 (App Shell And Architecture), RFC 0003 (Workspace Tabs)

## Summary

Add a reusable Svelte split-pane layout component so i0i panels can be resized
like VS Code without each panel implementing its own resize logic.

The decision is:

- Build one generic `ResizableSplit` component.
- The layout component owns pointer handling, pane sizes, constraints, reset,
  and persistence.
- Individual panes only provide content and a small sizing config.
- Use this first for the main app layout:

```text
left workspace panel | center reader/PDF panel | right inspector/chat panel
```

Plain English version: panels should not know how to resize themselves. The
layout should know how to resize panes.

## Problem

Different i0i workflows want different screen real estate:

- reading PDFs wants a large center pane,
- discovery wants more left-panel width,
- notes/chat wants a larger inspector,
- small screens need tighter constraints.

If every panel implements custom resize behavior, the app will accumulate
duplicated pointer-event code and inconsistent behavior. We need one layout
primitive that panels can reuse.

## Goals

- Let users drag dividers between panes.
- Make the PDF/reader pane easy to enlarge.
- Allow left and right panels to shrink within sensible minimum widths.
- Persist pane sizes locally.
- Keep panel content dumb: no per-panel resize code.
- Support future reuse for other split layouts.

## Non-Goals

- No full workspace layout engine.
- No arbitrary floating/docking panels.
- No drag-to-reorder tabs.
- No nested split editor grid yet.
- No backend persistence for layout preferences.
- No responsive redesign of every panel in this RFC.

## Design

Create a reusable component:

```text
src/lib/components/layout/ResizableSplit.svelte
```

Pane config:

```ts
export type ResizePane = {
  id: string;
  min: number;
  max?: number;
  default: number;
  collapsible?: boolean;
};
```

Example use:

```svelte
<ResizableSplit
  orientation="horizontal"
  storageKey="main-layout"
  panes={[
    { id: "left", min: 260, default: 360 },
    { id: "reader", min: 480, default: 900 },
    { id: "inspector", min: 280, default: 360, collapsible: true }
  ]}
>
  {#snippet pane(id)}
    {#if id === "left"}
      <WorkspacePanel />
    {:else if id === "reader"}
      <ReaderView />
    {:else if id === "inspector"}
      <Inspector />
    {/if}
  {/snippet}
</ResizableSplit>
```

This is the Svelte equivalent of a trait-like contract:

```text
pane content + pane sizing config
```

The pane does not implement resize behavior. The parent split component does.

## Behavior

- Dragging a divider resizes the neighboring panes.
- Pane sizes respect `min` and optional `max`.
- The center reader pane should take remaining flexible space.
- Double-clicking a divider resets the layout to defaults.
- Sizes persist to `localStorage` under `storageKey`.
- Invalid persisted sizes are ignored and defaults are used.
- Keyboard/accessibility support should be basic but real:
  - divider has `role="separator"`,
  - divider exposes orientation,
  - arrow keys can nudge the split after focus.

## Main Layout Defaults

Initial layout:

```text
left:      min 260, default 360
reader:    min 480, default flexible / remaining
inspector: min 280, default 360, collapsible later
```

The reader should be the dominant pane. When the window grows, extra width
should primarily benefit the center reader/PDF area.

## Risks

- Pointer-event bugs can make dragging feel janky.
- Persisted layout values can break future UI if not validated.
- Too-small minimums can make panel content overlap or become unreadable.
- Too many options in the first component can overengineer a simple split.

Mitigation:

- Keep v1 horizontal-only if that is enough for the main app shell.
- Validate persisted sizes against current pane configs.
- Use fixed min widths and simple pixel sizes first.
- Add collapse buttons later if drag resizing works well.

## Implementation Plan

1. Add `ResizableSplit.svelte`.
2. Add a small TypeScript type for pane config if useful.
3. Replace the current main app grid columns with `ResizableSplit`.
4. Configure left, reader, and inspector panes.
5. Persist sizes with `localStorage`.
6. Add reset-on-double-click.
7. Add basic keyboard nudge support.
8. Run frontend validation.

## Validation

Manual:

```text
1. Drag left/reader divider and confirm Discover/Vault width changes.
2. Drag reader/inspector divider and confirm PDF can become larger.
3. Confirm panes do not shrink below minimum width.
4. Double-click divider and confirm defaults return.
5. Reload app and confirm sizes persist.
6. Resize app window and confirm layout stays coherent.
```

Commands:

```bash
pnpm check
cargo check
```

If implementation touches shared app shell behavior, also run:

```bash
cargo test
```

## Future Work

- Collapsible side panes.
- Layout presets: Reading, Research, Notes.
- Vertical split support.
- Nested splits inside the reader or inspector.
- Command-palette actions for reset/collapse/focus pane.

## Implementation Notes

Implemented in the RFC 0045 pass:

- added generic `ResizableSplit.svelte`,
- made the vault explorer/workspace shell resizable,
- made reader/PDF vs reader inspector resizable,
- made discover results vs discover inspector resizable,
- made vault paper list vs vault inspector resizable,
- persisted pane sizes in `localStorage`,
- added double-click reset and keyboard arrow nudging.

Validation passed:

```bash
pnpm check
cargo check
cargo test
git diff --check
```
