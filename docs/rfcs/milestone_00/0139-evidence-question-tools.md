# RFC 0139: Evidence Questions for Papers and Vaults

- Status: Complete
- Date: 2026-09-06
- Parent: [Milestone 00, M00-10a](../../../milestones/milestone_00.md)
- Depends on: RFCs 0129, 0134; shared accounting from RFC 0135 for managed runs
- Implementation requires separate approval

## Outcome

An agent can ask a focused question of a paper or Vault and receive an answer
with source references. This is optional delegation; direct reading remains a
complete path through the research loop.

## Tools

| Tool | Input | Result |
| --- | --- | --- |
| `reader_ask` | Paper ID, question, optional passage references | Answer, cited passage records, coverage and usage |
| `vault_ask` | Vault ID and question | Answer, relevant paper IDs, cited passage records, coverage and usage |

Reuse existing retrieval and LLM services, extracting only the focused answering
operation needed from chat orchestration. Do not call a chat command that silently
creates a visible thread. These tools do not save notes, conversations, or State.

Restrict retrieval to the scoped paper or Vault. This tool does not automatically
search the web, ask itself recursively, or invoke another research agent.
`vault_ask` can answer "which papers discuss X" using returned metadata and source
evidence; it must distinguish metadata matches from inspected full-text matches.

Use a configured model and bounded retrieved context, initially at most 8 passages
and 12,000 characters per answer. Count delegated model calls and returned source
text against shared limits. If text is pending or unavailable, report coverage;
do not block indefinitely or fill gaps with imagined paper content.

Return cited passage text with RFC 0129 references so the caller can inspect the
evidence behind the interpretation. Resolve model citation labels against the
retrieved set. Unknown labels fail validation instead of linking to another paper.
Return a clear insufficient-evidence answer when warranted. The model answer
itself is not a new primary source; State updates cite the underlying passages.

## Verification and Acceptance

- Fixture questions produce resolvable citations to the scoped sources.
- Offline response fixtures cover invented citation IDs, abstract-only coverage,
  insufficient evidence, and cross-vault references.
- Tests confirm no thread/note/State creation and no recursive search calls.
- An explicit live question check records its model, passages, and answer for
  review; normal tests do not invoke the LLM.
- Failure or cancellation consumes no unstarted delegated work and reports any
  actual usage already incurred.

## Implementation Record

Implemented on 2026-09-07 as the `reader_ask` and `vault_ask` tools on the
existing project-scoped MCP server. Both tools share a bounded evidence-question
service that uses local hybrid retrieval, distinguishes full text, abstracts,
metadata matches, and unavailable material, and makes at most one configured
model call. Answers may cite only the registered RFC 0129 passages supplied to
that call; unknown or inconsistent inline citation labels are rejected.

Managed Runs charge inspected text and started model calls against their shared
limits. Only citations from a validated answer become eligible for later State
updates. Asking a question does not create a chat thread, note, State revision,
or nested search run. Unavailable source material returns deterministic
insufficient-evidence coverage without calling a model.

The real MCP transport tests cover scoped paper questions, supplied passage
references, abstract-only and unavailable coverage, Vault retrieval, no hidden
writes, and no recursive search. Storage tests cover managed accounting and
citation eligibility. The full Rust library suite passes with 594 tests and 8
explicit/live tests ignored; `pnpm check` passes with no errors or warnings. The
explicit OpenRouter check also passes: the configured model correctly reported
insufficient evidence for a deliberately weak passage without inventing a
citation.
