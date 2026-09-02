# RFC 0122: Run Finalization at the Safe Boundary

- Status: Implemented and verified
- Date: 2026-09-02
- Area: Projects / Research Harness / Lifecycle
- Parent: RFC 0108
- Depends on: RFC 0117, RFC 0118, RFC 0121

## Summary

Make the persisted Run lifecycle match the autonomous pipeline. A scholarly
search becoming ready is not the end of a Harness Run: candidate decisions,
State reconciliation, operational reflection, usage accounting, and checkpoint
facts must finish before the Run becomes terminal and the Project may start its
next cycle.

## Problem

The current linked Search Run marks its Harness Run `ready`, records the stop
decision, and returns the Harness to `idle` before reconciliation begins. During
that window a scheduled or manual Run can start against incomplete State,
causing stale Change Sets and out-of-order checkpoints. Activity also says the
Run stopped before its Project changes and reflection occur.

## Decision

### Reconciling phase

When a linked Search Run finishes successfully, persist the Harness Run as
`reconciling`. This is an active, non-terminal phase. It blocks another Run and
appears in live Activity/progress alongside planning, searching, assessing, and
ranking.

Reconciliation, optional automatic Change Set application, exact usage
accounting, and operational reflection all accept only this phase (with
`ready` retained where needed for backward-compatible/manual test fixtures).

### Explicit finalization

Add one store operation invoked by the manager after post-search work:

1. require the linked Harness Run to be `reconciling`;
2. fill unchanged resulting State/Vault revisions when no mutation occurred;
3. set status `ready`, stop reason, and completion time;
4. append the terminal lifecycle and structured stop-decision events;
5. settle cycle/unproductive counters and configured terminal conditions; and
6. return the Harness from `running` to its requested post-Run state.

These changes commit in one transaction. Only after this operation does the Run
become a completed checkpoint or the scheduler become eligible to claim the
next cycle.

Failed and cancelled searches still terminalize immediately without applying a
partial State revision. Restart recovery treats `reconciling` like any other
interrupted active phase.

## UI

The Project header and Activity show `Reconciling Research State` as the live
phase. Run controls treat it as active. No new navigation or user decision is
introduced.

## Non-Goals

- Combining all network/model work into one database transaction.
- Retrying a failed model call indefinitely.
- Changing Manual/Propose review semantics; a validated proposed Change Set is
  a legitimate terminal Run result.

## Acceptance Criteria

1. Search readiness moves a linked Harness Run to `reconciling`, not `ready` or
   an idle Harness.
2. Another manual/scheduled Run cannot start while reconciliation is active.
3. Terminal Run and stop events occur after Change Set, usage, and reflection
   events in sequence order.
4. Finalization atomically records fallback resulting revisions, completion,
   counters, stop conditions, and requested post-Run Harness state.
5. Failed, cancelled, and restarted reconciling Runs terminate without partial
   State changes.
6. Checkpoints are advertised as terminal/restorable only after finalization.
7. Focused ordering, exclusivity, failure, recovery, and scheduler tests, Rust
   tests, frontend type checking, and the production build pass.

## Approval

Approved on 2026-09-02 under the user's standing instruction to author,
approve, and implement each focused RFC needed to complete RFC 0108 without a
separate approval round.

## Verification

Harness-linked Search Runs are attached before their worker starts. Successful
search completion enters the indexed active `reconciling` phase; Change Set,
usage, and reflection events precede atomic finalization and terminal Activity.
Focused tests prove exclusivity, event order, checkpoint availability, terminal
limits, and restart recovery. The full available Rust suite, frontend state
tests, type checking, and production build pass.
