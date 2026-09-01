# i0i Research Context

i0i helps a reader study papers while keeping source evidence, personal
thinking, and generated conversation distinguishable.

## Language

**Project**:
A goal-directed research workspace that owns one Vault and its project
documents. It is the boundary within which an autonomous research process
operates.
_Avoid_: Workspace, enriched Vault

**Vault**:
The bibliography belonging to one Project. It groups Papers relevant to that
Project; a Paper may belong to many Vaults.
_Avoid_: Bibliography, Project

**Paper**:
A canonical scholarly publication that may belong to multiple Vaults without
duplicating the publication or its source material.
_Avoid_: Vault paper, project paper

**Project document**:
A user- or agent-authored research artifact belonging to a Project, such as a
survey, proposal, or working note. Its content is not source evidence merely
because it belongs to the Project.
_Avoid_: Source, evidence

**Source evidence**:
Text extracted from a paper or another inspected source. It can support factual
claims and carries a resolvable citation.
_Avoid_: Memory, prior knowledge

**Reader note**:
A reader-authored interpretation, question, reminder, or hypothesis attached
to a paper or passage. It records the reader's thinking and is not evidence.
_Avoid_: Fact, evidence

**Conversation history**:
Prior user and assistant turns associated with a paper. It preserves continuity
but may contain generated errors and is not evidence.
_Avoid_: Fact, source, verified answer

**Research history**:
The reader notes and conversation history associated with a paper. It may guide
a new conversation but cannot independently support claims about the paper.
_Avoid_: Paper memory, evidence base
