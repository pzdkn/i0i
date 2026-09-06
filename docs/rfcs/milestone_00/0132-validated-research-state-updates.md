# RFC 0132: Validated Research State Updates

- Status: Implemented and verified
- Date: 2026-09-06
- Parent: [Milestone 00, M00-05b](../../../milestones/milestone_00.md)
- Depends on: RFC 0131
- Implementation approved and completed on 2026-09-06

## Outcome

Agents can add or revise understanding atomically with evidence references and
an inspectable history. Concurrent or retried requests cannot lose or duplicate
updates.

## Tool Contract

`state_update(project_id, base_revision, request_id, changes)` returns the new
revision and affected entry IDs. Changes are a bounded list, initially at most
20, each with a reason and one operation:

| Operation | Required content |
| --- | --- |
| Create | Local operation key, minimal kind, statement, epistemic status, evidence, relationships |
| Revise | Existing entry ID, statement, epistemic status, complete desired evidence/relationship lists |
| Set lifecycle | Existing entry ID and active/contested/superseded status |

Create results map operation keys to assigned entry IDs. Relationships in this
first version target existing entries; link newly created entries in a later
request after obtaining IDs. There is no hard-delete operation.

Each new source evidence link includes a passage reference, `supports`,
`contradicts`, or `context`, and an explanation. Existing links can be retained by
ID, including legacy unclassified links. Revising an entry preserves its stored
original kind; this API does not silently turn a stored Gap into a Question.

## Validation and Commit

Reuse the existing LibraryStore/reconciliation validation for scope, provenance,
and epistemic rules. Add explicit evidence-relationship storage without guessing
old classifications. Resolve references to exact excerpts and source versions.
For managed runs, verify newly cited passages were delivered by a reading or
evidence tool before the update. A question or speculative hypothesis need not
have evidence; source-supported findings must satisfy the source-evidence rules.

An abstract can support a limited interpretation with abstract-only provenance.
If the current evidence schema only supports extraction chunks, add a tagged
metadata-abstract source reference; do not manufacture a full-text chunk ID.
Notes, model answers, and task summaries stay context, not primary evidence.

Check retry receipt first: identical principal/request/payload returns its prior
result even when base_revision is now old. A different payload under the same ID
is a conflict. For a new request, compare base_revision and validate every change
inside one transaction. Commit all changes, one revision, and the retry receipt
together. A failure commits none. A stale revision returns current revision;
the caller must read and reconcile rather than overwrite automatically.

Rust verifies structure and source integrity, not scientific entailment. The
research procedure and RFC 0126's judge evaluate interpretation quality.

## Verification and Acceptance

- Supporting and conflicting evidence coexist without losing qualifiers.
- Invalid references, scope violations, or one invalid operation reject the
  entire batch without advancing revision.
- Concurrent writes from one base revision permit only one new commit.
- Lost-response retries return the original IDs/revision; changed payloads fail.
- Reopening the store retains original entry versions and source references.
- Existing user-authored entries and legacy evidence remain readable.
- Successful updates emit the app's State refresh event after commit.

## Implementation Notes

The scoped MCP server now exposes `state_update` for bounded create, revise, and
lifecycle batches. The LibraryStore validates the complete batch, acquires an
immediate SQLite write transaction, creates one State revision, writes all entry
versions and typed evidence relationships, advances the head, and records the
retry receipt atomically. Competing writers therefore observe a revision
conflict instead of overwriting one another.

Passage evidence must have been issued to the same connection grant and must
still resolve inside the Project Vault. Provider abstracts are materialized as
explicit `metadata_abstract` sources and chunks, preserving their limited
provenance without presenting them as PDF full text. Existing evidence can be
retained by ID during revision, including legacy `unspecified` relationships.

Receipt lookup happens before ephemeral passage resolution. An identical retry
therefore returns its original revision and IDs even after the MCP server
restarts; a changed payload under the same request ID fails. Successful new
commits emit `research_state_updated` for the native UI.

Focused Store and real-MCP tests cover typed supporting, contradictory, and
context evidence; abstract evidence; rollback of an invalid batch; simultaneous
writes from one base revision; restart retries; changed retry payloads; reopening
the database; and revision/lifecycle updates that preserve the original kind.
