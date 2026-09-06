# RFC 0131: Research State Read Contract

- Status: Implemented and verified
- Date: 2026-09-06
- Parent: [Milestone 00, M00-05a](../../../milestones/milestone_00.md)
- Depends on: RFCs 0128, 0129
- Implementation approved and completed on 2026-09-06

## Outcome

Expose the project's current understanding as findings, hypotheses, and questions
while preserving the richer provenance already stored by i0i.

## Tool Contract

`state_read(project_id, entry_ids?, cursor?, limit?)` returns a State revision,
current revision, entries with evidence and relationships, and a next cursor.
Use default 25/max 100 entries plus the shared response-size bound. Oversized
entry evidence lists use continuation rather than silent omission.

Each entry exposes `id`, `kind`, `statement`, `evidence`, and `related_entries`.
Also retain `epistemic_status`, `lifecycle`, `original_kind`, and revision/run
provenance. These are meaningful qualifiers, not additional concepts the user
must configure for every entry.

## Existing-Type Mapping

| Stored kind | Minimal kind | Preservation |
| --- | --- | --- |
| Finding | finding | Keep epistemic status and lifecycle |
| Hypothesis | hypothesis | Keep speculative or supported qualification |
| Question | question | Keep existing text |
| Gap | question | Preserve `original_kind: gap` and unchanged statement |
| ExperimentIdea | question | Preserve `original_kind: experiment_idea` and unchanged statement |

This is a projection, not a database migration that rewrites old entries. An
experiment idea's original wording need not be grammatically a question.

Evidence references reuse stored paper/source/extraction/chunk spans and excerpts.
Expose the existing support note as the explanation. Existing evidence lacks a
typed support/contradiction field: report legacy relationship as `unspecified`
until explicitly classified; do not infer support from the existence of a link.
New relationship storage is RFC 0132. Reader notes/chat remain contextual links
and cannot silently become primary source evidence.

Preserve existing relation kinds such as contests, derived-from, and supersedes
when projecting `related_entries`. No graph database is introduced.

## Revision Semantics

The first page selects one immutable State revision. Every continuation reads
that revision even if a new update arrives. Return current revision separately
so clients can detect changes. Historical evidence must resolve to its original
source version or report unavailable, never redirect to newer text.

## Verification and Acceptance

- Fixtures for every current entry kind retain text, provenance, qualifiers,
  context, and relationship meaning through the projection.
- A concurrent update does not mix revisions across paginated reads.
- Legacy evidence is explicitly unclassified rather than invented as support.
- State snapshots and evidence are available through real MCP with scope checks.
- This RFC exposes reads only; existing State mutation behavior remains intact.

## Implementation Notes

The scoped MCP endpoint now exposes `state_read`. The tool projects current
Research Entries to `finding`, `hypothesis`, or `question`, while returning the
stored kind, epistemic status, lifecycle, first/last revisions, origin Run, and
revision Run. Existing evidence keeps exact paper, source, extraction, chunk,
page, offsets, excerpt, and support note; its relationship is explicitly
`unspecified`. Entry relations and contextual links retain their stored kinds.

The first call materializes one immutable revision into an app-session snapshot.
Continuation cursors remain bound to the same grant and Project. Later pages
return that selected revision and independently report the latest current
revision, so a concurrent update is visible without mixing old and new entries.
Optional entry IDs are validated as a complete Project-scoped set.

A real MCP test covers every minimal projection category, verifies that a gap
retains `originalKind: gap`, and creates a concurrent State revision between
pages. The continued page remains on the original revision and reports the new
head separately. The existing Research State storage suite continues to cover
evidence, context, relations, histories, and source validation.
