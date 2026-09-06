# RFC 0130: Agent Reader Notes

- Status: Implemented and verified
- Date: 2026-09-06
- Parent: [Milestone 00, M00-04b](../../../milestones/milestone_00.md)
- Depends on: RFCs 0128, 0129
- Implementation approved and completed on 2026-09-06

## Outcome

An agent creates a note on a passage it read, and that note appears in the normal
i0i reader with its quoted text, source location, and agent authorship.

## Tools

| Tool | Input | Result |
| --- | --- | --- |
| `reader_add_note` | Paper ID, optional passage reference, body, request ID | Persisted note/thread IDs, quote, location, author |
| `reader_list_notes` | Paper ID, optional cursor/limit | Existing notes and annotations, anchors/authors, next cursor |

Resolve anchors from RFC 0129's references. Do not trust model-supplied rectangle
coordinates or substitute a matching sentence from another document version.
Without an anchor, create a document-level note. A metadata abstract reference
can support a quoted metadata note but must not masquerade as a PDF selection.

Reuse ChatService and existing threads. Current ThreadAnchor variants include
text offsets and PDF rectangles; map source passages to these only where real
geometry or offsets are available. For a PDF span without reliable rectangles,
persist its exact quote and versioned page location and support navigation to
that page. Add a narrow source-passage anchor if needed. Do not invent highlight
geometry or silently attach the note to the whole document instead.

Record the calling agent and optional run separately from researcher authorship.
Use a transaction to persist the note and retry receipt according to RFC 0128.
Return the same note on a retried request, and reject the same request ID with a
different payload. Validation failures write nothing.

Emit the existing thread/library refresh mechanism after commit. Note listing
uses stable ordering and RFC 0128's pagination convention. Notes remain context,
not primary evidence for a finding merely because an agent wrote them.

## Verification and Acceptance

- A real MCP call creates a note visible in the open Reader without reload.
- Clicking it reveals the correct source location and exact quoted passage.
- Test document-level and passage notes, duplicate text on different pages,
  missing geometry, stale source references, and out-of-scope papers.
- A lost-response retry yields one note and retains agent/run provenance.
- Existing user-created text and rectangle notes still render and navigate.

This RFC adds note creation and listing, not note editing/deletion tools, a new
annotation editor, or an LLM question-answering service.

## Implementation Notes

The authenticated MCP endpoint now exposes `reader_add_note` and
`reader_list_notes`. Passage notes accept only opaque references issued by
`reader_read` to the same connection grant. Before writing, the backend verifies
the paper, source, extraction, offsets, and abstract version are still current.
Document notes omit the passage reference.

Agent passages use a `sourcePassage` thread anchor. It stores exact source
offsets and quoted text plus a zero-based PDF page when available; it never
invents rectangle geometry. Opening the thread navigates the PDF to that page or
focuses the HTML offsets. Agent caller and run identity are persisted separately
from user authorship and shown in the normal Reader thread.

The note and its mutation receipt commit in one SQLite transaction. Identical
retries return the original thread and entry IDs; conflicting reuse of a request
ID is rejected. The listing is a stable paginated snapshot and includes both
thread notes and notes attached to ordinary Reader annotations. A committed MCP
write emits `chat_scope_updated`, causing an open Reader to refresh without a
page reload.

Verification includes a real MCP client performing read, anchored write,
idempotent retry, and listing; transaction tests retain an existing user note
and agent/run provenance. The complete Rust suite passes (557 tests, 7 ignored
live tests), and `pnpm check` reports no diagnostics.
