# RFC 0129: Reader Evidence Access

- Status: Implemented and verified
- Date: 2026-09-06
- Parent: [Milestone 00, M00-04a](../../../milestones/milestone_00.md)
- Depends on: RFC 0128
- Implementation approved and completed on 2026-09-06

## Outcome

An agent can inspect paper metadata and read bounded passages of real document
text without opening the viewer. Returned references can later anchor notes and
State evidence.

## Tools

| Tool | Input | Result |
| --- | --- | --- |
| `vault_get_paper` | Vault ID, paper ID | Metadata; source/extraction IDs; acquisition and text availability |
| `reader_read` | Paper ID, optional one-based inclusive page range, cursor | Availability, coverage, versioned passages, next cursor |

The paper must belong to the scoped Vault. Return full text only from an actual
saved source/extraction. If acquisition or extraction is needed, request it once
through the existing managers and return pending with an explanation. Subsequent
reads observe progress. A permanent failure returns its reason and source URL;
do not loop extraction indefinitely on every read.

An available abstract may be returned as `coverage: abstract_only` with a
metadata-version reference. It is not a full-text chunk. HTML documents without
page numbers use section/offset references and reject PDF page-range input.

## Passage Contract

Each passage contains its exact text, paper ID, source ID, extraction/version ID,
chunk ID when present, page/section location, and an opaque `passage_ref`. Resolve
that reference server-side to the canonical source span. Preserve the existing
offset convention; expose no ambiguous byte-versus-character offsets to agents.

Bound each response to a configured text budget, initially 12,000 characters.
Split oversized chunks into referenced subspans; do not cut text without a
continuation cursor. Cursors bind to source version and range. Re-extraction must
not reinterpret an old reference as a span in new text. Retain referenced source
versions under existing document retention rules or report unavailability.

Coverage reports what was returned and whether more content exists. It does not
claim the agent understood every passage. Reading may queue acquisition and
extraction but does not create notes or State entries.

## Implementation Boundary

Reuse ReaderService, document acquisition, extraction, and existing chunk storage.
Where Tauri bootstrap is required, use the same services in evaluation. Add only
the targeted text access needed; do not implement a second extraction pipeline or
render PDF pages in MCP. Record passage deliveries for run evidence validation.

## Verification and Acceptance

- Fixed PDF and HTML fixtures yield exact text and resolvable references.
- Test page-range validation, pagination, oversized chunks, pending extraction,
  unavailable sources, and abstract-only coverage.
- Repeated reads reuse acquisition/extraction work; re-extraction does not move
  existing reference targets.
- An MCP client reads a scoped paper without the viewer being mounted.
- Notes, figure extraction, and semantic questions remain separate RFCs.

## Implementation Notes

`vault_get_paper` and `reader_read` now run through the production MCP handler.
PDF reads use persisted `DocumentChunk` rows and their canonical character
offsets. Saved HTML reads use the frozen `source_text` sidecar. Oversized text is
split on Unicode character boundaries and each response is capped at 12,000
characters with a source-version-bound cursor.

Each returned opaque passage reference resolves in the app session to its exact
paper, source, extraction, chunk, page range, character range, and quote. This is
the shared anchor used by later note and State tools. Remote/cached work reuses
the existing download and extraction managers; pending and permanent failures
remain explicit. Abstract-only evidence is labeled and content-versioned by hash.

Focused tests cover a real MCP client reading persisted PDF text, reference
resolution, exact HTML text, pagination, Unicode offsets, page validation,
abstract-only coverage, pending acquisition, and stored failure reasons.
