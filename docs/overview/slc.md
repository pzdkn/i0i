# i0i Small Lovable Complete

Status: Draft  
Version target: v0.1.0  
Product: i0i

## TLDR

i0i is an IDE for knowledge curation.

The first lovable version should help a researcher discover or import a real paper, save it to a vault, read it, create anchored notes, ask AI about visible context, save useful answers as durable knowledge, and find that thinking again later.

Everything beyond that loop belongs in Stretch.

## Product Thesis

Researchers do not lack tools. They lack one durable place where literature, reading, notes, AI conversations, and distilled insights live together.

Today:

- Zotero organizes papers.
- Obsidian organizes notes.
- AI chat helps explore ideas, but useful answers disappear into transient conversations.

i0i should turn reading and AI-assisted thinking into persistent research memory.

## Target Audience

Primary:

- academics and researchers
- industrial researchers
- deeply curious technical readers

Secondary:

- knowledge workers
- growth-minded people building durable expertise

## v0.1.0 Promise

The first complete product loop is:

```text
discover or import a paper
  -> save it to one or more vaults
  -> read it
  -> create anchored notes
  -> ask a question about the paper
  -> save the useful answer as durable knowledge
  -> revisit the paper, notes, and insights later
```

This is the core of i0i: AI work becomes inspectable, reusable, and connected to the source literature.

## v0.1.0 Must Have

### Vaults

- create, rename, and delete vaults
- browse papers by vault
- add papers to one or more vaults
- remove papers from vaults
- show whether a discovered paper is already in the local library

### Discovery And Import

The app must support both finding new papers and bringing known papers into the library.

Minimum support:

- search for papers inside i0i
- show normalized paper candidates with enough metadata to evaluate them
- preserve source and provenance information
- import by common researcher inputs such as DOI, arXiv URL, source URL, local PDF, or manual metadata
- persist paper metadata and local assets, or stable pointers to those assets

Discovery and import should normalize into one local model:

```text
source / identifier / file
  -> normalized metadata
  -> optional PDF asset
  -> local Paper
  -> Vault membership
```

The implementation should keep a provider boundary from the start, so i0i is not hard-coded around one paper source.

### Reader

- open a paper from a vault
- display the original PDF
- extract text into a readable local representation
- show paper metadata
- keep the reading layout stable and focused

The PDF remains the source of truth. Extracted text exists so notes, search, and AI context can refer back to the paper.

### Anchored Notes

- select text
- create a note attached to that source span
- persist, edit, and delete notes
- show note highlights in the Reader
- return from a note to its source location

### AI Ask

- ask a question about the current paper
- make the active context visible
- choose a simple context scope, such as selected text or current paper
- show the answer in the Reader
- save useful answers as notes or insight artifacts

The important product move is not "chat with PDFs." It is turning useful AI output into durable, source-linked knowledge.

### Persistence

- use local SQLite
- keep vaults, papers, memberships, notes, and saved insights durable across restarts
- keep the data model simple enough to inspect and evolve

## v0.1.0 Should Not Include

- visual agent workflow builders
- recurring cron jobs
- multi-agent orchestration
- fully autonomous literature scouting
- full Zotero replacement
- full PDF annotation engine
- perfect metadata normalization
- perfect PDF extraction
- cloud sync
- collaboration
- account system

These may be valuable later, but they are not required for the first complete loop.

## Stretch

Agents are a likely differentiator once the curated-memory loop exists.

The future thesis:

```text
Agents operate over your curated research memory
and leave behind inspectable, reusable artifacts.
```

Possible stretch features:

- chat with an entire vault or paper collection
- configurable literature search agents
- scheduled literature scouting
- comparative insight generation
- research-gap finding across a collection
- saved literature briefs that update over time
- workflow-builder interfaces for agent handoffs
- knowledge-growth views that show what the user knows and where to grow
- adaptive reading that collapses or simplifies familiar sections

Example future tasks:

- find the five most interesting explainable AI papers this week
- find papers from 2011 to 2022 that use the MS COCO dataset
- identify an unaddressed gap across an interpretability vault

## Suggested Sequence

```text
v0.1.0
  vaults + discovery/import + reader + anchored notes + ask current paper + save insight

v0.2.0
  better extraction + ask vault + richer insight artifacts

v0.3.0
  first predefined research agent

later
  configurable agents + scheduling + workflow builder
```

## Success Criteria

v0.1.0 is successful if a user can say:

```text
I brought a real paper into i0i,
read it,
captured my own notes,
asked useful questions,
saved the useful answers,
and found that thinking again later.
```

That is small, lovable, and complete.
