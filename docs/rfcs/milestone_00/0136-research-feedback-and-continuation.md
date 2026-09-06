# RFC 0136: Research Feedback and Continuation

- Status: Implemented; controlled multi-run evaluation pending
- Date: 2026-09-06
- Parent: [Milestone 00, M00-08](../../../milestones/milestone_00.md)
- Depends on: RFC 0135
- Implementation approval: granted for Milestone 00

## Outcome

Later searches reflect what earlier reading established, and a new run benefits
from previous outcomes without depending on an indefinitely growing chat thread.

## Within a Run

After an evidence assessment or State commit, instructions ask Codex to identify
the remaining uncertainty and choose the next action. Include the committed
revision and changed entry IDs in tool results. Follow-up search instructions
explain the new question and can identify motivating State entries.

Do not create a second Rust planning loop that competes with Codex. The agent
already receives tool results in its context. Instrument observable searches,
passage deliveries, commits, and outcomes so RFC 0126 can verify causal order.

## Across Runs

Persist a compact outcome alongside the run using existing reflection/checkpoint
storage where practical. Request a structured final response with:

| Field | Meaning |
| --- | --- |
| Summary | What was learned, including failed or inconclusive investigation |
| Task outcomes | Child run IDs, motivating entry IDs, learned points and cited references |
| Unanswered questions | Specific uncertainties that remain |
| Next direction | Suggested next investigation and why it follows |

Validate IDs against the run's observable records. The summary is interpretation,
not new primary evidence and not a State mutation. Store it separately from the
immutable State revisions. If malformed or missing, keep computed run outcomes
and record summary failure; do not invent a reflection or undo committed research.

Each new thread receives current State, a bounded Vault overview, and up to three
recent run outcomes within a 12,000-character history budget. Backend settings
control those defaults. Prior directions may be stale; instructions explicitly
prioritize current evidence and user instructions over an old recommendation.
Use tools to obtain additional State or papers beyond the initial overview.

Avoid repeat searches when nothing changed, but permit justified repetition for
new evidence, broader coverage, or a prior external failure. "No result found"
must not become "this has never been studied."

## Verification and Acceptance

- Controlled multi-iteration evaluation shows a State commit followed by a
  meaningfully adapted search, then further reading and assessment.
- New-run evaluation starts a fresh thread that reads current State and uses
  previous outcomes without replaying an identical initial investigation.
- Judge relevance and adaptation separately from deterministic event order.
- Tests cover malformed final summaries, stale references, bounded history,
  and preservation of results when summary generation fails.
- No task-board UI or additional MCP mutation tool is required by this RFC.

## Implementation Record

Implemented on 2026-09-06. Managed Codex turns now request a structured final
outcome. i0i validates every reported child Search Run, motivating State entry,
and cited passage against records observed by that Run before retaining the
outcome in reflection storage. Missing, malformed, or stale output is recorded
as a summary failure without undoing papers, notes, or State revisions.

New Runs receive at most three recent validated outcomes within a 12,000-character
budget. Both bounds are backend preferences under `research.*`. The prompt gives
current instructions and evidence precedence over previous recommendations and
requires a reason before repeating an earlier search. Passage delivery and State
commit events provide deterministic ordering for the milestone evaluator.

The Rust suite passes with 580 tests and 7 explicit ignores, including stale
reference rejection, bounded continuation history, event ordering, structured
output parsing, and preservation after summary failure. `pnpm check` reports no
diagnostics. The controlled multi-iteration and fresh-thread scenarios remain
part of RFC 0126's final on-demand milestone evaluation.
