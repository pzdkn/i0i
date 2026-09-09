# RFC 0145: Concurrent Research Activity Sequencing

- Status: Implemented and verified
- Date: 2026-09-09
- Depends on: RFCs 0135 and 0144

## Problem

Research Run activity uses a per-Run integer sequence with a unique constraint
on `(run_id, sequence)`. Event insertion currently performs two separate SQL
statements:

1. read `max(sequence) + 1`; and
2. insert the event with that sequence.

Reader delivery and Codex tool-lifecycle events can be written concurrently on
different SQLite connections. Two writers may therefore observe the same
maximum and attempt the same sequence. The resulting unique-constraint error
escapes the activity logger and fails an otherwise healthy Research Run.

The Run started at `2026-09-09 08:15:01` demonstrates this failure after ten
papers had been inspected. Its final successful event used sequence 88, then
the competing writer failed with:

```text
UNIQUE constraint failed: harness_events.run_id, harness_events.sequence
```

## Decision

Allocate the next per-Run sequence and insert the event in one SQLite write
statement. SQLite's single-writer serialization then makes sequence selection
part of the insert instead of leaving a read/write race between statements.

Keep the existing event table, ordering contract, ids, and caller API. Do not
introduce an in-memory mutex: events originate on multiple asynchronous paths,
and SQLite remains the durable source of ordering.

Activity persistence remains part of the Run's correctness contract. We will
fix sequencing rather than silently discard event-write failures.

## Non-Goals

- Redesigning the Activity UI.
- Reordering already persisted events.
- Making activity writes best-effort.
- Adding a second event queue or background writer.

## Acceptance

- Sequence allocation and insertion no longer have a SQL statement boundary.
- Concurrent writers can append events to one Run without a uniqueness error.
- Persisted sequences remain unique and strictly increasing for that Run.
- Existing event validation and transaction callers remain unchanged.
- A focused concurrent-writer regression test fails on the old implementation
  and passes on the new implementation.
- The full Rust suite, Svelte checks, and Cargo checks pass.

## Verification

- The synchronized 24-writer regression test reproduced the original unique
  constraint failure before the implementation change and passes afterward.
- Full Rust library suite: 618 passed, 11 intentionally ignored live tests.
- `pnpm check`: 0 errors and 0 warnings.
- `cargo check --no-default-features`: passed.
