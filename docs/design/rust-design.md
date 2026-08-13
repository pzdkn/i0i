# Rust design patterns, checked against this backend

Date: 2026-08-13
Source: [Rust Design Patterns](https://rust-unofficial.github.io/patterns/)
(rust-unofficial), read from
[the book's own markdown](https://github.com/rust-unofficial/patterns/tree/main/src)
— idioms, design patterns, anti-patterns, functional, and the design-principles
appendix.
Scope: `src-tauri/src`, 34,051 lines across 97 files, at commit `8d795eb`.

This is a survey, not a plan. Every claim below is anchored to a file and line
so it can be checked, and the ranking is deliberately short: the book's own
patterns index warns that Rust often needs no pattern where another language
would, and AGENTS.md says to introduce abstractions sparingly. So this document
says **no** more often than it says yes, and the "does not apply" section is
load-bearing rather than filler.

---

## 1. Already satisfied — do nothing

| Pattern / idiom | Evidence |
|---|---|
| **Strategy** (behavioural) | Seven trait seams already: `DiscoveryProvider` (`commands/discovery/provider.rs:55`), `Planner` and `Clock` and `CandidateSource` (`services/research/{planner,clock,source}.rs`), `HttpFetcher` and `BrowserRuntime` (`services/source_acquisition/types.rs:141,156`), `TextEmbedder` (`services/embedding/mod.rs:74`). The book's Strategy page describes exactly this — an abstract skeleton with swappable implementations — and the patterns index notes traits are how Rust does it without ceremony. |
| **Use borrowed types for arguments** | Zero `&String` and zero `&Vec<T>` parameters in the whole backend. Signatures already take `&str` / `&[T]`. |
| **`mem::{take, replace}`** | Already used where it belongs: `domain/chunking.rs:212` takes the pending buffer out of the chunker, `services/llm.rs:451` takes the accumulated tool calls out of the stream state. |
| **`#[deny(warnings)]`** (anti-pattern) | Not present. `main.rs:2` carries only the Windows subsystem attribute. |
| **Deref polymorphism** (anti-pattern) | Zero `impl Deref` anywhere. |
| **Avoid complex type bounds with custom traits** | Zero `where` clauses with three or more bounds. The traits above are plain object-safe `Send + Sync` seams. |
| **The Default trait** | 23 `derive(Default)` plus 6 hand-written `impl Default`. |

---

## 2. Worth doing, ranked

### 2.1 The error type is thrown away at the seam — *Idiomatic Errors*, adapted

**Finding.** 131 signatures return `Result<_, String>`, including
`type StoreResult<T> = Result<T, String>` (`storage/library_store.rs:30`), which
is the return type of all 94 public store methods. Two modules *do* define an
error type and then discard it:

- `services/research/error.rs` — `ResearchError(pub String)`, with
  `impl From<ResearchError> for String` at :29, whose only purpose is to flatten
  it back to a string.
- `commands/discovery/error.rs` — `DiscoveryError { message: String }`, whose
  doc comment says the quiet part: *"Keep this boring until we actually need
  typed variants."*

The one real enum, `SourceAcquisitionError`
(`services/source_acquisition/types.rs:90`), is the exception.

**Why it matters here, not in the abstract.** The cost is already visible in
shipped code. RFC 0079 R7.5 had to *re-parse provider intent out of a string*
because a 402 had been flattened into prose (`services/llm.rs:252`); the fix was
to append the provider's body text rather than to carry a variant. A caller that
wants to branch on "out of credits" versus "request too large" has nothing to
match on.

**Caveat on the citation.** This book has no "use a typed error" page — the
nearest is *Idiomatic Errors* under FFI idioms, which is about serialising rich
errors across a boundary that cannot hold them. That maps honestly onto the
Tauri command boundary (`Result<T, String>` is what `invoke` can carry) but it
argues for the opposite of what we do: **rich types inside, flattened only at
the boundary**. Today we flatten at the source and carry strings the whole way.

**Shape of the change.** Typed errors per service, with `impl From<X> for String`
kept at the `#[tauri::command]` layer only. Doing it everywhere at once is a
34k-line refactor; the seam worth converting first is `services/llm.rs`, where
callers demonstrably want to branch.

**Would warrant an RFC.** Yes — it changes 131 signatures if taken to its end.

### 2.2 Three copies of the same `unsafe` block — *Contain unsafety in small modules*

**Finding.** The backend has exactly one `unsafe` operation:
`sqlite3_auto_extension` + `mem::transmute` to register `sqlite-vec`. It appears
**three times in one file** — production `register_sqlite_vec()`
(`storage/library_store.rs:4858`) and twice more inside `mod tests`
(`:7677`, `:7888`), because the tests could not reach the private helper.

**Why it matters.** The book's argument is auditability: *"this restricts the
unsafe code that must be audited."* Right now an audit has three sites to check
and two of them are copies that can drift from the one that ships. The safety
comment justifying the transmute is written once, on the production copy.

**Shape of the change.** A small `storage/sqlite_vec.rs` holding the `unsafe`
behind a safe `pub(crate) fn register()` with the `Once`, called by both the
store and the tests. This is the cheapest item in this document and the most
clearly correct.

**Would warrant an RFC.** No. It is a move plus a `pub(crate)`.

### 2.3 `library_store.rs` is 8,529 lines — *Prefer small crates*, read as modules

**Finding.** One file, 94 public methods on a single `LibraryStore`, two `impl`
blocks (`:73`, `:3942`), 5,694 lines of implementation and 2,835 of tests
(`#[cfg(test)]` at `:5695`). It
holds vaults, papers, sources, extractions, chunks, FTS, vectors, highlights,
threads and context items.

**How the book applies.** The page is literally about *crates*, and splitting
this into published crates would be silly. What transfers is its reasoning —
"small units that do one thing well … easier to understand, encourage more
modular code" — plus SRP from the design-principles appendix. The repo already
demonstrates the pattern it wants: `services/research/` is ten files totalling 2,039
lines — ~200 each, each named for one job.

**Shape of the change.** Split by table family into `storage/` submodules
(`vaults.rs`, `papers.rs`, `sources.rs`, `chunks.rs`, `annotations.rs`) with
`impl LibraryStore` blocks per file — Rust allows the impl to be spread across
modules in the same crate, so this is a move, not a redesign, and the public API
does not change.

**Honest counterweight.** AGENTS.md: *"Avoid large refactors unless they are
required for correctness or substantially improve clarity"* and *"Keep related
code close."* A 5,700-line file is a real navigation cost; a mechanical split
that leaves 94 methods on one struct fixes the file size and not the
responsibility count. Do it when a slice of it is being changed anyway, not as a
standalone project.

**Would warrant an RFC.** Yes, if done wholesale.

### 2.4 Positional `&str` walls — *Newtype*, scoped to where it prevents a bug

**Finding.** Ids are `String` everywhere in `domain/` (`paper_id`, `source_id`,
`vault_id`, `chunk_id`, `extraction_id`, …). On its own that is fine. What is
not fine is a call whose arguments can be swapped silently:

```rust
// storage/library_store.rs:413
pub fn start_document_extraction(
    &self,
    source_id: &str,
    extractor: &str,
    extractor_version: &str,
    annotation_source_id: &str,
    force: bool,
) -> StoreResult<DocumentExtraction>
```

Four consecutive `&str`, two of them ids of different things
(`source_id` vs `annotation_source_id`). Swap them and it compiles, runs, and
writes an extraction against the wrong annotation source. Same shape at
`add_local_source_to_vault` — five positional `&str` plus two literals at its
only call site (`storage/library_store.rs:1153`, called from `:1128`) — and `cancel_discovery_pdf_acquisition(source_id, paper_id)`
(`services/reader_service.rs:150`).

**Recommendation.** Do **not** newtype every id — that is a 34k-line change for
a type-safety property most call sites do not need, and the book's own index
cites YAGNI. Newtype the two or three that are actually confusable at a call
site (`SourceId` vs `PaperId` is the pair that shows up adjacent), or convert
the worst signatures to a params struct, which fixes the same bug class with
less ceremony.

**Would warrant an RFC.** Only if applied broadly.

### 2.5 `CompletionRequest` is built 17 times by hand — *Builder*

**Finding.** 17 literal `CompletionRequest { .. }` constructions across
`services/`, each spelling out every field. Seven of them say
`max_tokens: None`. That repetition is not cosmetic: RFC 0083's OpenRouter
budgeting question turns on which call paths bound `max_tokens` and which do
not, and today the answer is only visible by reading all 17 sites.

**Shape of the change.** `CompletionRequest::new(model, messages)` plus
`.max_tokens(n)`, `.tools(..)`, `.json(..)`. The book's Builder page is exactly
this, and it makes the defaults one line in one place instead of an invariant
spread over seven files.

**Would warrant an RFC.** No — internal, mechanical, test-covered.

### 2.6 One in-flight map entry can outlive its task — *RAII guards*

**Finding.** `ReaderService.acquisitions`
(`services/reader_service.rs:67`) maps source id → `JoinHandle`, so a re-opened
tab does not start a duplicate download. The entry is removed *inside* the
spawned task after the await (`:745`). The cancel path is correct — it removes
before it aborts (`:154`). But if the task **panics**, the removal line is never
reached and the entry is immortal: that source can never be downloaded again for
the life of the process, and the UI shows "Fetching PDF…" forever.

**Shape of the change.** A small guard struct holding `Arc<Mutex<HashMap<..>>>`
and the key, removing the entry in `Drop`, which runs on unwind as well as on
the happy path. This is the book's RAII-guard pattern used for its actual
purpose rather than for tidiness.

**Would warrant an RFC.** No.

---

## 3. Checked and rejected

**RAII guard for `.part` files.** Tempting — 18 references to `.part` temp files
in `services/reader_service.rs` — but at all three write sites (`:297`, `:383`,
`:856`) the `fs::write` and the `fs::rename` are adjacent with nothing fallible
between them. A `.part` can only be orphaned if the rename itself fails or the
process dies, and a `Drop` guard does not help with the second. Decoration.

**Command.** The agent's tool dispatch (`services/chat/agent_loop.rs:343`)
matches a tool name to an action, which looks like Command. It is a `match` over
a handful of arms that never needs to be stored, queued, or undone. The book's page is
about reifying operations as objects for exactly those reasons; none apply.

**Clone to satisfy the borrow checker.** 415 `.clone()` calls, which sounds
alarming until you look: they are overwhelmingly `String`/`Arc` clones crossing
`async move` boundaries (`services/reader_service.rs:731-733` is typical), which
is the ownership the spawn genuinely requires. The book explicitly says clones
are fine when the alternative is fighting the checker for no measurable gain.
No action beyond keeping `cargo clippy` in the loop, which already flags the
redundant ones.

**Interpreter, Visitor, Fold.** No AST, no grammar, no tree we transform into a
tree of the same shape. `pdf_layout.rs` and `domain/chunking.rs` walk structures,
but they fold flat sequences into flat sequences; the pattern would add a trait
and a dispatch for nothing.

**FFI patterns — Object-Based APIs, Type Consolidation into Wrappers.** Both
pages are for crates *exporting* a C API. We consume FFI (pdfium, sqlite-vec)
and export none.

**On-Stack Dynamic Dispatch.** The idiom is about avoiding a heap allocation
when a `Box<dyn Trait>` was only needed to unify two branches. All five
`Box<dyn>` in the backend are rusqlite `ToSql` bindings built into a `Vec`
(`storage/library_store.rs:824-977`), where the boxing is the API's requirement.

**Functional Optics, Generics as Type Classes.** Interesting, and a solution to
problems this codebase does not have.

**Privacy for Extensibility, Compose Structs.** Both are about evolving a
*published* API without breaking downstream crates. This is one binary with no
external consumers; the `#[non_exhaustive]` ceremony buys nothing.

---

## 4. If you only do two things

1. **§2.2** — fold the three `unsafe` copies into one small module. An hour, and
   it makes the only unsafe code in the backend auditable in one place.
2. **§2.5** — a builder for `CompletionRequest`, so "which calls bound their
   token budget" is one line to read instead of 17.

Both are contained, both are covered by existing tests, neither needs an RFC.
Everything above them in the ranking is more valuable and more expensive; §2.1
is the one that would actually change how the backend reports failure, and it
should be a decision, not a drive-by.
