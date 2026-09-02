# Autonomous Research Projects — Implementation Audit

Date: 2026-09-02
Source of truth: [Autonomous Research Projects](autonomous-research-projects.md)

## Result

RFCs 0109–0122 now implement the Project/Vault/Documents foundation and the
complete bounded autonomous research loop. Completed scholarly searches create
validated candidate decisions and reviewable Change Sets, materialize inspected
evidence, reconcile typed Research State, retain durable checkpoints, and expose
the complete authority and audit surfaces in the Project Research workspace.
Each later Run is oriented by its immutable starting State, prior direction,
and bounded operational observations. A Run remains active until reconciliation,
usage, reflection, and checkpoint facts have crossed one safe finalization
boundary.

All available automated checks pass. The final Tauri screenshot pass could not
run because this sandbox denies localhost binding (`listen EPERM`); this is an
environment limitation rather than a product-test failure.

## Requirement-to-evidence matrix

| Design requirement | Current evidence | Audit result | Delivery RFC |
| --- | --- | --- | --- |
| Project owns exactly one Vault | Schema constraints, migration tests, RFC 0109 | Proven | — |
| Papers remain canonical across Vaults | Shared Paper rows and membership tests | Proven | — |
| Ordinary Markdown Project documents | Document CRUD/editor and RFC 0110 tests | Proven | — |
| Versioned Harness settings and immutable Run snapshots | Immutable configuration versions and per-Run effective instruction snapshots | Proven | RFC 0116 |
| Explicit instruction layers and inspectable effective Run instructions | Persisted policy/researcher/settings/context stack and Activity inspection | Proven | RFC 0116 |
| Scope/exclusions, autonomy level, and writable-document authority | Typed configuration, same-Project validation, Settings UI, and tests | Proven | RFC 0116 |
| Current State and prior learning guide the next discovery cycle | Linked search goal is rendered from the immutable Run stack with active State, prior next direction, and bounded same-Project observations | Proven | RFC 0120 |
| Manual and scheduled bounded Runs | Shared Run path, scheduler, recovery and stop tests | Proven | — |
| A Run cannot overlap its unfinished reconciliation | Indexed active `reconciling` phase blocks another Run until atomic finalization | Proven | RFC 0122 |
| Discover, inspect, accept/reject, add Papers, extract evidence, reconcile State | Bounded reconciliation planner, validated Change Sets, exact abstract evidence, and atomic application tests | Proven | RFC 0117 |
| Typed Research State with qualitative epistemic distinctions | Revisioned entries, validation, evidence/context separation and UI | Proven | — |
| Notes/chats/context never become evidence | Research State validators, separate context links, and reconciliation validation | Proven | RFC 0112 / RFC 0117 |
| Append-only structured Activity answering what/why/usage/next | Per-kind validated bounded events cover progress, decisions, mutations, usage, stopping, reflection, and restoration | Proven | RFC 0118 |
| Completed Run is an inspectable/restorable checkpoint | Persisted State/Vault revisions, decisions, usage, reflection, Run detail, and active-Project-validated append-only restoration | Proven | RFC 0118 |
| Operational reflection and reviewable improvements | Production reconciliation persists typed reflection from actual telemetry; three compatible Runs create one idempotent reviewable proposal | Proven | RFC 0114 / RFC 0121 |
| Terminal Activity and checkpoints occur after all post-search work | Focused ordering tests prove Change Set, usage, and reflection precede ready/stop events; recovery finalizes interrupted reconciliation | Proven | RFC 0122 |
| Create six ordinary documents from pinned State | RFC 0115 store/UI tests and provenance model | Proven | — |
| Project Research UI matches intended hierarchy and interaction | Counted/sortable Run-filtered State, focus-managed Details, grouped Activity/checkpoints, complete Settings, and provenance navigation | Proven by code and focused state tests | RFC 0119 |
| Full automated and runtime verification | 524 Rust tests pass with 6 ignored and 9 Obscura-filtered tests; the earlier full run identified only 5 localhost-dependent Obscura failures; 6 frontend state tests, type checking, and production build pass | Automated verification proven; runtime screenshots environment-blocked | RFC 0119 and final audit |

## Completion

RFCs 0116–0122 passed their focused acceptance checks. RFCs 0120–0122 close the
final audit gaps between persisted structures and the production autonomous
loop: context now drives planning, live Runs generate reflection, and Run
completion occurs only at the safe boundary. Parent RFC 0108 is implemented. A
future desktop session may capture the intended screenshots without reopening
product scope.
