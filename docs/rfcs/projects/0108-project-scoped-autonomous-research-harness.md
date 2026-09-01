# RFC 0108: Project-Scoped Autonomous Research Harness

- Status: Approved design; implementation not started
- Date: 2026-09-01
- Area: Projects / Research automation
- Builds on: RFC 0037, RFC 0076, RFC 0077, RFC 0088, RFC 0091, RFC 0097,
  RFC 0104, RFC 0107
- Design companion: [Autonomous Research Projects](../../design/autonomous-research-projects.md)

## Summary

Make **Project** the goal-directed research boundary in i0i. Every Project owns
exactly one **Vault**, which acts as that Project's bibliography, and any number
of ordinary **Project documents**. A Paper remains canonical and may belong to
many Vaults; a Vault belongs to one Project only.

Each Project also owns one **Research Harness**. The Harness can repeatedly
search for publications, update a typed Research State, record what it did, and
propose improvements to its own future operation. Its controls live in a
project-scoped side panel rather than in the document tree.

The Research State is the structured, inspectable source from which the user
may create a survey, research-gap analysis, hypothesis report, experiment plan,
or another document. The Harness does not grow one undifferentiated document
forever, and generated synthesis never becomes source evidence merely through
repetition.

This RFC fixes the product vocabulary, ownership model, epistemic boundaries,
and target interaction. It is the architectural parent for focused delivery
RFCs; approving it does not approve implementing all phases as one change.

## User Goal and Mental Model

A researcher begins with a goal such as:

> Improve the interpretability of low-rank adaptation methods.

They want i0i to keep investigating that goal over multiple bounded cycles:

1. find relevant publications through browser and scholarly API discovery;
2. inspect and filter them;
3. add accepted Papers to the Project's Vault;
4. extract source-supported findings;
5. revise the Project's current Research State;
6. identify uncertainty, disagreements, gaps, hypotheses, and experiment ideas;
7. choose the next search direction; and
8. repeat until paused or a configured stop condition is reached.

The accumulated result should support several different outcomes. A survey is
one option, not the defining output. The researcher may instead create an
experiment plan, related-work section, gap analysis, grant proposal, hypothesis
register, or a custom document.

The expected mental model is:

```text
Project
├── Vault                 the Project's bibliography
├── Documents             authored outputs and working material
└── Research Harness      the controlled autonomous process
    ├── Research State    what the process currently holds
    ├── Runs              bounded executions
    └── Activity          the durable account of what happened
```

These are not all files. Vault and Research Harness are structured application
state. Documents are files or file-like authored artifacts. Activity and the
Project overview are views derived from structured records.

## Domain Language and Invariants

### Project

A Project is a goal-directed research workspace. It owns exactly one Vault, its
Project documents, and one Research Harness.

The Project is the autonomy boundary: schedules, budgets, writable documents,
research direction, checkpoints, and stop controls never leak across Projects.

### Vault

A Vault is the bibliography belonging to one Project. It is not a Project and
does not own documents or a Research Harness.

The ownership and membership cardinalities are:

```text
Project 1 ─── 1 Vault
Vault   * ─── * Paper
```

- Creating a Project creates its Vault in the same transaction.
- A Vault cannot exist without a Project after migration.
- A Project cannot own multiple Vaults in this design.
- A Paper is stored once and may belong to many Vaults.
- Adding an existing Paper to another Project creates only another Vault
  membership; it does not duplicate metadata, sources, extraction, or PDF data.

### Project document

A Project document is a user- or agent-authored artifact belonging to a
Project. Survey drafts, meeting notes, experiment plans, grant proposals, and
working notes are ordinary documents rather than separate domain entities.

The first supported authoring format is Markdown. LaTeX and other formats may
be added later without changing Project ownership. A generated document records
the Research Run and Research State revision from which it was created.

Project documents are not source evidence. Citations inside them point back to
source evidence held by canonical Papers.

### Research Harness

The Research Harness is the Project-scoped combination of:

- its research instructions and operating constraints;
- its schedule, budget, autonomy, and stop configuration;
- its current typed Research State;
- its bounded Research Runs;
- its append-only Activity records; and
- its reflection and self-improvement proposals.

The Activity log is part of the Harness, but it is not the whole Harness. The
log describes what happened; the Harness also defines what may happen and holds
the current research state resulting from those actions.

### Research State

Research State is a structured index of the Project's current findings,
questions, gaps, hypotheses, and experiment ideas. It replaces the vague idea
of a single ever-growing "living synthesis" document.

Research State is not itself a survey or manuscript. It is inspectable source
material from which the UI can render a current-understanding view and from
which the user can create Project documents.

### Research Run

A Research Run is one bounded manual or scheduled execution of the Harness. It
captures the exact configuration used, inputs considered, Activity emitted,
state changes proposed or applied, costs, stop reason, and final reflection.

Every completed Run is a checkpoint. A separate Checkpoint entity is not
required in the first design.

## Epistemic Model

The Harness must preserve qualitative differences between its inputs and
outputs. Every Research Entry has both a semantic kind and an epistemic status.

Semantic kinds:

- **Finding** — a claim or observation relevant to the research goal.
- **Question** — something the Project does not yet answer.
- **Gap** — an inferred absence, limitation, or unresolved disagreement.
- **Hypothesis** — a proposition that could be investigated or falsified.
- **Experiment idea** — a proposed way to test a question or hypothesis.

Epistemic statuses:

- **Source-supported** — supported by resolvable source evidence.
- **Agent synthesis** — an interpretation across evidence, not a source claim.
- **Researcher context** — derived from notes, chats, preferences, or prior
  intentions and not evidence.
- **Speculative** — a gap, hypothesis, or proposal that is explicitly
  conjectural.

The following invariants are mandatory:

1. A source-supported entry has at least one resolvable evidence link.
2. Reader notes and conversation history may guide attention, queries, and
   interpretation but cannot support a factual claim.
3. Prior assistant output is fallible generated material, not evidence.
4. An agent synthesis retains links to the entries from which it was derived.
5. A research gap means that the bounded search did not find sufficient
   evidence; it must not be phrased as proof that no evidence exists.
6. Hypotheses and experiment ideas remain visibly speculative when copied into
   a Project document.
7. Repetition across Runs does not promote a statement to source-supported.
8. Contradicted or superseded entries remain in history rather than being
   silently erased.

## Research Harness Behavior

### One bounded cycle

A Run proceeds through explicit phases:

```text
orient
  → plan queries
  → discover
  → inspect and deduplicate
  → assess relevance
  → extract evidence-linked findings
  → reconcile with Research State
  → reflect on gaps and failures
  → select or propose the next direction
  → checkpoint
```

Rust owns orchestration, budgets, cancellation, persistence, and state
transitions. Model calls fill bounded typed outputs within those transitions;
the model does not control an unbounded recursive process.

Browser discovery through Obscura and scholarly API discovery feed the same
candidate pool. Existing Paper identity and Vault membership rules continue to
deduplicate results before accepted Papers enter the Project Vault.

### Configuration

The first Harness configuration needs only:

- research goal;
- editable Project research instructions;
- structured query guidance with preferred and excluded concepts;
- optional scope and exclusions;
- source policy;
- run cadence or manual-only mode;
- per-Run paper and cost budgets;
- maximum cycle count or end date;
- stop after a configured number of unproductive Runs;
- autonomy level; and
- writable Project documents.

The autonomy level separates three behaviors:

| Level | Behavior |
| --- | --- |
| `Manual` | Prepare a Run and wait for the user to start it. |
| `Propose` | Run automatically but propose Research State/document changes. |
| `Automatic` | Run and apply allowed changes within the Project boundary. |

Automatic mode may add Papers to the Project Vault and update Research State.
It does not silently overwrite arbitrary user-authored documents. A document
must be explicitly marked as Harness-writable; otherwise the Harness proposes
a diff or creates a new generated document.

The app-owned system policy is not Project configuration and is not editable or
self-modifying. It defines orchestration contracts, typed output schemas,
provenance rules, epistemic boundaries, and budget enforcement. Project
Settings expose the researcher's editable instructions and query guidance.
Each Run records both its configuration snapshot and the version of the product
policy used, and an advanced read-only view may show the effective instruction
layers for inspection.

### Stop and pause

Pause prevents future scheduled Runs and does not terminate a currently
committing state transition halfway through. Stop ends the loop after the
current safe boundary. Cancellation during discovery or model work records a
cancelled Run without applying a partial Research State revision.

Stop reasons are explicit:

- user stopped;
- configured cycle limit;
- configured end date;
- budget exhausted;
- no meaningful additions for the configured number of Runs;
- goal judged sufficiently covered; or
- unrecoverable failure.

Budget exhaustion and convergence are not failures and must not imply complete
coverage.

## Self-Improvement

Each Run produces a bounded reflection about the operation of the Harness, not
only about the research topic. It may record:

- useful and unproductive query formulations;
- recurring irrelevant result classes;
- unavailable or failing sources;
- terminology discovered from accepted Papers;
- coverage or venue bias;
- repeated relevance mistakes;
- scope drift; and
- rules that appear too strict or too weak.

Operational observations are kept separate from Research Entries. For example,
"queries containing `explanation` returned application papers" is a Harness
observation; it is not a finding about LoRA.

The first version follows **capture aggressively, promote conservatively**:

1. Runs append operational observations to Activity.
2. Recurring observations may produce a proposed configuration change.
3. The proposal identifies the contributing Runs, names one target setting,
   shows its exact before/after value, and previews the expected effect.
4. Accepting the proposal updates the current Harness configuration and emits
   an Activity event.
5. The next Run snapshots the new configuration.

Low-risk query vocabulary may be configured for automatic adaptation later.
Changing the Project goal, evidence policy, autonomy, writable documents, or
stop conditions always remains an explicit user decision in the first version.
No improvement may modify the app-owned system policy.

## Interaction Design

### Project navigation

The current shell's `VAULT` mode becomes `PROJECTS`. The left Explorer lists
Projects and expands the selected Project into its three centre-workspace
views:

```text
Improve LoRA Interpretability
├── Research
├── Vault
└── Documents
```

Selecting a Project opens a Project workspace tab and lands on Research. The
Project header shows its goal, Harness status, next Run, and the primary Run
controls. It is a derived UI surface, not a document.

The Vault keeps its existing name and paper-oriented interactions. Inside a
Project, its purpose as the bibliography should be evident from placement and
supporting copy; the product does not introduce a second "Bibliography" entity.

### Research view and Harness inspector

Research State is an interactive list in the large centre pane. Filters expose
Findings, Questions, Gaps, Hypotheses, and Experiment ideas. Each row shows its
semantic kind, epistemic status, concise text, evidence or premise count, and
revision state.

The existing contextual right-inspector region has three sections while
Research is active:

```text
Details | Activity | Settings
```

- **Details** shows the selected Research Entry, its evidence or premises,
  revisions, and available actions.
- **Activity** shows ordered Run and event history with the current phase,
  accepted/rejected Papers, state changes, failures, reflections, stop reason,
  and Harness Improvement proposals. Pending proposals appear as review cards
  above the chronological log.
- **Settings** contains the goal, schedule, budgets, autonomy, sources, writable
  destinations, and stop conditions. Accepted improvements appear in its
  configuration history.

Primary controls remain visible at the top of the panel:

```text
Run now · Pause/Resume · Stop
```

The panel shows which configuration belongs to the current Run. Editing
settings during an active Run affects only the next Run.

The Harness is structured application state and UI controls, not a collection
of documents. Vault retains its paper/suggestion inspector, Documents may use a
document outline/citation inspector, and Reader retains Info/Notes/Chat. The
Project header's compact Harness status returns the user to Research → Activity
from those views.

### Creating outputs

Research outputs are created through one action:

```text
Create from research…
```

Initial templates may include:

- Survey;
- research-gap analysis;
- hypothesis report;
- experiment plan;
- related-work section; and
- custom document.

Templates are generation choices, not domain types or permanent navigation
items. Every output becomes an ordinary Project document and records the Run
and Research State revision used to create it.

Experiment ideas may first appear as structured cards in Current state. The
user can refine one in place or promote it into a Project document.

### Activity legibility

Activity must answer:

- What is the Harness doing now?
- Which queries and sources did it use?
- Which Papers did it accept or reject, and why?
- What changed in Research State or a writable document?
- What did the Run cost?
- Why did it stop?
- What will the next Run do differently?

Do not expose raw prompts, provider response bodies, secrets, or unbounded log
text in the UI. Detailed diagnostic logging remains available separately.

## Minimal Data Shape

The domain remains explicit without assigning every UI surface its own entity:

```text
Project
  id
  title
  goal?

Vault
  id
  project_id             unique, required
  title
  path

VaultPaper
  vault_id
  paper_id

ProjectDocument
  id
  project_id
  title
  format                 markdown initially
  content/path
  harness_writable
  created_from_run_id?
  created_from_state_revision?

ResearchHarness
  project_id             unique
  status
  current_configuration
  current_state_revision

ResearchEntry
  id
  project_id
  semantic_kind
  epistemic_status
  text
  status
  created_by_run_id
  supersedes_entry_id?

ResearchEvidenceLink
  entry_id
  paper_id
  source_id
  extraction_id?
  locator
  relationship           supports | contradicts | qualifies | motivates

ResearchRun
  id
  project_id
  status
  configuration_snapshot
  starting_state_revision
  resulting_state_revision?
  started_at
  finished_at?
  stop_reason?
  summary?
  reflection?

ResearchEvent
  id
  run_id
  sequence
  kind
  summary
  structured_detail?
  occurred_at

HarnessImprovement
  id
  harness_id
  contributing_run_ids
  rationale
  proposed_configuration_patch
  status                  proposed | accepted | rejected | superseded
  decided_at?
```

`Current state`, `Activity`, Project overview, and completed checkpoints are
projections over these records. They are not separate persisted documents.

The configuration may begin as a versioned serialized value owned by the
Harness. Every Run stores an immutable snapshot, so later configuration edits
never make historical Runs uninterpretable.

## Checkpoints

Every successfully completed Run is a checkpoint consisting of:

- its configuration snapshot;
- starting and resulting Research State revisions;
- the Project Vault membership revision used;
- Project document revisions changed by the Run; and
- its ordered Activity.

Checkpoints support audit, inspection, and restoration of the same Project.
They do not create branches or new Projects in this design.

## Migration from Vault-Focused i0i

Existing Vaults are preserved rather than reinterpreted as Projects themselves:

1. Create one Project for each existing Vault.
2. Link that Vault exclusively to the new Project.
3. Preserve Vault ids, paths, titles, and Paper memberships.
4. Give each Project an inactive default Research Harness.
5. Leave existing Paper, source, extraction, annotation, and chat identities
   unchanged.

An existing Vault named `self-supervised` therefore appears under a Project
initially titled `self-supervised`. The user may rename the Project without
renaming the Vault; the two names describe different concepts after migration.

Project creation and its Vault creation are atomic. Failure to create either
leaves neither record behind.

## Alternatives Considered

### Make Project an enriched Vault

Add goals, documents, and automation directly to the existing Vault entity.
This minimizes the number of nouns and migration work.

**Rejected.** A Vault answers which Papers belong together; a Project answers
what research work is being pursued. The user explicitly wants the Vault to
remain the consistent name for the bibliography and the Project to own
concrete documents and autonomous work.

### Allow several Vaults per Project

This could separate core Papers, background reading, rejected Papers, and
methodological references.

**Rejected for the first design.** Status, tags, saved views, and Research Entry
relationships can express these distinctions without multiplying bibliography
containers. One Project has one Vault.

### Store all accumulated research in one Markdown dossier

Append a summary after each Run and use that document as the next Run's memory.

**Rejected.** It conflates evidence, synthesis, notes, and hypotheses; makes
deduplication and supersession difficult; and produces a document that becomes
longer without necessarily becoming more informative.

### Make each output a special feature

Add permanent Survey, Gap Analysis, Hypotheses, and Experiment Ideas sections.

**Rejected.** These are views or generated outputs over the same Research
State. Only Project documents and structured Research Entries need persistence.

### Treat Activity as the whole Harness

Reconstruct current configuration and Research State only from an event log.

**Rejected.** Activity is valuable for auditability, but full event sourcing
would add complexity to ordinary reads and migrations. Persist current state
explicitly and keep Activity append-only as its history.

## Scope

This RFC includes:

- Project as the goal-directed ownership boundary;
- exactly one Vault per Project and one Project per Vault;
- many-to-many Paper membership across Vaults;
- ordinary Project documents with Markdown as the first format;
- one Project-scoped Research Harness;
- typed Research State with evidence links and epistemic status;
- bounded Runs and append-only Activity;
- completed Runs as checkpoints;
- the contextual Harness panel; and
- generation of ordinary documents from selected Research State.

## Non-Goals

- Collaborative or cloud-synchronized Projects.
- Multiple Vaults inside one Project.
- Cross-Project autonomous Runs.
- Forking or branching a Project from a checkpoint.
- Autonomous modification of arbitrary user-authored documents.
- Treating notes, chats, synthesis, gaps, or hypotheses as source evidence.
- A general workflow-graph editor.
- Dynamic unbounded sub-agent spawning.
- Experiment execution, code execution, or laboratory automation.
- Selecting the final survey structure or publication venue.
- Supporting every document format in the first implementation.

## Ordered Delivery RFCs

This design should be delivered through focused RFCs in this order:

1. **Project and Vault ownership migration** — persist Project, enforce the
   one-to-one Vault relationship, migrate existing Vaults, and make Project the
   navigation boundary.
2. **Project documents** — create, edit, delete, and persist Markdown documents
   inside one Project.
3. **Research Harness configuration and manual Run** — add the side panel,
   bounded manual execution, configuration snapshots, cancellation, and
   Activity.
4. **Typed Research State and evidence links** — persist Findings, Questions,
   Gaps, Hypotheses, Experiment Ideas, and their epistemic provenance.
5. **Scheduled Runs and stopping** — execute persisted schedules safely and
   enforce cycle, time, cost, convergence, and inactivity limits.
6. **Harness reflection and improvement proposals** — detect recurring
   operational patterns and propose traceable configuration changes.
7. **Create from research** — generate ordinary Project documents from a
   selected Research State revision.

Approval and completion are RFC-specific. This parent design does not authorize
implementing every item at once.

## Acceptance Criteria for This Design

- Project, Vault, Paper, Project document, Research Harness, Research State,
  Research Run, and Activity have non-overlapping definitions.
- The relationship is unambiguous: one Project owns one Vault; one Vault
  belongs to one Project; Papers may belong to many Vaults.
- Survey and other aggregate outputs are ordinary Project documents rather
  than permanent product sections.
- Experiment ideas are structured Research Entries and may be promoted into a
  Project document.
- Research Loop controls are specified as a contextual Harness panel rather
  than a document-tree item.
- Activity is an append-only part of the Harness, not a synonym for the whole
  Harness.
- Research State replaces the unbounded living-dossier concept.
- Evidence, agent synthesis, researcher context, and speculation remain
  qualitatively distinct in the data model and UI.
- Every future implementation phase has a focused follow-up RFC and independent
  approval boundary.

## Approval

Approved on 2026-09-01. The user requested this RFC after settling the core
domain relationships and previously authorized agent-authored RFCs to be
approved without a separate approval round. Implementation remains unstarted
and must proceed through the focused RFCs listed above.
