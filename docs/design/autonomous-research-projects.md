# Autonomous Research Projects — Current Design

Status: Current product design; implementation is split into RFCs
Date: 2026-09-01
Related: [RFC 0108](../rfcs/projects/0108-project-scoped-autonomous-research-harness.md)

## Product idea

i0i should support long-running, goal-directed research rather than only
reading and grouping Papers.

A researcher creates a Project with a broad goal such as:

> Improve the interpretability of low-rank adaptation methods.

The Project's Research Harness repeatedly discovers publications, inspects
their relevance, accumulates evidence-linked research state, identifies what
is missing, and chooses the next bounded direction. The process continues on a
schedule or manually until the researcher pauses it or a configured stop
condition is reached.

The accumulated research can be used to create many kinds of Project document:
a survey, related-work section, research-gap analysis, hypothesis report,
experiment plan, proposal, or a custom artifact. A survey is one possible
output, not the organizing concept.

## Core model

```text
Project 1 ───────── 1 Vault
   │                   │
   │                   └── * VaultPaper * ── Paper
   │
   ├── * ProjectDocument
   │
   └── 1 ResearchHarness
          ├── ResearchState
          ├── * ResearchRun
          └── * ResearchEvent
```

The compact user-facing model is:

```text
Project
├── Vault
├── Documents
└── Research Harness
```

### Project

A Project is the goal-directed boundary for a piece of research. It owns one
Vault, its documents, and its Research Harness.

The Project is also the autonomy boundary. The Harness may not search, write,
or change state in another Project merely because the Projects contain some of
the same Papers.

### Vault

The Vault is the Project's bibliography. The product continues to call it
**Vault** for consistency with the existing paper-library experience.

Each Project has exactly one Vault, and each Vault belongs to exactly one
Project. A Paper may belong to many Vaults. The Paper, its sources, extracted
text, PDF, and metadata remain canonical and are not duplicated.

For example:

```text
Project: Improve LoRA Interpretability
└── Vault
    ├── LoRA: Low-Rank Adaptation of Large Language Models
    ├── AdaLoRA: Adaptive Budget Allocation for Parameter-Efficient...
    └── DoRA: Weight-Decomposed Low-Rank Adaptation
```

### Documents

Documents are user- or agent-authored artifacts inside the Project. Survey
drafts, meeting notes, experiment plans, grant proposals, and working notes are
ordinary documents, not separate product entities.

Markdown is the first authoring format. LaTeX may be supported later as another
format without changing the Project model.

Examples:

```text
Documents
├── Related work.md
├── Candidate experiments.md
├── Meeting notes.md
└── Survey draft.md
```

A document can cite Papers from the Project Vault. Its prose does not become
source evidence merely because the Harness generated it or because it contains
citations.

## Research Harness

The Research Harness is the controlled, inspectable environment in which the
autonomous research loop operates. It is more than an Activity log.

```text
Research Harness
├── Configuration       what it should and may do
├── Research State      what it currently holds about the topic
├── Runs                bounded executions
├── Activity            what happened during those Runs
└── Improvements        proposed changes to future operation
```

### Configuration

The configuration includes:

- research goal;
- editable Project research instructions;
- structured query guidance, including preferred and excluded concepts;
- scope and exclusions;
- permitted discovery sources;
- evidence rules;
- manual or scheduled cadence;
- paper, time, and cost budgets;
- stop conditions;
- autonomy level; and
- which documents, if any, the Harness may update automatically.

Each Run stores an immutable configuration snapshot. Editing Settings during a
Run changes only the next Run.

### Instruction layers

The Harness does not expose one undifferentiated editable system prompt. Its
effective instructions have explicit layers:

```text
1. Product system policy            app-owned, read-only
   orchestration contract, output schemas, provenance and epistemic rules

2. Project research instructions    researcher-owned, editable
   priorities, terminology, methods of interest, exclusions, quality bar

3. Structured Harness settings      researcher-owned, editable
   query guidance, sources, schedule, budgets, autonomy, stop conditions

4. Run context                      generated and immutable for that Run
   current Research State, gaps, prior observations, remaining budget
```

The fixed system policy ensures that changing Project instructions cannot turn
notes into evidence, remove provenance requirements, bypass budgets, or grant
the Harness additional write authority. The researcher can inspect the
effective instruction stack for a Run, but edits only layers 2 and 3.

Self-improvement never rewrites the product system policy. It proposes a typed
change to a named Project setting, such as query guidance, and shows the exact
before/after value before acceptance.

### Research State

Research State is a structured index of the Project's current understanding.
It is not one indefinitely growing document.

It contains typed entries:

```text
Finding
Question
Gap
Hypothesis
Experiment idea
```

Each entry also carries an epistemic status:

```text
Source-supported
Agent synthesis
Researcher context
Speculative
```

This makes the difference between the following entries visible and
machine-checkable:

```text
SOURCE-SUPPORTED FINDING
LoRA constrains the weight update to a low-rank factorization.
Evidence: Hu et al., §4

AGENT SYNTHESIS
Several methods treat rank mainly as a resource-allocation problem rather than
as an interpretable functional decomposition.
Derived from: 6 source-supported findings

HYPOTHESIS · SPECULATIVE
Individual rank components may acquire functionally distinct roles.
Motivated by: findings F12, F18, F21
```

The UI may render Research State as **Current understanding**, grouped into
Findings, Questions, Gaps, Hypotheses, and Experiment ideas. A generated
document is a projection from a chosen Research State revision, not the source
of truth for that state.

### Runs

A Research Run is one bounded manual or scheduled cycle:

```text
orient
  → plan queries
  → discover through Obscura and scholarly APIs
  → inspect and deduplicate
  → assess relevance
  → add accepted Papers to the Vault
  → extract evidence-linked findings
  → reconcile Research State
  → reflect
  → choose or propose the next direction
  → checkpoint
```

A sequence of Runs is the research loop. The loop is behavior, not a separate
file or navigation node.

Every completed Run is also a checkpoint. It retains the configuration used,
starting and resulting Research State revisions, Vault membership revision,
affected document revisions, Activity, costs, and stop reason.

### Activity

Activity is the append-only account of what the Harness did. It is part of the
Harness, but not a synonym for the Harness.

A Run emits ordered structured events such as:

```text
Cycle started
Query planned
Search completed
Paper inspected
Paper accepted
Paper rejected
Research entry added
Research entry contested
Contradiction found
Direction proposed
Harness improvement proposed
Cycle completed
```

The Activity view is rendered from these records:

```text
09:00  Cycle 8 started
09:01  Searched “LoRA rank functional specialization”
09:03  Inspected 28 Papers
09:05  Added 3 Papers to the Vault
09:07  Added 2 Findings
09:08  Marked 1 Finding as contested
09:09  Proposed a new search synonym
09:10  Cycle completed · budget reached
```

Activity should answer what happened, why something was accepted or rejected,
what changed, what the Run cost, why it stopped, and what the next Run intends
to do differently.

### Self-improvement

The Harness records operational observations separately from research content.

```text
Research entry:
  "AdaLoRA dynamically allocates rank according to an importance score."

Harness observation:
  "Queries containing 'explanation' repeatedly return application papers
   rather than mechanistic studies."
```

Operational observations may concern query quality, source failure, relevance
mistakes, coverage bias, scope drift, or wasted work. Recurring observations
can produce a traceable proposed configuration change.

Self-improvement is stored in three related places rather than in the side
panel itself:

```text
ResearchRun.reflection
  raw operational observations from one Run

HarnessImprovement
  a durable proposed configuration change, its rationale, contributing Runs,
  proposed patch, and proposed | accepted | rejected status

ResearchEvent
  the append-only audit record that the improvement was proposed, accepted,
  rejected, or superseded
```

The side panel is the view and control surface for these records. It is not
their storage location.

```text
Observed in Runs 3, 4, and 6
  “LoRA explanation” produced mostly application papers.

Proposed improvement
  Add “mechanistic”, “subspace”, “intrinsic dimension”, and
  “functional decomposition” to the query vocabulary.
```

The design follows **capture aggressively, promote conservatively**. Accepting
an improvement changes the current Harness configuration; old Runs retain
their original configuration snapshots. The Harness does not autonomously
change the Project goal, evidence policy, writable documents, autonomy level,
or stop conditions in the first version.

## Epistemic boundaries

The following are qualitatively different and must remain different in storage,
prompts, UI, and generated documents:

| Material | May guide research? | May support a factual claim? |
| --- | --- | --- |
| Extracted source evidence | Yes | Yes, with a resolvable citation |
| Reader note | Yes | No |
| Prior user chat | Yes | No |
| Prior assistant chat | Yes, cautiously | No |
| Agent synthesis | Yes | No; its component evidence must support the claim |
| Research gap | Yes | No; it is an inference from bounded coverage |
| Hypothesis | Yes | No; it is explicitly speculative |
| Experiment idea | Yes | No; it is a proposal |

Additional invariants:

- A source-supported entry must resolve to inspected evidence.
- Notes and chats may generate queries but cannot become citations.
- Repeating a generated statement across Runs never turns it into evidence.
- A gap means "not established by this bounded search," not "nothing exists."
- Contested and superseded entries remain visible in history.

## Interface

### Mapping onto the current shell

i0i already has a global mode rail, left Explorer, workspace tabs, a large
centre surface, and contextual right inspectors. Projects should reuse that
geometry rather than introduce a second navigation system.

| Current surface | Project design |
| --- | --- |
| `VAULT` mode | `PROJECTS` mode |
| Vault Explorer | Project Explorer; the selected Project expands to Research, Vault, and Documents |
| Vault workspace tab | Project workspace tab |
| Vault Home | The Project's Vault view, substantially unchanged |
| Vault Inspector | Remains contextual to the Vault view |
| Discover target Vault | Target Project; accepted Papers enter its one Vault |
| Reader workspace | Remains a Paper tab and remembers the Project from which it opened |
| Right inspector | Project Details, Harness Activity, and Harness Settings while Research is active |

Selecting a Project in the left Explorer opens its Project view in the centre
workspace. It lands on **Research**, not on a generic overview document.

```text
PROJECTS
└── Improve LoRA Interpretability
    ├── Research
    ├── Vault                         42
    └── Documents                      3
        ├── Related work.md
        └── Meeting notes.md
```

### Project view wireframe

```text
┌────────────────────────────────────────────────────────────────────────────────────┐
│ i0i / Improve LoRA Interpretability     : search this project…     42 papers       │
├────┬───────────────────┬────────────────────────────────────────────────────────────┤
│ P  │ EXPLORER          │ # Improve LoRA Interpretability      ● Running · Cycle 8  │
│ R  │                   │   Understand functional structure in LoRA updates          │
│ O  │ PROJECTS          │   [Run now] [Pause] [Stop]                                │
│ J  │ ▾ Improve LoRA…   ├──────────────────────────────────────────┬─────────────────┤
│    │   Research        │ Research     Vault     Documents         │ RESEARCH HARNESS│
│ F  │   Vault       42  ├──────────────────────────────────────────┤                 │
│ I  │ ▾ Documents    3  │ [All] Findings Questions Gaps Hypotheses │ Details Activity│
│ N  │   Related work    │                    Experiment ideas      │          Settings│
│ D  │   Meeting notes  ├──────────────────────────────────────────┤                 │
│    │                   │ FINDING · SOURCE-SUPPORTED               │ Cycle 8         │
│ R  │                   │ Rank allocation is usually treated as   │ 28 inspected    │
│ E  │                   │ an efficiency problem…        6 sources │ 3 accepted      │
│ A  │                   ├──────────────────────────────────────────┤ 2 findings      │
│ D  │                   │ GAP · AGENT SYNTHESIS                    │                 │
│    │                   │ Few studies test whether rank components │ Next direction  │
│ S  │                   │ acquire distinct functions…   4 sources │ LoRA subspaces  │
│ T  │                   ├──────────────────────────────────────────┤                 │
│ U  │                   │ HYPOTHESIS · SPECULATIVE                 │ Improvement  1  │
│ D  │                   │ Individual components may specialize…   │ [Review]        │
│ Y  │                   │                               3 premises │                 │
├────┴───────────────────┴──────────────────────────────────────────┴─────────────────┤
│ Ready · next run tomorrow 09:00 · budget 6/10 papers                               │
└────────────────────────────────────────────────────────────────────────────────────┘
```

The Project header is a UI surface, not a document. It holds the title, concise
goal, Harness status, and primary Run controls. `Research`, `Vault`, and
`Documents` are centre-workspace views.

### Research State list

Research State occupies the main pane of the Research view. It is an
interactive list, not a Markdown file and not a narrow side-panel list.

```text
┌─ RESEARCH STATE ─────────────────────────────────────────────────────────────┐
│ : search current understanding…                          [Create from… ▾]   │
├──────────────────────────────────────────────────────────────────────────────┤
│ [All 38] [Findings 17] [Questions 6] [Gaps 5] [Hypotheses 7] [Ideas 3]      │
│ Sort: recently changed                                      Cycle: All ▾    │
├──────────────────────────────────────────────────────────────────────────────┤
│ ▌FINDING                  SOURCE-SUPPORTED                     Cycle 8       │
│ │ Rank allocation is usually treated as an efficiency problem rather than  │
│ │ as an interpretable functional decomposition.                            │
│ └ 6 sources · 2 supporting syntheses                              [Open ›] │
├──────────────────────────────────────────────────────────────────────────────┤
│ ▌QUESTION                 AGENT SYNTHESIS                      Cycle 8       │
│ │ Do individual LoRA rank components learn stable, separable functions?    │
│ └ motivated by 4 findings · unanswered                            [Open ›] │
├──────────────────────────────────────────────────────────────────────────────┤
│ ▌GAP                      AGENT SYNTHESIS · QUALIFIED           Cycle 7       │
│ │ The reviewed literature contains little direct component-level analysis. │
│ └ bounded search: 63 Papers · 4 related findings                   [Open ›] │
├──────────────────────────────────────────────────────────────────────────────┤
│ ▌HYPOTHESIS               SPECULATIVE                           Cycle 8       │
│ │ Rank components acquire functionally distinct roles during adaptation.   │
│ └ 3 premises · no direct evidence                                   [Open ›] │
├──────────────────────────────────────────────────────────────────────────────┤
│ ▌EXPERIMENT IDEA          SPECULATIVE                           Cycle 8       │
│ │ Ablate individual rank components and compare task-specific degradation. │
│ └ tests hypothesis H7                         [Promote to document] [Open ›] │
└──────────────────────────────────────────────────────────────────────────────┘
```

The filter row exposes Findings, Questions, Gaps, Hypotheses, and Experiment
ideas. Each row shows:

- semantic kind;
- epistemic status;
- concise entry text;
- evidence, premise, or context count;
- contested or superseded state when relevant; and
- the Run that last changed it.

Selecting a row opens its evidence, derivation, revision history, and actions
under **Details** in the right inspector. An Experiment idea can be promoted to
a Project document from there.

The list uses text labels in addition to color or icons, so epistemic status is
never communicated by color alone. Keyboard focus moves through rows; opening a
row moves focus to Details and returning restores the selected row.

### Research Harness inspector

The Harness is an application service represented by UI controls and structured
records. It is not a set of documents. While the Project's Research view is
active, the existing right-inspector region contains:

```text
Details | Activity | Settings
```

Activity is the default Harness view when no Research State entry is selected:

```text
┌─ RESEARCH HARNESS ──────────────────────────┐
│ ● Running · Cycle 8                         │
│ Next run  Tomorrow, 09:00                   │
│ [Run now]       [Pause]       [Stop]        │
├──────────────────────────────────────────────┤
│ Details     [Activity 1]     Settings       │
├──────────────────────────────────────────────┤
│ NEEDS REVIEW                                │
│ Improve future search queries               │
│ Target: Search guidance › Preferred concepts│
│                                             │
│ Runs 3, 4, and 6 repeatedly found that      │
│ “LoRA explanation” favored application      │
│ papers over mechanistic studies.            │
│                                             │
│ Before                                      │
│ interpretation, explanation                 │
│                                             │
│ After                                       │
│ interpretation, mechanistic, subspace,      │
│ intrinsic dimension                         │
│                                             │
│ Preview query                               │
│ "LoRA mechanistic subspace analysis"       │
│                                             │
│ [Reject]       [Edit]      [Accept next run] │
├──────────────────────────────────────────────┤
│ CURRENT RUN                                 │
│ ● Searching Brave · 18 candidates           │
│ ● Searching arXiv · 12 candidates           │
│ ● Deduplicated · 24 Papers                  │
│ ◌ Inspecting · 7 / 10                       │
│ ○ Reconciling Research State                │
├──────────────────────────────────────────────┤
│ RUN HISTORY                                 │
│ Cycle 7 · 3 added · converged         [›]   │
│ Cycle 6 · 1 added · budget reached    [›]   │
└──────────────────────────────────────────────┘
```

Settings is a form inside the same inspector, not a settings document:

```text
┌─ RESEARCH HARNESS ──────────────────────────┐
│ ○ Paused                                    │
│ [Run now]                      [Resume]      │
├──────────────────────────────────────────────┤
│ Details      Activity       [Settings]      │
├──────────────────────────────────────────────┤
│ GOAL                                        │
│ Improve LoRA interpretability               │
│ [Edit goal…]                                │
├──────────────────────────────────────────────┤
│ RESEARCH INSTRUCTIONS                       │
│ Prioritize mechanistic and empirical work.  │
│ Treat application-only papers as background.│
│ [Edit instructions…]                        │
├──────────────────────────────────────────────┤
│ SEARCH GUIDANCE                             │
│ Prefer  mechanistic · subspace · ablation   │
│ Exclude application-only                    │
│ [Edit guidance…]                            │
├──────────────────────────────────────────────┤
│ SCHEDULE                                    │
│ Run       [Weekly ▾]   on [Monday ▾]        │
│ Next      Mon 7 Sep, 09:00                  │
├──────────────────────────────────────────────┤
│ AUTONOMY                                    │
│ [Propose changes ▾]                         │
│ ☑ Add accepted Papers to this Vault         │
│ ☐ Update selected Documents automatically   │
├──────────────────────────────────────────────┤
│ SOURCES                                     │
│ ☑ Browser / Obscura  ☑ arXiv  ☑ OpenAlex   │
├──────────────────────────────────────────────┤
│ PER-RUN BUDGET                              │
│ Inspect [10] Papers       Cost limit [—]    │
├──────────────────────────────────────────────┤
│ STOP WHEN                                   │
│ [20] cycles or [3] unproductive cycles      │
├──────────────────────────────────────────────┤
│ CONFIGURATION HISTORY                       │
│ v4 · accepted query vocabulary change [›]  │
│ [View effective instructions]               │
│                                             │
│                              [Save settings] │
└──────────────────────────────────────────────┘
```

- **Details** explains the selected Research State entry and links to its
  evidence or premises.
- **Activity** shows Runs, ordered events, operational observations, and Harness
  Improvement proposals. A pending proposal appears as a review card at the top
  rather than being buried in the chronological log.
- **Settings** shows the goal, schedule, budgets, sources, autonomy, writable
  documents, and stop conditions. It also shows accepted improvements as
  configuration history.

The Project header keeps status and `Run now`, `Pause/Resume`, and `Stop`
visible while any Harness-inspector section is selected.

A pending improvement adds a count badge to **Activity**. Reviewing it shows
the contributing Runs, target setting, before/after values, and example effect
before the user chooses **Accept**, **Edit**, or **Reject**. Acceptance updates
the next-Run configuration; it never changes the active Run. Edit lets the user
adjust the proposed patch without editing an internal system prompt. Rejection
preserves the proposal and decision in Activity.

The first version does not need a fourth permanent **Improvements** section. If the
number of proposals later warrants a dedicated view, it can be added as a
filtered Activity view without changing the domain model.

When **Vault** is active, the current paper/suggestion inspector remains on the
right. When **Documents** is active, that area may hold document outline and
citation details. The compact Harness status in the Project header remains
visible; activating it returns to Research → Activity. The Harness does not
displace the Reader's Info, Notes, and Chat inspector.

Research Loop is therefore behavior controlled by UI elements, not a document
or a node in the left navigation.

### Creating research outputs

One action turns selected Research State into an ordinary Project document:

```text
Create from research…
├── Survey
├── Related-work section
├── Research-gap analysis
├── Hypothesis report
├── Experiment plan
└── Custom document
```

The choice determines the initial structure and generation instruction. It does
not create a new permanent product area.

Experiment ideas remain cards or rows in Current until the researcher promotes
one into a full document.

## Checkpoints

Every completed Run is a checkpoint for audit, inspection, and restoration of
the same Project. Project forking and branching are deliberately deferred; a
checkpoint does not create a new Project or Vault.

## Example lifecycle

```text
1. Create Project “Improve LoRA Interpretability”
2. i0i creates its empty Vault and inactive Harness
3. Add seed Papers or import an existing Vault during migration
4. Configure a weekly Run with a ten-Paper review budget
5. Run manually once
6. Inspect accepted Papers, Findings, rejected candidates, and Activity
7. Enable scheduled Runs
8. Review a proposed query improvement after several cycles
9. Create a research-gap analysis from Cycle 6
10. Pause the Project while preserving its checkpoints and next direction
```

## Settled decisions

- Project is the goal-directed workspace.
- Every Project owns exactly one Vault.
- Every Vault belongs to exactly one Project.
- Papers may belong to many Vaults and remain canonical.
- Vault is the product term for the Project bibliography.
- Survey, experiment plan, and similar outputs are ordinary Documents.
- Experiment ideas can begin as structured UI entries.
- Research Loop controls live in a contextual side panel.
- Activity is structured Harness history, not the whole Harness.
- Research State is structured and typed rather than one growing dossier.
- Notes and chats are context, not source evidence.
- A completed Run is a checkpoint.
- Project forking and branching are outside the current design.

## Open design questions

These decisions belong to focused follow-up RFCs:

1. Whether Markdown Documents are database-backed, filesystem-backed, or use a
   database index over files.
2. How scheduled Runs execute when the desktop app is closed.
3. Which Research State changes Automatic mode may apply without review.
4. How evidence locators reuse the existing extraction, chunk, and annotation
   structures.
5. How much of the Harness panel remains visible while reading a Paper versus
   editing a Project document.
6. How Research State revisions are stored efficiently without adopting full
   event sourcing.

## Delivery boundary

This document is the design north star, not an instruction to implement the
whole system at once. RFC 0108 records the decision and orders the required
focused implementation RFCs.
