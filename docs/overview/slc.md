# i0i Small Lovable Complete

Status: Draft  
Version target: v0.1.0  
Product: i0i

## Product Thesis

i0i is an IDE for knowledge curation.

The core pain is not that researchers lack places to store papers or notes. The core pain is that literature, notes, and AI-assisted thinking are split across tools:

- Zotero organizes literature.
- Obsidian organizes notes.
- Chat tools help explore papers, but the insights are transient.

i0i should become the durable home where papers, reading, annotations, AI conversations, and distilled insights live together.

## Target Audience

Primary:

- academics and researchers
- industrial researchers
- deeply curious technical readers

Secondary:

- knowledge workers
- growth-minded people building durable expertise

## v0.1.0 SLC Promise

The first lovable version should let a user complete this loop:

```text
discover, search, or import a real paper
  -> evaluate candidate papers
  -> add selected papers to one or more vaults
  -> open it in the Reader
  -> create anchored notes
  -> ask something about the paper
  -> save the useful answer as durable knowledge
  -> revisit the paper, notes, and insights later
```

This loop matters because it turns reading and AI exploration into persistent research memory.

## v0.1.0 Must Have

### Vaults

- create vaults
- rename vaults
- delete vaults
- browse papers by vault
- add one paper to one or more vaults
- remove papers from vaults
- see which discovered papers are already in the local library

### Discovery And Search

The SLC must support finding new papers, not only importing papers the user already knows.

Minimum v0.1.0 discovery:

- search for papers from inside i0i
- query multiple providers through a common adapter boundary
- show normalized paper candidates
- show enough metadata to decide whether a candidate is worth saving
- indicate whether a candidate is already in the library
- add one or more candidates to one or more vaults
- preserve source/provenance information for discovered candidates

Discovery should feel like the beginning of the curation loop:

```text
query
  -> candidates
  -> inspect metadata / abstract / source
  -> add to vault
  -> read and think
```

Useful v0.1.0 filters:

- keywords
- date range
- source/provider
- venue or category when supported by the provider

Non-goals for v0.1.0 discovery:

- scheduled discovery agents
- personalized ranking
- exhaustive provider-specific advanced search syntax
- automatic deduplication across every possible metadata variant

### Paper Ingestion

The app needs a way to bring real literature in. Without ingestion, the product loop is incomplete.

Discovery is for finding candidates. Ingestion is for turning a candidate, URL, identifier, or local PDF into a durable local paper.

Minimum v0.1.0 ingestion:

- manual import by DOI, arXiv URL, PDF file, source URL, or metadata entry
- handle the most common paper source patterns researchers actually paste into the app
- import discovered candidates into one or more vaults
- add manually imported papers to one or more vaults
- persist imported paper metadata in SQLite
- persist the local PDF file or a stable pointer to it

The ingestion system should normalize every source into the same local paper model:

```text
external identifier / URL / file
  -> normalized metadata
  -> optional PDF asset
  -> local Paper
  -> Vault membership
```

Provider support should be designed as adapters:

```text
SearchProvider
  -> search(query, filters)
  -> returns normalized paper candidates

ImportProvider
  -> resolve(identifier/url/file)
  -> returns normalized paper metadata and optional PDF/source assets
```

The exact v0.1.0 provider list should be decided separately, but the product should be built around the top common source types researchers actually use:

1. DOI
2. arXiv
3. PubMed
4. Semantic Scholar
5. OpenAlex
6. Crossref
7. ACL Anthology
8. OpenReview
9. publisher landing pages such as ACM, IEEE, Springer, Nature, Elsevier, and similar
10. local PDF files

This does not mean every provider must be equally deep in v0.1.0. It means i0i should not hard-code itself around one source. The adapter boundary matters from the beginning.

Non-goal for v0.1.0:

- full Zotero replacement
- scheduled search agents
- perfect metadata normalization
- perfect coverage for every publisher website

### Reader

- open a paper from a vault
- display the original PDF
- support text mode by extracting text from the PDF
- represent extracted figures and captions where possible
- turn extracted content into a readable text or markdown-like representation
- show paper metadata
- keep Reader layout stable and focused

The Reader does not need a perfect PDF annotation engine in v0.1.0, but it does need to respect the original paper as the source of truth.

Reader modes:

```text
PDF mode
  original paper view

Text mode
  extracted text, sections, figure captions, and rough markdown-like structure

Split mode
  optional later mode for PDF + extracted text side by side
```

Extraction should produce a local representation that later features can reuse:

```text
pdf
  -> extracted text blocks
  -> figure/caption references where possible
  -> source offsets or anchors
  -> readable text/markdown view
  -> AI context chunks
```

This extraction layer is important because notes, highlights, and AI context all need stable references back to the source paper.

### Anchored Notes

- select text
- create a note attached to that selected span
- persist note body and anchor
- edit note body
- remove note
- show saved note highlights in the Reader
- click a note in the Inspector to return to the anchored source text

### AI Context And Ask

Minimum v0.1.0 version:

- ask a question about the current paper
- choose what context is provided to the AI
- show an answer in the Reader Inspector
- allow saving a useful answer as a durable note or insight artifact

This is the key bridge from transient AI chat to persistent knowledge.

Context control is part of the product, not an implementation detail. The user should be able to understand and shape what the AI sees.

Minimum context controls:

- scope: selected text, current paper, selected notes, whole vault
- prompt: system prompt or task framing
- source mode: full extracted text, summary, saved notes, or previous insight artifacts
- agent access: whether an agent can inspect other papers in the vault
- visibility: show which papers, notes, summaries, or chunks are included

The Ask interface should make the active context visible:

```text
question
  + system prompt / task framing
  + context scope
  + included papers / notes / summaries
  -> answer with cited sources
  -> save as note or insight artifact
```

This creates a shared context layer:

```text
Ask this paper
Ask this vault
Agent literature scout
Agent research-gap finder
```

All of them need the same core ability:

```text
construct an inspectable AI context from papers, notes, summaries, prompts, and vault scope
```

Later, agents can use the same context machinery to access papers in a vault, inspect summaries, and produce durable artifacts.

### Persistence

- use local SQLite
- keep vaults, papers, memberships, and notes durable across app restarts
- keep the data model simple enough to inspect and evolve

## v0.1.0 Should Not Include

- visual agent workflow builder
- recurring cron jobs
- multi-agent orchestration
- fully autonomous literature scouting
- complex citation graph editing
- full PDF annotation engine
- perfect PDF text and figure extraction
- cloud sync
- collaboration
- account system

These are promising, but they are not needed for the first complete product loop.

## Differentiating Future

Agents are likely a major differentiator, but they should build on top of the core curated memory loop.

The future agent thesis:

```text
Agents operate over your curated research memory
and leave behind inspectable, reusable artifacts.
```

Examples:

- weekly literature scout for a vault
- compare new papers against existing notes
- find common gaps across a collection
- produce a research-question memo
- generate candidate papers to add to a vault
- update a saved literature brief

The key product distinction is not merely "chat with papers." It is:

```text
AI work becomes durable, inspectable, and connected to your curated literature.
```

## Suggested Product Sequence

```text
v0.1.0
  discovery/search + multi-provider ingestion + PDF reader + text extraction + vaults + anchored notes + context-aware ask paper + save insight

v0.2.0
  better extraction quality + ask vault + richer insight artifacts

v0.3.0
  first predefined research agent, likely a literature scout

later
  configurable agents, scheduling, multi-source search, workflow builder
```

## Success Criteria

v0.1.0 is successful if a user can say:

```text
I brought a real paper into i0i,
or discovered one from inside i0i,
read it,
captured my own notes,
asked useful questions,
saved the useful answers,
and can find that thinking again later.
```

That is small, lovable, and complete.
