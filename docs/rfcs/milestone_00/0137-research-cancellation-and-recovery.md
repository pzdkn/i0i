# RFC 0137: Research Cancellation and Recovery

- Status: Implemented; live interruption acceptance pending
- Date: 2026-09-06
- Parent: [Milestone 00, M00-09a](../../../milestones/milestone_00.md)
- Depends on: RFC 0135
- Implementation approval: granted for Milestone 00

## Outcome

Cancel, timeouts, app shutdown, and agent crashes terminate research honestly
without losing already committed papers, notes, or State changes.

## Lifecycle Contract

```text
starting -> running -> completed
starting/running -> canceling -> canceled
starting/running -> failed
previously active at app restart -> interrupted
```

Map these semantics onto existing persisted statuses with a terminal reason where
possible. Do not replace all old enums or reinterpret historical runs. A deadline
can end as canceled with reason `time_limit`; a crash is interrupted/failed as
appropriate, never successful completion.

The first Cancel action atomically closes admission for new run work, revokes its
MCP grant, signals child searches, and interrupts the Codex turn. Repeated calls
return current status without duplicate events. In-flight State/note commits
check run authorization inside their commit transaction: a commit serialized
before cancellation is preserved; one after it is rejected. Use a consistent
lock/transaction ordering to avoid deadlocks.

Allow a 5-second interruption grace period, configurable in the backend, then
terminate a nonresponsive owned agent process. If a reused process hosts other
active runs, record all affected runs as interrupted; do not leave them running
in the UI. Clean up owned tasks and endpoint grants.

Durable acquisition for already saved papers may continue through the normal
library queue (RFC 0134). It is not a surviving autonomous research task and
cannot make later State updates on behalf of the canceled run.

## Restart

On startup, mark formerly active runs interrupted and preserve their committed
results. Revoke old process/session credentials. Do not auto-resume interrupted
research; the user starts a new run from current State. Existing independent
scheduled runs may still follow their schedule, but must not replay an interrupted
run as though it had never committed anything.

## Verification and Acceptance

- Race tests cover Cancel versus State commit and completion, repeated Cancel,
  timeouts, and nonresponsive runtime shutdown.
- Inject process EOF and reopen the database to verify interrupted status,
  retained updates, and no automatic restart of the old run.
- Check active child searches end and revoked credentials cannot mutate data.
- Verify shared durable source acquisition is not accidentally deleted.
- Live interruption is an explicit test; offline race tests run routinely.

## Implementation Record

Implemented on 2026-09-06. Managed Runs now enter a persisted `canceling` state
under an immediate SQLite transaction. That transaction closes admission for
new notes, State updates, Vault additions, and child Searches while preserving
mutations that committed first. It also records cancellation once and marks
owned child Searches for cancellation. The controller immediately revokes the
Run's MCP grant, signals active Search workers, and interrupts Codex.

Codex interruption uses the configured five-second grace period. A process that
does not acknowledge interruption is terminated, process EOF is propagated to
every active Run, and a later Run starts a fresh process. Native app exit invokes
the same cleanup before closing the MCP endpoint. Startup recovery runs before
Search recovery, marks abandoned managed Runs interrupted, cancels their child
Searches, and leaves committed papers, sources, notes, and State untouched.

The Rust suite passes with 585 tests and 7 explicit ignores. Focused tests cover
cancel-versus-write ordering, repeated cancellation, completion winning a late
cancel, managed finalization interrupted by restart, retained sources, process
EOF, and forced shutdown of a nonresponsive fake runtime. `pnpm check` reports no
diagnostics. The installed-Codex live interruption check remains part of the
final explicit milestone acceptance run.
