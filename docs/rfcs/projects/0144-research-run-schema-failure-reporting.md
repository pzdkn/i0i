# RFC 0144: Research Run Schema Compatibility and Failure Reporting

- Status: Implemented and verified
- Date: 2026-09-09
- Depends on: RFC 0143

## Problem

The first production Runs after RFC 0143 fail before making any tool call. The
model API rejects the synthesis output schema because each `operation`
discriminator uses `const` without declaring `type: string`.

The durable Run then records only `agent_failed`. Codex includes the actionable
provider error in the failed turn, but the controller discards it when handling
`turn/completed`, leaving the inspector unable to explain the failure.

## Decision

1. Declare every synthesis operation discriminator as a string with a constant
   value.
2. Capture the bounded error message supplied with a failed Codex turn.
3. Return that error through the existing controller failure path so it is
   logged and recorded as `agent_failed` activity before terminal finalization.
4. Keep user-visible errors bounded and do not persist raw provider responses.

## Non-Goals

- Retrying invalid schemas automatically.
- Changing the synthesis domain model.
- Adding provider-specific schema branches.
- Redesigning the Run inspector.

## Acceptance

- All three synthesis operation schemas contain both `type: string` and their
  expected `const` value.
- A failed `turn/completed` event exposes its provider message to the controller.
- The resulting Run activity contains the bounded actionable failure rather
  than only `agent_failed`.
- Focused regression tests cover schema discriminators and failed-turn parsing.
- Rust tests, Svelte checks, and Cargo checks pass.

## Verification

- Focused controller tests: 9 passed.
- Full Rust library suite: 617 passed, 11 intentionally ignored live tests.
- `pnpm check`: 0 errors and 0 warnings.
- `cargo check --no-default-features`: passed.
