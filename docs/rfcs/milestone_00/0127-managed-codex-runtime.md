# RFC 0127: Managed Codex Runtime

- Status: Complete
- Date: 2026-09-06
- Parent: [Milestone 00, M00-02](../../../milestones/milestone_00.md)
- Depends on: none; RFC 0126 defines eventual evaluation
- Implementation requires separate approval
- Verified runtime: codex-cli 0.153.4 using generated v2 protocol schemas

## Outcome

Rust can start an installed Codex app-server, create an agent thread, send an
instruction, receive structured events, and interrupt a turn without a terminal.
This RFC establishes runtime compatibility, not the research procedure.

## Contract

| Operation | Input | Result |
| --- | --- | --- |
| Ensure runtime | Configured executable and startup deadline | Ready runtime or actionable installation/authentication error |
| Start thread | Model, working directory, instructions, scoped MCP configuration | Thread ID |
| Start turn | Thread ID and input | Turn ID; subsequent events arrive asynchronously |
| Interrupt turn | Thread and turn IDs | Interruption request followed by terminal event |
| Shutdown | Runtime handle | Process and owned IO tasks terminate |

Use the supported app-server stdio protocol through a managed child process.
Separate stderr diagnostics from protocol stdout. Correlate requests and events
by IDs; do not parse terminal presentation text. Keep this module concrete and
Codex-specific rather than introducing a provider framework.

Read the executable path, model, startup deadline, and interruption grace period
from the existing backend configuration system. Proposed deadlines are 15 seconds
for startup and 5 seconds for interruption before process termination. Record the
tested Codex version and protocol schema during implementation; reject unsupported
protocol behavior with an explicit error rather than guessing field layouts.

Reuse a ready process while the app is open. Each research run gets a new thread.
MCP credentials and project context must not bleed between threads. If the tested
runtime cannot isolate those settings per thread, use one process per project
context and document that narrow adjustment before completing this RFC.

Use supported local Codex authentication. Detect missing authentication and give
setup guidance; do not assume a model subscription or API key is available.
Check the effective tool configuration: the research profile must not gain shell,
unrelated MCP servers, or direct network tools that bypass i0i's contracts.
Do not modify the user's global Codex settings to configure the research profile.

## Scope

Reuse installed Codex for this milestone. Downloading, bundling, signing, remote
execution, and embedding Claude are separate future work. The app-server
protocol is described in [official documentation](https://learn.chatgpt.com/docs/app-server).
Confirm concrete launch/configuration fields against the installed version.

The runtime accepts MCP configuration from the controller. A minimal test server
may establish compatibility here; production MCP ownership is RFC 0128.

## Verification

- Protocol fixture tests cover event correlation, startup failure, malformed
  messages, EOF, interruption, and bounded shutdown with readable errors.
- An explicitly invoked smoke check starts real Codex, performs one call to a
  test MCP tool, observes its result, and interrupts a subsequent active turn.
- Verify two threads cannot use each other's scoped credentials or tools after
  RFC 0128 provides scoped grants.
- Record version, authentication method, effective tool list, and smoke outcome.
  No credentials or private reasoning traces enter the report.
- Missing authentication is blocked, not a successful smoke test.

## Acceptance

The live initialization check and process cleanup pass. Complete the MCP call,
tool-isolation, and interruption checks with RFC 0128's server before changing
this RFC to implemented and verified. Public interfaces document timing, event
ordering, and failure behavior.

## Acceptance Record

Verified on 2026-09-07 with codex-cli 0.153.4. The controlled RFC 0126
evaluations started real scoped Codex threads and completed authenticated calls
to the i0i MCP server while inherited MCP servers, shell, and direct web tools
were disabled. Project/run grants and cross-scope failures are covered by the
MCP transport tests. The explicit installed-Codex interruption test observed an
`interrupted` terminal event, confirmed the process remained usable, and then
shut it down cleanly.
