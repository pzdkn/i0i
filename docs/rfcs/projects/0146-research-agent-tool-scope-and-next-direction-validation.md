# RFC 0146: Research Agent Tool Scope and Next-Direction Validation

- Status: Implemented and verified
- Date: 2026-09-09
- Depends on: RFCs 0143 and 0144

## Problem

The Research Run started at `2026-09-09 08:28:50` completed its searches and
paper reads, then failed during final synthesis with:

```text
Next direction must name at least one motivating State entry
```

The project started from an empty Research State. The model returned a useful
textual `nextDirection`, but left `nextDirectionEntryIds` empty. The current
validator treats that missing optional relationship as fatal and discards the
entire otherwise completed Run.

The same Run also attempted to call `state_update` twice. RFC 0143 intentionally
removed that tool from the managed research grant because State changes must be
committed once during final synthesis. The calls were denied correctly, but the
MCP server still advertised the unavailable tool to Codex. The agent therefore
saw an action that it could never successfully perform.

## Decision

### Advertise only granted tools

Pass the existing `RESEARCH_AGENT_TOOLS` list to Codex through the MCP server's
`enabled_tools` configuration. The visible Codex tool catalog and the
server-side connection grant will then express the same capability boundary.

The server-side grant remains authoritative. The Codex allow-list improves the
agent's tool selection; it does not replace authorization.

### Keep next-direction linkage optional

`nextDirection` is an operational continuation hint. It may be returned without
`nextDirectionEntryIds`, especially when a project begins with an empty State
or the recommendation is broader than one durable entry.

The validator will use this one-way rule:

- entry ids without a textual `nextDirection` are invalid;
- a textual `nextDirection` with no entry ids is valid; and
- supplied entry ids must still resolve through the synthesis transaction.

This preserves useful provenance when the model supplies it without making an
optional relationship capable of erasing a completed research investigation.
RFC 0143's stronger wording is superseded by this rule.

## Scope

- Add `RESEARCH_AGENT_TOOLS` as Codex's `mcp_servers.ioi.enabled_tools` list.
- Relax the duplicate synthesis-shape checks in the controller and storage
  boundary to the same one-way invariant.
- Update prompts and tests so they encourage linked directions without claiming
  that the link is mandatory.
- Keep all existing limits and State-entry reference validation.

## Non-Goals

- Adding `state_update` back to managed Research Runs.
- Automatically inventing a motivating State entry or relation.
- Retrying arbitrary semantic synthesis failures.
- Changing search, reading, ranking, or State category behavior.
- Redesigning the Run inspector.

## Acceptance

- Codex is configured to discover exactly the tools in
  `RESEARCH_AGENT_TOOLS`; `state_update` is not visible to the managed agent.
- The server still rejects any ungranted tool call independently of Codex
  configuration.
- A synthesis with a textual next direction and no motivating entry ids
  validates and can complete the Run.
- Motivating entry ids without a textual next direction remain invalid.
- Supplied motivating ids continue to be resolved and validated atomically.
- Focused controller and storage tests cover both accepted and rejected shapes.
- The Rust test suite and Cargo checks pass.

## Verification

- Codex thread configuration exposes exactly the 14 managed research tools and
  omits `state_update`.
- Focused controller and storage tests cover unlinked next directions and the
  rejected inverse case.
- Rust library suite: 622 passed, 11 intentionally ignored live tests.
- `cargo check --no-default-features`: passed with existing dead-code warnings.
- `pnpm check`: 0 errors and 0 warnings.
