# RFC 0133: Bounded Search Tools

- Status: Implemented
- Date: 2026-09-06
- Parent: [Milestone 00, M00-06a](../../../milestones/milestone_00.md)
- Depends on: RFC 0128; reuses existing SearchManager
- Implemented: 2026-09-06

## Outcome

An agent starts several focused searches, observes incremental candidates, and
cancels work through MCP. Searches share a parent budget and return evidence
candidates without changing Research State.

## Contracts

| Tool | Input | Result |
| --- | --- | --- |
| `search_start` | Instructions, search parameters, optional model ID, request ID | Search ID, run ID, initial status |
| `search_get` | Run ID, optional cursor/limit | Status, latest activity, candidates, next cursor, errors, usage |
| `search_cancel` | Run ID | Current status and cancellation-requested flag |

Parameters include result limit (default 10, range 1..100), optional inclusive
year bounds, venue filter, and seed paper IDs. Instructions carry task purpose;
optional existing State entry IDs identify its motivation. Resolve models from
configured supported models; omitted model uses the application's default.
Do not add provider-selection settings to the everyday UI.

Reuse browser/provider search and ranking already implemented. Define venue
matching against normalized metadata, apply year bounds locally if necessary,
and report records excluded for unknown required metadata. Do not pretend every
provider supports every filter natively. Model query variants can run concurrently
within the shared limits. No Semantic Scholar reintroduction or new ranker here.

## Execution and Results

Use SearchManager for child search runs without attaching them to legacy
top-level reconciliation. Retry receipt and durable enqueue intent commit together;
a repeated start request returns the same run ID and never launches another job.
Recover an unstarted intent explicitly; do not report it as running forever.

Existing Search pools span runs. Expose this run's candidates and candidate
changes, not the full historical pool as if newly found. Persist candidate events
with monotonically ordered sequence IDs, reusing existing progress storage where
possible. `search_get` paginates those events up to a captured high-water mark;
clients upsert by candidate ID. Rank changes cannot skip or duplicate identities.
An empty active page means no new results, not completion.

Reserve shared provider-query and delegated-model-call budgets before dispatch.
Parent context is supplied by i0i. External grants get independent bounded task
budgets. Default child-search concurrency is 2, configurable in the backend.
Return readable limit status without dispatching unbudgeted work.

Cancellation prevents new queries and cancels owned requests where supported.
One provider failure retains other results and records the error. Repeated
cancellation does not append another lifecycle action. Child searches cannot
start top-level project research recursively.

## Verification and Acceptance

- Two delayed source fixtures demonstrate actual overlapping queries and a
  shared cap, including simultaneous budget exhaustion.
- Candidate events remain complete under reranking, polling, and partial errors.
- Start retries, cross-project run IDs, cancel repetition, and model/filter
  validation have focused tests.
- A real MCP client receives candidates while a run is still active.
- The controlled source boundary plugs into RFC 0126; live provider checks remain
  explicitly invoked. No results are saved to a Vault by this RFC.

## Implementation Notes

- `search_start`, `search_get`, and `search_cancel` are available through the
  authenticated project-scoped MCP endpoint and reuse `SearchManager`.
- Child starts are atomic and retry-safe. Managed child runs reserve their full
  provider-query and delegated-model allowances against the parent Run before
  dispatch; external searches retain their own bounded strategy.
- Candidate snapshots and activity are persisted per run. Poll cursors capture a
  stable high-water mark, then advance to candidates arriving while the run is
  still active. Explicit year and venue filters also apply to previews.
- Search cancellation records one durable request and becomes a no-op after the
  first request or after the run reaches a terminal status.
- The optional `model_id` accepts any non-empty OpenRouter model identifier, in
  line with the application's existing model configuration. Omitting it uses the
  configured planner/chat default.

Verification completed with 565 Rust tests passing, 7 explicit live/manual tests
ignored, and `pnpm check` reporting zero errors or warnings. Existing delayed
browser-source tests verify query overlap; new storage and real-MCP tests cover
budget reservations, retry identity, scope, cancellation, stable pagination, and
incremental candidates while active.
