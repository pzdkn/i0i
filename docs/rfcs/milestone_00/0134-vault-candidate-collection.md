# RFC 0134: Vault Candidate Collection

- Status: Implemented
- Date: 2026-09-06
- Parent: [Milestone 00, M00-06b](../../../milestones/milestone_00.md)
- Depends on: RFCs 0129, 0133
- Implemented: 2026-09-06

## Outcome

An agent saves a discovered paper into its scoped Vault and can subsequently
read the acquired document through Reader tools. Saving does not open a tab.

## Tool Contract

`vault_add_paper(vault_id, candidate_id, request_id)` returns paper ID, membership
status, source IDs, and acquisition/text status. The candidate must be accessible
through the caller's scoped searches. Do not accept arbitrary filesystem paths
or substitute a caller-supplied metadata object for the stored candidate.

Reuse the existing library addition and source-acquisition services. Preserve
metadata, identifiers, URLs, and discovery provenance. Apply existing identity
normalization; this RFC does not add speculative fuzzy title deduplication.
Repeated additions reuse paper identity and Vault membership.

Persist the paper/membership, retry receipt, and acquisition intent together.
Queue acquisition after commit using the same recoverable mechanism as other
saved papers. Return promptly while downloads/extraction continue. Reader and
metadata tools expose their progress. Acquisition failure leaves an honest saved
metadata record with its source URL; it does not claim readable full text.

## Shared Work and UI

Multiple searches may discover the same paper. Acquisition belongs to the durable
document source once saved, not solely to the search that discovered it.
Canceling one search or the parent research run stops its waiting/research work
but does not delete a saved document or cancel acquisition required by another
consumer. App shutdown retains acquisition intent for the normal startup queue.

Emit existing library/source events so Vault metadata and Reader availability
refresh. Do not automatically open imported or discovered papers. The agent can
wait for text by polling bounded Reader calls with the parent's deadline.

## Verification and Acceptance

- Real MCP save followed by Reader calls returns fixture document text.
- Duplicate candidates and lost-response retries produce one paper membership.
- Acquisition failure, pending extraction, and HTML/PDF availability are visible.
- Two consumers share a source; canceling one does not remove the other's data.
- UI membership refreshes without opening a Reader tab.
- No Research State change occurs merely because a candidate was saved.

## Implementation Notes

- `vault_add_paper` resolves candidate metadata only from project-scoped agent
  Search events. Callers cannot inject metadata or filesystem paths.
- Paper metadata, one Vault membership, durable PDF source intent, and the retry
  receipt commit in one immediate SQLite transaction. Same-payload retries return
  the original receipt; duplicate additions retain one membership.
- Available PDF sources enter the existing download and extraction queues after
  commit. Candidates without a PDF remain honest metadata records and return the
  landing URL with `unavailable` acquisition status.
- The backend emits `library_updated`; Svelte refreshes its library cache without
  changing the selected tab or opening the saved paper.

Verification completed with 566 Rust tests passing, 7 explicit live/manual tests
ignored, and `pnpm check` reporting zero errors or warnings. The real-MCP slice
saves and retries one candidate, observes one Vault membership, completes a
fixture extraction, and reads the resulting full-text passage.
