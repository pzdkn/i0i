# RFC 0128: Local MCP Server and Vault Listing

- Status: Implemented and verified
- Date: 2026-09-06
- Parent: [Milestone 00, M00-03](../../../milestones/milestone_00.md)
- Depends on: existing LibraryStore; independently testable from RFC 0127
- Implementation approved and completed on 2026-09-06

## Outcome

Expose one authenticated loopback MCP server owned by i0i. An external client can
list its authorized vault and papers using the same application data as the UI.

## Transport and Ownership

Use an existing Rust MCP SDK with Streamable HTTP support; confirm its supported
protocol version during implementation. Do not implement JSON-RPC framing by
hand. Bind to loopback on an ephemeral port and reject invalid Host/Origin values.
The application owns startup, shutdown, and endpoint discovery.

A local connection grant binds an opaque credential to a project, its Vault,
caller identity, permitted tools, and optional parent run. Supply managed Codex
grants internally; an explicit local setup action may issue an external-agent
grant. Never expose every project through an unauthenticated endpoint or record
credentials in activity logs. Ending a run revokes its grant; external grants
last for the app session and can be revoked separately.

Pass a concrete call context to capabilities: scope, caller, optional run,
deadline/cancellation, and usage accounting. Agents cannot choose these trusted
fields through tool arguments. Keep module boundaries close to existing services;
this context is not a general middleware framework.

## Initial Tools

| Tool | Input | Result |
| --- | --- | --- |
| `vault_list` | Optional cursor and limit | Authorized vault IDs/names and next cursor |
| `vault_list_papers` | Vault ID, optional title/year filters, cursor and limit | Paper IDs/metadata, membership snapshot, next cursor |

Use a default page of 25 and maximum 100. Sort by stable IDs. A cursor binds to
the scope, filters, and captured membership revision or snapshot; it must not
silently skip records when membership changes. Prefer existing revision support;
otherwise keep a bounded session snapshot and return `cursor_expired` after its
expiry. Snapshot TTL defaults to 10 minutes in backend configuration.

Return structured MCP tool results. Domain failures expose a stable code and
readable message: invalid input, out of scope, not found, conflict, unavailable,
limit reached, or internal failure. Pending acquisition is a normal status, not
a protocol error. Do not leak existence information about out-of-scope records.

## Mutation Convention for Later Tools

Mutating tools accept `request_id`, generated once per logical operation by the
caller. Scope retry identity by principal, tool, and request ID. The same payload
returns the recorded result; reusing the ID with different input is a conflict.
Persistence and mutation occur in one transaction in the owning capability.
This RFC specifies the convention; notes and State implement it in their RFCs.

Tool traces identify caller/run, operation, timing, outcome, and references.
Evaluation may retain bounded tool payloads for evidence checks; normal activity
need not duplicate every paper's full text. Emit existing application updates
after commits so MCP mutations later refresh the UI.

## Verification and Acceptance

- Use a real MCP client against the local transport to discover and call listing
  tools; returned contents match LibraryStore and UI data.
- Test invalid/revoked credentials, cross-project IDs, cursor scope and expiry,
  and orderly endpoint shutdown.
- Verify no model call is needed for listing and logs contain no credentials.
- Document external-agent setup and cleanup with a development command or app
  action; do not add a large configuration panel.
- No Reader, Search, or State mutation implementation is included here.

## Implementation Notes

The app starts one `rmcp` Streamable HTTP server on an ephemeral loopback port.
`LocalMcpServer` owns its lifecycle, opaque app-session grants, tool permissions,
and stable paper-list snapshots. The SDK validates loopback hosts and the two
Tauri origins; i0i rejects missing or revoked bearer credentials before MCP
dispatch. Logs contain caller/run labels but never credentials.

Managed runs call `LocalMcpServer::issue_grant`. During development, the explicit
Tauri commands `create_external_mcp_grant(project_id)` and
`revoke_external_mcp_grant(bearer_token)` provide the same app-session setup and
cleanup for an external client without changing global agent configuration.

Verified with `rmcp` 3.2.0 using a real Streamable HTTP client. Focused tests
cover discovery and invocation of both tools, Store-backed results, revoked
credentials, cursor expiry, and orderly shutdown. The final Codex-owned call and
thread-isolation check remains part of the controller integration in RFC 0135.
