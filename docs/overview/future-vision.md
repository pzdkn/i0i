# i0i Future Vision

Status: Draft  
Product: i0i

## Vision

i0i should become an IDE for knowledge curation.

The long-term product is not only a place to store papers or notes. It is a workspace where users can interact with their curated knowledge, discover new sources, ask questions across papers, distill insights, and grow a durable research memory over time.

## Core Future Thesis

The strongest future direction is:

```text
AI agents operate over the user's curated literature and notes,
then leave behind durable, inspectable artifacts.
```

This is different from a transient chat session. The output of AI work should become part of the user's knowledge base.

## Future Feature Pillars

### Configurable Search Agent

A search agent should help users discover new literature on an ongoing basis.

Possible controls:

- number of papers to return
- sources/providers
- venues
- date range
- keywords
- topics
- recurring schedule
- inclusion/exclusion criteria

Example:

```text
Every week, find the five most interesting explainable AI papers relevant to my research,
and summarize them with respect to the problem they solve and their methodology.
```

The agent should produce:

- candidate papers
- source/provenance
- rationale for each candidate
- short summaries
- suggested vault targets
- saved search report artifact

### Ask Papers And Collections

i0i should support questions over different context scopes:

- selected text
- one paper
- saved notes for one paper
- one vault
- a collection of papers
- summaries and previously saved insight artifacts

Examples:

```text
Explain the methodology of this paper.
```

```text
Compare these three papers with respect to their assumptions.
```

```text
Among the papers in my interpretability vault, find a common unaddressed gap that can be formulated into a research question.
```

### Durable Insight Artifacts

AI answers should not disappear into chat history.

Useful outputs should become artifacts such as:

- paper notes
- vault notes
- comparative tables
- literature briefs
- research-gap memos
- reading plans
- open questions
- claim/evidence summaries
- "things I now believe" records

These artifacts should remain connected to the papers, notes, and prompts that produced them.

### Agentic Research Workflows

Future i0i agents could be wired together into multi-step research routines.

Possible agent actions:

- search for candidate papers
- inspect papers in a vault
- summarize papers
- compare papers
- identify gaps
- propose research questions
- hand off artifacts to another agent
- run multiple discussion/refinement rounds

Example workflow:

```text
search agent
  -> candidate papers
  -> summarizer agent
  -> comparison agent
  -> gap-finder agent
  -> saved research memo
```

The interface could eventually borrow ideas from tools like n8n or Simulink, but i0i should not start there. The first version should feel like a research assistant with saved outputs, not a visual programming product.

### Comparative Insights

i0i should help users compare papers and collections.

Possible comparisons:

- problem addressed
- methodology
- datasets
- assumptions
- limitations
- evaluation metrics
- claimed contributions
- relation to prior work

Example:

```text
Find all papers between 2011 and 2022 that use the MS COCO dataset,
then compare what task they used it for.
```

### Knowledge Growth Model

i0i could model what the user knows and where they could grow.

Possible features:

- track topics the user has read deeply
- identify weak areas in a vault
- show how knowledge has grown over time
- suggest next papers based on gaps
- surface unresolved questions
- create incentives to keep curating and learning

The goal is not gamification for its own sake. The goal is to make knowledge growth visible.

### Adaptive Reader

The Reader could adapt to what the user already knows.

Possible behavior:

- collapse sections that seem familiar
- expand sections likely to be new or important
- simplify background sections
- highlight methods or assumptions relevant to the user's goals
- connect paragraphs to previous notes or known concepts

This should come after the core Reader, extraction, notes, and AI context layers are reliable.

## Small But Cool Features

### Question Parking Lot

While reading, users should be able to capture unresolved questions without breaking flow.

Examples:

```text
Why do they choose this dataset?
Is this assumption still used in newer papers?
How does this compare to ViT?
Can this method scale to long contexts?
```

This fits naturally into the Reader Inspector:

```text
Notes | Questions | Lineage | Ask | Meta
```

Possible interactions:

- select text, then create an anchored question
- open the Questions tab and type a paper-level question
- later answer a question with AI
- promote a useful answer into a note or insight artifact
- surface repeated questions as possible vault-level research gaps

This is likely one of the best near-term small features because questions are a natural bridge between reading, notes, and AI.

### Paper Memory Card

Each paper could have a compact memory card: a durable compressed understanding of what matters about that paper.

Possible fields:

- why saved
- main contribution
- method
- datasets
- limitations
- useful quotes
- my notes
- open questions

Important product constraint:

```text
The memory card should be passive and on-demand.
It should never pop up after reading or interrupt the user.
```

The card can exist as soon as a paper enters the library, with mostly empty fields. It can be filled manually, drafted by AI on demand, or gradually assembled from notes and questions.

Reading status should stay manual at first:

```text
unread
skimmed
reading
read
reference
```

Later, i0i may suggest memory-card updates, but it should not force them.

### Optional Save Reason

When a paper is saved, the user may eventually want to remember why.

But this should not interrupt the add-to-vault flow.

Better model:

- add paper instantly
- show `why saved` as an optional field in the Paper Memory Card
- allow adding or editing it later
- possibly support a batch-level reason when saving many papers at once

Examples:

```text
uses MS COCO for grounding
good baseline for XAI survey
maybe relevant to sparse attention
cited by the paper I am reading
```

This is useful metadata, but it should remain optional and non-blocking.

### Reference Prefetch

When a paper is opened, i0i could prefetch metadata for the papers it references.

Goal:

```text
current paper references
  -> prefetch referenced paper metadata
  -> click reference
  -> open referenced paper inside i0i
  -> inspect / ask / annotate
  -> optionally save into a vault
```

Why this is cool:

- references become explorable, not dead text
- lineage discovery becomes part of reading
- users can follow curiosity without leaving i0i
- saving a referenced paper into a vault becomes one click away

This should start small:

- parse references from extracted text where possible
- resolve obvious DOI/arXiv/reference metadata
- cache resolved referenced papers locally
- show a lightweight "open in i0i" action
- allow adding the referenced paper to a vault

Non-goal for the first version:

- perfect citation parsing
- full citation graph
- complete bibliography manager
- guaranteed PDF availability for every reference

## Future Sequence

Near future:

- ask current paper with inspectable context
- save AI answers as notes or insight artifacts
- ask a vault or selected paper collection
- better paper discovery and import

Mid future:

- configurable search agent
- saved literature scout reports
- comparative insight artifacts
- research-gap memos
- richer artifact provenance

Far future:

- multi-agent workflows
- recurring scheduled agents
- visual workflow builder
- adaptive Reader
- knowledge growth model

## Product Boundary

The future vision should not turn i0i into only an automation tool.

The center should remain:

```text
curated literature
  -> reading
  -> thinking
  -> durable notes and artifacts
  -> AI-assisted discovery and synthesis
```

Agents are powerful because they operate inside that curated memory, not because they are agents in the abstract.
