# RFC 0070: Export Vault Citations as BibTeX

Status: Implemented
Date: 2026-08-05
Product: i0i
Target: Tauri v2 + Svelte, macOS first

## Summary

Let the user export every paper in a vault to a single `.bib` file for use in a
LaTeX document. One vault, one `.bib`, chosen via a native Save dialog.

Generation happens in Rust (where the papers live and where `cargo test` covers
the tricky parts); the frontend only picks the destination path.

## Context

A vault holds papers with `title`, `authors[]`, `venue`, and `year` (the
`papers` table). When the user writes a LaTeX paper, they want a `.bib` sitting
next to their `.tex` so they can `\cite{...}`. Today there is no way to get one.

This is a deliberately **best-effort, lossy, offline** export ("B-now"):

- It emits `@article` / `@inproceedings` / `@misc` from stored metadata only.
- It does **not** emit `doi`. There is no `doi` column on the `papers` table —
  DOIs are discovered transiently during metadata enrichment
  (`metadata_enrichment.rs`) but never persisted onto a paper. Storing DOIs is a
  separate, independently useful change and is left to a follow-up RFC.

The result is a genuinely usable `.bib` that compiles and cites; a purist may
tidy a few entries by hand.

## Goals

- One action: "Export citations (.bib)" for the current vault.
- Native Save dialog, default filename `<vault title>.bib`, default directory
  the vault's on-disk `path`.
- Deterministic, stable `authorYEARword` cite keys (e.g. `lecun2015deep`) with
  collision suffixing.
- Author names rendered `Family, Given`, joined with ` and `.
- Entry type inferred from `venue`.
- LaTeX special characters escaped.
- Pure generation logic unit-tested in Rust.

## Non-Goals

- **No DOI** (follow-up RFC — needs a schema change + enrichment write-back).
- No per-paper selection; the unit is the whole vault.
- No merge/dedupe against an existing `.bib`; we overwrite the chosen path.
- No `abstract` / `keywords` / `url` fields in v1.
- **No Unicode transliteration.** Author/title text is written as-is (UTF-8);
  we do **not** convert `é` to `\'e`. Modern LaTeX (`biber`/`inputenc`) handles
  UTF-8. (Cite *keys* are still ASCII-folded — see below.)
- No new Tauri plugin. Writing happens in Rust via `std::fs`.

## Product Decision

A single vault-scoped action produces a single file.

```text
Export citations (.bib)
  -> native Save dialog (default: <vault.path>/<vault.title>.bib)
  -> user confirms a path
  -> Rust builds BibTeX from the vault's papers
  -> Rust writes the file (overwrite if it exists)
```

The action is the vault's existing top-panel "Export .bib" button in
`VaultHome`, alongside "Import PDF" and "Add web page" — the discoverable place
for vault-wide actions. It is disabled when the vault has zero papers.

## BibTeX Generation

New pure module: `src-tauri/src/services/bibtex.rs`.

```rust
/// A minimal citation record — deliberately NOT the full `Paper` struct, so we
/// never touch the chat tables (Paper.highlight_count is computed on read).
pub struct CiteRecord {
    pub title: String,
    pub authors: Vec<String>,
    pub venue: String,
    pub year: i32,
}

pub fn to_bibtex(records: &[CiteRecord]) -> String
```

### Deterministic ordering (correctness-critical)

Cite keys must be **stable across exports**. If two papers share a base key and
suffixes (`a`/`b`) are assigned in an unstable query order, a re-export could
swap which paper is `lecun2015deep` vs `lecun2015deepa` and silently break a
`.tex` that already compiled.

Rule: **sort records before assigning keys**, by

```text
(first-author family, year, title)
```

Assign base keys, then assign collision suffixes in that same order. A re-export
of an unchanged vault MUST produce byte-identical output.

### Cite key

`firstAuthorFamily + year + firstSignificantTitleWord`, ASCII-folded and
lowercased → `lecun2015deep`.

- First significant title word skips stopwords (`a`, `an`, `the`, `of`, `on`,
  `in`, `for`, `and`, `to`); strip to `[a-z0-9]`.
- Fallbacks (note the fields are **not** `Option` — use value sentinels):
  - No usable author (authors empty or first name blank) → `anon`.
  - `year <= 0` → `nd` (no date) in place of the year.
- Collisions: append `a`, `b`, `c`, … in the sorted order above.

### Author rendering

`split_author("Yann LeCun") -> ("LeCun", "Yann")` → emitted `LeCun, Yann`.

- Split on the **last** whitespace: last token = family, the rest = given.
- Single-token name (`"Plato"`) → family only, no comma.
- Already-comma form (`"LeCun, Yann"`) → kept as-is.
- Join multiple authors with ` and `.

### Entry type (from `venue`, which is `not null`)

- `venue.trim().is_empty()` → `@misc`.
- Case-insensitive contains `conf` / `proceedings` / `workshop` / `symposium`
  → `@inproceedings`, venue emitted as `booktitle`.
- Case-insensitive contains `arxiv` → `@misc`, venue emitted as `howpublished`.
- Otherwise → `@article`, venue emitted as `journal`.

### Escaping (order matters)

Escape LaTeX specials in `title` / `venue` / author fields: `& % $ # _ { } ~ ^ \`.

**Escape `\` first** (or do a single char-by-char pass), otherwise escaping `&`
to `\&` and then escaping `\` double-escapes it.

Emit the title double-braced — `title = {{...}}` — so BibTeX styles do not
lowercase it. This is the single biggest "tidy by hand" item, for one line.

### Example output

```bibtex
@article{lecun2015deep,
  title = {{Deep Learning}},
  author = {LeCun, Yann and Bengio, Yoshua and Hinton, Geoffrey},
  journal = {Nature},
  year = {2015}
}
```

## Rust Store Access

The command needs only `title, authors_json, venue, year` for one vault — **not**
computed counts. Do **not** call a full-`Paper` hydrator.

Preferred: a targeted query joining through `vault_papers`.

```sql
select p.title, p.authors_json, p.venue, p.year
from papers p
join vault_papers vp on vp.paper_id = p.id
where vp.vault_id = ?;
```

Store method:

```rust
pub fn cite_records_for_vault(
    &self,
    vault_id: &str,
) -> Result<Vec<CiteRecord>, String>
```

(Final ordering is imposed in `to_bibtex`, so the query order does not matter.)

## Proposed Tauri Command

```rust
#[tauri::command]
pub fn export_vault_bibtex(
    store: tauri::State<'_, LibraryStore>,
    vault_id: String,
    dest_path: String,
) -> Result<usize, String>
```

Responsibilities:

- Validate `vault_id` and `dest_path` are non-empty.
- Load `cite_records_for_vault`.
- If empty, return a user-readable error (the UI also disables the action).
- `std::fs::write(dest_path, to_bibtex(&records))`.
- Return the number of entries written (for a confirmation toast/log).

Writing via `std::fs` in a Rust command needs **no** Tauri fs capability —
capabilities gate the JS API surface, not Rust. This is the concrete reason the
export writes in Rust rather than wiring `@tauri-apps/plugin-fs`.

## Proposed Frontend Bridge

```ts
export async function exportVaultBibtex(
  vaultId: string,
  path: string,
): Promise<number>;
```

## Proposed Frontend Interaction

`VaultHome.svelte`

- Wires the existing top-panel "Export .bib" button (disabled when the vault has
  no papers) to the handler below.
- On click:

```ts
import { save } from "@tauri-apps/plugin-dialog";

const path = await save({
  defaultPath: `${vault.path}/${vault.title}.bib`,
  filters: [{ name: "BibTeX", extensions: ["bib"] }],
});
if (path) {
  const count = await exportVaultBibtex(vault.id, path);
  // toast: `Exported ${count} citations`
}
```

`save` is already permitted: the main window capability includes
`dialog:default`, whose permission set covers `allow-save` in Tauri v2. No
capability change required.

## Teaching Notes

Mental model:

```text
Rust owns the data and the file write.
The frontend only asks the OS "where should this go?".
```

Why generation lives in Rust:

- The papers already live in the Rust store; no need to round-trip them to JS.
- The hard parts (stable cite keys, author splitting, escaping, entry-type
  inference) are pure functions that `cargo test` exercises directly — and there
  is currently no JS test runner in this repo.
- Writing in Rust sidesteps `plugin-fs` entirely.

What can go wrong (and how this RFC prevents it):

- **Unstable cite keys** across exports → sort before keying; assert
  byte-identical re-export.
- **Double-escaping** LaTeX specials → escape `\` first / single pass.
- **Compile errors from `Option` assumptions** → `venue`/`year` are `not null`;
  use `is_empty()` / `year <= 0`, not `is_none()`.
- **Dragging chat tables into an export** → use `CiteRecord`, not `Paper`.
- **Lowercased titles** in the rendered bibliography → double-brace titles.

## Open Questions

- Should a later RFC add DOI persistence (schema + enrichment write-back) and
  then a `doi` field here?
- Should re-export offer append/merge against an existing `.bib` instead of
  overwrite?
- Should per-paper selection export a subset later?

## Acceptance Criteria

- The top-panel "Export .bib" action in `VaultHome` is wired and disabled for an
  empty vault.
- Choosing a path writes a `.bib` containing one entry per paper in the vault.
- Cite keys follow `authorYEARword`; collisions get `a`/`b`/`c` suffixes.
- Re-exporting an unchanged vault produces **byte-identical** output
  (covered by a `cargo test`).
- Authors render `Family, Given`, joined with ` and `; single-token and
  already-comma names handled.
- Entry type is `@inproceedings` / `@article` / `@misc` per the venue rules.
- Missing author → `anon`; `year <= 0` → `nd`.
- LaTeX specials are escaped without double-escaping; titles are double-braced.
- No `doi` field is emitted.
- Export does not touch chat tables (uses `CiteRecord`, not `Paper`).
- `pnpm check` passes.
- `pnpm build` passes.
- `cargo check` passes.
- `cargo test` passes.
