# i0i

i0i is a macOS-first Tauri desktop app for knowledge curation.

The app is currently an active construction site. It has a Svelte frontend, a Rust/Tauri backend, and a local SQLite-backed library store for vaults, papers, and reader notes.

## Prerequisites

- Node.js
- pnpm
- Rust
- Tauri v2 system prerequisites

For Tauri setup details, use the official guide:

https://tauri.app/start/prerequisites/

## Install

```bash
pnpm install
```

## Run The Desktop App

```bash
pnpm tauri dev
```

This starts the Svelte frontend through Vite and opens it inside the Tauri desktop shell.

## Useful Commands

```bash
pnpm dev
```

Runs the frontend only in the browser. This is useful for layout work, but Rust commands and desktop behavior are only fully tested through Tauri.

```bash
pnpm check
```

Runs Svelte and TypeScript checks.

```bash
pnpm build
```

Builds the frontend.

```bash
pnpm tauri build
```

Builds the desktop app.

## Project Shape

```text
src/
  lib/
    bridge/       Svelte-to-Tauri command wrappers
    domain/       frontend domain types
    features/     UI features such as Vault, Discover, Reader
    state/        frontend state/cache modules

src-tauri/
  src/
    commands/     Tauri command handlers callable from Svelte
    domain/       Rust domain models
    storage/      SQLite store and persistence logic
```

Mental model:

```text
Svelte UI -> bridge invoke(...) -> Tauri command -> Rust store -> SQLite
```

The frontend should call Rust through small bridge functions, not by scattering raw `invoke(...)` calls through UI components.

## Changelog

- [x] RFC 0124 - Project Research now uses one instruction, one bounded paper count, automatic validated enrichment, and progressively disclosed Run Activity.
- [x] RFC 0123 - Research Runs now handle nullable planner responses, retry once, report the failed stage, and have deterministic pipeline and live smoke tests.
- [x] RFC 0122 - Harness Runs now remain active through reconciliation and finalize atomically only after Change Set, usage, reflection, and checkpoint facts are durable.
- [x] RFC 0121 - Production reconciliation now records validated operational reflections from actual telemetry and can create recurring, reviewable Harness improvements.
- [x] RFC 0120 - Each Harness search is now oriented by its immutable Research State, prior next direction, and bounded same-Project operational observations.
- [x] RFC 0108 - Projects now provide the complete autonomous research harness: one Vault, typed Research State, bounded recurring Runs, reviewable reconciliation, durable checkpoints, and ordinary generated Documents.
- [x] RFC 0119 - The Project Research workspace now exposes goal and cycle status, counted/sortable Run-filtered State, keyboard focus restoration, grouped Run Activity, checkpoint controls, and document citation provenance.
- [x] RFC 0118 - Terminal Research Runs now retain structured Activity, actual usage, monotonic State/Vault/document revisions, complete checkpoints, and append-only same-Project State restoration.
- [x] RFC 0117 - Completed Harness searches now produce bounded, reviewable Change Sets that can atomically add accepted Papers, exact abstract evidence, and typed Research State entries.
- [x] RFC 0116 - Research Harnesses now have explicit bounded authority, immutable configuration history, and inspectable effective instructions for every Run.
- [x] RFC 0115 - Selected entries from an immutable Research State revision can now create provenance-preserving Markdown documents in six transparent output shapes.
- [x] RFC 0114 - Completed Runs now record bounded operational reflections and can produce typed, researcher-reviewed Harness improvements without self-modifying policy.
- [x] RFC 0113 - Research Harnesses now support DST-aware local schedules, bounded startup catch-up, lifecycle controls, and durable stop limits.
- [x] RFC 0112 - Projects now have revisioned typed Research State with validated evidence, separate working context, immutable history, and an interactive inspector.
- [x] RFC 0111 - Projects now have a persisted Research Harness with versioned settings, immutable manual Runs, cancellation, and append-only Activity.
- [x] RFC 0110 - Projects now contain durable Markdown documents with an editor and explicit Harness write consent.
- [x] RFC 0109 - Projects now atomically own one Vault, and existing Vaults migrate without changing identity or Paper membership.
- [x] RFC 0070 - Vault papers can now be exported as a BibTeX (`.bib`) file for LaTeX.
- [x] RFC 0071 - The reader now has a Zotero-style tool panel and a collapsible Info/Notes/Chat sidebar.
- [x] RFC 0072 - Reader titles no longer clip, notes attach reliably, and un-extracted imports are visible to the AI.
- [x] RFC 0073 - Marks can be opened from the list to jump the document, Note and Ask are explicit intents, and PDF render cost is bounded.
- [x] RFC 0074 - Sticky notes can be placed anywhere on a page, with Zotero-style annotation tools.
- [x] RFC 0075 - PDFs now extract into sub-page blocks and spans, chunk structurally, and embed locally.
- [x] RFC 0076 - Papers can be searched by wording and by meaning at once, from the title bar or any scope.
- [x] RFC 0077 - Chat has a ContextManager: passages can be added, compacted, and cited back to the page they came from.
- [x] RFC 0078 - The AI now decides for itself when to search the paper, keeps what it needs, and lists only the passages it actually used.
- [x] RFC 0079 - Annotations can be deleted from the list with their conversations, focus mode clears the desk, notes stack, and every answer about an indexed paper cites passages.
- [ ] RFC 0080 - Right-clicking an annotation opens a menu instead of deleting it, and the delete control is a corner icon rather than a bar. (implemented; awaiting manual verification)
- [ ] RFC 0081 - Focus mode hides every bar and panel, and each one comes back by pointing at the edge it hides behind. (implemented; awaiting manual verification)
- [ ] RFC 0082 - Each hidden bar has a visible handle you can aim at, overshoot, and pin open. (implemented; awaiting manual verification)
- [ ] RFC 0083 - HTML cross-references read as their figure number, and no link in an article can navigate the app away. (implemented; awaiting manual verification)
- [ ] RFC 0084 - Opening a paper adds a tab instead of closing the one you were reading. (implemented; awaiting manual verification)
- [ ] RFC 0085 - Notes and Chat each list only their own kind, and a mark can be deleted from the page it lives on. (implemented; awaiting manual verification)
- [ ] RFC 0088 - Deep research reflects once per round, narrows query breadth, retires dry providers, and reports convergence separately. (partially implemented: tasks 1 and 3; evaluation and browsing remain)
- [x] RFC 0091 - Vaults now suggest up to five missing papers using Deep Research, citation neighbours, local similarity, and durable Add or Dismiss decisions.
- [x] RFC 0094 - Vault suggestions now prepare reviewable query paths, run only selected searches concurrently, persist controls, and stream progress in the contextual inspector.
- [x] RFC 0095 - Vault suggestions now preserve query evidence, rank against the closest vault paper, suppress reviews by default, and omit weak results.
- [ ] RFC 0100 - Obscura now starts eagerly through one shared lifecycle, and Discover gates Find on visible browser readiness. (implemented; awaiting manual UI verification)
- [ ] RFC 0101 - Explicit web requests now route deterministically, with honest typed success, empty, and unavailable outcomes. (implemented; awaiting live model verification)
- [x] RFC 0103 - Browser discovery now logs bounded Obscura page evidence and reports Google Scholar challenges honestly.
- [x] RFC 0104 - Quick Find now merges concurrent browser and scholarly API results, preserving partial successes.
- [x] RFC 0105 - Obscura setup now installs rendering-enabled stealth releases and launches them in stealth mode.
- [x] RFC 0106 - Browser discovery now uses Brave Search by default and parses only primary web results.
