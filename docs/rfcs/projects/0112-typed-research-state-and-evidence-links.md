# RFC 0112: Typed Research State and Evidence Links

- Status: Implemented and verified
- Date: 2026-09-02
- Area: Projects / Research State
- Parent: RFC 0108
- Depends on: RFC 0109, RFC 0110, RFC 0111

## Summary

Add a durable, revisioned Research State to each Project. Research State is an
interactive structured index of Findings, Questions, Gaps, Hypotheses, and
Experiment Ideas rather than an indefinitely growing document. Every entry has
an explicit epistemic status, derivation links, and revision history.

Source-supported entries must cite resolvable evidence from canonical Paper
extractions. Notes, chats, Project documents, prior assistant output, and
Harness instructions may be recorded as researcher context or motivation, but
cannot satisfy the evidence requirement for a factual claim.

This RFC delivers storage, validation, commands, and the Project Research UI
for manually creating and inspecting Research State. It also lets a completed
Harness Run atomically publish a prepared Research State revision. Automated
model extraction and reconciliation are deliberately deferred to the next
execution-focused RFC so this epistemic boundary can be verified independently.

## Domain Model

Each Project has a monotonically increasing Research State revision number.
Revision `0` is the empty initial state. A committed change creates the next
revision and records its originating Run when applicable.

### Research Entry

A `ResearchEntry` has:

- stable id and owning Project id;
- semantic kind: `finding`, `question`, `gap`, `hypothesis`, or
  `experiment_idea`;
- epistemic status: `source_supported`, `agent_synthesis`,
  `researcher_context`, or `speculative`;
- concise text;
- lifecycle state: `active`, `contested`, or `superseded`;
- the State revision in which it first appeared and last changed;
- optional originating Run id; and
- timestamps.

An entry id remains stable across revisions. Editing, contesting, or
superseding an entry appends an immutable `ResearchEntryRevision`; history is
never overwritten.

### Evidence Links

A `ResearchEvidenceLink` belongs to one entry revision and stores:

- canonical Paper id;
- source id;
- extraction id and chunk id;
- a bounded quoted excerpt copied at commit time;
- source offsets and page range copied from the chunk; and
- an optional short support note.

The durable copied locator and excerpt preserve auditability if derived chunks
are later regenerated. The link must resolve to a chunk belonging to the named
Paper, source, and extraction at creation time. The Paper must belong to the
Project Vault at commit time.

### Derivation and Context Links

An `EntryRelation` connects an entry revision to another Research Entry using
one of these typed relations:

- `derived_from`;
- `motivated_by`;
- `contests`; or
- `supersedes`.

Researcher context is stored separately as a `ResearchContextLink` with a typed
origin such as `reader_note`, `chat_turn`, `project_document`, or
`project_instruction`. It may motivate any entry, but it is never counted as
source evidence and never appears in the citation count.

## Epistemic Invariants

Validation occurs in Rust inside the same transaction that commits a State
revision.

1. `source_supported` is permitted only for a `finding` and requires at least
   one valid `ResearchEvidenceLink`.
2. Evidence links may target only canonical Paper extraction chunks from the
   same Project Vault.
3. `agent_synthesis` requires at least one `derived_from` entry relation. Its
   component source links remain visible; the synthesis itself is not rendered
   as a direct source quotation.
4. A `gap` must use `agent_synthesis` or `speculative`, retain its bounded-search
   qualification, and cannot be `source_supported`.
5. A `hypothesis` or `experiment_idea` must be `speculative`.
6. `researcher_context` entries and context links never satisfy evidence or
   derivation requirements for `source_supported` or `agent_synthesis`.
7. Repetition, multiple Runs, or assistant authorship never promotes epistemic
   status.
8. Contesting and superseding append new history; no accepted revision is
   deleted or silently rewritten.
9. A failed or cancelled Run cannot publish a partial State revision.

## Revision Transactions

One Research State mutation is committed atomically:

1. load the Project's current revision;
2. validate all entry, evidence, relation, and context changes against that
   revision and current Vault membership;
3. reject the write if the caller's expected revision is stale;
4. create the next `ResearchStateRevision`;
5. append entry revisions and their links;
6. update the materialized current-entry rows; and
7. append ordered Harness Activity describing the changes when a Run is the
   origin.

The current list is materialized for simple queries. Immutable revisions and
links provide history without reconstructing all current State through event
sourcing.

A completed Harness Run records both `starting_state_revision` and
`resulting_state_revision`. Publishing a prepared mutation and marking the Run
ready occur in one transaction. Manual edits create a revision without a Run.

## Commands

Add narrow Project-scoped commands:

- `get_research_state(project_id, revision?)` returns one revision and its
  visible entries;
- `get_research_entry(entry_id, revision?)` returns content, evidence,
  derivation/context links, and revision history;
- `create_research_entry(project_id, expected_revision, draft)`;
- `revise_research_entry(entry_id, expected_revision, draft)`; and
- `set_research_entry_lifecycle(entry_id, expected_revision, lifecycle,
  reason)`.

Drafts use distinct typed arrays for evidence, entry relations, and researcher
context. The bridge does not expose a generic unvalidated JSON patch.

## Interaction

Replace RFC 0111's empty Research State placeholder with the main-pane list:

```text
RESEARCH STATE                         : search current understanding…
[All] [Findings] [Questions] [Gaps] [Hypotheses] [Experiment ideas]

FINDING · SOURCE-SUPPORTED · Cycle 3
Rank allocation is usually treated as an efficiency problem…
3 sources                                                   [Open ›]

GAP · AGENT SYNTHESIS · QUALIFIED
The bounded review found little component-level analysis…
derived from 4 findings                                     [Open ›]

HYPOTHESIS · SPECULATIVE
Individual rank components may acquire distinct functions…
3 premises                                                  [Open ›]
```

The list supports text search, kind filters, and lifecycle labels. Every row
uses text labels in addition to color. Selecting a row opens **Details** in the
existing Research Harness inspector and shows:

- full text and epistemic qualification;
- source evidence with Paper title, page/chunk locator, and excerpt;
- derivation and researcher-context sections kept visually separate;
- lifecycle and immutable revision history; and
- actions to revise, contest, or supersede the entry.

An entry editor exposes fields appropriate to its kind and status. Choosing
`source_supported` requires selecting extracted passages from Papers already in
the Project Vault. Context sources appear under **Motivation / working
context**, never under **Evidence**.

The current State revision is visible in the Research header. Switching to a
historical revision is read-only and visibly labelled; returning to Current
restores editing.

## Activity

Run-originated commits append structured events for each added, revised,
contested, or superseded entry and one summary event for the State revision.
Event payloads include entry ids and before/after revision numbers so Activity
can open the exact historical record.

Manual changes are also auditable in Research State history, but do not invent
a Research Run or Cycle number.

## Migration

Every existing Project receives Research State revision `0`. No existing note,
chat, Project document, or assistant response is migrated into Research State.
Promotion requires an explicit typed entry creation so the epistemic status and
links are chosen and validated.

## Non-Goals

- Automatically extracting entries from newly found Papers.
- Model-driven reconciliation, contradiction detection, or gap generation.
- Scheduled Runs, autonomy levels, or stop conditions.
- Harness reflection and improvement proposals.
- Generating Project documents from Research State.
- Full event sourcing or Project forking.
- Treating a citation string in Markdown as a resolvable evidence link.

## Acceptance Criteria

1. Every existing and new Project has an empty revisioned Research State.
2. All five semantic kinds and four epistemic statuses round-trip through Rust,
   SQLite, Tauri commands, and TypeScript types.
3. Invalid kind/status combinations are rejected transactionally.
4. A source-supported Finding cannot be committed without at least one valid
   evidence link to an extracted chunk of a Paper in the Project Vault.
5. Notes, chats, documents, and Project instructions remain typed context and
   cannot satisfy source-evidence validation.
6. Agent synthesis retains entry derivations; contested and superseded entries
   retain immutable history.
7. Optimistic revision checks prevent concurrent edits from overwriting newer
   State.
8. Completed Run checkpoints can record starting and resulting State revisions;
   failed/cancelled Runs cannot expose a partial revision.
9. Project Research renders searchable/filterable State rows and a Details
   inspector with evidence, derivation, context, and revision history visibly
   separated.
10. Migration, validation, revision, evidence-resolution, lifecycle, and UI
    behavior have focused tests.
11. Rust tests, frontend type checking, and the production frontend build pass.

## Approval

Approved on 2026-09-02 under the user's instruction to author, approve,
implement, verify, and commit each focused RFC from RFC 0108 without a separate
approval round.
