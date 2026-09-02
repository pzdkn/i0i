# RFC 0115: Create Project Documents from Research

- Status: Implemented and verified
- Date: 2026-09-02
- Area: Projects / Documents / Research State
- Parent: RFC 0108
- Depends on: RFC 0110, RFC 0112

## Summary

Create an ordinary Markdown Project document from selected entries in one
immutable Research State revision. The researcher chooses an output shape—
Survey, Related-work Section, Research-gap Analysis, Hypothesis Report,
Experiment Plan, or Custom Document—and i0i produces a new document with
durable provenance and resolvable Paper citations.

The output shape controls the initial structure and bounded generation
instruction. It does not introduce Survey, Gap Report, or Experiment Plan as
new domain entities or permanent navigation areas. Generated prose remains an
authored artifact, not Research State and not source evidence.

## Generation Request

`CreateFromResearchRequest` contains:

- Project id;
- exact Research State revision;
- non-empty selected Research Entry ids as they existed in that revision;
- output shape;
- proposed document title;
- optional bounded custom instruction; and
- optional originating Research Run id when generation is launched from a
  completed checkpoint.

Supported output shapes are:

- `survey`;
- `related_work`;
- `research_gap_analysis`;
- `hypothesis_report`;
- `experiment_plan`; and
- `custom`.

The selected revision and entry ids are immutable inputs. If Current Research
State changes while generation runs, the job continues from its recorded
revision and the resulting document says so. A request cannot silently expand
to every newer entry.

## Generation Job

Document creation is asynchronous and represented by a durable
`ResearchDocumentGeneration`:

- id and owning Project id;
- status: `queued`, `generating`, `ready`, `failed`, or `cancelled`;
- output shape and researcher instruction;
- State revision and selected entry ids;
- optional originating Run id;
- fixed product generation-policy version;
- model identifier;
- resulting Project document id when ready;
- error, timestamps, and cancellation request; and
- bounded usage metrics.

At most one generation job for the same Project and exact request fingerprint
may be active. Retrying a failed job creates a new job linked to the failed one;
it never mutates the prior attempt's audit record.

No Project document is created until generated content and citations pass final
validation. Failure or cancellation therefore cannot leave a partial document
in the Documents list.

## Input Assembly

Rust assembles a bounded, typed generation packet:

1. load the exact Research State revision;
2. require every selected entry to belong to that Project and revision;
3. retain semantic kind, epistemic status, lifecycle, text, and qualification;
4. include resolvable evidence excerpts for source-supported Findings;
5. include derivation links for agent syntheses;
6. include researcher context only in a separately labelled context section;
7. include speculative premises for gaps, hypotheses, and experiment ideas;
8. mint stable generation-only citation handles; and
9. truncate only at entry/evidence boundaries while recording omitted counts.

Contested and superseded entries are excluded by default. The creation dialog
may explicitly include them, and the generation packet then labels their
lifecycle so they cannot be presented as settled conclusions.

The product generation policy is app-owned and read-only. Custom instructions
may request tone, audience, ordering, or emphasis, but cannot remove epistemic
labels, invent evidence, expand Project boundaries, or overwrite documents.

## Output Shapes

Each shape provides a transparent initial outline, not an opaque hidden prompt.

### Survey

- Scope and bounded corpus
- Established findings
- Themes and disagreements
- Open questions and qualified gaps
- Hypotheses and future work
- References

### Related-work Section

- Concise thematic synthesis
- Method or evidence contrasts
- Limitations of the bounded set
- References

### Research-gap Analysis

- Coverage statement
- Supported background
- Qualified gaps (“not established in this bounded review”)
- Competing explanations and unanswered questions
- Candidate next searches
- References

### Hypothesis Report

- Motivating findings
- Explicitly speculative hypotheses
- Supporting and opposing premises
- Falsifiable predictions
- References

### Experiment Plan

- Research question and hypothesis
- Proposed intervention or measurement
- Variables, controls, and expected observations
- Risks and alternative explanations
- Evidence motivating the design
- References

### Custom Document

The researcher supplies a short purpose and optional outline. The same
epistemic and citation policy still applies.

## Citation Model

Generated Markdown uses document-local references such as `[@hu2022lora]`.
Every emitted key resolves through a durable `ProjectDocumentCitation` row:

- document id and unique citation key;
- canonical Paper id;
- Research Evidence Link ids used by the generation;
- Paper title/authors/year snapshot for audit and display; and
- created timestamp.

Keys are generated deterministically from Paper metadata and disambiguated
within the document. The renderer resolves a generated document's local map
before the general Vault reference index. Metadata edits therefore do not break
old generated references, and two Papers with the same derived key remain
unambiguous.

The citation map is not evidence by itself; it points to the canonical Paper
and the Research Evidence Links that justified including it. A References
section is rendered from this map rather than trusted as free-form model text.

## Epistemic Output Rules

1. Only source-supported Findings may be phrased as direct factual claims, and
   each such claim must use at least one citation handle supplied with that
   Finding.
2. Agent synthesis is introduced as analysis or interpretation and cites its
   component evidence rather than claiming to be a source quotation.
3. Researcher context is labelled as goals, assumptions, preferences, or
   working context and cannot receive a source citation merely because a Paper
   is mentioned nearby.
4. Gaps retain bounded-search qualification; “no evidence exists” is rejected
   unless that exact claim is itself source-supported, which a Gap cannot be.
5. Hypotheses and experiment ideas remain explicitly speculative or proposed.
6. Contested and superseded material is visibly qualified.
7. The model may use only citation handles present in the generation packet.
8. Repetition or polished prose never promotes epistemic status.

Final validation rejects unknown citation handles, missing required citations,
unresolvable local citation rows, omitted epistemic qualification, and output
that exceeds the configured document budget. Validation errors fail the job;
the system does not silently strip provenance and save the remaining prose.

## Project Document Provenance

On successful validation, one transaction:

1. creates an ordinary `ProjectDocument` with `format = markdown`;
2. stores `created_from_state_revision` and optional
   `created_from_run_id`;
3. stores the generation job id and output shape;
4. inserts its document-local citation map;
5. marks the generation ready with the resulting document id; and
6. appends Harness Activity linking the State revision, selected entries,
   output shape, and document.

Generated documents default to `harness_writable = false`. The researcher may
opt in later using the existing document setting. Generation always creates a
new document and never overwrites a user-authored or previously generated
document.

The document editor shows a provenance banner:

```text
Generated from Research State revision 12 · 14 entries · Survey
Created 2 Sep 2026                                      [View source state]
```

Later manual edits change the document but do not alter its origin metadata or
the immutable generation record.

## Interaction

Research State adds **Create from…**. With no selection, the action is disabled
and explains why. With selected entries it opens a compact dialog:

```text
CREATE FROM RESEARCH
14 entries selected · State revision 12

Type       [Research-gap analysis ▾]
Title      [LoRA interpretability research gaps]
Include    ☑ Findings  ☑ Questions  ☑ Gaps
           ☐ Contested/superseded entries
Direction  [Focus on component-level analysis…]

Epistemic labels and source citations are always preserved.
                                      [Cancel] [Create document]
```

Choosing an Experiment Idea's **Promote to document** action opens the same
dialog with that idea, its hypothesis, premises, and motivating Findings
preselected and `experiment_plan` selected.

While generating, Activity and Documents show one cancellable progress row.
On success, the new ordinary document opens in the existing Documents editor.
On failure, no document appears; the error and retry action remain in Activity.

Historical Research State revisions are eligible inputs. The dialog and
provenance banner visibly say that the output is based on a historical
revision.

## Commands

Add narrow commands:

- `create_document_from_research(request)` returns the generation job;
- `get_research_document_generation(generation_id)`;
- `cancel_research_document_generation(generation_id)`; and
- `retry_research_document_generation(generation_id)`.

Progress arrives through a typed Tauri event. Project document loading returns
its local citation map and generation provenance when present.

## Non-Goals

- A permanent Survey, Gap Analysis, Hypothesis, or Experiment product area.
- Treating generated prose as Research State or source evidence.
- Automatically publishing or exporting the result.
- Overwriting arbitrary Project documents.
- Continuous document synchronization after Research State changes.
- LaTeX generation or conversion.
- Cross-Project evidence or document generation.
- Model-selected expansion beyond the chosen State revision and entries.

## Acceptance Criteria

1. All six output shapes create the same ordinary Markdown Project document
   type with shape-specific transparent structure.
2. Every generation pins one immutable Research State revision and explicit
   selected entries; later State changes cannot change its input or provenance.
3. Source evidence, synthesis, researcher context, gaps, hypotheses, and ideas
   remain qualitatively distinct in the assembled packet and output.
4. Generated factual claims use only supplied resolvable citation handles;
   unknown or missing citations fail validation before document creation.
5. Document-local citation keys remain resolvable after Paper metadata changes
   and are unambiguous under key collisions.
6. A successful transaction stores origin revision, optional Run, generation
   record, citation map, document, and Activity together.
7. Failed or cancelled generation creates no partial Project document.
8. Generated documents default to not Harness-writable and are never used as
   source evidence.
9. Research State selection, Create-from dialog, Experiment Idea promotion,
   progress/cancellation, provenance banner, and source-State navigation work.
10. Input-boundary, template, citation, epistemic validation, transaction,
    collision, cancellation, and focused UI tests pass.
11. Rust tests, frontend type checking, and the production frontend build pass.

## Approval

Approved on 2026-09-02 under the user's instruction to author, approve,
implement, verify, and commit each focused RFC from RFC 0108 without a separate
approval round.

## Verification

Implemented on 2026-09-02. Store tests cover all six transparent document
shapes, immutable State-revision inputs, citation-key collisions and metadata
snapshots, cancellation, epistemic validation, and transaction rollback with
no partial document. Focused UI tests cover selection and historical-revision
provenance. The full Rust suite passed with 511 tests and 6 live/integration
tests ignored; frontend type checking and the production build also passed.
