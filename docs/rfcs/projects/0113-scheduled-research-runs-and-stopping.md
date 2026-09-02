# RFC 0113: Scheduled Research Runs and Stopping

- Status: Implemented and verified
- Date: 2026-09-02
- Area: Projects / Research Harness
- Parent: RFC 0108
- Depends on: RFC 0111, RFC 0112

## Summary

Extend the Project Research Harness from manual execution to persisted,
bounded scheduled Runs. Add deterministic due-time calculation, one bounded
startup catch-up, pause/resume/stop controls, and Project-level stop conditions
for cycle count, end date, consecutive unproductive Runs, convergence, wall
time, and cost-producing calls.

Scheduling is local-first. Runs execute while i0i is open. If a schedule became
due while the app was closed, the next launch may enqueue exactly one catch-up
Run and then advances to the next future occurrence. It never replays every
missed interval.

This RFC does not add model-driven Research State extraction, self-improvement,
or document generation. Scheduled and manual Runs use the same Harness Run
creation, immutable snapshot, SearchManager queue, State publication boundary,
and Activity ledger.

## Harness Configuration

Add these researcher-owned settings to the versioned Harness configuration.

### Schedule

`HarnessSchedule` contains:

- `enabled`;
- cadence: `daily` or `weekly`;
- local wall-clock time (`HH:MM`);
- optional weekday for weekly cadence;
- IANA timezone captured from the application environment;
- durable `next_run_at` UTC timestamp; and
- durable `last_scheduled_for` and `last_started_at` timestamps.

Monthly cadence is excluded initially because “the 31st” and month-end policy
need a separate product decision. Manual mode is represented by
`schedule.enabled = false`, not by a fake frequency.

The next occurrence is computed from cadence, local time, weekday, and timezone
using calendar arithmetic. It is never calculated by adding a fixed number of
seconds across daylight-saving transitions.

### Run and Loop Limits

`HarnessStopConditions` contains optional limits:

- `maximum_cycles`;
- `end_at` UTC timestamp;
- `maximum_unproductive_runs`;
- `maximum_run_seconds`;
- `maximum_provider_queries`;
- `maximum_llm_calls`; and
- `stop_on_convergence`.

The existing depth preset remains the default source of per-Run query/LLM
budgets. Explicit configured ceilings may only lower those budgets, never
increase them past the product-owned policy ceiling.

Provider query and LLM-call ceilings are the enforceable cost envelope in this
RFC. The UI labels them as call/query limits rather than claiming a currency
amount. Monetary cost is not shown or enforced until the configured provider
returns trustworthy per-request usage and prices.

## Harness Lifecycle

Harness lifecycle becomes:

```text
inactive → idle ↔ paused
             ↓
           running
             ↓
       idle | paused | stopped
```

- **Pause** prevents future scheduled Runs. It does not cancel the active Run.
  If Pause is requested during a Run, the Run finishes and the Harness settles
  in `paused`.
- **Resume** re-enables the persisted schedule and computes the next future
  occurrence. It does not immediately replay missed intervals.
- **Stop** prevents future scheduled Runs, records a user stop reason, clears
  `next_run_at`, and settles in `stopped` after any active Run is cancelled.
  A stopped Harness can be reactivated only by saving a new enabled schedule or
  explicitly choosing Resume.
- **Run now** remains available while idle or paused, provided no Run is active
  and Project-level terminal stop conditions have not fired. A manual Run does
  not shift the scheduled occurrence.

All transitions append Activity events. Pause, Resume, Stop, schedule changes,
and automatic stop-condition decisions retain actor, timestamp, and reason.

## Scheduler

Rust owns a single Project Harness scheduler attached to application startup.
It uses an injectable clock for deterministic tests.

On each bounded tick:

1. query persisted Harnesses whose `next_run_at <= now`, status permits
   scheduling, and no Run is active;
2. order them by due time and Project id;
3. claim at most one due Harness transactionally by creating its immutable
   Harness Run and advancing `next_run_at` to the first future occurrence;
4. enqueue the existing bounded SearchManager execution; and
5. emit the same UI events as a manual Run.

The scheduler never directly performs research and never bypasses
`create_harness_run`. A partial unique index continues to enforce one active
Run per Project under races between Run now and the ticker.

Cross-Project execution uses one global scheduled-Run queue. At most one
scheduled Harness Run is launched per tick; SearchManager's existing
single-search guard remains authoritative for search execution. This prevents
startup stampedes against Obscura, model APIs, or scholarly resolvers.

## Startup Recovery and Catch-Up

At startup:

1. Runs left active by an interrupted process are marked `failed` with
   `application_restarted`; no partial Research State revision is published.
2. Their Harnesses leave `running` and return to the persisted requested
   post-Run state (`idle`, `paused`, or `stopped`).
3. Each overdue enabled schedule is eligible for one catch-up Run.
4. Claiming that Run advances `next_run_at` directly to the first occurrence
   after `now`, skipping all other missed occurrences.

If several Projects are overdue, they remain durably due and are claimed one
per tick in deterministic order. Closing the app does not lose or duplicate a
claimed occurrence.

## Stop Evaluation

Stop conditions are evaluated before claiming a Run, during execution at hard
budget boundaries, and after completion.

### Before a Run

Do not enqueue when:

- maximum completed cycle count is reached;
- the configured end time has passed;
- the consecutive unproductive threshold is reached;
- the previous completed Run converged and `stop_on_convergence` is enabled;
- the Harness is paused or stopped; or
- another Run is active.

Terminal configured conditions set the Harness to `stopped`, clear
`next_run_at`, and append a structured `harness_stopped` Activity event. The
one-active-Run condition merely defers the due occurrence.

### During a Run

- Existing provider-query and LLM-call rails remain hard ceilings.
- The configured wall-time deadline is passed into the research loop and
  checked before each new phase or external call.
- Cancellation and timeout return explicit stop reasons.
- A timeout, cancellation, or failure never publishes a partial State revision.

### After a Run

A Run is productive when it adds at least one accepted Paper or commits at
least one meaningful Research State change. Discovery candidates alone do not
count. The Harness stores the resulting consecutive-unproductive count.

Convergence is a completed Run outcome, not inferred from an empty provider
response. When convergence stopping is enabled, it stops future Runs only
after the completed checkpoint is durable.

## Persistence

Persist the schedule and stop conditions inside the versioned Harness
configuration snapshot. Materialize current scheduling fields on the Harness
row for indexed due queries:

- `schedule_enabled`;
- `next_run_at`;
- `last_scheduled_for`;
- `requested_post_run_status`;
- `completed_cycle_count`;
- `consecutive_unproductive_runs`; and
- `terminal_stop_reason`.

Scheduled Harness Runs add `trigger = manual | scheduled | startup_catch_up`
and `scheduled_for`. A unique `(project_id, scheduled_for)` index makes one
occurrence idempotent across ticks and restarts.

Configuration edits during an active Run affect only later claims. The current
Run keeps its immutable schedule and stop-condition snapshot.

## Interaction

Extend the existing Research Harness Settings panel:

```text
SCHEDULE
Run [Weekly ▾] on [Monday ▾] at [09:00]
Timezone Europe/Berlin
Next Mon 7 Sep, 09:00

PER-RUN LIMITS
Inspect [10] Papers · Provider queries [8] · Model calls [12]
Wall time [20] minutes

STOP WHEN
[20] completed cycles or [3] unproductive Runs
End date [—] · ☑ Stop when converged
```

The Project header always shows current status, next occurrence, and applicable
controls:

- idle: `Run now`, `Pause`, `Stop`;
- running: `Pause after Run`, `Stop and cancel`;
- paused: `Run now`, `Resume`, `Stop`;
- stopped: terminal reason and `Resume`.

Activity distinguishes `Scheduled`, `Startup catch-up`, and `Manual` Runs. It
shows skipped intervals, stop decisions, recovery failures, current budget
consumption, and the next scheduled occurrence. The UI never implies that Runs
continue while the desktop application is closed.

## Commands

Add narrow commands:

- `pause_research_harness(project_id)`;
- `resume_research_harness(project_id)`;
- `stop_research_harness(project_id)`; and
- extend `save_research_harness` with validated schedule and stop conditions.

The scheduler uses store/service methods directly rather than invoking Tauri
commands internally.

## Non-Goals

- Running while the desktop application is closed.
- Operating-system launch agents, cloud workers, or push notifications.
- Replaying every missed scheduled interval.
- Monthly/custom cron schedules.
- Currency cost claims without trustworthy provider usage data.
- Multiple simultaneous Runs for one Project.
- Automatic Harness improvement proposals.
- Project forking or scheduled document publication.

## Acceptance Criteria

1. Daily and weekly schedules round-trip with timezone-aware next-occurrence
   calculation across daylight-saving changes.
2. Manual and scheduled Runs share the same immutable Harness Run path and
   one-active-Run constraint.
3. The scheduler claims due Projects deterministically, launches at most one
   scheduled Run per tick, and is idempotent under repeated ticks.
4. Startup recovery fails interrupted Runs without publishing partial Research
   State and performs at most one catch-up per overdue Project.
5. Pause, Resume, and Stop have the defined active/idle behavior and append
   durable Activity.
6. Maximum cycles, end date, unproductive streak, convergence, wall time,
   provider-query, and LLM-call limits produce explicit durable stop reasons.
7. A manual Run does not shift the next scheduled occurrence.
8. Editing Settings during a Run cannot mutate that Run's schedule or limits.
9. The Harness UI exposes schedule, next occurrence, limits, status controls,
   and the honest app-open execution boundary.
10. Pure clock/schedule tests, persistence tests, race/idempotency tests,
    recovery tests, stop-condition tests, and focused UI tests pass.
11. Rust tests, frontend type checking, and the production frontend build pass.

## Approval

Approved on 2026-09-02 under the user's instruction to author, approve,
implement, verify, and commit each focused RFC from RFC 0108 without a separate
approval round.
