# RFC 0124: Simple Incremental Project Research

- Status: Proposed; implementation requires approval
- Date: 2026-09-03
- Area: Projects / Research Harness / UI
- Parent: RFC 0108
- Builds on: RFC 0111 through RFC 0122
- Should follow: RFC 0123

## Summary

Replace the exposed Research Harness control room with one understandable
workflow:

```text
write research instructions
  -> Run research
  -> accepted Papers enter the Project Vault
  -> evidence updates Research State
  -> the next Run starts from the larger Vault and newer State
```

The underlying bounded execution, provenance, validation, Run snapshots, and
checkpoints remain. They are implementation guarantees rather than a form the
researcher must configure.

## Problem

The current interface exposes the architecture of the Harness instead of the
researcher's task. Its Settings panel separately asks for a goal, researcher
instructions, scope, exclusions, preferred concepts, excluded concepts,
sources, depth, paper budget, autonomy, Paper authority, writable documents,
schedule, three per-Run limits, four stopping rules, product policy, and
configuration history.

Several controls overlap semantically. Their combinations produce cryptic
states such as a stopped Harness that must be resumed before a manual Run. The
Activity view gives routine lifecycle events the same weight as results and
failures. A disabled **Details** tab appears broken until the user discovers
that selecting a Research State entry enables it.

The intended product is considerably simpler: the researcher tells i0i what to
investigate, starts a bounded search, and receives a richer library and an
incrementally improved Research State.

## User Model

The default Project Research surface has only three concepts:

- **Research instructions**: what to investigate, prioritize, include, or
  avoid, expressed naturally in one field.
- **Run**: one bounded asynchronous attempt to improve this Project.
- **Research State**: the current evidence-linked understanding accumulated
  across successful Runs.

The Vault is always part of this loop. A successful Run may add accepted Papers
to the Project Vault; the user does not grant that fundamental behavior for
every Project.

## Interaction

### Default right panel

```text
RESEARCH

[ Research instructions                         ]
[                                                ]

Papers to find  [ - ] 10 [ + ]    [ Run research ]

CURRENT RUN
Searching 2 of 4 queries...
[ Cancel ]

LAST RUN
8 papers added · 5 findings · 2 questions
Completed 14:32

History (6) >
```

The instruction field and paper count are directly editable. **Run research**
saves changed instructions before starting, so there is no separate everyday
**Save settings** step.

While a Run is active, **Run research** is replaced by **Cancel**. There is no
manual **Stop**, **Pause**, or **Resume** control. Those words describe a
long-lived scheduler, not a single manual research action.

### Activity disclosure

Activity uses progressive disclosure:

1. show the active Run and its latest meaningful progress event;
2. otherwise show a compact summary of the most recent completed or failed Run;
3. place older Runs behind **History (N)**;
4. within an expanded Run, show its summary first and keep detailed lifecycle
   events collapsed behind **Technical activity**.

Routine configuration saves and inactive lifecycle changes do not appear as
research Activity. Failures remain prominent and name the failed stage.

### Research State details

The persistent inspector navigation has **Activity** and **Settings** only.
**Details** is not a disabled tab. Selecting a Research State entry opens a
contextual detail view in the same panel with a clear back action. Closing it
returns to the previously selected inspector view.

## Configuration Decision

### One canonical instruction

Replace these researcher-facing fields:

- goal;
- researcher instructions;
- scope;
- exclusions;
- preferred concepts; and
- excluded concepts

with one required `instructions` string. The field supports ordinary prose,
including explicit inclusion or exclusion requests. The planner derives its
focused queries from that instruction plus the current Project context.

Existing configurations migrate without losing text by composing non-empty
legacy fields into a readable instruction document once. Historical Run
snapshots keep their original typed configuration.

The Project's title remains its short identity; it is not duplicated as a
second research-goal field.

### Minimal visible settings

The default UI exposes only:

- Research instructions;
- Papers to find, as a bounded integer stepper/input; and
- optional scheduling in a collapsed **Schedule** section when scheduling is
  retained in the product.

Provider-query count, model-call count, wall time, convergence rules, source
resolvers, and search depth use tested application defaults. They remain
bounded backend configuration, not ordinary user decisions. A later expert-mode
RFC may expose them if real usage demonstrates a need.

All visible numeric values use native numeric inputs or steppers with explicit
minimum and maximum values. No numeric setting is represented by an unrestricted
text field.

### Automatic Project enrichment

Every successful Run automatically applies validated accepted Papers and
validated Research State changes to its own Project. Remove the visible
`may_add_papers`, autonomy, and writable-document controls from this workflow.

This grant is narrow:

- Papers may be added only to the current Project Vault;
- State changes must pass existing evidence and epistemic validation;
- the Run retains a checkpoint and Change Set explaining the update; and
- user-authored documents are never rewritten by this RFC.

The Research State therefore grows monotonically by revision. A later Run sees
the current Vault, current State, prior direction, and bounded prior observations
already captured by the existing Run context.

### Internal product policy

Remove **Product research policy** and the policy version from the normal UI.
The policy remains an internal, versioned orchestration contract required for
reproducibility and safety. It is not a setting and presenting it beside user
inputs incorrectly suggests that the researcher must understand or configure
it.

### Configuration history

Remove configuration history from the default Settings view. Add **Clear
configuration history** as a secondary destructive action in the Settings
overflow menu, with confirmation.

Clearing removes standalone historical configuration-version rows while
retaining:

- the current configuration;
- the monotonically increasing current version number; and
- immutable configuration snapshots already attached to historical Runs.

It does not delete Runs, Activity, checkpoints, Papers, or Research State.

## Lifecycle Corrections

The backend must reject no-op lifecycle transitions. In particular, stopping
an already stopped or inactive Harness must not mutate state or append another
Activity event.

For the simplified manual flow:

- idle or failed -> **Run research** is available;
- queued through reconciling -> only **Cancel** is available;
- cancelled, failed, or ready -> **Run research** is available again.

A failed Run never requires resaving instructions or explicitly resuming the
Harness. Scheduled lifecycle state may remain internally for compatibility,
but it must not obstruct a manual Run unless a Run is already active.

## Result Summary

Each terminal Run presents one human-readable summary rather than raw status
and configuration vocabulary:

```text
8 papers added to Vault
5 findings and 2 questions added to Research State
3 candidates rejected as out of scope
Next: compare rank sensitivity across adapter families
```

For a failure:

```text
Planning failed
The selected model returned no usable answer. Run again or choose another model.
```

Configuration version, policy version, raw IDs, JSON, provider counters, and
structured telemetry belong only in expanded technical details.

## Compatibility and Migration

1. Read current `HarnessConfiguration` records and compose their legacy
   instruction fields into the new canonical instruction.
2. Keep deserializing historical Run snapshots in their original shape.
3. Preserve existing Vault membership, Research State revisions, Runs,
   checkpoints, and evidence links.
4. Default every Project to automatic validated Paper and State enrichment.
5. Keep scheduler records dormant when scheduling is not exposed; do not delete
   scheduled history during this UI simplification.

The migration must be idempotent. It must not append a configuration version or
Activity event merely because the application starts.

## Tests

### Lifecycle tests

- Repeated stop on an inactive or stopped Harness is rejected or is a silent
  no-op and appends no event.
- A failed, cancelled, or completed Run can be followed immediately by another
  manual Run.
- Only one active Run is allowed.
- Cancel is available only for an active Run.

### Migration tests

- All non-empty legacy instruction fields survive in the canonical instruction.
- Migration is idempotent.
- Historical Run snapshots remain readable.
- Clearing configuration history retains the current configuration and Run
  snapshots.

### Incremental research tests

- A successful Run adds accepted Papers without a separate authority toggle.
- Validated State changes create exactly one newer revision.
- The next Run context includes the newly added Papers and current State.
- User-authored documents remain untouched.

### UI tests

- The default panel contains one instruction field, one paper-count control,
  and one primary Run action.
- Active progress replaces historical noise; older Runs are collapsed.
- Only an active Run shows Cancel.
- Selecting a State entry opens contextual Details without a disabled tab.
- Technical policy, raw configuration versions, and advanced limits are absent
  from the default surface.

## Acceptance Criteria

1. A new researcher can start a useful Run after entering one instruction and
   without understanding Harness lifecycle or authority terminology.
2. Successful Runs automatically and visibly enrich the Project Vault and
   Research State.
3. The latest State and Vault are inputs to the next Run.
4. Repeated inactive Stop actions cannot create history noise.
5. Activity defaults to current/latest information and discloses older or
   technical records only on request.
6. Details is contextual and never appears as an unexplained disabled tab.
7. Configuration history can be cleared without damaging Run reproducibility.
8. Numeric controls are bounded number inputs or steppers.
9. RFC 0123's reliable Run tests pass alongside migration, lifecycle, and UI
   tests for this RFC.
10. Frontend type checking and the production build pass.

## Implementation Approval

Not yet approved. This RFC records the simplification decision only.
