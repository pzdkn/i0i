# RFC 0118: Durable Run Checkpoints and Structured Activity

- Status: Implemented and verified
- Date: 2026-09-02
- Area: Projects / Research Harness / Audit
- Parent: RFC 0108
- Depends on: RFC 0114, RFC 0117

## Summary

Make every completed Research Run a complete, inspectable checkpoint and make
Activity answer what happened, why, what changed, what the Run consumed, why it
stopped, and what comes next. Add typed event detail and safe same-Project State
restoration without introducing Project forks.

## Checkpoint Model

Extend `HarnessRun`/`ResearchCheckpoint` with immutable facts:

- starting and resulting Research State revisions;
- starting and resulting Project Vault membership revisions;
- applied change-set id;
- accepted and rejected candidate counts;
- added Paper ids;
- affected document id/revision pairs (empty until a permitted document-update
  feature exists);
- provider-query, LLM-call, iteration, and inspected-candidate usage;
- stop reason and completion/convergence distinction;
- reflection id and next direction; and
- creation/completion timestamps.

Vault membership receives a monotonic revision incremented in the same
transaction as membership changes. Project documents receive a monotonic
content revision so future Harness writes and current manual edits can be
identified without adopting full event sourcing.

## Structured Activity

`ResearchEvent` adds:

- stable typed kind;
- bounded human summary;
- optional structured detail JSON validated for that kind;
- phase and progress counts where applicable; and
- actor (`researcher`, `harness`, `scheduler`, or `system`).

Required event families cover Run lifecycle, queries/provider outcomes,
candidate inspection and decisions, Vault membership changes, Research State
changes, reconciliation failures/reviews, reflection/next direction, stop
decisions, recovery, and checkpoint restoration.

Events never store secrets, raw provider bodies, raw prompts, or unbounded text.
Existing string-only events migrate as `legacy` detail without losing order.

## Usage

The research loop returns its actual `BudgetUsage`. Search Run and Harness Run
persist the same values. Activity and Run detail label query/call counts rather
than claiming currency cost. A provider may add monetary usage later through a
separate pricing RFC.

## Restoration

`restore_research_checkpoint(project_id, run_id)` does not rewind or delete history. It
creates a new Research State revision whose active entry versions match the
selected checkpoint's resulting revision, records which later entries became
superseded by restoration, and appends a restoration event.

Restoration:

- requires the caller's active Project id and rejects a Run owned by any other
  Project before reading or changing Research State;
- requires an explicit confirmation in the UI;
- restores Research State only, not deleted Papers or old document content;
- is unavailable for Runs without a resulting State revision; and
- preserves all intermediate revisions for audit.

## Commands and UI

Add:

- `get_research_checkpoint(run_id)`;
- `list_research_checkpoints(project_id)`; and
- `restore_research_checkpoint(project_id, run_id, expected_current_revision)`.

Activity groups events under Runs. Selecting a Run shows configuration,
effective instructions, decisions, Papers, State changes, usage, reflection,
stop reason, and next direction. A valid completed checkpoint offers **Restore
Research State** with its exact effect and limitations.

## Non-Goals

- Project forking or branching.
- Restoring deleted source files or Paper metadata.
- Reverting arbitrary user-authored documents.
- Currency accounting without provider usage/pricing evidence.
- Storing raw debug logs as Activity.

## Acceptance Criteria

1. Every terminal Run exposes a complete checkpoint with State/Vault revisions,
   decisions, usage, reflection linkage, stop reason, and next direction.
2. Vault membership and document revisions advance transactionally on changes.
3. Structured events retain order and validate bounded type-specific detail.
4. Activity exposes query/provider progress, accept/reject reasons, State/Vault
   changes, usage, stop reason, and next direction without secrets.
5. Actual provider-query/LLM/iteration/candidate usage matches the bounded loop.
6. Restoration creates a new auditable State revision and never rewinds or
   erases later history.
7. Cross-Project, stale, incomplete, and missing-State restorations fail safely.
8. Run detail and restore UI work from persisted records after restart.
9. Migration, event, usage, checkpoint, restoration, and focused UI tests pass.
10. Full Rust tests, frontend type checking, and production build pass.

## Implementation Verification

- Search and Harness Runs persist exact bounded-loop provider-query, model-call,
  iteration, and inspected-candidate counts, including reconciliation calls.
- Vault membership and Project document content revisions advance through
  transactional SQLite triggers.
- Terminal Runs expose persisted checkpoint commands and UI detail; restoration
  requires the active Project id, rejects cross-Project Runs before State
  access, appends a new State revision, and records superseded later entries.
- Known structured Activity kinds validate their own bounded detail schema
  rather than accepting arbitrary JSON objects.
- Focused checkpoint, restoration, revision, structured-event, and UI-state
  tests pass. The Rust suite passes when localhost-dependent Obscura tests are
  excluded; frontend type checking and the production build pass.

## Approval

Approved on 2026-09-02 under the user's standing instruction to author,
approve, and implement each focused RFC needed to complete RFC 0108 without a
separate approval round.
