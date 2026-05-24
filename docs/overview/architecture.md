# i0i Architecture Overview

Status: Draft  
Date: 2026-05-24  
Product: i0i  
Target: Tauri v2 + Svelte, macOS first

## Purpose

This document defines the product-wide architecture direction for i0i.

The goal is an easy, modular, understandable codebase with little coupling. A new contributor should be able to browse the project and quickly answer:

- Where does the app shell live?
- Where does each feature live?
- Where are Tauri bridge calls made?
- Where are Rust commands registered?
- Where does domain logic belong?
- Where will future storage and integrations fit?

## Architecture Principles

- Keep feature code grouped by product area.
- Keep shared UI primitives separate from feature screens.
- Keep Tauri bridge calls in one frontend layer.
- Keep Rust command handlers thin.
- Keep domain types separate from command wiring.
- Avoid global state until there is a real need.
- Avoid a database until the UI and data model are clearer.
- Prefer plain data structures over clever abstractions.
- Make it obvious where to add the next feature.

## Frontend Structure

```text
src/
  routes/
    +layout.ts
    +page.svelte

  lib/
    app/
      AppShell.svelte
      ActivityRail.svelte
      TitleBar.svelte
      StatusBar.svelte

    design/
      theme.css

    bridge/
      tauri.ts

    domain/
      paper.ts
      vault.ts

    mock/
      papers.ts
      vault-tree.ts

    features/
      vault/
        VaultHome.svelte
        VaultExplorer.svelte
        PaperList.svelte
        VaultInspector.svelte

      reader/
        README.md

      discover/
        README.md

      graph/
        README.md

      ask/
        README.md

      study/
        README.md
```

## Frontend Layer Responsibilities

`routes/`
: SvelteKit routing. For now, it should stay tiny and mount the app.

`lib/app/`
: The persistent desktop shell: title bar, activity rail, tab area, status bar, app-wide layout.

`lib/design/`
: Design tokens and global theme CSS copied/adapted from `design/ui/src/theme.css`.

`lib/bridge/`
: Frontend wrappers around Tauri `invoke`. Svelte components should call typed functions here instead of calling `invoke` everywhere.

`lib/domain/`
: TypeScript domain types such as `Paper`, `VaultFolder`, `VaultStatus`, `Annotation`, and `StudyCard`.

`lib/mock/`
: Temporary static data. This lets us build product UI before committing to persistence.

`lib/features/`
: Feature-owned UI. Each feature should be understandable on its own.

## Rust Structure

```text
src-tauri/
  src/
    main.rs
    lib.rs

    commands/
      mod.rs
      vault.rs
      reader.rs
      discover.rs
      graph.rs
      ask.rs
      study.rs

    domain/
      mod.rs
      paper.rs
      vault.rs
      annotation.rs
      graph.rs

    services/
      mod.rs
      vault_service.rs
      import_service.rs
      search_service.rs
      pdf_service.rs
      qa_service.rs
      study_service.rs

    storage/
      mod.rs
      db.rs
      repositories.rs

    integrations/
      mod.rs
      zotero.rs
      bibtex.rs
      semantic_scholar.rs
```

## Rust Layer Responsibilities

`lib.rs`
: Builds the Tauri app, installs plugins, registers commands, and owns app startup.

`commands/`
: Thin Tauri command handlers. They translate frontend calls into service calls and return serializable data.

`domain/`
: Core Rust structs and enums. These should not know about Tauri.

`services/`
: Product logic. Examples: import papers, resolve metadata, extract PDF text, search vault, generate study cards.

`storage/`
: Future persistence. Likely SQLite, but this document does not decide that yet.

`integrations/`
: External systems such as Zotero, BibTeX, Semantic Scholar, arXiv, or local LLM tools.

## ASCII Architecture Diagram

```text
                       i0i desktop app
                             |
                             v
+--------------------------------------------------------------+
|                         Svelte UI                            |
|                                                              |
|  +------------------+   +----------------+   +-------------+ |
|  | App Shell        |   | Feature Views  |   | Design CSS  | |
|  | title/status/nav |   | vault/read/... |   | theme tokens| |
|  +------------------+   +----------------+   +-------------+ |
|             |                    |                   |        |
|             +--------------------+-------------------+        |
|                                  |                            |
|                                  v                            |
|                         bridge/tauri.ts                      |
+----------------------------------|---------------------------+
                                   |
                             invoke(command)
                                   |
                                   v
+--------------------------------------------------------------+
|                         Tauri / Rust                         |
|                                                              |
|  +------------------+   +----------------+   +-------------+ |
|  | commands/        |-->| services/      |-->| storage/    | |
|  | thin handlers    |   | app logic      |   | future db   | |
|  +------------------+   +----------------+   +-------------+ |
|             |                    |                   |        |
|             v                    v                   v        |
|  +------------------+   +----------------+   +-------------+ |
|  | domain/          |   | integrations/  |   | filesystem  | |
|  | core types       |   | zotero/arxiv   |   | PDFs/cache  | |
|  +------------------+   +----------------+   +-------------+ |
+--------------------------------------------------------------+
```

## Data Boundary

Near term:

- Svelte owns presentation and interaction state.
- Frontend mock data can own visible paper lists while the product shape is still forming.
- Rust commands should expose small product-relevant capabilities through Tauri.

Later:

- Rust should own persistence and heavy local work.
- Svelte should continue to own presentation and interaction state.
- The bridge should remain explicit and typed.

The core bridge mental model is:

```text
Svelte component -> bridge function -> Tauri invoke -> Rust command -> serialized response
```

## Planned Feature Fit

### Vault

Starts as mock data in Svelte. Later, Rust services and storage own the real paper library.

Likely modules:

- `features/vault`
- `domain/paper.ts`
- `commands/vault.rs`
- `services/vault_service.rs`
- `storage/repositories.rs`

### Import and Export

BibTeX import/export and Zotero sync should live behind Rust services. The frontend should see simple commands like `import_bibtex`, `export_bibtex`, and eventually `sync_zotero`.

Likely modules:

- `features/import`
- `commands/vault.rs`
- `services/import_service.rs`
- `integrations/bibtex.rs`
- `integrations/zotero.rs`

### Reader

The first reader can use extracted/mock text. PDF rendering can come later, likely with frontend PDF rendering and Rust-side metadata/text extraction.

Likely modules:

- `features/reader`
- `domain/annotation.ts`
- `commands/reader.rs`
- `services/pdf_service.rs`

### Annotation

Annotations should be domain objects linked to paper IDs, page/paragraph anchors, and optional selected text. Avoid coupling annotations directly to one PDF renderer.

Likely modules:

- `features/reader`
- `domain/annotation.rs`
- `services/vault_service.rs`
- `storage/repositories.rs`

### Discovery

Discovery can begin as a static ranked feed. Later it can combine semantic similarity, citations, authors, venue filters, and saved searches.

Likely modules:

- `features/discover`
- `commands/discover.rs`
- `services/search_service.rs`
- `integrations/semantic_scholar.rs`

### Graph

Graph should not be hardcoded into the paper list. Treat graph data as a separate query result: nodes, edges, labels, and reasons.

Likely modules:

- `features/graph`
- `domain/graph.rs`
- `commands/graph.rs`
- `services/search_service.rs`

### Ask

Vault Q&A should be isolated behind a service boundary. The UI asks a question and receives an answer with citations. The command should not leak provider details into Svelte.

Likely modules:

- `features/ask`
- `commands/ask.rs`
- `services/qa_service.rs`

### Study

Study mode should consume papers, notes, annotations, and concepts. It should not be built until the vault and annotation models settle.

Likely modules:

- `features/study`
- `commands/study.rs`
- `services/study_service.rs`
