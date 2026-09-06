# RFC 0135: Codex-Driven Project Research Loop

- Status: Implemented; fixed-corpus live acceptance pending
- Date: 2026-09-06
- Parent: [Milestone 00, M00-07](../../../milestones/milestone_00.md)
- Depends on: RFCs 0127 through 0134
- Implementation approval: granted for Milestone 00

## Outcome

The existing project Run action starts managed Codex with i0i's research tools.
One run can search, save papers, inspect real passages, and persist a justified
State update. i0i controls lifecycle; Codex selects the research actions.

## Application Boundary

Retain `run_project_research` as the public Tauri entry point. Move only the
necessary orchestration behind a backend service callable by Tauri and RFC 0126.
Starting returns the persisted HarnessRun promptly. Store runtime thread/turn IDs
and a run execution kind so historical runs keep their meaning.

Allow one active top-level run per project, enforced atomically in storage.
Snapshot current instructions, State revision, Vault membership context, model,
and limits before dispatch. Start a fresh Codex thread and a run-scoped MCP grant.
Child search runs use RFC 0133; they do not enter legacy harness reconciliation.

## Procedure Supplied to Codex

1. Inspect current State and Vault contents before deciding what is missing.
2. Choose targeted or broad research tasks and state their purpose in searches.
3. Save useful candidates, read relevant passages, and track coverage limitations.
4. Compare findings with existing entries, seek conflicting evidence, and update
   State only when warranted, using actual passage references.
5. Continue investigating unresolved questions within the remaining limits, or
   finish with a concise result and remaining questions.

These instructions guide behavior; they do not force every run to search first
or add a finding. Scope, reference integrity, revision conflicts, and execution
limits are enforced in Rust. Papers and notes remain inspectable even when a
later step fails. Do not discard validated updates at run completion.

## Limits and Finalization

Use existing backend configuration, not additional routine UI fields. Initial
defaults: 600 seconds per run, 6 child searches, 12 provider queries, 20 delegated
LLM calls, 10 distinct papers read, 120,000 returned source-text characters, and
2 concurrent child searches. Existing user paper target remains a separate
bounded desired result count. Record effective values in the run snapshot.

Reserve tool budgets before work. Re-reading a paper does not consume another
distinct-paper slot but does consume returned-text budget. Cached reads still
consume the agent context budget. Codex's own usage is recorded where available;
wall-time/turn interruption bounds runtime activity, not a promised exact dollar
cap. Questions and summaries later share delegated-call limits.

Map agent events into persisted observable progress. On normal completion,
finish only after owned child searches are terminal; cancel leftover searches
and record that action. Derive counts and changed references from committed
records, not the agent's claims about what it did. No-op research can complete
honestly. Agent failure is a failed run with preserved committed work.

Do not run the old final reconciliation after Codex already committed State.
Keep old run history and unrelated standalone search behavior intact. Existing
scheduled project runs must route through this controller with the same active
run check and limits; scheduling itself is not redesigned here.

## Verification and Acceptance

- Controller tests cover one active run, instruction/context snapshots, bounded
  dispatch, failed tools, retained commits, and one finalization path.
- Recorded State revisions and papers match the resulting run summary.
- Run RFC 0126's one-iteration scenario explicitly with real Codex/MCP.
- Evidence is read before it is used, and the final source-supported update
  resolves to inspected fixture passages.
- Routine test runs do not launch paid live evaluation. Crash recovery is
  completed in RFC 0137, and UI presentation in RFC 0138.

## Implementation Record

Implemented on 2026-09-06. `run_project_research` and scheduled claims now use
one `ProjectResearchController`, which starts a fresh scoped Codex thread and
returns the persisted Run before the turn completes. The controller snapshots
the model and effective limits, issues and revokes the run's MCP grant, records
observable tool lifecycle, cancels owned child searches before finalization, and
derives its summary from run-attributed Reader, Vault, Search, and State records.

Storage tests cover atomic singleton Runs, Reader and child-search limits,
run-attributed Vault additions, and preservation of committed State after an
agent failure. The full Rust suite passes with 574 tests and 7 explicit ignores;
`pnpm check` reports no diagnostics. The installed Codex 0.153.4 app-server also
passes the explicit initialization and shutdown smoke test.

The real fixed-corpus search-read-update scenario remains a milestone acceptance
gate. RFC 0126's production adapter and corpus are not yet implemented, so this
RFC must not be marked complete in the repository Changelog until that scenario
passes.
