# RFC 0140: Evidence-Based Vault Summary

- Status: Implemented; explicit live check pending
- Date: 2026-09-06
- Parent: [Milestone 00, M00-10b](../../../milestones/milestone_00.md)
- Depends on: RFC 0139
- Implementation requires separate approval

## Outcome

`vault_summary` gives an agent a bounded overview of the collection with explicit
coverage, references, and freshness. It does not imply every paper has been read.

## Tool Contract

`vault_summary(vault_id)` returns summary text, paper/source references, captured
membership identity, generation time, and coverage counts: total papers, metadata
considered, abstracts inspected, full-text papers inspected, and unavailable text.
Report model usage with the same accounting used by evidence-question tools.

Capture Vault membership before summarization. Use deterministic stable paper
ordering and a bounded evidence selection, initially up to 25 papers and 12,000
characters of source/metadata context. Report selected paper IDs and whether the
sample was truncated. A summary of a subset must not claim exhaustive findings
or ranking of all papers in a large vault.

Summarize themes, approaches, and useful entry points. Distinguish collection
metadata from claims supported by abstracts or full text. Link each substantive
source claim to the inspected material. Reuse RFC 0139's retrieval/citation
validation rather than introduce another agent or search loop.

Generate on demand. Initial implementation needs no persistent summary cache;
freshness is the captured snapshot plus generation time. If membership changes
during generation, return the snapshot used and indicate newer membership exists.
Do not write the summary into Research State or a user-authored document.

## Failure and Limits

An empty vault returns an empty collection result without an LLM call. An entirely
metadata-only collection may receive a labeled metadata overview. If the delegated
model is unavailable, return an error with coverage metadata, not a fabricated
summary. Caller cancellation and parent usage limits apply.

## Verification and Acceptance

- Tests cover empty, metadata-only, mixed-availability, and truncated collections.
- A concurrent membership update does not mislabel the summarized snapshot.
- References resolve, source coverage counts match actual selection, and no
  notes/State/documents are written.
- An explicit live summary check is reviewed for coverage honesty and citations.
- This completes the agreed tool surface; RFC 0126's final milestone acceptance
  runs after this feature and the native UI checks are complete.

## Implementation Record

Implemented on 2026-09-07 as the project-scoped `vault_summary` MCP tool. It
captures one Vault membership revision, sorts stable paper IDs, samples at most
25 papers, and inspects at most one representative full-text passage or abstract
per sampled paper. Metadata is bounded separately before the remaining portion
of the shared 12,000-character context budget is distributed evenly across
source passages.

The result reports every selected paper, its evidence class and inspected source
reference when present, validated citations, coverage counts, model usage,
generation time, and both captured and current membership revisions. Empty and
metadata-only Vaults return deterministic labeled overviews without an LLM call.
Source-backed summaries reuse RFC 0139's model and citation validator; failures
retain captured coverage and incurred usage. No cache, note, document, or State
write is created.

Real MCP transport tests cover empty, metadata-only, mixed, truncated, and
concurrently changing Vaults. They verify resolvable references, stable sampling,
honest counts, bounded context, failure details, and absence of hidden writes.
The full Rust library suite passes with 600 tests and 9 explicit/live tests
ignored; `pnpm check` passes with no errors or warnings. The explicit live
summary check is implemented but remains pending because OpenRouter reset the
connection on both attempts before returning an HTTP response.
