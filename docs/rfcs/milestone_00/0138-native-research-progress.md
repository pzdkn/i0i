# RFC 0138: Native Research Progress and Evidence Navigation

- Status: Implemented; native acceptance pending
- Date: 2026-09-06
- Parent: [Milestone 00, M00-09b](../../../milestones/milestone_00.md)
- Depends on: RFCs 0130, 0136, 0137
- Implementation requires separate approval

## Outcome

The existing research surface shows what the agent is doing, updates the Vault
and State as changes commit, and lets users inspect the resulting evidence.
Keep RFC 0124's single instruction and Run/Cancel interaction.

## Interaction

| Situation | Visible behavior |
| --- | --- |
| Starting | Busy indicator and "Starting agent"; Cancel available |
| Active | Latest meaningful search, reading, or State-update activity |
| Incremental commit | Vault/State/notes refresh and committed counts update |
| Completed | Compact result summary and remaining question; older runs collapsed |
| Failed/interrupted | Readable reason plus retained result counts; Run starts fresh |
| Missing Codex/authentication | Concrete setup guidance and retry; never indefinite loading |

Use the right context panel for activity. Keep expanded tool details and history
secondary. Show observable tool actions and agent responses, not private reasoning
traces. No terminal, new control-room layout, or large settings form.

Subscribe to events before starting a run and fetch a persisted snapshot after
subscription; deduplicate by event sequence. On reconnect, refresh from storage
so a lost event cannot leave stale "running" state. Initial data fetch failures
must be visible rather than looking like an empty successful run.

Source references open the actual saved paper and navigate to the referenced page
or passage where supported. Missing historical source versions show an honest
unavailable state. Notes from RFC 0130 display agent authorship and exact quotes.

Retain user-edited research instructions during activity refresh. Cancel is
available only for an active run and reflects the canceling state. Do not
auto-open newly added papers or discard current panel sizes/selected tabs.

## Verification and Acceptance

- Focused UI tests exercise start, progress, completion, error, reconnect, and
  cancellation from realistic backend event sequences.
- Native Tauri acceptance starts a real research run, observes incremental Vault
  and State updates, opens evidence and an agent note, then cancels another run.
- Restart after an injected interruption shows retained results and no resumed
  old run. Missing authentication produces actionable status.
- Record desktop screenshots and test steps for the native checks, including
  compact and wide panel sizes with no overlap or inaccessible controls.
- Browser mocks alone do not satisfy native acceptance. Run RFC 0126 separately;
  its backend trace does not prove this UI works.

Keep changes within the current ProjectResearch/Reader and bridge/event flow.
Use existing icons, components, typography, and resizable panel conventions.

## Implementation Record

Implemented on 2026-09-06 through the existing `ProjectResearch` surface and
Tauri event bridge. Managed Run lifecycle and observable agent activity now
emit refresh events, while Vault and Research State mutations reuse their
existing update events. The panel presents one current status, compact outcomes,
collapsed technical history, actionable runtime failures, and a canceling state.

Persisted checkpoints expose the validated final agent outcome after restart.
Source-evidence controls open the saved paper and queue its cited PDF page until
the PDF.js page element exists. Refreshes are ordered and project-scoped, do not
overwrite edited instructions, and recover from missed events by reading the
persisted snapshot.

Focused Rust storage/controller tests, the Research UI helper tests, and
`pnpm check` pass. The milestone's native Tauri screenshots, live interruption,
and final on-demand acceptance suite remain pending; this RFC must not be marked
complete until those checks are recorded.
