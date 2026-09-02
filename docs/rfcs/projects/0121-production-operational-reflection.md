# RFC 0121: Production Operational Reflection

- Status: Implemented and verified
- Date: 2026-09-02
- Area: Projects / Research Harness / Self-improvement
- Parent: RFC 0108
- Repairs: RFC 0114
- Depends on: RFC 0117, RFC 0120

## Summary

Make operational reflection part of the real autonomous Run path. The current
storage and review model is complete, but production Runs only create a generic
`wasted_work` observation when search adds nothing; the richer typed
observations and improvement proposals are reachable only through direct test
fixtures.

The validated reconciliation response will include a bounded operational
reflection based on actual Run telemetry. Rust persists it after reconciliation
and supplies trusted metrics itself. This reuses the existing bounded model call
and does not introduce another call or exceed the Run budget.

## Decision

### Typed model output

Extend `RunReconciliationPlan` with an optional backward-compatible
`operational_reflection` containing:

- bounded summary and next direction;
- up to 20 typed observations;
- signature, severity, confidence, and bounded description;
- optional allow-listed target and exact proposed value; and
- proposal-eligibility flag.

The model does not provide authoritative metrics JSON. Rust attaches the actual
queries, provider outcomes, candidate/decision counts, usage, and stop reason
when it persists the reflection.

### Telemetry input

The reconciliation prompt receives a bounded typed telemetry packet assembled
from the linked Search Run and structured Activity:

- executed queries;
- provider completion/failure counts;
- iterations and actual budget usage;
- stop reason; and
- candidate count.

No raw provider body, prompt transcript, API key, or unbounded error is passed.

### Persistence order

Completing search no longer writes a premature reflection. After reconciliation
and usage accounting, the manager persists exactly one reflection:

- validated model reflection when available;
- otherwise a deterministic telemetry-only fallback; or
- a visible reflection-failure Activity event if persistence fails.

The Research Run remains successful if reflection fails. `next_direction` from
the validated reconciliation plan remains available in the checkpoint even
when no reflection row exists.

### Real improvement path

Proposal-eligible observations use the existing RFC 0114 allow-list and
recurrence rules. Three compatible production Runs can therefore create a
reviewable Harness Improvement without test-only insertion. Goal, evidence
policy, schedules, budgets, autonomy, stop conditions, and writable Documents
remain forbidden targets.

## Non-Goals

- Autonomous acceptance of improvements.
- A second model call for reflection.
- Topic claims or Research State entries derived from operational telemetry.
- Persisting raw prompts or provider responses.

## Acceptance Criteria

1. Production reconciliation can return all RFC 0114 observation kinds and an
   allow-listed exact proposed setting value.
2. Rust validates reflection bounds, enum values, target/value compatibility,
   and the sensitive-target prohibition before persistence.
3. The prompt contains bounded actual queries, provider outcomes, usage, stop
   reason, and candidate counts without raw bodies or secrets.
4. Exactly one reflection is persisted after reconciliation and actual usage
   accounting; no premature generic reflection is created.
5. Invalid/missing model reflection produces a deterministic fallback or a
   visible failure without invalidating the research checkpoint.
6. Three compatible production-style reconciliation reflections create one
   idempotent reviewable improvement under the existing recurrence rules.
7. Reflection next direction reaches both the checkpoint and the following
   Run's orientation packet.
8. Focused validation, sequencing, recurrence, fallback, and isolation tests,
   Rust tests, frontend type checking, and the production build pass.

## Approval

Approved on 2026-09-02 under the user's standing instruction to author,
approve, and implement each focused RFC needed to complete RFC 0108 without a
separate approval round.

## Verification

Reconciliation now receives bounded persisted telemetry and returns a validated
optional operational reflection with the existing RFC 0114 observation and
patch types. Rust supplies authoritative metrics, persists ordered observation
Activity, and falls back safely when the model omits reflection. A production-
style recurrence test proves three compatible Run reflections create exactly
one reviewable improvement. The full available Rust suite, frontend state
tests, type checking, and production build pass.
