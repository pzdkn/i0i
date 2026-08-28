# RFC 0107: Relevant Research History for Paper Chats

- Status: Approved
- Date: 2026-08-28
- Area: Chat / Reader
- Builds on: RFC 0034, RFC 0061, RFC 0077, RFC 0078, RFC 0079

## Summary

Give every paper-scoped chat access to relevant notes and conversations already
created for the same paper. Search this **research history** for each question,
include only a small relevant set, and show the reader exactly what was used.

Research history is enabled in `Relevant` mode by default. The Context surface
lets the reader choose `Relevant`, `Notes only`, or `Off` for a thread. There
is no "include everything" mode: replaying every note and conversation would
consume the context window, slow every answer, and amplify obsolete assistant
output.

## User Goal and Mental Model

Notes and earlier chats are the reader's accumulated understanding of a paper.
When asking a new question, the reader should not need to remember which thread
contains a prior conclusion or manually paste an earlier note. They expect i0i
to remember relevant work attached to the paper while keeping the current
question grounded in the paper itself.

The feature has three qualitatively different inputs:

- **Source evidence** is extracted paper text and remains citable with `[C…]`.
- **Reader notes** are the reader's interpretations, questions, reminders, or
  hypotheses. They describe the reader's thinking, not facts about the paper.
- **Conversation history** is prior dialogue. User turns express earlier
  questions or instructions; assistant turns are generated artifacts that may
  be incomplete or wrong.

Reader notes and conversation history are collectively **research history**.
Research history can guide continuity and attention, but it is never evidence
and never independently supports a factual claim.

## Current Behavior

`ContextManager` currently assembles:

1. the current selection;
2. summaries and persistent passage context for the open thread;
3. the open thread's own entries;
4. paper chunks retrieved for this turn; and
5. the paper-text fallback.

The current thread already has conversational continuity because its entries
are replayed. What is missing is **cross-thread research history**:

- notes stored on the paper's highlights; and
- questions and answers in the paper's other chat threads.

Both are already paper-scoped in SQLite, but neither is searchable by
`ContextManager`. `SearchService` indexes paper text only.

## Options Considered

### Include all notes and chats on every turn

This is simple and maximally recall-oriented, but it scales badly. A heavily
annotated paper could spend most of the prompt on unrelated history. Old
assistant answers would also be repeated as if they were current knowledge.

**Rejected.** It is surprising, expensive, and weakens grounding.

### Require manual selection for every history item

The existing persistent Context list could be extended so the reader manually
adds each note or chat. This is controllable but does not solve the primary
problem: the reader must already remember where the relevant thought lives.

**Rejected as the default.** Explicitly attaching a historical artifact can be
a separate follow-up if automatic relevance retrieval proves insufficient.

### Search research history automatically, with a visible setting

Run one bounded local search using the current question and selected passage.
Include the strongest matching notes and prior turns, expose them in the
answer's Context details, and allow history retrieval to be narrowed or
disabled.

**Selected.** It provides continuity without turning the full paper history
into ambient prompt baggage.

## Decision

### 1. Add two distinct research-history sources

Research history contains two non-interchangeable source types attached to the
current paper but outside the current chat thread:

- **Reader notes:** non-empty user notes on highlights, including their
  anchored excerpt for orientation; and
- **Conversation history:** user questions and assistant answers from other
  threads, retaining their roles and order.

Do not flatten these into generic text records at the domain or prompt layer.
Their different authorship and reliability are part of their meaning.

Exclude:

- the current thread, because its entries are already replayed;
- deleted notes, highlights, threads, or entries;
- other papers and vault-wide content;
- transient streaming text and failed turns; and
- generated context summaries, to avoid recursively summarizing summaries.

Current-thread exclusion is by stable thread id, not title or anchor.

### 2. Search locally and deterministically

Create a paper-scoped research-history search seam owned by `ContextManager`.
Back it with SQLite FTS5 over normalized history documents rather than adding
another model call before every answer.

One indexed history document represents either:

- one user note plus its anchored paper excerpt; or
- one prior conversational turn, grouping a question with its following
  assistant answer when both exist.

Search text is the current question plus the selected passage, when present.
Return at most six history items and spend at most 2,000 estimated tokens
inside the existing context budget. Do not increase the overall model-context
cap.

Ranking uses FTS relevance first, with bounded tie-break preferences:

1. user notes;
2. conversations whose user question matches strongly;
3. conversations containing a pinned assistant answer;
4. other conversations; and
5. newer activity when relevance is otherwise equal.

These preferences improve trust and continuity without special-casing paper
titles, note text, or individual queries.

### 3. Encode the qualitative boundary in types, prompts, and references

Render the two history types in separate prompt blocks with separate handle
namespaces:

```text
Reader notes — subjective reader-authored material, not source evidence:
[N1] Reader note · passage on page 4
This assumption seems stronger than the ablation supports.

Conversation history — prior dialogue, not source evidence:
[H1] Prior conversation · "Why does normalization help?"
Earlier answer: ...
```

The model may use research history to preserve continuity, recognize the
reader's ideas, recall unresolved questions, or avoid repeating prior work. It
must:

- describe `[N…]` as something the reader noted, suspected, asked, or intended;
- preserve user and assistant roles in `[H…]` instead of presenting a chat as
  one shared conclusion;
- treat every earlier assistant answer as fallible generated output;
- use `[C…]` source evidence for claims attributed to the paper;
- never use `[N…]` or `[H…]` as factual support or corroboration; and
- explicitly frame an unsupported historical claim as prior thinking rather
  than silently carrying it forward.

If a note or prior chat raises a factual claim relevant to the new question,
the retrieval phase should use it to formulate a paper search, then ground the
answer in matching `[C…]` passages. When no source evidence supports it, the
answer may discuss it only as the reader's note or a prior conversational
claim, with that uncertainty intact.

`[N…]` and `[H…]` identify provenance, not citations. Clicking one opens the
originating note or thread. Their references live in different fields from
`ContextCitation`; they never enter the paper-evidence citation map, citation
count, or References UI.

### 4. Make `Relevant` the default and keep control near Context

The Context disclosure in the open chat gains a compact **Research history**
control with three modes:

| Mode | Behavior |
| --- | --- |
| `Relevant` | Search notes and prior chats for every question. Default. |
| `Notes only` | Search user notes but exclude prior conversations. |
| `Off` | Do not search or include research history. |

This is progressive disclosure, not a primary composer action. It belongs next
to the existing Context controls because that is where readers inspect what the
model can see. The selected mode persists per thread. A new or virtual thread
starts in `Relevant` mode.

After an answer, the existing context summary shows a compact line such as:

```text
History used: 2 notes · 1 prior chat
```

Expanding it lists the exact items, their source type, a bounded preview, and
an action to open the originating note/thread. When no item matches, show
`History searched · no relevant items` in details only; do not add empty-state
noise beside every answer.

The control is keyboard reachable, has an accessible label that includes the
current mode, and never changes mode merely because a search returned nothing.

### 5. Preserve history provenance without making it persistent context

History selected for one answer is ephemeral. Persist its typed reference
metadata on that answer so reopening the thread still shows what the model saw.
Do not add it to `chat_context_items`: automatically retrieved history is
recalculated for each question, while explicitly kept source passages retain
their existing, separate lifecycle.

### 6. Fit research history into ContextManager's existing precedence

Prompt assembly becomes:

1. current selection;
2. compaction summaries;
3. current-thread entries;
4. explicit persistent context;
5. relevant reader notes, then conversation history;
6. paper chunks retrieved for this question; and
7. paper-text fallback with the remaining budget.

The selection and current thread retain their current behavior. Research
history, retrieved chunks, and fallback text share the existing bounded context
budget. When space is tight, explicit user-kept source context wins over
automatic history retrieval.

## Data and Interfaces

Introduce domain types rather than overloading paper citations:

```rust
enum ResearchHistoryMode {
    Relevant,
    NotesOnly,
    Off,
}

struct ReaderNoteRef {
    handle: String,
    source_id: String,
    highlight_id: String,
    preview: String,
}

struct ConversationHistoryRef {
    handle: String,
    thread_id: String,
    question_entry_id: String,
    answer_entry_id: Option<String>,
    thread_title: String,
    question_preview: String,
    answer_preview: Option<String>,
}
```

`ChatContextSummary` gains separate `reader_notes` and
`conversation_history` collections plus a flag recording whether history
search ran. Old stored summaries deserialize with empty collections and
`false`.

`chat_threads` gains a research-history mode defaulting to `relevant`. The
virtual anchor ask carries its selected mode until the first successful turn
creates the thread.

The research-history FTS index is derived data. Inserts, edits, and deletes of
notes and chat entries keep it synchronized; startup migration can rebuild it
from the canonical tables. Indexed rows retain their source type and authorship
instead of erasing those distinctions.

## Scope

### In scope

- Same-paper reader notes and cross-thread conversations as separately typed,
  searchable research history.
- Default relevance search with `Relevant`, `Notes only`, and `Off` modes.
- Bounded local FTS retrieval inside the existing context budget.
- Separate `[N…]`/`[H…]` provenance and inspectable per-answer history
  summaries.
- Navigation back to the originating note or thread.
- Backward-compatible storage and serialization.

### Out of scope

- Automatically including all paper history.
- Cross-paper or vault-wide research history.
- Treating notes or assistant answers as paper evidence.
- Embedding/vector search for research history in the first implementation.
- Editing notes or old messages from the context drawer.
- Automatically rewriting, merging, or deleting prior history.
- A new model call solely to decide which history items to include.

## Acceptance Criteria

- [ ] A new paper chat can retrieve a relevant note created elsewhere on that
  paper.
- [ ] A chat can retrieve a relevant question/answer pair from another thread
  on the same paper.
- [ ] The current thread is not duplicated through research-history retrieval.
- [ ] Research history from another paper is never returned.
- [ ] `Relevant` is the default; `Notes only` excludes chats; `Off` performs no
  history search.
- [ ] At most six items and 2,000 estimated tokens of research history enter
  one answer without increasing the overall context cap.
- [ ] User notes rank ahead of equally relevant assistant output.
- [ ] Reader notes appear only under `[N…]`; prior chats appear only under
  `[H…]`; neither enters the `[C…]` source-evidence citation map.
- [ ] A factual claim from `[N…]` or `[H…]` is either grounded independently in
  `[C…]` evidence or explicitly described only as prior reader/conversation
  content.
- [ ] Each stored answer reports whether history was searched and which items
  were included.
- [ ] Clicking a history item opens its originating note or prior thread.
- [ ] Deleted or edited notes and chats disappear from or update in search.
- [ ] Old threads and context summaries remain readable after migration.
- [ ] Focused tests cover indexing, scope isolation, ranking, modes, budgets,
  prompt labeling, navigation metadata, deletion, and backward compatibility.
- [ ] Full Rust tests, frontend checks, and the production frontend build pass.

## Verification Plan

1. Add store fixtures with two papers, multiple notes, and multiple threads.
2. Add failing research-history search tests for relevance, ranking, scope
   isolation, current-thread exclusion, edits, and deletes.
3. Add ContextManager tests for modes, precedence, budgets, `[N…]`/`[H…]`
   labeling, role preservation, and separation from `[C…]` evidence.
4. Add UI tests for the mode control, empty/non-empty summaries, and navigation
   to notes and prior threads.
5. Run the full Rust and frontend verification suites.
6. Manually ask a question that depends on a note and then on an earlier chat;
   confirm the answer exposes the correct history sources without presenting
   either as fact or text from the paper.

## Implementation Approval

Approved by the user on 2026-08-28. The user requested this RFC and previously
authorized the agent to write and auto-approve RFCs. Implementation remains a
separate step under the repository's RFC-first workflow.
