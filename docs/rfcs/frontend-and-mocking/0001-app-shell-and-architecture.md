# RFC 0001: App Shell and Vault Home

Status: Draft  
Date: 2026-05-24  
Product: i0i  
Target: Tauri v2 + Svelte, macOS first

## Summary

Build the smallest product-shaped foundation for i0i: a static app shell using the existing design language, a Vault/Home screen with mock paper data, and one product-relevant Rust command exposed through Tauri.

This RFC uses the product-wide architecture defined in [`docs/overview/architecture.md`](../overview/architecture.md).

## Context

i0i is a research vault for learning and discovery. The product direction combines:

- Reference management
- Paper discovery
- Research lineage graphs
- Knowledge vault curation
- Reading and annotation
- Vault Q&A
- Study workflows

The design direction already exists in `design/ui`: a dense, IDE-like research workstation with an amber-on-black visual system, IBM Plex Mono, sharp rectangles, hairline borders, compact rows, an activity rail, explorer, editor pane, inspector, tabs, and status bar.

The current app is a minimal Tauri/Svelte bridge demo. The next step should turn it into the smallest recognizable i0i foundation without jumping into hard backend features too early.

## Goals

- Implement the Layout A-style app shell in Svelte.
- Use the fixed i0i design language from the existing mockups.
- Show a Vault/Home screen with static mock data.
- Keep one Rust command to demonstrate the Tauri bridge in a product-relevant way.
- Establish a folder structure that can grow feature by feature.
- Keep the codebase easy to browse and explain.

## Non-Goals

- No PDF rendering yet.
- No database persistence yet.
- No Zotero sync yet.
- No BibTeX parser yet.
- No semantic search yet.
- No graph engine yet.
- No LLM/Q&A implementation yet.
- No study scheduler yet.

These features are anticipated in the architecture, but not implemented in this first increment.

## First Increment

Build this:

- A macOS-first desktop shell.
- Left activity rail with modes:
  - Vault
  - Find
  - Read
  - Graph
  - Ask
  - Study
- Explorer pane with mock vault folders.
- Main editor area showing a paper list.
- Right inspector showing selected folder/paper details.
- Bottom status bar.
- One Tauri command: `get_vault_status`.
- The Svelte UI displays the returned Rust status somewhere small, likely the status bar.
- Use mock data for visible paper and vault content.
- Keep panes visually compatible with future collapse behavior, but collapsible panes are not required in this first increment.
- Use `i0i` in lowercase as the fixed product name in app chrome.

The command can return a simple typed object, for example:

```rust
struct VaultStatus {
    paper_count: usize,
    unread_count: usize,
    sync_state: String,
}
```

The values can be mock/static at first. The point is to teach the Svelte-to-Rust bridge using a product-shaped example.

## Architecture Reference

Use [`docs/overview/architecture.md`](../overview/architecture.md) for the product-wide architecture, folder structure, layer responsibilities, ASCII diagram, data boundary, and planned feature fit.

For this RFC, the relevant slice is:

- `src/lib/app` for the persistent shell.
- `src/lib/design` for the i0i theme.
- `src/lib/bridge` for typed Tauri calls.
- `src/lib/domain` for frontend types.
- `src/lib/mock` for temporary paper and vault data.
- `src/lib/features/vault` for the Vault/Home screen.
- `src-tauri/src/commands/vault.rs` for the thin Rust command handler, if/when command files are split.

## Data Boundary

For the first increment:

- Frontend mock data owns the visible paper list.
- Rust owns only `get_vault_status`.

This is intentionally modest. It avoids designing a database too early while still preserving the Tauri mental model:

```text
Svelte component -> bridge function -> Tauri invoke -> Rust command -> serialized response
```

Later:

- Rust should own persistence and heavy local work.
- Svelte should own presentation and interaction state.
- The bridge should remain explicit and typed.

## Proposed First Command

Frontend wrapper:

```ts
export async function getVaultStatus(): Promise<VaultStatus> {
  return invoke<VaultStatus>("get_vault_status");
}
```

Rust command:

```rust
#[tauri::command]
fn get_vault_status() -> VaultStatus {
    VaultStatus {
        paper_count: 234,
        unread_count: 12,
        sync_state: "local".to_string(),
    }
}
```

The exact code can change during implementation, but the shape should stay simple.

## Validation Plan

- `pnpm check`
- `cargo check`
- `cargo fmt --check`
- `pnpm build`
- `pnpm tauri dev`
- Manual visual check against Layout A design references.

## Open Questions

Resolved:

- `i0i` is the fixed lowercase product name.
- Collapsible panes are preferred long term, but not required in the first increment.
- Use mock data for the first increment.

## Recommendation

Start with a static Layout A-style Vault/Home shell and one Rust status command.

This is the smallest useful increment because it proves the product shape, preserves the design language, and keeps the architecture simple enough to understand before adding persistence, PDF reading, search, or AI features.
