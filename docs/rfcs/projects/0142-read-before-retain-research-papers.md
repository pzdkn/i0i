# RFC 0142: Source-Complete, Read-Before-Retain Research Papers

- Status: Implemented and verified
- Date: 2026-09-08
- Depends on: RFCs 0133 through 0135

## Problem

A managed Research Run can add papers to the Project Vault without reading them.
In the observed Run `harness_run_1788896330519046000`, the agent made ten
successful `vault_add_paper` calls but called `reader_read` for only five distinct
papers. The Run therefore reported “added 10” and “read 5.”

This is permitted by the current contract. `paperBudget` limits additions, while
`maximumDistinctPapersRead` and `maximumReturnedTextChars` independently limit
reading. The prompt asks the agent to save useful candidates and read relevant
passages, but no invariant connects the two actions.

For i0i, the Project Vault is a curated research library, not the agent's
temporary candidate queue.

The same observed Run also retained five papers without an actionable link:
three had abstract-only source rows whose URL was null, and two had no source row
at all. All five papers with PDF sources were read; none of the five linkless
papers were read. At least one original Search candidate contained a DOI landing
page that was lost when it became a saved Paper.

The cause is concrete: the managed `PaperCandidate -> PaperDraft` conversion
creates a source only when `pdfUrl` exists. `externalUrl` is used merely as that
PDF source's landing page and is discarded when `pdfUrl` is absent.

## Outcome

“Papers to find” means papers to investigate and retain, not papers to dump into
the Vault. Every paper retained by a Run has been opened through the Reader
capability and assessed for the Run's purpose.

Every retained paper also has an actionable provenance link: a local/PDF source,
an HTML source, or at minimum the provider landing page or DOI. Metadata without
any route back to its source is not sufficient for automatic retention.

Not every retained paper must support a final State entry. A useful paper may
provide contradictory evidence, background, a method, or a documented research
limitation. The Run must record why it was retained.

## First Implementation

Keep the existing MCP tools and avoid building a second staging database.

1. The agent may add a candidate because saving currently enables acquisition
   and Reader access.
   The conversion preserves both source locations:
   - with `pdfUrl`, create the PDF source and retain `externalUrl` as its landing
     page;
   - without `pdfUrl` but with `externalUrl`, create an acquireable web source;
   - with neither URL, reject automatic retention and report `no_source`.
2. Before the Run can finish `ready`, every newly added paper must have at least
   one recorded `reader_read` attempt in that same Run.
3. After reading, the agent records a bounded disposition for each new paper:
   `evidence_used`, `background`, `contradictory`, `unavailable`, or `irrelevant`,
   plus a short reason.
4. `evidence_used`, `background`, and `contradictory` remain in the Vault.
   `irrelevant` additions are removed from the Project Vault before finalization.
5. `unavailable` papers remain only when the agent records why their metadata or
   abstract is still useful and an actionable landing page exists; otherwise
   they are removed.

The finalizer validates coverage from persisted `agent_vault_additions` and
`agent_reader_usage`; it does not trust the final prose summary. A missing
disposition prevents a successful finalization and is reported as incomplete.

## Budget Semantics

Rename the visible control from **Papers to find** to **Papers to investigate**.
It is the maximum number of new papers the Run may retain.

Reserve enough reading budget for the requested count. The controller derives a
per-paper text allowance from `maximumReturnedTextChars` and does not permit the
agent to exhaust all reading capacity on early papers while unread additions
remain. Follow-up reads may use remaining capacity after every addition has had
its first assessment.

The agent should normally process candidates incrementally: add, await readable
status, read a focused passage set, decide whether to retain, then continue. It
must not add the full quota first merely because search returned enough titles.

## Future Direction

A later RFC may introduce run-scoped acquisition so candidates can be read before
ever becoming Vault members. That is architecturally cleaner but is not required
for this correction.

## Acceptance

- A ready Run cannot have a newly retained paper with no same-Run Reader attempt.
- A ready Run cannot retain a newly added paper without a PDF, HTML, landing-page,
  DOI, or other actionable source link.
- Candidate `externalUrl` survives saving even when `pdfUrl` is absent, and the
  Reader/Vault UI exposes it as the source fallback.
- The checkpoint reports discovered, attempted, read, unavailable, removed, and
  retained counts separately.
- Every retained paper has a persisted disposition and reason.
- Irrelevant temporary additions are removed without affecting papers that were
  already in the Vault before the Run.
- Repeated add/remove retries are idempotent.
- Cancellation preserves already assessed retained papers and reports unassessed
  additions honestly; restart recovery can finish cleanup safely.
- Reading budget allocation guarantees one bounded first assessment per allowed
  addition before follow-up reading consumes the remainder.
- Tests cover all dispositions, external-URL-only candidates, candidates with no
  source, unavailable extraction, duplicate candidates, cancellation during
  acquisition, and the invariant that retained is a subset of attempted.
- The explicit research-loop evaluation asserts the same invariant.
