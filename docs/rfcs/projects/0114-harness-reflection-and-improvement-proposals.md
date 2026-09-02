# RFC 0114: Harness Reflection and Improvement Proposals

- Status: Implemented and verified
- Date: 2026-09-02
- Area: Projects / Research Harness
- Parent: RFC 0108
- Depends on: RFC 0111, RFC 0112, RFC 0113

## Summary

Add bounded operational reflection to completed Research Runs and durable,
reviewable Harness Improvement proposals. Reflection describes how the research
process operated—query quality, source failures, irrelevant result classes,
coverage bias, scope drift, and wasted work—not what the Project knows about
the research topic.

Recurring compatible observations may produce a typed proposal that changes
one allow-listed Project setting. A proposal names its contributing Runs,
shows an exact before/after value, previews the expected effect, and remains
`proposed` until the researcher accepts, edits, or rejects it. The Harness does
not silently rewrite its configuration or system policy.

This RFC implements the “capture aggressively, promote conservatively” design.
It does not add general self-modifying prompts, arbitrary configuration JSON
patches, or autonomous acceptance.

## Operational Reflection

Every successfully completed Run may persist one immutable
`HarnessReflection`. Failure to produce a reflection does not turn an otherwise
successful research checkpoint into a failed Run; instead Activity records a
reflection failure.

A reflection contains:

- originating Run id and Project id;
- fixed reflection-policy version;
- bounded natural-language summary;
- zero or more typed `HarnessObservation` records;
- optional next-direction suggestion;
- timestamps; and
- the exact Run configuration version and operational metrics considered.

### Observation Types

Each observation has one of these kinds:

- `query_quality` — a formulation produced low relevance or useful recall;
- `irrelevant_result_class` — a recurring class of off-goal candidates;
- `source_failure` — an enabled resolver or browser path repeatedly failed;
- `terminology` — accepted Papers exposed useful or misleading vocabulary;
- `coverage_bias` — results repeatedly overrepresented a venue, method, era,
  or application class;
- `relevance_error` — acceptance/rejection behavior appears inconsistent with
  Project instructions;
- `scope_drift` — queries or accepted candidates moved outside the goal; or
- `wasted_work` — repeated searches yielded no meaningful additions.

An observation stores:

- stable normalized signature;
- severity and confidence;
- concise description;
- structured supporting metrics and query/source identifiers;
- relevant Harness setting target when one exists; and
- whether it is eligible to contribute to a proposal.

Observation text is generated analysis, not research evidence. It cannot be
linked as a source-supported Research Entry and never appears in the Research
State evidence count.

## Reflection Inputs and Boundary

The app-owned reflection policy receives only bounded operational material:

- planned and executed queries;
- provider/browser outcomes and failures;
- candidate acceptance/rejection aggregates and reasons;
- Run budgets, duration, and stop reason;
- before/after Research State counts and Vault membership counts;
- current researcher-owned query guidance and source settings; and
- prior unresolved observation signatures, not their free-form prose history.

It does not receive authority to issue tools or mutate state. The model returns
one schema-validated reflection object. Rust validates observation kinds,
setting targets, metrics, lengths, and proposal eligibility before persistence.

Notes, chats, Project documents, and Research State prose are not evidence for
an operational observation. They may shape Project instructions, but the
reflection must point to actual Run telemetry when claiming a repeated process
problem.

## Recurrence and Proposal Creation

Proposal detection is deterministic after a reflection is stored.

1. Group eligible observations by Project, normalized signature, and target
   setting.
2. Consider only successfully completed Runs with distinct Run ids.
3. Require at least three compatible observations within the most recent ten
   completed Runs.
4. Exclude observations already consumed by a still-pending or decided
   proposal for that signature and configuration lineage.
5. Ask the bounded proposal generator for one typed patch, or construct the
   direct set patch when the observation already names exact vocabulary.
6. Validate the patch against the allow-list and current configuration.
7. Persist one proposal and append `harness_improvement_proposed` Activity.

Repeated ticks or reopening Activity cannot create duplicate proposals. A
unique proposal fingerprint covers Project, observation signature, target,
base configuration version, and normalized after value.

## Harness Improvement

`HarnessImprovement` stores:

- id, Project id, and status: `proposed`, `accepted`, `rejected`, or
  `superseded`;
- one target setting;
- base configuration version;
- exact typed before and proposed-after values;
- rationale and expected effect preview;
- contributing Run and observation ids;
- proposal-policy version;
- optional researcher-edited after value;
- decision actor, reason, and timestamps; and
- resulting configuration version when accepted.

### Allow-Listed Patches

The first version permits one of these typed targets:

- `preferred_concepts` — add/remove normalized concepts;
- `excluded_concepts` — add/remove normalized concepts; or
- `metadata_resolvers` — enable/disable `open_alex` or `arxiv` while retaining
  required browser discovery.

The patch schema contains target-specific values, not a JSON Pointer or generic
map. Values are normalized, deduplicated, length-bounded, and shown exactly in
the UI.

The following are never improvement targets in this version:

- Project goal;
- researcher instructions;
- product system or reflection policy;
- evidence or epistemic rules;
- schedule, budgets, stop conditions, or autonomy;
- writable document permissions; or
- Research State entries and lifecycle.

## Decisions

### Accept

Acceptance is one transaction:

1. require proposal status `proposed`;
2. require the current configuration field to equal the proposal's exact
   before value and the base version to remain compatible;
3. apply the typed after value;
4. increment the Harness configuration version;
5. mark the proposal `accepted` with the resulting version; and
6. append ordered Activity containing the proposal and configuration ids.

An active Run keeps its old immutable snapshot. The accepted change is visible
only to the next Run.

### Edit and Accept

Edit changes only the target-specific proposed value within the same allow-list
and re-runs validation and preview generation. It does not expose an internal
prompt. The researcher sees the edited exact diff before a separate acceptance
action.

### Reject

Rejection requires an optional short reason, preserves the proposal and
contributors, and appends Activity. It does not suppress future observations
forever; a materially new recurrence after the rejection may create a new
proposal against a later configuration.

### Supersede

If the relevant configuration field changes before decision, the proposal is
not silently rebased. It becomes `superseded`, Activity explains the mismatch,
and a later recurrence may propose against the new value.

## Persistence

Add normalized tables for:

- `harness_reflections`;
- `harness_observations`;
- `harness_improvements`;
- `harness_improvement_observations`; and
- `harness_improvement_runs`.

Reflection and observation rows are immutable. Improvement status changes are
constrained transitions and decisions retain their history through Activity.
Deleting a Project cascades these records; deleting or editing a Project
document cannot affect them.

## Interaction

Pending proposals appear at the top of Research → Activity and add a badge to
the Activity tab:

```text
NEEDS REVIEW
Improve future search queries
Target: Preferred concepts

Observed in Runs 3, 4, and 6
“LoRA explanation” repeatedly favored application papers.

Before
interpretation, explanation

After
interpretation, mechanistic, subspace, intrinsic dimension

Expected effect
Preview: "LoRA mechanistic subspace analysis"

[Reject] [Edit] [Accept next Run]
```

The card opens contributing Run telemetry and individual observations. Before,
After, and Preview are separate labelled regions. Accept states explicitly that
the active Run will not change.

Decided proposals remain in chronological Activity with accepted/rejected/
superseded labels and the resulting configuration version. Settings shows
accepted improvements in configuration history; it does not add a permanent
fourth Improvements tab.

## Commands

Add narrow commands:

- `list_harness_improvements(project_id, status?)`;
- `get_harness_improvement(improvement_id)`;
- `edit_harness_improvement(improvement_id, proposed_value)`;
- `accept_harness_improvement(improvement_id)`; and
- `reject_harness_improvement(improvement_id, reason?)`.

Reflection generation and recurrence detection are internal post-Run services,
not UI commands. The bridge never accepts a generic configuration patch.

## Non-Goals

- Autonomous proposal acceptance.
- Editing product-owned system or reflection prompts.
- Free-form code, SQL, JSON Patch, or tool generation.
- Changing goals, evidence policy, schedules, budgets, autonomy, stop
  conditions, or document permissions.
- Treating operational observations as research findings.
- Cross-Project observation aggregation.
- A dedicated permanent Improvements navigation section.

## Acceptance Criteria

1. Completed Runs can persist bounded immutable reflections and typed
   operational observations; reflection failure leaves the research checkpoint
   intact and is visible in Activity.
2. Operational observations are stored and rendered separately from Research
   State and cannot satisfy source-evidence validation.
3. Proposal detection requires compatible observations from at least three
   distinct completed Runs and is idempotent.
4. Every proposal retains contributing Runs/observations, exact typed
   before/after values, rationale, preview, policy version, and base
   configuration version.
5. Only preferred concepts, excluded concepts, and optional metadata resolvers
   can be patched; all sensitive targets and browser removal are rejected.
6. Accepting a proposal atomically versions the Harness configuration without
   mutating active or historical Run snapshots.
7. Edited proposals remain target-validated; rejection and supersession are
   durable and append Activity.
8. Stale proposals cannot overwrite intervening configuration changes.
9. Activity exposes pending-review cards, contributing evidence, decisions,
   and configuration history without presenting an editable system prompt.
10. Reflection validation, recurrence, deduplication, patch allow-list,
    optimistic acceptance, lifecycle, and focused UI tests pass.
11. Rust tests, frontend type checking, and the production frontend build pass.

## Approval

Approved on 2026-09-02 under the user's instruction to author, approve,
implement, verify, and commit each focused RFC from RFC 0108 without a separate
approval round.

## Verification

Implemented and verified on 2026-09-02. RFC 0121 completed the production
path: reconciliation receives bounded actual telemetry, returns validated typed
operational observations, and persists exactly one reflection with Rust-owned
metrics. Three compatible production-style Run reflections create one
idempotent, reviewable improvement. The allow-list, recurrence, acceptance,
staleness, and Project-boundary tests pass.
