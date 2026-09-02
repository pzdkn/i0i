# RFC 0116: Harness Authority and Effective Instructions

- Status: Implemented and verified
- Date: 2026-09-02
- Area: Projects / Research Harness / Configuration
- Parent: RFC 0108
- Depends on: RFC 0111, RFC 0113

## Summary

Complete the researcher-controlled authority boundary for a Project Research
Harness. Add explicit research scope, exclusions, autonomy, Paper-addition
authority, and writable-document selection to the versioned configuration.
Persist every configuration version and the effective instruction stack used
by every Run so the researcher can inspect exactly what governed historical
work without exposing secrets or editable raw system prompts.

This RFC changes configuration and Run snapshots only. It does not yet apply
candidate or Research State changes; RFC 0117 consumes the authority snapshot.

## Configuration

Extend `HarnessConfiguration` with:

- `scope`: optional bounded topic/method/population scope;
- `exclusions`: explicit out-of-scope classes distinct from query vocabulary;
- `autonomy`: `manual`, `propose`, or `automatic`;
- `may_add_papers`: whether an applying Run may add accepted Papers to the
  Project's one Vault; and
- `writable_document_ids`: existing same-Project documents the Harness may
  update when a later focused RFC defines an update operation.

`excluded_concepts` remains query guidance. `exclusions` is a research-boundary
statement and is included in planning and reconciliation validation.

Autonomy means:

- `manual`: Runs may be started only by the researcher and produce a reviewable
  change set;
- `propose`: manual or scheduled Runs produce a reviewable change set; and
- `automatic`: manual or scheduled Runs may apply a validated change set within
  the recorded Paper/document authority.

A manual-autonomy Harness cannot enable a schedule. Changing authority while a
Run is active affects only later Runs because the active Run keeps its snapshot.

Writable document ids grant no operation by themselves. They are stored now so
future document-update RFCs cannot invent authority after a Run starts.

## Configuration History

Add immutable `HarnessConfigurationVersion` rows:

- Project id and version;
- complete normalized configuration JSON;
- actor: `researcher`, `improvement`, or `migration`;
- optional source improvement id;
- reason; and
- creation timestamp.

Initialization records version 1. Every Settings save or accepted improvement
inserts the next row in the same transaction that updates the current Harness.
Old versions are never rewritten.

## Effective Instruction Stack

Each Run stores an immutable `EffectiveInstructionStack`:

1. product policy identifier and human-readable policy summary;
2. Project research instructions;
3. normalized structured settings, including authority and limits; and
4. bounded Run context: starting Research State revision, current active-entry
   summaries, Project Vault Paper ids/revision, prior Run reflection/next
   direction, and remaining budget ceilings.

The stack contains no API keys, provider response bodies, or hidden chain of
thought. The product policy is identified and summarized, not editable. The
Run context is a typed snapshot, not a prompt transcript.

## Commands and UI

Add:

- `list_harness_configuration_versions(project_id)`;
- `get_harness_run_instructions(run_id)`.

Settings adds Scope, Exclusions, Autonomy, Add accepted Papers, and writable
document selection. Configuration history lists immutable versions. A Run
detail action opens the effective instruction layers with read-only product
policy and Run context and clearly labels the researcher-editable layers.

## Validation

- Scope/instructions/exclusions have bounded lengths.
- Browser discovery remains mandatory.
- Writable ids must be distinct existing documents in the same Project.
- Manual autonomy rejects an enabled schedule.
- `may_add_papers = false` is respected even in automatic mode.
- No authority setting may be changed by Harness Improvement proposals.

## Non-Goals

- Applying Papers or State changes.
- Letting the researcher edit the product policy.
- Persisting raw provider prompts or secrets.
- Giving the Harness permission to update arbitrary documents.
- Defining automatic document rewrite behavior.

## Acceptance Criteria

1. All authority fields round-trip, validate Project boundaries, and appear in
   immutable configuration snapshots.
2. Every current and future configuration version is durably inspectable.
3. Accepted Harness Improvements create attributed configuration versions.
4. Every Run stores the four-layer effective instruction stack it actually
   used, including bounded starting State/Vault/prior-reflection context.
5. Editing Settings cannot mutate an active or historical Run's instructions.
6. Settings and Run detail expose the intended controls and read-only layers.
7. Migration supplies conservative defaults: manual autonomy, no schedule, no
   automatic Paper authority, and no writable documents.
8. Focused persistence, validation, boundary, and UI tests pass.
9. Rust tests, frontend type checking, and the production build pass.

## Approval

Approved on 2026-09-02 under the user's standing instruction to author,
approve, and implement each focused RFC needed to complete RFC 0108 without a
separate approval round.

## Verification

Implemented on 2026-09-02. Persistence tests cover conservative legacy
migration, immutable configuration history and Run snapshots, Project-bounded
document authority, scheduled-autonomy validation, accepted-improvement
attribution, and the captured State/Vault/budget context. The Settings and Run
detail surfaces expose authority, version history, and all four effective
instruction layers. The full Rust suite passed with 513 tests and 6
live/integration tests ignored; frontend type checking and the production build
also passed. Runtime screenshot verification remains deferred to RFC 0119
because this environment cannot bind the local development server.

RFC 0120 subsequently connected this immutable context to discovery planning:
the linked search receives the exact starting State, prior next direction, and
bounded same-Project operational observations captured by the Run. The final
available suite passes 524 Rust tests with 6 ignored and 9 Obscura-filtered
tests, plus frontend state tests, type checking, and the production build.
