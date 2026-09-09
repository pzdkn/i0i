# RFC 0149: Readable Research Entry Links

- Status: Implemented; local interaction tests passed, visual verification pending
- Date: 2026-09-09
- Scope: relationship links in the Research Entry inspector

## Problem

The Derivation section in `ProjectResearch.svelte` displays relationship labels
beside raw `targetEntryId` values. Users cannot identify a premise from an opaque
identifier or follow it to its evidence.

The existing State snapshot contains entry statements, and `openEntry` already
loads entry details at the selected State revision. Reuse these capabilities.

## Design

Replace each raw-id line with a compact, navigable statement in the existing
right-hand inspector. Keep the relationship label as secondary text above it:

```text
Derivation

Derived from
  Adaptive clipping reduced divergence under synthetic noise.

Derived from
  The benefit disappeared under high momentum.
```

- Use the target entry's actual statement, never an LLM-generated title.
- Render it as a text-link-style button, clamped to two lines with wrapping.
  No cards, identifier chips, new panels, or graph viewer.
- Preserve the actual relationship: Derived from, Motivated by, Contests, etc.
  Keep the stored order and direction; do not relabel every link as a derivation.
- Reveal the full statement on hover and keyboard focus using the existing
  tooltip pattern. The accessible button name includes the full statement.
- Use existing typography, link colors, hover, and visible focus treatments.
  Links must fit narrow inspector widths without horizontal overflow.

Inline links keep provenance next to the claim. A separate graph or modal would
add navigation and visual complexity for this simple task and is out of scope.

## Interaction

Clicking a relationship opens the target in the same inspector, including its
statement, evidence, relations, and history. Do not open a new app tab or reset
the State list's filters, scroll position, or selected revision.

The existing Back control returns to the previous inspected entry after following
a relationship. From the initial entry it returns to the previous inspector tab
and restores focus to the originating list entry. Use a small local entry trail,
not a new routing system. Clear that trail on Project or State-revision changes.

After navigation, focus the detail heading. On Back, restore focus to the link
that initiated navigation when it is available. Relation buttons support Enter
and Space; there is no hover-only action.

Keep the current entry visible while loading another one. Prevent repeated
activation while that navigation is pending. A failed load keeps the current
detail and shows the existing error treatment; do not push failed navigation
onto the Back trail. Ignore stale responses after switching Project or revision.

## Data And Missing Targets

Resolve labels from the complete `researchState.entries` collection, not the
filtered visible list. Contested or superseded entries remain valid link targets.
Use the same selected State revision for both labels and fetched details, so
historical views never silently display a newer statement.

If a target is genuinely absent from that snapshot, show noninteractive
"Entry unavailable" with its relationship label. Do not expose its raw id or
silently substitute a current-revision entry. Identifiers remain internal keys.

No database migration, new Tauri command, model call, or metadata field is needed.

## Acceptance And Local Tests

- Relationship rows show their labels and target statements, never raw ids.
- A target hidden by list filters is still resolved and navigable.
- Click and keyboard activation open the correct entry at the selected revision.
- Back traverses followed entries and then restores the initial inspector/list focus.
- Missing targets, failed loads, repeated clicks, and stale responses do not
  navigate to incorrect entries or corrupt the Back trail.
- Historical and non-active entries preserve their existing read-only/lifecycle behavior.
- Long statements remain readable through tooltip/detail view and fit a narrow panel.
- Verify with focused component/navigation tests, local UI inspection, and
  `pnpm check`. No paid research end-to-end evaluation is required.

## Non-Goals

- Changing the Research State schema or relation semantics.
- Renaming entry ids or generating short titles.
- Redesigning evidence references, research reports, or the rest of the inspector.

## Implementation And Verification

- `ProjectResearch.svelte` resolves relation targets from the unfiltered State
  snapshot and opens details through the existing revision-aware bridge.
- Relation navigation retains a local detail trail, restores focus on Back, and
  ignores stale responses after Back, Project changes, revision changes, or unmount.
  Failed loads leave the current detail and trail intact.
- Local edits clear old-revision history while keeping the updated entry open.
  No backend command, database schema, model, or research-run behavior changed.
- `pnpm test:ui`: seven component tests pass using actual Svelte DOM and mocked
  Tauri IPC. Covers labels, filtered/missing targets, Back/focus, failed and
  duplicate requests, historical revisions, Project changes, and lifecycle edits.
- Nine existing `research-state-ui.test.ts` tests pass. `pnpm check` reports
  zero errors and warnings. No paid model calls or live research evaluations ran.
- The browser connector reported no available browsers. Narrow-panel layout,
  native keyboard activation, and tooltip positioning still need visual/browser
  verification; jsdom does not establish pixel layout correctness. Do not mark
  the repository Changelog complete until that check is done.

`vitest.config.ts` scopes the new DOM runner to `*.component.test.ts`; existing
Node tests retain their current runner. Vitest and jsdom are development-only
dependencies, not desktop runtime dependencies.
