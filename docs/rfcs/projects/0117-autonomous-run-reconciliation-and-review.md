# RFC 0117: Autonomous Run Reconciliation and Review

- Status: Implemented and verified
- Date: 2026-09-02
- Area: Projects / Research Harness / Research State
- Parent: RFC 0108
- Depends on: RFC 0112, RFC 0113, RFC 0116

## Summary

Connect a completed bounded scholarly search to Project knowledge. A Run must
make constrained candidate decisions, materialize inspectable evidence for
accepted Papers, prepare typed Research State changes, and either stage or
atomically apply that change set according to its immutable autonomy snapshot.

This is the missing execution path that turns repeated searches into a growing,
evidence-linked Research State rather than a disconnected candidate inbox.

## Reconciliation Plan

After search ranking succeeds, a bounded reconciliation call receives:

- the exact Run goal, scope, exclusions, authority, and remaining budget;
- current active Research State summaries;
- ranked candidates from this Run only, with stable candidate handles;
- candidate metadata and bounded abstracts; and
- prior operational next direction as context, never as evidence.

It returns a typed `RunReconciliationPlan`:

- one decision per considered candidate: `accept` or `reject`, reason, and
  relevance confidence;
- proposed source-supported Findings quoting exact spans from accepted
  candidate abstracts;
- proposed Questions, qualified Gaps, Hypotheses, and Experiment Ideas;
- typed derivation/motivation relationships using existing or proposed entry
  handles; and
- a bounded next research direction.

The model can select only supplied candidate and entry handles. It cannot
invent a Paper, citation, source excerpt, or Project authority.

## Evidence Materialization

An accepted candidate with an abstract receives a canonical `metadata_abstract`
source and extraction inside the Project library. Its block/chunk text is the
exact provider-resolved abstract. A source-supported Finding must quote a
substring of that text and links to the resulting chunk. Abstract evidence is
visibly labelled as abstract evidence; it is not presented as full-paper
inspection.

Candidates without inspectable text may be accepted as bibliography entries
but cannot support source-supported Findings. Existing ready extractions are
preferred when the Paper already resolves canonically.

## Validation

Before persistence, Rust enforces:

- every candidate decision covers a supplied candidate exactly once;
- accepted count does not exceed the Run paper budget;
- rejected candidates record a non-empty bounded reason;
- scope/exclusion violations cannot be accepted;
- every evidence quote is an exact substring of its selected source;
- Findings with `source_supported` have resolvable evidence;
- synthesis/context/speculation retain the RFC 0112 epistemic constraints;
- gaps use bounded-search qualification;
- relations resolve within the same Project and plan; and
- the plan stays within entry/text/relation limits.

Invalid output is retried once with validation errors. A second failure marks
reconciliation failed without adding Papers or changing State.

## Application and Autonomy

Persist every validated plan as an immutable `HarnessChangeSet` with status
`proposed`, `applied`, `rejected`, `superseded`, or `failed`.

- Manual and Propose autonomy leave the plan proposed for researcher review.
- Automatic autonomy applies it only if its Paper-addition authority permits
  the accepted Papers.
- If current Research State changed after planning, applying supersedes the
  stale plan; it is never silently rebased.

Applying is one transaction that:

1. revalidates Run, Project, authority, and starting State revision;
2. upserts canonical Papers and adds allowed Vault memberships;
3. stores abstract source/extraction/chunks for cited accepted Papers;
4. inserts one Research State revision containing the typed changes;
5. records candidate decisions and next direction;
6. updates the Run's resulting State/Vault facts; and
7. appends structured Activity.

Failure rolls back the complete application. Rejection preserves the plan and
decision reason. A user may edit only bounded plan fields through the same
validator; they cannot attach unsupported citations.

## Commands and UI

Add:

- `get_harness_change_set(run_id)`;
- `apply_harness_change_set(id)`;
- `reject_harness_change_set(id, reason)`; and
- `edit_harness_change_set(id, patch)` for bounded researcher corrections.

Activity shows a review card with candidate accept/reject reasons and proposed
State entries. Automatic applications remain inspectable but require no click.
The Project Vault and Research State refresh after application.

## Non-Goals

- Claiming full-paper inspection when only an abstract was inspected.
- Executing experiments or writing documents.
- Cross-Project evidence or state changes.
- Unbounded model-generated prose.
- Automatically contesting/superseding existing entries in the first version;
  new potential contradictions are proposed for review.

## Acceptance Criteria

1. A ready Harness search always records a validated change set or an explicit
   reconciliation failure.
2. Candidate decisions are exhaustive, bounded, and retain accept/reject reasons.
3. Accepted Papers enter only the owning Project Vault and remain canonical.
4. Source-supported Findings resolve to exact inspected abstract/extraction
   text; unsupported claims fail before persistence.
5. Gaps, hypotheses, experiments, synthesis, and context retain their distinct
   epistemic status and qualification.
6. Manual/Propose Runs wait for review; Automatic Runs apply only their
   snapshotted authority.
7. Stale, rejected, failed, or cancelled plans create no partial Papers or State.
8. Successful application atomically advances State and records resulting Run
   provenance and Activity.
9. Fixed-corpus fake-planner tests cover acceptance, rejection, evidence,
   scope, stale plans, all autonomy modes, and rollback.
10. Focused UI tests, Rust tests, frontend type checking, and build pass.

## Approval

Approved on 2026-09-02 under the user's standing instruction to author,
approve, and implement each focused RFC needed to complete RFC 0108 without a
separate approval round.

## Verification

Implemented on 2026-09-02. The bounded reconciliation planner has one explicit
correction attempt, exhaustive candidate decisions, exact abstract-span
validation, Project and Paper authority checks, epistemic constraints, and
cycle-safe planned relations. Change Sets remain reviewable in Manual and
Propose modes and apply atomically in Automatic mode. Store tests cover
abstract materialization, exact evidence, successful application, rejection,
stale State, and transaction rollback. Fixed-corpus planner tests cover both
autonomy routing and the two-attempt validation boundary; focused UI tests
cover review counts and action availability. The full Rust suite passed with
524 tests and 6 live/integration tests ignored; frontend type checking and the
production build also passed.
