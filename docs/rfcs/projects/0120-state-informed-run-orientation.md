# RFC 0120: State-Informed Run Orientation

- Status: Implemented and verified
- Date: 2026-09-02
- Area: Projects / Research Harness / Planning
- Parent: RFC 0108
- Depends on: RFC 0112, RFC 0114, RFC 0116

## Summary

Make every Research Run plan discovery from its immutable Project context, not
only from the original Project goal. Query planning must receive the current
Research State, the previous Run's next direction, and a bounded set of recent
operational observations captured in the Run instruction stack.

This closes the feedback loop: one Run's findings, gaps, and reflection can
change what the following Run searches for. Researcher-context entries may
guide queries but remain explicitly non-evidentiary.

## Problem

The current Run captures Research State and `prior_next_direction` in its
effective instructions, but the Deep Research search is created from Harness
configuration alone. The reconciliation step sees the accumulated context only
after discovery. Repeated Runs can therefore repeat the initial search rather
than pursue the recorded next direction or unresolved State.

## Decision

### Immutable orientation packet

Extend `EffectiveRunContext` with bounded recent operational observations. Each
observation contains only its typed kind and bounded description; raw prompts,
provider responses, and secrets are excluded.

The existing immutable context remains the source of:

- starting Research State revision and active entry summaries;
- Project Vault identity, membership revision, and Paper ids;
- previous next direction;
- provider/model/Paper ceilings; and
- recent operational observations from completed Runs in the same Project.

### Orient before discovery

When the Harness Run and its immutable instruction stack are created, update
the linked search goal in the same transaction with a deterministic orientation
packet rendered from that exact stack. The packet includes:

1. original goal, scope, exclusions, researcher instructions, and query
   vocabulary;
2. previous next direction, when present;
3. bounded active Research State grouped by semantic kind and epistemic status;
4. bounded operational observations; and
5. an explicit rule that `researcher_context`, notes, chats, documents, prior
   assistant output, hypotheses, and experiment ideas may guide queries but are
   not source evidence.

The search manager subsequently reads this persisted goal. There is no preview
or second context read that could diverge from the Run snapshot.

### Boundedness

- At most 100 active Research Entries, already bounded by RFC 0116.
- At most 20 recent operational observations.
- Entry and observation text is character-bounded before rendering.
- The rendered orientation packet has a fixed maximum size and fails Run
  creation rather than silently dropping the epistemic policy.

## UI

Run detail's read-only **Bounded context** section shows the number of active
State entries, recent operational observations, and whether a previous next
direction was supplied. No new settings or navigation node are introduced.

## Non-Goals

- Automatically importing all raw notes or chat transcripts into a Run.
- Treating Research State summaries as citations.
- Letting operational observations modify configuration without the existing
  Harness Improvement review path.
- Adding another model call solely for orientation.

## Acceptance Criteria

1. A Harness search is oriented from the exact immutable context stored on its
   Run before discovery starts.
2. The prior next direction and active Research State appear in the persisted
   search input and can affect query planning.
3. Recent same-Project operational observations are bounded, snapshotted, and
   visible in Run detail.
4. Other Projects' State, observations, and directions never enter the packet.
5. Researcher context and speculative material are explicitly labelled as
   query guidance rather than source evidence.
6. Editing State, settings, or reflections after Run creation does not mutate
   the persisted search input or effective instruction stack.
7. Legacy Runs deserialize with no observations.
8. Focused persistence/isolation/immutability tests, Rust tests, frontend type
   checking, and the production build pass.

## Approval

Approved on 2026-09-02 under the user's standing instruction to author,
approve, and implement each focused RFC needed to complete RFC 0108 without a
separate approval round.

## Verification

The linked search goal is now rewritten in the same transaction that creates
the immutable Harness Run, using that Run's exact State, next direction, and up
to 20 same-Project operational observations. Focused tests prove Project
isolation and immutability after later State/reflection edits. Run detail shows
the captured observation and direction counts. The full available Rust suite,
frontend state tests, type checking, and production build pass.
