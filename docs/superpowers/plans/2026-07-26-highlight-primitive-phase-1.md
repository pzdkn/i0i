# Highlight Primitive + Capability Layer (RFC 0058, Phase 1) — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the highlight a standalone, colored, author-agnostic primitive; notes and chat threads attach to it; the same operations are callable by the UI now and by the agent later (RFC 0059).

**Architecture:** A new `highlights` table owns the locator + color + label + author. `chat_threads` gains a nullable `highlight_id`; note/ask keep the existing thread/entry machinery but key off a highlight (or the document). One author-agnostic `HighlightService` owns every verb, exposed through Tauri commands now and an agent tool registry later. Highlight visibility becomes "the highlight exists" (both readers), decoupling pinning from visibility and fixing the after-ask-vanishes bug.

**Tech Stack:** Rust (rusqlite, serde, Tauri v2), Svelte 5 runes, CSS Custom Highlight API (HTML), PDF.js rect overlay (PDF).

## Global Constraints

- Rust: per-op SQLite connections, WAL + busy_timeout already configured in `LibraryStore`; follow existing `read_*`/`append_*` helper patterns in `storage/library_store.rs`.
- Named color palette (verbatim, closed set): `yellow` (default), `green`, `blue`, `red`, `purple`, `orange`. Stored as the name string, never hex.
- `Locator` serde tag key is `kind`; variants `textOffset` / `pdfRect` (camelCase), matching the existing `ThreadAnchor` payloads so migration is a field lift.
- Backend never slices `source_text` by TextOffset offsets — offsets are browser-space only (RFC 0056). Preserve this: the backend stores/echoes locators, it does not resolve them.
- Author enum values (serde): `user`, and `agent` with a `model` string.
- Run backend tests with: `cargo test --no-default-features --features embeddings` (from `src-tauri/`). Frontend gate: `pnpm check`.
- The user commits; do not `git commit` unless a step says so — instead leave the working tree clean per task and let the reviewer commit. (This project's owner commits manually.)

---

### Task 1: Shared domain types (`HighlightColor`, `Locator`, `HighlightAuthor`, `Highlight`)

**Files:**
- Create: `src-tauri/src/domain/highlight.rs`
- Modify: `src-tauri/src/domain/mod.rs` (add `pub mod highlight;`)

**Interfaces:**
- Produces: `HighlightColor` (enum, serde lowercase), `Locator` (enum tag `kind`), `HighlightAuthor` (enum), `Highlight` (struct), `HighlightColor::default() == Yellow`, `Locator::source_id() -> &str`.

- [ ] **Step 1: Write the failing test** in `src-tauri/src/domain/highlight.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_serdes_as_lowercase_name() {
        assert_eq!(serde_json::to_string(&HighlightColor::Yellow).unwrap(), "\"yellow\"");
        assert_eq!(
            serde_json::from_str::<HighlightColor>("\"red\"").unwrap(),
            HighlightColor::Red
        );
        assert_eq!(HighlightColor::default(), HighlightColor::Yellow);
    }

    #[test]
    fn locator_round_trips_with_kind_tag_and_exposes_source() {
        let loc = Locator::TextOffset {
            source_id: "src-1".into(),
            start_offset: 10,
            end_offset: 40,
        };
        let json = serde_json::to_string(&loc).unwrap();
        assert!(json.contains("\"kind\":\"textOffset\""));
        assert_eq!(loc.source_id(), "src-1");
        let back: Locator = serde_json::from_str(&json).unwrap();
        assert_eq!(back, loc);
    }

    #[test]
    fn author_serdes_user_and_agent() {
        assert_eq!(serde_json::to_string(&HighlightAuthor::User).unwrap(), "\"user\"");
        let agent = HighlightAuthor::Agent { model: "x".into() };
        let json = serde_json::to_string(&agent).unwrap();
        assert!(json.contains("\"model\":\"x\""));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd src-tauri && cargo test --no-default-features --features embeddings domain::highlight`
Expected: FAIL — module/types not defined.

- [ ] **Step 3: Write the types** (top of `src-tauri/src/domain/highlight.rs`):

```rust
//! The highlight primitive (RFC 0058): a standalone, colored mark on a passage.
//! Notes and chat threads attach to a highlight; the locator lives here, lifted
//! out of the thread's embedded anchor.

use serde::{Deserialize, Serialize};

/// Fixed, named palette. Stored as the name string (never hex) so UI, storage,
/// and the future agent tool share one vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum HighlightColor {
    #[default]
    Yellow,
    Green,
    Blue,
    Red,
    Purple,
    Orange,
}

/// WHERE a highlight sits on a rendered source. Payloads mirror the legacy
/// `ThreadAnchor` selection variants so migration is a field lift. Offsets are
/// browser-space (RFC 0056); the backend never resolves them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum Locator {
    #[serde(rename = "textOffset")]
    TextOffset {
        #[serde(rename = "sourceId")]
        source_id: String,
        #[serde(rename = "startOffset")]
        start_offset: i64,
        #[serde(rename = "endOffset")]
        end_offset: i64,
    },
    #[serde(rename = "pdfRect")]
    PdfRect {
        #[serde(rename = "sourceId")]
        source_id: String,
        #[serde(rename = "pageIndex")]
        page_index: i32,
        #[serde(rename = "rectsJson")]
        rects_json: String,
    },
}

impl Locator {
    pub fn source_id(&self) -> &str {
        match self {
            Locator::TextOffset { source_id, .. } | Locator::PdfRect { source_id, .. } => source_id,
        }
    }

    /// Storage discriminator, shared with the legacy `anchor_kind` values.
    pub fn kind_str(&self) -> &'static str {
        match self {
            Locator::TextOffset { .. } => "text_offset",
            Locator::PdfRect { .. } => "pdf_rect",
        }
    }
}

/// Who created a highlight. The agent variant carries the model id.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum HighlightAuthor {
    User,
    Agent {
        #[serde(rename = "model")]
        model: String,
    },
}

impl HighlightAuthor {
    pub fn kind_str(&self) -> &'static str {
        match self {
            HighlightAuthor::User => "user",
            HighlightAuthor::Agent { .. } => "agent",
        }
    }
    pub fn model(&self) -> Option<&str> {
        match self {
            HighlightAuthor::User => None,
            HighlightAuthor::Agent { model } => Some(model),
        }
    }
}

/// The primitive, as returned across IPC.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Highlight {
    pub id: String,
    pub paper_id: String,
    pub source_id: String,
    pub locator: Locator,
    pub excerpt: String,
    pub color: HighlightColor,
    pub label: Option<String>,
    pub author: HighlightAuthor,
    pub created_at: String,
    pub updated_at: String,
}
```

Add `pub mod highlight;` to `src-tauri/src/domain/mod.rs`.

- [ ] **Step 4: Run test to verify it passes**

Run: `cd src-tauri && cargo test --no-default-features --features embeddings domain::highlight`
Expected: PASS (3 tests).

- [ ] **Step 5: Leave the working tree clean for review** (no commit; the owner commits).

---

### Task 2: `highlights` table + `chat_threads.highlight_id` column + storage CRUD

**Files:**
- Modify: `src-tauri/src/storage/library_store.rs` (schema in the `init` `create table` block near line 1409; add CRUD helpers near the other chat helpers)

**Interfaces:**
- Consumes: `domain::highlight::{Highlight, HighlightColor, Locator, HighlightAuthor}`.
- Produces on `LibraryStore`:
  - `insert_highlight(paper_id, &Locator, excerpt, HighlightColor, label: Option<&str>, &HighlightAuthor) -> StoreResult<Highlight>`
  - `list_highlights(paper_id) -> StoreResult<Vec<Highlight>>`
  - `recolor_highlight(id, HighlightColor) -> StoreResult<()>`
  - `set_highlight_label(id, label: Option<&str>) -> StoreResult<()>`
  - `remove_highlight(id) -> StoreResult<()>`

- [ ] **Step 1: Write the failing test** (in the `tests` module of `library_store.rs`, alongside the existing chat tests):

```rust
#[test]
fn highlight_crud_round_trips() {
    let store = LibraryStore::open_in_memory().expect("store");
    store.init().expect("init");
    let paper_id = store.seed_paper_for_test("Paper"); // existing test helper; see note
    let loc = crate::domain::highlight::Locator::TextOffset {
        source_id: "src-1".into(),
        start_offset: 5,
        end_offset: 25,
    };
    let hl = store
        .insert_highlight(
            &paper_id,
            &loc,
            "quoted text",
            crate::domain::highlight::HighlightColor::Yellow,
            Some("important"),
            &crate::domain::highlight::HighlightAuthor::User,
        )
        .expect("insert");
    assert_eq!(hl.color, crate::domain::highlight::HighlightColor::Yellow);
    assert_eq!(hl.excerpt, "quoted text");

    store
        .recolor_highlight(&hl.id, crate::domain::highlight::HighlightColor::Red)
        .expect("recolor");
    let listed = store.list_highlights(&paper_id).expect("list");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].color, crate::domain::highlight::HighlightColor::Red);

    store.remove_highlight(&hl.id).expect("remove");
    assert!(store.list_highlights(&paper_id).expect("list2").is_empty());
}
```

> Note: use whatever paper-seeding helper the existing chat tests use (search the test module for how a `paper_id` is created — e.g. an `add_paper*`/`create_vault` pair). If none is reusable, insert a minimal paper row inline as the neighboring tests do.

- [ ] **Step 2: Run test to verify it fails**

Run: `cd src-tauri && cargo test --no-default-features --features embeddings highlight_crud`
Expected: FAIL — `insert_highlight` not found.

- [ ] **Step 3: Add the schema** inside `init`'s SQL batch (right after the `chat_entries` table / indexes, ~line 1444):

```sql
create table if not exists highlights (
  id text primary key,
  paper_id text not null,
  source_id text not null,
  locator_kind text not null,            -- 'text_offset' | 'pdf_rect'
  start_offset integer,
  end_offset integer,
  page_index integer,
  rects_json text,
  excerpt text not null,
  color text not null,
  label text,
  author_kind text not null,             -- 'user' | 'agent'
  author_model text,
  created_at text not null,
  updated_at text not null
);

create index if not exists idx_highlights_paper
  on highlights(paper_id, created_at);
```

Add the `highlight_id` column to the `chat_threads` create-table (nullable):

```sql
              highlight_id text,
```

(Insert it right after `rects_json text,` in the existing `chat_threads` definition. Because `create table if not exists` won't alter an existing table, ALSO add a guarded migration — see Task 3 Step 3 for the `alter table` that adds the column to already-created DBs.)

- [ ] **Step 4: Add the CRUD helpers** (near the other chat helpers). Use the crate's existing id/time helpers (search for how threads generate `id` and `created_at`; reuse the same `new_id()` / `now_rfc3339()` style functions):

```rust
pub fn insert_highlight(
    &self,
    paper_id: &str,
    locator: &crate::domain::highlight::Locator,
    excerpt: &str,
    color: crate::domain::highlight::HighlightColor,
    label: Option<&str>,
    author: &crate::domain::highlight::HighlightAuthor,
) -> StoreResult<crate::domain::highlight::Highlight> {
    use crate::domain::highlight::Locator;
    let conn = self.conn()?;
    let id = new_id();                 // reuse existing id helper
    let now = now_rfc3339();           // reuse existing time helper
    let color_str = serde_json::to_string(&color).ok().map(|s| s.trim_matches('"').to_string())
        .unwrap_or_else(|| "yellow".into());
    let (start, end, page, rects) = match locator {
        Locator::TextOffset { start_offset, end_offset, .. } =>
            (Some(*start_offset), Some(*end_offset), None, None),
        Locator::PdfRect { page_index, rects_json, .. } =>
            (None, None, Some(*page_index), Some(rects_json.clone())),
    };
    conn.execute(
        "insert into highlights
           (id, paper_id, source_id, locator_kind, start_offset, end_offset,
            page_index, rects_json, excerpt, color, label, author_kind,
            author_model, created_at, updated_at)
         values (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?14)",
        params![
            id, paper_id, locator.source_id(), locator.kind_str(),
            start, end, page, rects, excerpt, color_str, label,
            author.kind_str(), author.model(), now
        ],
    )?;
    self.read_highlight(&id)
}

fn read_highlight(&self, id: &str) -> StoreResult<crate::domain::highlight::Highlight> {
    let conn = self.conn()?;
    conn.query_row(
        "select id, paper_id, source_id, locator_kind, start_offset, end_offset,
                page_index, rects_json, excerpt, color, label, author_kind,
                author_model, created_at, updated_at
         from highlights where id = ?1",
        params![id],
        row_to_highlight,
    )
    .map_err(Into::into)
}

pub fn list_highlights(
    &self,
    paper_id: &str,
) -> StoreResult<Vec<crate::domain::highlight::Highlight>> {
    let conn = self.conn()?;
    let mut stmt = conn.prepare(
        "select id, paper_id, source_id, locator_kind, start_offset, end_offset,
                page_index, rects_json, excerpt, color, label, author_kind,
                author_model, created_at, updated_at
         from highlights where paper_id = ?1 order by created_at",
    )?;
    let rows = stmt.query_map(params![paper_id], row_to_highlight)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

pub fn recolor_highlight(
    &self,
    id: &str,
    color: crate::domain::highlight::HighlightColor,
) -> StoreResult<()> {
    let color_str = serde_json::to_string(&color).ok()
        .map(|s| s.trim_matches('"').to_string()).unwrap_or_else(|| "yellow".into());
    self.conn()?.execute(
        "update highlights set color = ?2, updated_at = ?3 where id = ?1",
        params![id, color_str, now_rfc3339()],
    )?;
    Ok(())
}

pub fn set_highlight_label(&self, id: &str, label: Option<&str>) -> StoreResult<()> {
    self.conn()?.execute(
        "update highlights set label = ?2, updated_at = ?3 where id = ?1",
        params![id, label, now_rfc3339()],
    )?;
    Ok(())
}

pub fn remove_highlight(&self, id: &str) -> StoreResult<()> {
    self.conn()?.execute("delete from highlights where id = ?1", params![id])?;
    Ok(())
}
```

Add the row mapper near the other `row_to_*` helpers:

```rust
fn row_to_highlight(row: &rusqlite::Row) -> rusqlite::Result<crate::domain::highlight::Highlight> {
    use crate::domain::highlight::{Highlight, HighlightAuthor, HighlightColor, Locator};
    let locator_kind: String = row.get(3)?;
    let source_id: String = row.get(2)?;
    let locator = if locator_kind == "pdf_rect" {
        Locator::PdfRect {
            source_id: source_id.clone(),
            page_index: row.get::<_, Option<i32>>(6)?.unwrap_or(0),
            rects_json: row.get::<_, Option<String>>(7)?.unwrap_or_default(),
        }
    } else {
        Locator::TextOffset {
            source_id: source_id.clone(),
            start_offset: row.get::<_, Option<i64>>(4)?.unwrap_or(0),
            end_offset: row.get::<_, Option<i64>>(5)?.unwrap_or(0),
        }
    };
    let color: HighlightColor =
        serde_json::from_str(&format!("\"{}\"", row.get::<_, String>(9)?)).unwrap_or_default();
    let author = match row.get::<_, String>(11)?.as_str() {
        "agent" => HighlightAuthor::Agent { model: row.get::<_, Option<String>>(12)?.unwrap_or_default() },
        _ => HighlightAuthor::User,
    };
    Ok(Highlight {
        id: row.get(0)?,
        paper_id: row.get(1)?,
        source_id,
        locator,
        excerpt: row.get(8)?,
        color,
        label: row.get(10)?,
        author,
        created_at: row.get(13)?,
        updated_at: row.get(14)?,
    })
}
```

- [ ] **Step 5: Run test to verify it passes**

Run: `cd src-tauri && cargo test --no-default-features --features embeddings highlight_crud`
Expected: PASS.

- [ ] **Step 6: Leave the working tree clean for review.**

---

### Task 3: Backfill migration — existing anchored threads → highlights

**Files:**
- Modify: `src-tauri/src/storage/library_store.rs` (add `alter table` guard + a `migrate_threads_to_highlights` run inside/after `init`)

**Interfaces:**
- Consumes: `insert_highlight`, the `chat_threads` legacy anchor columns.
- Produces: `LibraryStore::migrate_threads_to_highlights() -> StoreResult<usize>` (idempotent; returns rows migrated). Called from `init` after table creation.

- [ ] **Step 1: Write the failing test:**

```rust
#[test]
fn backfill_makes_anchored_threads_into_visible_highlights() {
    let store = LibraryStore::open_in_memory().expect("store");
    store.init().expect("init");
    let paper_id = store.seed_paper_for_test("Paper");
    // Create a legacy-style anchored thread via the existing note path.
    let anchor = crate::domain::chat::ThreadAnchor::TextOffset {
        source_id: "src-1".into(), start_offset: 3, end_offset: 12, selected_text: "abc".into(),
    };
    store.add_note_at_anchor_with_creation("paper", &paper_id, &anchor, "note body").expect("note");

    let migrated = store.migrate_threads_to_highlights().expect("migrate");
    assert_eq!(migrated, 1);
    let highlights = store.list_highlights(&paper_id).expect("list");
    assert_eq!(highlights.len(), 1);
    assert_eq!(highlights[0].excerpt, "abc");
    // The thread now references the new highlight.
    // (Assert via a helper that reads highlight_id — add `thread_highlight_id(id)`.)

    // Idempotent: second run migrates nothing.
    assert_eq!(store.migrate_threads_to_highlights().expect("again"), 0);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd src-tauri && cargo test --no-default-features --features embeddings backfill_makes`
Expected: FAIL — `migrate_threads_to_highlights` not found.

- [ ] **Step 3: Add the column guard + migration.** In `init`, after the `create table` batch, add an idempotent column add and run the backfill:

```rust
// chat_threads.highlight_id may predate this column on an existing DB.
let _ = conn.execute("alter table chat_threads add column highlight_id text", []);
```

(Wrap in `let _ =` because a second launch errors "duplicate column"; that's the idempotency guard.)

Then implement:

```rust
/// One-time backfill (RFC 0058): every anchored thread (text_offset / pdf_rect)
/// with no highlight yet becomes a User highlight, and the thread points at it.
/// Document-anchored threads are left with highlight_id = NULL. Idempotent.
pub fn migrate_threads_to_highlights(&self) -> StoreResult<usize> {
    use crate::domain::highlight::{HighlightAuthor, HighlightColor, Locator};
    let conn = self.conn()?;
    let mut stmt = conn.prepare(
        "select t.id, t.scope_id, t.anchor_kind, t.source_id, t.start_offset,
                t.end_offset, t.selected_text, t.page_index, t.rects_json
         from chat_threads t
         where t.highlight_id is null and t.anchor_kind in ('text_offset','pdf_rect')",
    )?;
    struct Legacy { thread_id: String, paper_id: String, kind: String, source: Option<String>,
        start: Option<i64>, end: Option<i64>, text: Option<String>, page: Option<i32>, rects: Option<String> }
    let rows: Vec<Legacy> = stmt
        .query_map([], |r| Ok(Legacy {
            thread_id: r.get(0)?, paper_id: r.get(1)?, kind: r.get(2)?, source: r.get(3)?,
            start: r.get(4)?, end: r.get(5)?, text: r.get(6)?, page: r.get(7)?, rects: r.get(8)?,
        }))?
        .collect::<Result<_, _>>()?;
    let mut count = 0;
    for row in rows {
        let source_id = row.source.unwrap_or_default();
        let locator = if row.kind == "pdf_rect" {
            Locator::PdfRect { source_id, page_index: row.page.unwrap_or(0),
                rects_json: row.rects.unwrap_or_default() }
        } else {
            Locator::TextOffset { source_id, start_offset: row.start.unwrap_or(0),
                end_offset: row.end.unwrap_or(0) }
        };
        let excerpt = row.text.unwrap_or_default();
        let hl = self.insert_highlight(&row.paper_id, &locator, &excerpt,
            HighlightColor::default(), None, &HighlightAuthor::User)?;
        conn.execute("update chat_threads set highlight_id = ?2 where id = ?1",
            params![row.thread_id, hl.id])?;
        count += 1;
    }
    Ok(count)
}
```

Call `self.migrate_threads_to_highlights()?;` at the end of `init` (after the schema batch). Add a tiny `thread_highlight_id(id) -> StoreResult<Option<String>>` reader for the test.

- [ ] **Step 4: Run test to verify it passes**

Run: `cd src-tauri && cargo test --no-default-features --features embeddings backfill_makes`
Expected: PASS.

- [ ] **Step 5: Run the full backend suite** (migration touches `init`, which every store test calls):

Run: `cd src-tauri && cargo test --no-default-features --features embeddings`
Expected: all pass (no regressions in existing chat/store tests).

- [ ] **Step 6: Leave the working tree clean for review.**

---

### Task 4: `HighlightService` (author-agnostic ops) + re-key note/ask to a target

**Files:**
- Create: `src-tauri/src/services/highlight/mod.rs`
- Modify: `src-tauri/src/services/mod.rs` (add `pub mod highlight;`)
- Modify: `src-tauri/src/services/chat/service.rs` (add `add_note_at_target` / `ask_at_target_streamed` that resolve a `HighlightTarget` to a thread by `highlight_id`; keep the old anchor methods as thin shims during transition)

**Interfaces:**
- Consumes: `LibraryStore` highlight CRUD (Task 2), `ChatService`, `domain::highlight::*`.
- Produces:
  - `HighlightTarget` (enum): `Highlight { id: String }` | `Document { paper_id: String }`.
  - `HighlightService::new(store: LibraryStore) -> Self`
  - `create_highlight(paper_id, Locator, excerpt, HighlightColor, label, HighlightAuthor) -> Result<Highlight, String>`
  - `recolor(id, HighlightColor) -> Result<(), String>`
  - `set_label(id, Option<String>) -> Result<(), String>`
  - `remove(id) -> Result<(), String>`
  - `list(paper_id) -> Result<Vec<Highlight>, String>`

- [ ] **Step 1: Write the failing test** in `src-tauri/src/services/highlight/mod.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::highlight::{HighlightAuthor, HighlightColor, Locator};
    use crate::storage::library_store::LibraryStore;

    fn service() -> (HighlightService, String) {
        let store = LibraryStore::open_in_memory().unwrap();
        store.init().unwrap();
        let paper_id = store.seed_paper_for_test("Paper");
        (HighlightService::new(store), paper_id)
    }

    #[tokio::test]
    async fn create_is_author_agnostic() {
        let (svc, paper) = service();
        let loc = Locator::TextOffset { source_id: "s".into(), start_offset: 0, end_offset: 4 };
        let user = svc.create_highlight(&paper, loc.clone(), "quote", HighlightColor::Yellow, None,
            HighlightAuthor::User).await.unwrap();
        assert_eq!(user.author, HighlightAuthor::User);
        let agent = svc.create_highlight(&paper, loc, "quote", HighlightColor::Red,
            Some("exp".into()), HighlightAuthor::Agent { model: "m".into() }).await.unwrap();
        assert!(matches!(agent.author, HighlightAuthor::Agent { .. }));
        assert_eq!(svc.list(&paper).await.unwrap().len(), 2);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd src-tauri && cargo test --no-default-features --features embeddings services::highlight`
Expected: FAIL — module not found.

- [ ] **Step 3: Implement the service:**

```rust
//! Author-agnostic capability layer for highlights (RFC 0058). Every verb takes
//! an explicit author; the UI (Tauri commands) and, later, the agent tool
//! registry (RFC 0059) call these same methods. The service takes an
//! already-resolved `Locator` — it never resolves quotes and never assumes an
//! operation completed synchronously on the client (PDF resolution is
//! client-side; see RFC 0058/0059).

use crate::domain::highlight::{Highlight, HighlightAuthor, HighlightColor, Locator};
use crate::storage::library_store::LibraryStore;

#[derive(Debug, Clone)]
pub enum HighlightTarget {
    Highlight { id: String },
    Document { paper_id: String },
}

#[derive(Clone)]
pub struct HighlightService {
    store: LibraryStore,
}

impl HighlightService {
    pub fn new(store: LibraryStore) -> Self {
        Self { store }
    }

    pub async fn create_highlight(
        &self,
        paper_id: &str,
        locator: Locator,
        excerpt: &str,
        color: HighlightColor,
        label: Option<String>,
        author: HighlightAuthor,
    ) -> Result<Highlight, String> {
        self.store
            .insert_highlight(paper_id, &locator, excerpt, color, label.as_deref(), &author)
            .map_err(|e| e.to_string())
    }

    pub async fn recolor(&self, id: &str, color: HighlightColor) -> Result<(), String> {
        self.store.recolor_highlight(id, color).map_err(|e| e.to_string())
    }

    pub async fn set_label(&self, id: &str, label: Option<String>) -> Result<(), String> {
        self.store.set_highlight_label(id, label.as_deref()).map_err(|e| e.to_string())
    }

    pub async fn remove(&self, id: &str) -> Result<(), String> {
        self.store.remove_highlight(id).map_err(|e| e.to_string())
    }

    pub async fn list(&self, paper_id: &str) -> Result<Vec<Highlight>, String> {
        self.store.list_highlights(paper_id).map_err(|e| e.to_string())
    }
}
```

Add `pub mod highlight;` to `src-tauri/src/services/mod.rs`.

- [ ] **Step 4: Run test to verify it passes**

Run: `cd src-tauri && cargo test --no-default-features --features embeddings services::highlight`
Expected: PASS.

- [ ] **Step 5: Leave the working tree clean for review.**

> Note on note/ask re-keying: the existing `add_note_at_anchor` / `ask_at_anchor_streamed` continue to work (they create anchored threads). Task 3's backfill converts anchored threads to highlights on `init`, and Task 9 wires the UI so new notes/asks target a `highlight_id`. A follow-up within this plan (Task 9's backend touchpoint) adds `add_note_at_target` that, given `HighlightTarget::Highlight { id }`, finds-or-creates the thread with that `highlight_id`. Keep this task's scope to the highlight CRUD service; the note/ask target wiring lands with the UI task that needs it.

---

### Task 5: Tauri commands + registration

**Files:**
- Create: `src-tauri/src/commands/highlight.rs`
- Modify: `src-tauri/src/commands/mod.rs` (add `pub mod highlight;`)
- Modify: `src-tauri/src/lib.rs` (construct `HighlightService`, `app.manage(...)`, add commands to `generate_handler!`)

**Interfaces:**
- Consumes: `HighlightService`, `domain::highlight::*`.
- Produces Tauri commands: `create_highlight`, `recolor_highlight`, `set_highlight_label`, `remove_highlight`, `list_highlights`.

- [ ] **Step 1: Write the command module:**

```rust
//! Tauri commands: the UI front door to `HighlightService` (RFC 0058).

use crate::domain::highlight::{Highlight, HighlightAuthor, HighlightColor, Locator};
use crate::services::highlight::HighlightService;

#[tauri::command]
pub async fn create_highlight(
    service: tauri::State<'_, HighlightService>,
    paper_id: String,
    locator: Locator,
    excerpt: String,
    color: HighlightColor,
    label: Option<String>,
) -> Result<Highlight, String> {
    service
        .create_highlight(&paper_id, locator, &excerpt, color, label, HighlightAuthor::User)
        .await
}

#[tauri::command]
pub async fn recolor_highlight(
    service: tauri::State<'_, HighlightService>,
    id: String,
    color: HighlightColor,
) -> Result<(), String> {
    service.recolor(&id, color).await
}

#[tauri::command]
pub async fn set_highlight_label(
    service: tauri::State<'_, HighlightService>,
    id: String,
    label: Option<String>,
) -> Result<(), String> {
    service.set_label(&id, label).await
}

#[tauri::command]
pub async fn remove_highlight(
    service: tauri::State<'_, HighlightService>,
    id: String,
) -> Result<(), String> {
    service.remove(&id).await
}

#[tauri::command]
pub async fn list_highlights(
    service: tauri::State<'_, HighlightService>,
    paper_id: String,
) -> Result<Vec<Highlight>, String> {
    service.list(&paper_id).await
}
```

Add `pub mod highlight;` to `src-tauri/src/commands/mod.rs`.

- [ ] **Step 2: Register in `lib.rs`.** In `setup`, after `store.init()`:

```rust
let highlight_service = services::highlight::HighlightService::new(store.clone());
```

Add `app.manage(highlight_service);` next to the other `app.manage(...)` calls, and add to `generate_handler![ ... ]`:

```rust
commands::highlight::create_highlight,
commands::highlight::recolor_highlight,
commands::highlight::set_highlight_label,
commands::highlight::remove_highlight,
commands::highlight::list_highlights,
```

- [ ] **Step 3: Build to verify wiring**

Run: `cd src-tauri && cargo build --no-default-features --features embeddings`
Expected: compiles; no unused-warning on the new commands (they're referenced by `generate_handler!`).

- [ ] **Step 4: Leave the working tree clean for review.**

---

### Task 6: Frontend bridge + domain types

**Files:**
- Create: `src/lib/domain/highlight.ts`
- Create: `src/lib/bridge/highlight.ts`

**Interfaces:**
- Produces TS: `HighlightColor` (union), `Locator` (discriminated union, `kind`), `HighlightAuthor`, `Highlight`, and `HIGHLIGHT_COLORS: HighlightColor[]`; bridge fns `createHighlight`, `recolorHighlight`, `setHighlightLabel`, `removeHighlight`, `listHighlights`.

- [ ] **Step 1: Domain types** in `src/lib/domain/highlight.ts`:

```ts
export type HighlightColor = "yellow" | "green" | "blue" | "red" | "purple" | "orange";
export const HIGHLIGHT_COLORS: HighlightColor[] = ["yellow", "green", "blue", "red", "purple", "orange"];

export type Locator =
  | { kind: "textOffset"; sourceId: string; startOffset: number; endOffset: number }
  | { kind: "pdfRect"; sourceId: string; pageIndex: number; rectsJson: string };

export type HighlightAuthor = { kind: "user" } | { kind: "agent"; model: string };

export interface Highlight {
  id: string;
  paperId: string;
  sourceId: string;
  locator: Locator;
  excerpt: string;
  color: HighlightColor;
  label: string | null;
  author: HighlightAuthor;
  createdAt: string;
  updatedAt: string;
}
```

- [ ] **Step 2: Bridge** in `src/lib/bridge/highlight.ts` (follow the existing `bridge/*.ts` `invoke` pattern):

```ts
import { invoke } from "@tauri-apps/api/core";
import type { Highlight, HighlightColor, Locator } from "$lib/domain/highlight";

export function createHighlight(args: {
  paperId: string; locator: Locator; excerpt: string; color: HighlightColor; label?: string | null;
}): Promise<Highlight> {
  return invoke<Highlight>("create_highlight", {
    paperId: args.paperId, locator: args.locator, excerpt: args.excerpt,
    color: args.color, label: args.label ?? null,
  });
}

export function recolorHighlight(id: string, color: HighlightColor): Promise<void> {
  return invoke("recolor_highlight", { id, color });
}

export function setHighlightLabel(id: string, label: string | null): Promise<void> {
  return invoke("set_highlight_label", { id, label });
}

export function removeHighlight(id: string): Promise<void> {
  return invoke("remove_highlight", { id });
}

export function listHighlights(paperId: string): Promise<Highlight[]> {
  return invoke<Highlight[]>("list_highlights", { paperId });
}
```

- [ ] **Step 3: Type-check**

Run: `pnpm check`
Expected: 0 errors.

- [ ] **Step 4: Leave the working tree clean for review.**

---

### Task 7: Multi-color highlight rendering — HTML reader

**Files:**
- Modify: `src/lib/features/reader/HtmlReader.svelte`
- Modify: `src/lib/features/reader/ReaderView.svelte` (load `highlights` alongside threads; pass `highlights` to `HtmlReader`)

**Interfaces:**
- Consumes: `listHighlights` (Task 6), `Highlight`, `HIGHLIGHT_COLORS`.
- Produces: `HtmlReader` draws one CSS Custom Highlight per color from the `highlights` prop; click-on-mark still opens the linked thread if any.

- [ ] **Step 1:** In `ReaderView.svelte`, add `let highlights = $state<Highlight[]>([])`, load it in the same effect that loads threads/pins (`listHighlights(paperId)`), and pass `{highlights}` into the `HtmlReader` branch (replace the `{threads}`-derived marks). Reload it in the same reload path (line ~255–271) so marks/threads/pins stay in sync.

- [ ] **Step 2:** In `HtmlReader.svelte`, replace the single-highlight repaint `$effect` (currently registers `i0i-annotation` from `threads`) with a per-color registration from `highlights`:

```ts
// Repaint one CSS Custom Highlight per color from the highlights list.
$effect(() => {
  html;
  const current = highlights;
  if (!root || !html) return;
  const api = (CSS as unknown as { highlights?: Map<string, unknown> }).highlights;
  const Ctor = (globalThis as { Highlight?: new (...r: Range[]) => unknown }).Highlight;
  if (!api || typeof Ctor !== "function") return;

  const byColor = new Map<string, Range[]>();
  for (const hl of current) {
    if (hl.locator.kind !== "textOffset" || hl.locator.sourceId !== sourceId) continue;
    const start = locate(root, hl.locator.startOffset);
    const end = locate(root, hl.locator.endOffset);
    if (!start || !end) continue;
    const range = document.createRange();
    range.setStart(start.node, start.offset);
    range.setEnd(end.node, end.offset);
    (byColor.get(hl.color) ?? byColor.set(hl.color, []).get(hl.color)!).push(range);
  }
  const names = HIGHLIGHT_COLORS.map((c) => `i0i-hl-${c}`);
  for (const c of HIGHLIGHT_COLORS) api.set(`i0i-hl-${c}`, new Ctor(...(byColor.get(c) ?? [])));
  return () => names.forEach((n) => api.delete(n));
});
```

- [ ] **Step 3:** Replace the single `::highlight(i0i-annotation)` CSS with one rule per color, theme-aware:

```css
:global(::highlight(i0i-hl-yellow)) { background: rgba(242, 201, 76, 0.32); }
:global(::highlight(i0i-hl-green))  { background: rgba(111, 207, 151, 0.32); }
:global(::highlight(i0i-hl-blue))   { background: rgba(86, 156, 214, 0.32); }
:global(::highlight(i0i-hl-red))    { background: rgba(235, 87, 87, 0.32); }
:global(::highlight(i0i-hl-purple)) { background: rgba(187, 107, 217, 0.32); }
:global(::highlight(i0i-hl-orange)) { background: rgba(242, 153, 74, 0.32); }
```

- [ ] **Step 4:** Update `threadAt`/click handling to open a linked thread by the highlight's thread if present (keep current behavior for now: clicking a highlighted span opens its thread via the existing `onOpenThread`). Adjust `HtmlReader` props to accept `highlights: Highlight[]`.

- [ ] **Step 5: Type-check + manual smoke**

Run: `pnpm check` (expect 0 errors). Manual: open an HTML article, confirm a saved highlight renders in its color and persists after asking (no vanish).

- [ ] **Step 6: Leave the working tree clean for review.**

---

### Task 8: Multi-color rendering — PDF reader (drops the pin-gate; closes the vanishing bug)

**Files:**
- Modify: `src/lib/features/reader/PdfPage.svelte` and/or `PdfRenderedPage.svelte`
- Modify: `src/lib/features/reader/ReaderView.svelte` (pass `highlights` to the PDF branch)

**Interfaces:**
- Consumes: `highlights` prop.
- Produces: PDF marks derived from `highlights` (color per fill), no `pinnedCount` filter.

- [ ] **Step 1:** In `PdfPage.svelte`, replace the marks derivation (currently `threads.filter(t => t.pinnedCount > 0 && t.anchor.kind === "pdfRect" && t.anchor.sourceId === sourceId)`) with:

```ts
const marks = $derived(
  highlights.filter((hl) => hl.locator.kind === "pdfRect" && hl.locator.sourceId === sourceId),
);
```

Thread the `highlights` prop from `ReaderView` → `PdfPage` → `PdfRenderedPage`, replacing the pinned-thread source. Each mark's fill color = `hl.color` (map to the same rgba palette as Task 7 via a shared helper `highlightFill(color)` in a small `highlight-colors.ts`).

- [ ] **Step 2:** In `PdfRenderedPage.svelte`, color each rect fill by the mark's color (currently a single style); page filter stays `mark.locator.pageIndex === pageIndex`, rects from `mark.locator.rectsJson`.

- [ ] **Step 3: Manual smoke — the bug fix**

Open a PDF, select a passage, **Ask** (not note). Confirm the highlight now **persists** after the reply (previously vanished because asks aren't pinned). Also confirm an unpinned migrated PDF highlight is visible.

- [ ] **Step 4:** Run `pnpm check` (0 errors). Leave the working tree clean for review.

---

### Task 9: Selection toolbar — swatches + one-gesture Note/Ask

**Files:**
- Create: `src/lib/features/reader/SelectionToolbar.svelte`
- Modify: `src/lib/features/reader/ReaderView.svelte` (render toolbar on `selection`; sticky color state)
- Modify: `src/lib/features/reader/ReaderInspector.svelte` (accept an initial highlight target so Note/Ask attach to a just-created highlight)
- Modify: `src-tauri/src/services/chat/service.rs` + `src-tauri/src/commands/chat.rs` (add `add_note_at_target` / `ask_at_target_streamed` keyed by `highlight_id`)

**Interfaces:**
- Consumes: `createHighlight`, `recolorHighlight`, `HIGHLIGHT_COLORS`, the existing `noteAtAnchor`/`askAtAnchorStreamed` (re-pointed to a highlight target).
- Produces: a floating toolbar; a `stickyColor` reader-scoped state; `create-highlight-then-note/ask` in one gesture.

- [ ] **Step 1:** Build `SelectionToolbar.svelte`: props `{ selection, stickyColor, onHighlight(color), onNote, onAsk }`. Render `HIGHLIGHT_COLORS.map(...)` as swatch buttons (fill = palette), plus **Note** and **Ask** buttons; position it at the selection anchor (reuse the `x`/`y` the selection already carries — see `PdfRenderedPage.svelte:187` for the coordinate pattern; HTML selection can use the range's bounding rect).

- [ ] **Step 2:** In `ReaderView.svelte`: hold `let stickyColor = $state<HighlightColor>("yellow")`. On swatch click → `createHighlight({ paperId, locator: locatorFromSelection(selection), excerpt: selection.selectedText, color })`, set `stickyColor = color`, reload highlights, clear `selection`. On **Note**/**Ask** → create the highlight (stickyColor) first, then open `ReaderInspector` pointed at that `highlight_id` with the note composer / question box focused. `locatorFromSelection` maps the existing `ReaderTextSelection` (`anchorKind`, offsets or `rectsJson`/`pageIndex`) to a `Locator`.

- [ ] **Step 3 (backend touchpoint):** Add `add_note_at_target(&self, target: HighlightTarget, body)` and `ask_at_target_streamed(...)` to `ChatService`: for `HighlightTarget::Highlight { id }`, find-or-create the thread with `highlight_id = id` (add `find_or_create_thread_for_highlight(highlight_id) -> thread_id` to the store, looking up the highlight's paper for scope); for `Document`, the existing document-scope path. Add matching Tauri commands `add_note_at_highlight` / `ask_at_highlight_streamed` and register them. Keep the legacy anchor commands until Task 10 removes their last caller.

- [ ] **Step 4:** Tests — store: `find_or_create_thread_for_highlight` is idempotent (same highlight → same thread). Run `cargo test --no-default-features --features embeddings find_or_create_thread_for_highlight`.

- [ ] **Step 5:** `pnpm check` + manual: select → swatch marks in one click; select → **Ask** creates the mark and opens the thread focused (one gesture). Leave the tree clean for review.

---

### Task 10: Click-highlight popover + highlights rail badges

**Files:**
- Create: `src/lib/features/reader/HighlightPopover.svelte`
- Modify: `src/lib/features/reader/HtmlReader.svelte`, `PdfRenderedPage.svelte` (emit `onHighlightClick(highlightId)`)
- Modify: `ReaderView.svelte` (render popover; wire recolor/remove/add-note/ask)
- Modify: the threads-rail component used by `ReaderView` (show one row per highlight with `📝`/`💬` badges)

**Interfaces:**
- Consumes: `recolorHighlight`, `removeHighlight`, the target-based note/ask (Task 9), `highlights`, `threads`.
- Produces: clicking a mark opens a popover with **Add note · Ask · Recolor · Remove**; rail rows show note/thread presence.

- [ ] **Step 1:** `HighlightPopover.svelte`: props `{ highlight, hasNote, hasThread, onAddNote, onAsk, onRecolor(color), onRemove }`. Recolor row = the same swatch set; Remove calls `removeHighlight` then reloads.

- [ ] **Step 2:** In both readers, a plain click on a mark emits `onHighlightClick(highlightId)` (HTML: map click offset → covering highlight, generalizing the current `threadAt`; PDF: click within a mark's rect). `ReaderView` opens `HighlightPopover` anchored at the click.

- [ ] **Step 3:** Rail: render one row per `highlight` (color chip + `excerpt`), with `📝` when its thread has a note entry and `💬` when it has a question/answer entry (derive from the loaded `threads` by matching `highlight_id`), plus document-scoped threads. Clicking a row scrolls to the mark and opens its popover.

- [ ] **Step 4:** `pnpm check` + manual: click a mark → popover; recolor updates the mark live; remove deletes it; rail badges reflect note/thread presence.

- [ ] **Step 5:** Leave the tree clean for review.

---

## Self-Review Notes

- **Spec coverage:** primitive+locator (T1), storage+migration (T2–T3, incl. the flagged visible-PDF-marks migration), author-agnostic service + two-front-doors seam (T4–T5, with agent author path tested), fluent UX — color choice/one-gesture note-ask/after-the-fact popover (T9–T10), multi-color rendering both readers (T7–T8), pinning-decoupled visibility + the after-ask vanishing bug (T8). Agent tooling and quote→locator resolvers are RFC 0059 (out of scope here) — the T4 service + T5 second-door seam are the hooks they attach to.
- **Migration decision** is implemented as "migrate all anchored threads to visible highlights" per the user's approval; if they later prefer "pinned PDF only," T3's `where` clause gains `and (anchor_kind = 'text_offset' or pinned-exists)`.
- **Reuse note:** `new_id()` / `now_rfc3339()` names in T2/T3 are placeholders for the crate's actual id/time helpers — grep `library_store.rs` for how threads mint `id`/`created_at` and use those exact functions.
