# RFC 0111: Research Harness Configuration and Manual Runs

- Status: Implemented and verified
- Date: 2026-09-02
- Area: Projects / Research Harness
- Parent: RFC 0108
- Depends on: RFC 0109, RFC 0110

## Summary

Give every Project one persisted Research Harness with editable research
instructions and bounded search configuration. Add manual Run, cancellation,
immutable configuration snapshots, and append-only Activity. Reuse the existing
Deep Research `SearchManager` for query planning and discovery; the Harness is
the Project-scoped orchestration and audit layer around that engine.

This RFC does not yet extract typed Research State, schedule Runs, or propose
self-improvements.

## Model

`ResearchHarness` is created for existing and new Projects and stores:

- status (`inactive`, `idle`, `running`, or `paused`);
- research goal and editable Project research instructions;
- preferred and excluded query concepts;
- required browser discovery plus enabled OpenAlex/arXiv metadata resolvers;
- per-Run paper and search-depth budget;
- autonomy (`manual` in this RFC); and
- a monotonically increasing configuration version.

`ResearchRun` stores one immutable serialized configuration snapshot, the
product policy version, linked Deep Research search/run ids, lifecycle status,
timestamps, stop reason, and summary. `ResearchEvent` stores ordered Activity
records for that Run.

Only one Run may be active for a Project. Editing settings during a Run creates
the next configuration version and cannot change the active Run snapshot.

## Execution

Starting a manual Run:

1. validates that the goal is non-empty and no Run is active;
2. snapshots the Harness configuration;
3. creates a bounded Deep Research Search using the goal, editable instructions,
   query guidance, sources, and paper budget;
4. links the Harness Run to the Deep Research Search Run;
5. mirrors lifecycle changes into ordered Activity; and
6. finishes with an explicit stop reason or failure.

The product-owned planner policy remains fixed. Editable research instructions
and query guidance are passed as labelled user-owned context. Cancellation uses
the existing SearchManager cancellation token and records a cancelled Run.

## Interaction

Selecting **Research** in a Project opens a Project Research surface and the
right-side Harness inspector:

```text
Research State (empty until RFC 0112) | Details · Activity · Settings
                                      | Run now · Cancel
```

Activity shows durable phase records and linked search outcomes. Settings edits
goal, research instructions, preferred/excluded concepts, sources, depth, and
paper budget. The UI labels the effective product policy as read-only and saves
only Project-owned settings.

## Acceptance Criteria

1. Every Project has exactly one persisted Harness after migration and creation.
2. Harness settings round-trip and increment a configuration version.
3. A Run stores an immutable configuration snapshot and product policy version.
4. A manual Run delegates to the existing bounded Deep Research manager and
   records its linked search ids.
5. At most one active Run exists per Project.
6. Lifecycle changes and cancellation are reflected in append-only ordered
   Activity.
7. Editing settings cannot mutate an existing Run snapshot.
8. Project Research exposes functional Activity and Settings UI with Run and
   Cancel controls; the product system policy is inspectable but not editable.
9. Rust tests, frontend type checking, and the production frontend build pass.

## Approval

Approved on 2026-09-02 under the user's instruction to author, approve,
implement, verify, and commit each focused RFC from RFC 0108 without a separate
approval round.

## Verification

Implemented and verified on 2026-09-02.

- `cargo test` passed: 491 tests, 6 ignored live/manual tests.
- `pnpm check` passed with no errors or warnings.
- `pnpm build` completed successfully.
