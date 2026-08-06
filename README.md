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

- [x] RFC 0070 - Vault papers can now be exported as a BibTeX (`.bib`) file for LaTeX.
- [x] RFC 0071 - The reader now has a Zotero-style tool panel and a collapsible Info/Notes/Chat sidebar.
