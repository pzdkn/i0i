# RFC 0119: Research Workspace Conformance and Accessibility

- Status: Implemented and verified; runtime screenshots blocked by sandbox localhost policy
- Date: 2026-09-02
- Area: Projects / Research UI
- Parent: RFC 0108
- Depends on: RFC 0116, RFC 0117, RFC 0118

## Summary

Complete the intended Project Research interaction after the autonomous data
path is present. Align the Project header, Research State list, Harness
inspector, Run review, and keyboard behavior with the current design without
introducing new permanent product areas.

## Project Research Header

The header shows:

- Project title and concise goal;
- Harness status and current cycle;
- next Run with the honest app-open boundary; and
- always-visible `Run now`, `Pause/Resume`, and `Stop` actions.

The same compact status remains available from Project Vault and Documents and
returns to Research → Activity.

## Research State List

Add:

- visible counts for All and every semantic kind;
- recent-change sorting and optional Run/cycle filter;
- origin Run/cycle on each row;
- evidence, premise, and context counts labelled by meaning;
- lifecycle text for contested/superseded entries;
- empty/filter states; and
- selection controls for Create-from without turning the row into nested
  interactive elements.

Rows are keyboard navigable. Opening a row focuses the Details heading; closing
or returning restores focus to the same row. Epistemic state is always text,
never color-only.

## Harness Inspector

Keep exactly `Details | Activity | Settings`:

- Details shows evidence, derivation, context, immutable history, origin Run,
  and Experiment Idea promotion.
- Activity shows the current structured phase/progress, pending change-set and
  improvement review cards, grouped Run history, checkpoint detail, and
  restoration.
- Settings shows all RFC 0116 authority fields, schedules, sources, budgets,
  stop conditions, immutable configuration history, and effective instruction
  inspection.

No Research Loop, Checkpoint, Survey, or Improvement navigation node is added.

## Document Provenance

Generated documents keep the RFC 0115 banner and source-State navigation. The
Documents view also exposes local citation details without presenting generated
prose as evidence.

## Responsive and Empty States

The Research State remains the primary centre surface. At narrow widths the
Harness inspector may overlay or stack, but it must not reduce Research State
to a narrow side list. Empty Projects explain how to seed the Vault, configure
the Harness, or create a manual Research Entry.

## Verification

Add focused component/pure-state tests for filtering/counts/sorting, historical
selection, Run review actions, authority visibility, focus restoration, and
provenance navigation. Run the packaged Tauri UI where the environment permits
and capture the final Research, Activity, Settings, and generated-Document
states for manual inspection.

## Non-Goals

- A fourth permanent Improvements tab.
- Project overview documents.
- Survey/experiment permanent product sections.
- Replacing Reader Info/Notes/Chat.
- Visual rebranding unrelated to the interaction design.

## Acceptance Criteria

1. The Project header exposes goal, status, cycle, next Run, and primary controls.
2. The State list exposes counts, sort/filter, origin Run, meaningful provenance
   counts, lifecycle, and Create-from selection.
3. Keyboard focus moves into Details and returns to the originating row.
4. Activity shows live structured progress, review cards, grouped Run history,
   checkpoint detail, usage, stop reason, next direction, and restore action.
5. Settings exposes scope, exclusions, autonomy, Paper/document authority,
   configuration history, schedules, sources, budgets, and stop conditions.
6. Effective instruction layers are inspectable and only researcher-owned
   layers are editable.
7. Generated-document provenance and local citations remain navigable.
8. Research/Vault/Documents stay views under one Project; no rejected permanent
   navigation entities appear.
9. Focused interaction tests, accessibility checks, type checking, production
   build, and available runtime visual verification pass.

## Implementation Verification

- The Project header shows goal, Harness status/cycle, next-Run boundary, and
  lifecycle controls.
- Research State exposes semantic counts, recent/kind sorting, Run filtering,
  meaningful provenance labels, lifecycle text, and focus-managed Details.
- Activity groups structured events under Runs and exposes live progress,
  Change Sets, improvements, checkpoint facts, usage, stopping, next direction,
  and append-only restoration.
- Settings retain the three-tab inspector and expose the full authority and
  instruction model; generated Documents expose source-State and citation
  provenance.
- Six focused frontend state tests, Svelte type checking, the production build,
  and the available Rust suite pass. `pnpm tauri dev` could not bind `::1:1420`
  in this sandbox (`EPERM`), so runtime screenshots were not capturable here.

## Approval

Approved on 2026-09-02 under the user's standing instruction to author,
approve, and implement each focused RFC needed to complete RFC 0108 without a
separate approval round.
