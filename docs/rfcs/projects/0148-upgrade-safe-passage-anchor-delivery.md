# RFC 0148: Upgrade-Safe Passage Anchor Delivery

- Status: Implemented and locally verified
- Date: 2026-09-09
- Repairs: RFC 0147
- Scope: database upgrade and successful evidence delivery bookkeeping

## Confirmed Failure

The installed database has `agent_passage_anchors` without `source_version`.
RFC 0147's fresh schema includes that column, but its migration does not add it
to an existing table. `CREATE TABLE IF NOT EXISTS` cannot upgrade that table.

A read-only `EXPLAIN INSERT` reproduced the error without writing data:

```text
table agent_passage_anchors has no column named source_version
```

Run `harness_run_1788954773973226000` recorded 26 passage references. The entire
anchor table contained zero rows when inspected. Both `reader_read` and
`vault_ask` reach the same failing persistence path. This is an application
database regression, not evidence that the underlying papers are inaccessible.

There is a second defect: Reader delivery/citation records and success Activity
commit before anchor persistence. Consequently failed tool responses still
contribute to the "8/8 readable" summary. Evidence-question input accounting
also represents work attempted, not necessarily an answer delivered to the agent.

The report correctly states that Research State was unchanged. Its GAP/FINDING
display items describe operational problems; they are not committed State entries.

## Decision

### Upgrade Existing Databases

- Add the missing column through the existing `add_column_if_missing` mechanism.
- Use an empty legacy version marker for old rows; new deliveries always capture
  a real version. Do not fabricate historical versions from today's source data.
- Treat a legacy unversioned anchor as requiring a fresh read before new
  source-supported synthesis. Preserve historical records and existing evidence.
- Run the normal migration before recovery or evidence tools can access storage.
- Do not wipe the database or alter the user's previous Run outcomes.

### Commit Successful Delivery Together

Gather exact session anchors before entering storage. Within one transaction,
write successful passage/citation delivery, durable anchors, corresponding
successful-read accounting, and success Activity. Reuse the existing store bodies
with a shared connection; do not add a new general transaction abstraction.

An anchor persistence failure must roll back successful-delivery records and
events. Keep actual attempted reads and paid evidence-question work charged
against their limits, even if returning an answer fails. Derive successful
readability from completed durable deliveries, not reserved input text or attempts.
Keep unavailable-source read attempts valid for the existing retention rules.

Return the concrete storage failure at the tool boundary. No network retry,
additional model call, or source-download retry can repair a missing column.

## Local Verification

No paid end-to-end run is required for this repair.

1. Build a temporary database with the observed pre-upgrade anchor table, open
   it through normal initialization, and verify migration is idempotent.
2. Verify existing rows survive with an explicit unknown-version marker rather
   than invented provenance; newly delivered rows get real versions.
3. Exercise managed `reader_read` through real local MCP on that upgraded DB.
   Assert exact quotes, durable anchors, and successful delivery records agree.
4. Exercise evidence-question delivery with a deterministic answer fixture at
   the production accounting boundary; no live LLM is needed.
5. Inject an anchor-write failure. Assert no successful delivery/citation event
   or successful-read count leaks through, while actual incurred work remains
   accounted for and the original error is returned.
6. Run focused migration, MCP delivery, retention, and synthesis regressions.

## Acceptance

- Existing installations and new databases both support durable passage delivery.
- Reader and Vault evidence references can support synthesis after a successful
  read; legacy unversioned references cannot silently acquire invented provenance.
- A failed evidence tool cannot be reported as successfully delivering passages.
- No reset, manual production SQL patch, or historical Run rewriting is needed.
- Record local test results before marking implemented. Keep RFC 0147's separate
  live-acceptance limitation explicit; this repair does not claim to resolve it.

## Non-Goals

- Changing research instructions, providers, model selection, or synthesis retry limits.
- Redesigning GAP/FINDING presentation in the Run summary.
- Automatically rerunning the affected research investigation.

## Implementation And Verification

- Normal initialization adds `source_version` to existing anchor tables with
  an empty default for historical rows. Reopening twice is covered, and legacy
  unversioned anchors fail source-version validation rather than being backfilled.
- Reader attempts and evidence-question input/model costs remain charged
  separately. Successful passage records, anchors, and delivery Activity share
  a transaction for both Reader and question answers.
- Retention summaries and checkpoints count readable papers using versioned,
  durably delivered text. The terminal Run summary calls budgeted work
  "attempted evidence access" instead of claiming every attempt was a read.
- `passage_delivery_migrates_legacy_anchor_versions` reproduced the missing-column
  failure before the migration fix and passes afterward.
- `passage_delivery_rolls_back_reader_and_question_successes` injects an anchor
  insert failure, verifies zero success records/events/readable papers, verifies
  incurred costs remain charged, then verifies successful delivery after the
  injected failure is removed.
- The existing deterministic controller/MCP test now starts from the legacy
  anchor schema, upgrades it, and exercises real local HTTP Reader delivery and
  synthesis using a fake model process.
- Rust library suite: **637 passed, 11 ignored**. Cargo compile checks and
  `pnpm check` pass; Svelte reports zero errors and warnings. Diff checks pass.
- No paid model calls or live research evaluations were run for this repair.

Restart i0i with the updated build to apply the migration. Existing run outcomes
are not rewritten and investigations are not automatically rerun. RFC 0147's
separate live-acceptance limitation remains unchanged.
