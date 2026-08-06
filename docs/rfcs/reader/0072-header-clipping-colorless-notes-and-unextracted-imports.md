# RFC 0072: Three reader defects — clipped title, un-attachable notes, AI-blind imports

Status: Implemented (R1–R8 landed; `cargo test --lib` 269 passed, `pnpm check`
0 errors, `pnpm build` green. R4 additionally verified by running `init()`
against a copy of the live vault: all 58 highlights preserved, column nullable,
index recreated. Behavioral checks in the Verification table — header rendering,
extract-on-import, the AI-bar branches — still need a live app run.)
Date: 2026-08-06
Product: i0i
Target: Tauri v2 + SvelteKit (Svelte 5), macOS first
Builds on: RFC 0035 (background text extraction), RFC 0058 (highlight primitive),
RFC 0061 (annotated passage + note/ask separation), RFC 0064 (AI auto-highlight),
RFC 0071 (Zotero-style reader layout).
Fixes: `docs/bug-doc/bug-logs.md` items 1–3.

## Summary

Three reported defects, three unrelated causes, all now pinned to a specific
line of code with reproducible evidence:

1. **Title formatting** — the reader header's title row is the only flex item
   that absorbs the header pane's leftover height, and it clips its own content
   with no line-boundary clamp. Long titles are cut **mid-glyph**.
2. **Can't attach a note** — the live vault database still has
   `highlights.color text NOT NULL`. RFC 0061 made the color nullable **only in
   the `create table if not exists` shape**, which never runs on an existing
   database. Every color-less passage insert — the one-gesture Note/Ask path —
   fails with a constraint error the UI swallows into "Couldn't attach the note
   to this passage."
3. **Passage marking does nothing** — `import_local_pdfs` never queues text
   extraction. A locally imported PDF has **no extracted text until the next app
   restart**, so the chat context ships an empty paper body and the model
   correctly returns zero passages. The UI shows nothing at all when the list
   is empty.

None of the three share a fix. Each section below stands alone.

## Reproduction loop

One command, ~1 second, deterministic, runs against a **copy** of the live vault
so it never mutates user data (script in the appendix; save it as
`scripts/verify-reader-bugs.sh` when implementing):

```
== Bug 2: color-less passage (note/ask one-gesture) ==
  schema:  color text NOT NULL   <- legacy (pre-RFC-0061) shape
  RED   insert with color=NULL failed: NOT NULL constraint failed: highlights.color (19)

== Bug 3: cached PDFs with no extracted text ==
  RED   these papers have an empty source_text (AI sees no paper body):
        pdf:local:eb15b46f6a8f:eb15b46f6a8f  (imported 2026-08-06 08:05:06, method local_import)

RESULT: RED (bugs reproduced)
```

Bug 1 has no headless loop; its evidence is the box-model arithmetic below plus
the reported screenshot.

---

## Bug 1 — Title is cut mid-glyph

### Symptom

The paper title renders as a single band of amber text sliced horizontally
through the letterforms; the rest of the title is invisible, and the author line
beside it is cut at the same height. The **"Hide Threads · 0" button below the
title renders in full**.

### Root cause (evidence)

`ReaderHeader` is a fixed-height column flex box
(`ReaderHeader.svelte:47-56`, `overflow: hidden`) with three children:

| child | sizing | `ReaderHeader.svelte` |
| --- | --- | --- |
| `.meta-line` (PAPER · id · chips) | `flex-shrink: 0` | :58-63 |
| `.title-line` (h1 + authors) | `flex: 1 1 auto; min-height: 0; overflow: hidden` | :65-71 |
| `.action-line` ("Hide Threads", focus mode only) | `flex-shrink: 0; margin-top: auto` | :86-94 |

The clipping element is **`.title-line`, not the pane boundary** — that is why
the action row below the cut is fully visible. The h1 wraps freely (no clamp, no
`max-height`, no ellipsis, no scroll), so whatever does not fit in the leftover
height is sliced at an arbitrary sub-pixel offset inside a line box.

The leftover height is small because the header sits in a **fixed-pixel grid
track**: `ResizableSplit` pane `{ id: "header", min: 64, max: 320, default: 116 }`
(`ReaderView.svelte:1037`), persisted per user in `localStorage` under
`i0i.reader-header-split` (`ResizableSplit.svelte:83-89`). Approximate budget at
the **default** 116px, in focus mode:

```
116  pane
 -22  header padding (12 top + 10 bottom)          ReaderHeader.svelte:54
 -23  .meta-line (18px chip + 5px margin)          theme.css:148-158
 -32  .action-line (22px .btn + 10px padding-top)  theme.css:171-174
 ----
  39  left for .title-line   ÷ 26.1px per h1 line (18px × 1.45, theme.css:64)
     = 1.5 lines
```

So a title that wraps to two lines is already cut mid-glyph **at the default
size — no drag required**. The reported screenshot is cut even tighter (partway
through line *one*), which means that user's persisted pane height is below the
116px default; the mechanism is identical, only the severity differs. Normal
(non-focus) mode has the same defect with ~71px ≈ 2.7 lines — it just needs a
three-line title.

Two aggravating details:

- `.title-line` uses `align-items: baseline`, so the authors span aligns to the
  h1's **first** baseline and lands inside the same clipped band.
- The header duplicates metadata that RFC 0071 already moved into the
  inspector's **Info** section (`ReaderInspector.svelte:630-651` renders
  `MetadataPanel` with title, authors, venue, year, tags), so nothing is lost by
  making the header line-limited.

### Change

**R1 — never render a partial line.** Clamp the h1 to two lines and carry the
full string in a tooltip:

```svelte
<h1 title={document.title}>{document.title}</h1>
```

```css
h1 {
  display: -webkit-box;
  -webkit-box-orient: vertical;
  -webkit-line-clamp: 2;
  overflow: hidden;
}
```

**R2 — give those two lines somewhere to live.** Raise the header pane's `min`
from `64` to the header's real content height, and set `default` to at least
that. From the budget above: `22` padding + `23` meta + `32` action + `2 ×
26.1` title = **`129`** — already above today's `116` default, which is why the
default clips. **Derive the final constant only after R3 lands:** moving the
authors off the h1's baseline may give the byline its own ~16px band inside
`.title-line`, pushing the figure to ~`136`. Pick the number from a measured
`getBoundingClientRect()` on the rendered header, not from this table.

`ResizableSplit.loadSizes()` pipes persisted values through `clampSizes()`
(`ResizableSplit.svelte:77, 96-105`), so raising `min` **repairs
already-persisted too-small panes on next load** — no migration, no localStorage
reset.

R1 and R2 are both required and neither is sufficient: R1 guarantees the cut
always lands on a line boundary (never mid-glyph) whatever the pane height; R2
guarantees the two clamped lines actually fit at the default and minimum sizes.

**R3 — stop the authors line from riding the title's first baseline.** Drop
`align-items: baseline` on `.title-line` in favour of `align-items: flex-end`
(or move the authors under the title), so a two-line title does not pull the
byline into the clamp.

The 128px figure must be **measured** during implementation, not trusted from
the arithmetic above; the sizes it is derived from (`.chip` 18px, `.btn` 22px)
are theme constants, but the browser's line-box rounding is not.

### Alternatives considered

- **One truncated line (`.truncate`)** — most Zotero-like, and the Info tab
  already holds the full metadata. Rejected as the default because the reader
  header is currently the *only* always-visible place the title appears, and
  research titles are long by nature. Trivial to switch (`-webkit-line-clamp: 1`)
  if two lines still feels heavy.
- **Take the header out of `ResizableSplit` entirely** (render it as a
  `flex-shrink: 0` row above the split). This kills the whole defect class,
  since the header would size to its content. Rejected as the primary change
  only because RFC 0071 deliberately made the header a resizable pane; R1+R2
  keeps that affordance while making it safe. Worth revisiting if the header
  grows more rows.
- **Let the header scroll** (`overflow-y: auto`). Rejected: a scrollbar in a
  116px chrome strip is worse than a clamp, and it still hides the title.

---

## Bug 2 — "Couldn't attach the note to this passage"

### Symptom

Select a passage, type a note, hit Save → `Couldn't attach the note to this
passage.` Picking a highlight color **first** makes the note save fine.

### Root cause (evidence)

The live vault database has the **pre-RFC-0061 schema**:

```sql
-- ~/Library/Application Support/com.i0i.app/library.sqlite
CREATE TABLE highlights (
  ...
  color text not null,   -- <-- RFC 0061 says this should be nullable
  ...
, note text);
```

```
$ sqlite3 lib-copy.sqlite "insert into highlights (... color ...) values (... NULL ...)"
Error: stepping, NOT NULL constraint failed: highlights.color (19)
```

RFC 0061 relaxed the column **in the `create table if not exists` text only**
(`library_store.rs:1911`), which is inert on a database that already has the
table. The accompanying migration added the new `note` column but never touched
the constraint:

```rust
// library_store.rs:95-98
// RFC 0061: color became nullable and a note field was added. Existing
// DBs predate the `note` column; add it if missing ...
let _ = conn.execute("alter table highlights add column note text", []);
```

SQLite cannot drop a `NOT NULL` constraint with `ALTER TABLE`, so the column is
still `NOT NULL` on every database created before commit `06237e5`.

The failure path from there:

1. `saveNote()` has no `currentHighlight`, so it calls `onEnsureHighlight()`
   (`ReaderInspector.svelte:525-532`).
2. `ensureHighlightForSelection()` calls `createHighlight({..., color: null})`
   (`ReaderView.svelte:536-541`) — RFC 0061's color-less passage.
3. `insert_highlight` binds `color_str: Option<String> = None`
   (`library_store.rs:1334`) → the constraint fires → `create_highlight` returns
   `Err`.
4. The catch block **discards the message** and returns `null`
   (`ReaderView.svelte:543-546`); `saveNote` turns that `null` into the generic
   `"Couldn't attach the note to this passage."`
   (`ReaderInspector.svelte:529-531`).

Picking a color first works because `pickColor()` inserts with a **non-null**
color (`ReaderView.svelte:494-499`), satisfying the constraint; the note then
attaches to that existing row. Agent marks are unaffected — `create_agent_highlight`
always passes `Some(color)` (`commands/highlight.rs:81`).

**Same root cause, second symptom:** `ask()` on a fresh selection also calls
`onEnsureHighlight()` and *ignores* the null return
(`ReaderInspector.svelte:460-464`), so asking about a passage silently fails to
mark it — the conversation is created without its passage row.

### Change

**R4 — rebuild the `highlights` table when the constraint is legacy.** Add an
idempotent migration step in `LibraryStore::init()`, next to the existing
RFC 0061 `alter table` (`library_store.rs:95-98`):

1. `PRAGMA table_info(highlights)` — if `color` has `notnull = 0`, return
   immediately (the common case; costs one pragma per launch).
2. Otherwise, the standard SQLite table rebuild:
   - `PRAGMA foreign_keys = off` **outside** any transaction (rusqlite cannot
     change it inside one),
   - `BEGIN`, create `highlights_new` with the canonical shape from
     `create_schema` (`library_store.rs:1901-1918`),
   - `insert into highlights_new (<explicit column list>) select <explicit
     column list> from highlights` — never `select *`: the live DB has `note`
     appended **last** (it arrived via `alter table`) while `create_schema` puts
     it between `color` and `label`, so the two disagree on physical order,
   - `drop table highlights`, `alter table highlights_new rename to highlights`,
   - **recreate `idx_highlights_paper`** (`library_store.rs:1920-1921`) — it is
     dropped with the old table,
   - `COMMIT`, then `PRAGMA foreign_keys = on`.

No table declares a foreign key **referencing** `highlights` (verified against
the live schema), so nothing cascades; the 58 existing rows are copied verbatim.
Every highlight read path (`read_highlight`, `list_highlights`,
`list_highlights_by_author`) uses an explicit column list, so the column-order
normalization is invisible to them.

**R5 — stop swallowing the error.** `ensureHighlightForSelection` should let the
backend message reach the user instead of collapsing every failure into `null`.
Either rethrow after `readerLog`, or return `{ id } | { error }`. Had this been
in place, the report would have read `NOT NULL constraint failed:
highlights.color` and this section would have been five minutes of work.

### Regression test (must be written to fail first)

`LibraryStore::for_test` + `init()` builds the **already-correct** schema, so a
naive "insert a null color succeeds" test passes today and catches nothing. The
test must:

1. Create a database and execute the **legacy** `create table highlights (...
   color text not null ...)` DDL verbatim (plus one seeded row),
2. run `store.init()`,
3. assert `insert_highlight(..., color: None, ...)` returns `Ok`,
4. assert the pre-existing row survived with its color intact,
5. run `init()` twice to prove idempotency.

This is the test that would have caught the original drift, and the only shape
of it that will. Full code in **Suggested patches → R4**.

### Alternatives considered

- **`PRAGMA writable_schema` hack** to edit the stored DDL in place — faster, no
  copy, but it is unsupported surgery on the schema and silently corrupts on
  version mismatch. Rejected.
- **Sentinel color instead of NULL** (e.g. `"none"`) — avoids the migration but
  contradicts RFC 0061's model, in which "no color" is genuinely the absence of a
  mark, and would need its own migration for the rows that already exist.
  Rejected.
- **Wipe and re-create the table** — 58 real annotations. Absolutely not.

---

## Bug 3 — AI passage marking marks nothing

### Symptom

`Highlight with AI` spins, then the AI bar **disappears with no message and no
marks**. The dev console shows:

```
[chat] auto_highlight scope=paper:local:eb15b46f6a8f categories=3
[INFO] auto_highlight: parsed 0 passage(s) from structured output
```

### Root cause (evidence)

The model returned an empty list because **it was sent no paper text**.
`import_local_pdfs` (`commands/library.rs:52-110`) copies the file, registers a
`cached` document source, and returns — it never asks `PdfExtractionManager` to
extract the text. It does not even take that state. Extraction for an imported
PDF therefore happens only on the **next app launch**, via
`recover_and_queue_startup_extractions` (`pdf_extraction.rs:197-211`), which
sweeps every cached PDF source lacking a ready extraction.

The vault's own timestamps show the two paths side by side — downloads extract
one second later, imports extract hours later in restart-shaped batches:

| acquisition_method | source created | extraction created |
| --- | --- | --- |
| direct_http | 2026-07-26 08:06:04 | 2026-07-26 08:06:05 |
| direct_http | 2026-07-26 10:11:48 | 2026-07-26 10:11:49 |
| direct_http | 2026-07-26 10:12:24 | 2026-07-26 10:12:24 |
| local_import | 2026-07-26 16:23:47 | 2026-07-27 09:00:04 |
| local_import | 2026-07-26 16:24:34 | 2026-07-27 09:00:04 |
| local_import | 2026-07-26 16:49:20 | 2026-07-27 09:00:03 |
| local_import | 2026-08-05 06:56:27 | 2026-08-05 07:53:50 |
| local_import | 2026-08-05 06:57:34 | 2026-08-05 07:53:50 |
| local_import | 2026-08-05 20:58:11 | 2026-08-06 08:04:44 |
| **local_import** | **2026-08-06 08:05:06** | **(never)** |

The reported paper was imported at `08:05:06`, **22 seconds after** the
`08:04:44` startup sweep that would have caught it (the dev build's
`target/debug/i0i.d` was touched at 10:04 local = 08:04 UTC, confirming that
launch). It has been AI-blind ever since.

The chain from there is mechanical:

1. `get_reader_document` → `get_saved_reader_document`
   (`reader_service.rs:426-429`) → the paper's source is `pdf`, so it takes the
   extraction path; with no ready extraction, `source_text` is
   `String::new()` (`reader_service.rs:565-573`). (The `abstract_text` fallback
   at `reader_service.rs:788` belongs to the *discovery candidate* builder and
   is not reachable for a saved paper.)
2. `build_context` renders `Paper text:` followed by nothing
   (`chat/context.rs:48-60`).
3. `auto_highlight`'s system prompt instructs the model to copy quotes
   **verbatim from the paper text provided** and to "return an empty list" if
   nothing matches (`chat/service.rs:381-388`). It complies:
   `{"highlights": []}`.
4. `parse_auto_highlights` reports `parsed 0 passage(s)`
   (`chat/service.rs:405-410`).

**Independent confirmation from persisted state.** Every answer on this paper
recorded a context summary of `includedChars: 0`:

```json
{"paperTitle":"A Distributional View for Visual Mechanistic Interpretability…",
 "includedChars":0,"truncated":false}
```

Five asks on `local:eb15b46f6a8f` (08:15–08:50), all zero. The same pattern
appears on `local:0c210093d254`: imported 06:57:34, asked five times between
07:04 and 07:18 with `includedChars: 0` each time, extraction finally created at
07:53:50 by the next launch. This is systematic, not a one-off — **every asked
question about a freshly imported PDF has been answered from the title alone.**

**Why the UI said nothing.** With zero intents, `runAutoHighlight` leaves
`aiMarkedCount`, `aiUnresolvedCount` and `turnHighlightIds` all empty and
`aiError` blank (`ReaderView.svelte:636-662`), so the AI bar's render condition
`aiBusy || showTurnAffordance || aiError || aiUnresolvedCount > 0`
(`ReaderView.svelte:972`) goes false and the bar vanishes. There is no "0
passages" branch. The chat side does hint at it — `chatContextLabel` renders
`Context: title + metadata only` when `includedChars === 0`
(`ReaderInspector.svelte:605-618`) — but nothing connects that to "this document
has not been extracted yet."

### Change

**R6 — extract on import (the actual fix).** Give `import_local_pdfs` the
`PdfExtractionManager` state and queue each imported source right after
`add_local_pdf_to_vault`, mirroring what the download path already does at
`pdf_ingestion.rs:334`:

```rust
store.add_local_pdf_to_vault(&paper, &vault_id, &source_id, &source_url, &pdf_path.to_string_lossy())?;
pdf_extractions.queue_source(source_id.clone(), false);
```

`queue_source` is fire-and-forget (`pdf_extraction.rs:158-176`) and
`extract_source` skips sources that already have a ready extraction
(`pdf_extraction.rs:221-229`), so re-importing the same file stays a no-op.

**R7 — safety net on reader open.** `extract_paper_document`
(`commands/reader.rs:142-155`) is a fully wired command with **zero frontend
callers** — `extractPaperDocument` in `bridge/library.ts:177` is dead code. Wire
it: when `ReaderView` loads a PDF document whose `extractionId` is null, call it
once. `ReaderView` already listens for `document_extraction_updated`
(`ReaderView.svelte:256`), so the reader refreshes itself when the text lands.
This repairs every already-imported paper without a restart, including the two
in the table above.

Keep the trigger as written — a null `extractionId` means "no **ready**
extraction", not "no extraction row": `get_saved_reader_document` only accepts
`status == "ready"` (`reader_service.rs:469-487`), so rows stuck in `failed` or
`extracting` also yield a null id and an empty `source_text`, and they need the
same re-queue. Do not "tighten" this into a check for the row's existence.
`extract_source` skips genuinely ready sources anyway
(`pdf_extraction.rs:221-229`), so the call is cheap when there is nothing to do.

**R8 — fail loudly instead of silently.** Two guards:

- Backend: `ChatService::auto_highlight` returns a distinct error when the
  document's paper body is empty — `"This document has no extracted text yet…"`
  — rather than paying for a model call that cannot succeed. **Auto-highlight
  only.** The ask path stays non-blocking: an answer from title + metadata is
  degraded but not worthless, and the inspector already labels that turn
  `Context: title + metadata only` (`ReaderInspector.svelte:605-618`).
- Frontend: add the missing branch to the AI bar so a zero-intent run reports
  `The AI found no passages to mark.` instead of dismissing itself.

### Alternatives considered

- **Extract lazily inside `get_reader_document`** — makes a read path do
  background writes and would block the reader on a 30 MB PDF. Rejected; R7 gets
  the same recovery through the existing queue.
- **Send the PDF bytes to the model when `source_text` is empty** — plausible
  later (multimodal), but it hides the real defect and changes cost/latency
  characteristics. Rejected here.
- **Shorten the startup sweep interval / poll periodically** — treats the symptom.
  The import path should simply do what the download path already does.

---

## Suggested patches

Copy-pasteable starting points, ordered by the suggested sequencing (R6/R7/R8
first — they unblock every AI feature on imported papers and repair existing
data — then the migration, then the cosmetics). Line numbers are from the tree
at the time of writing; the surrounding context in each block is the real
anchor.

These blocks are what landed, with four deviations found during implementation:

0. **R3 was reverted, and R7 needed a companion fix.**
   - `.title-line` keeps `align-items: baseline`. The byline riding the h1's
     first baseline only mattered while that band was being clipped; R1+R2
     remove the clipping, and `flex-end` would park a single-line title at the
     bottom of the ~87px row `.title-line` becomes in normal mode (132 − 22
     padding − 23 meta), leaving a gap under the `PAPER …` line. Focus mode
     would have hidden the regression — which is the mode the screenshot came
     from.
   - **`document_extraction_updated` must be filtered to `status === "ready"`**
     (`ReaderView.svelte:256`). The manager emits on `extracting` (start) as
     well as on finish, and the listener's `refreshTick` bump nulls
     `readerDocument`, unmounting `PdfPage` and re-fetching the whole file.
     Harmless while extraction only ran at startup; with R7, opening an
     unextracted 30 MB PDF would thrash the reader twice mid-read. Nothing
     renders extraction status, so dropping the non-ready refreshes loses
     nothing.

1. **`aiNotice` is also cleared in `undoAi()`**, not just `keepAi()` — otherwise
   the notice outlives an "Undo all" on a later run.
2. **The migration grows the row count on a real vault, and that is correct.**
   Running `init()` against a copy of the reported vault took `highlights` from
   58 to 62 rows. The four additions are RFC 0058's `migrate_threads_to_highlights`
   healing anchored threads that have no passage row — *precisely the four
   threads bug 2 orphaned when the user asked about a passage*. It is
   pre-existing behavior on any `init()` (it inserts with a non-null default
   color, so the legacy schema never blocked it), not a side effect of the
   rebuild. A verification that asserts `count(before) == count(after)` will
   fail; assert instead that every pre-existing row survives unchanged.

### R6 — `import_local_pdfs` queues extraction

`src-tauri/src/commands/library.rs`

```diff
 use crate::domain::library::{
     DocumentSource, LibrarySnapshot, LocalPdfImport, LocalPdfImportResult, MetadataCandidate,
     PaperDraft, PaperMetadataUpdate, VaultDraft, VaultRenameDraft,
 };
+use crate::pdf_extraction::PdfExtractionManager;
 use crate::pdf_ingestion::PdfDownloadManager;

 #[tauri::command]
 pub fn import_local_pdfs(
     app: tauri::AppHandle,
     store: tauri::State<'_, LibraryStore>,
+    pdf_extractions: tauri::State<'_, PdfExtractionManager>,
     vault_id: String,
     files: Vec<LocalPdfImport>,
 ) -> Result<LocalPdfImportResult, String> {
@@
         store.add_local_pdf_to_vault(
             &paper,
             &vault_id,
             &source_id,
             &source_url,
             &pdf_path.to_string_lossy(),
         )?;
+        // RFC 0072 R6: extract the text now, exactly as the download path
+        // already does (`pdf_ingestion.rs:334`). Without this the paper has no
+        // `source_text` — and therefore no working AI — until the next
+        // launch's startup sweep picks it up.
+        pdf_extractions.queue_source(source_id, false);
         imported_paper_ids.push(paper_id);
```

`queue_source` is fire-and-forget and dedupes in-flight sources
(`pdf_extraction.rs:158-176`), and `extract_source` returns early for a source
that already has a ready extraction (`pdf_extraction.rs:221-229`), so
re-importing the same file stays a no-op. The command stays synchronous —
`extract_paper_document` already spawns from a sync command the same way
(`commands/reader.rs:142-155`).

### R7 — reader-open safety net

`src/lib/features/reader/ReaderView.svelte`

```diff
   import {
     cancelDiscoveryPdfAcquisition,
+    extractPaperDocument,
     getDiscoveryReaderDocument,
     getReaderDocument,
     openHtmlDocument,
   } from "$lib/bridge/library";
@@
   let refreshTick = $state(0);
   let documentLoadSequence = 0;
+  // RFC 0072 R7: papers this reader session has already nudged into
+  // extraction. `document_extraction_updated` bumps `refreshTick`, so an
+  // unguarded retry would loop forever on an extraction that keeps failing.
+  const extractionRequested = new Set<string>();
@@
         if (paper.id === paperId) {
           readerDocument = doc;
+          requestExtractionIfMissing(doc, paperId);
         }
```

```ts
  // A cached PDF with no *ready* extraction has an empty `source_text`, so chat
  // and AI marking silently do nothing (RFC 0072 bug 3). Queue the extraction
  // the import path missed; the `document_extraction_updated` listener above
  // refreshes the reader when the text lands.
  //
  // The trigger is `!doc.extractionId`, which means "no READY extraction" —
  // `get_saved_reader_document` only accepts `status == "ready"`
  // (`reader_service.rs:469-487`), so rows stuck in `failed`/`extracting` also
  // yield a null id and also need re-queueing. Do not "tighten" this into a
  // check for whether an extraction row exists.
  function requestExtractionIfMissing(doc: ReaderDocument, paperId: string) {
    if (activeCandidate || doc.contentKind !== "pdf" || doc.extractionId || !doc.pdfLocalPath) {
      return;
    }
    if (extractionRequested.has(paperId)) {
      return;
    }
    extractionRequested.add(paperId);
    void extractPaperDocument(paperId).catch((error) => {
      readerLog("extract-request-error", { paperId, error: errorDetail(error) }, "error");
    });
  }
```

`extractPaperDocument` (`bridge/library.ts:177`) and its command already exist
and have no other callers — this wires up dead code rather than adding surface.

### R8 — fail loudly instead of silently

**Backend.** `PreparedAsk` already carries the context summary, so the guard
needs no extra document fetch:

`src-tauri/src/services/chat/service.rs`, in `auto_highlight` (after the
`prepare_ask_at_anchor` call, ~line 375):

```diff
         let prep = self
             .prepare_ask_at_anchor(scope, &ThreadAnchor::Document, body)
             .await?;
+        // RFC 0072 R8: an unextracted PDF sends an empty paper body, so the
+        // model can only ever return an empty list. Say why, instead of paying
+        // for a call that cannot succeed.
+        if prep.summary.included_chars == 0 {
+            return Err(
+                "This document has no extracted text yet — marking can't run until \
+                 extraction finishes."
+                    .to_string(),
+            );
+        }
```

Deliberately **not** applied to `prepare_ask`/`prepare_ask_at_anchor`
themselves: an ask with no body text still returns something useful about the
title/metadata, and the inspector already labels that turn
`Context: title + metadata only` (`ReaderInspector.svelte:605-618`). Marking, by
contrast, is guaranteed to produce nothing.

**Frontend.** A zero-intent run currently leaves every AI-bar flag false, so the
bar unmounts and the user sees nothing at all:

`src/lib/features/reader/ReaderView.svelte`

```diff
   let aiError = $state("");
+  // RFC 0072 R8: a run that completed but marked nothing. Distinct from
+  // `aiError` (the run failed) and from `aiUnresolvedCount` (passages came
+  // back but couldn't be located in the rendered document).
+  let aiNotice = $state("");
@@
     aiBusy = true;
     aiError = "";
+    aiNotice = "";
     aiMarkedCount = 0;
     aiUnresolvedCount = 0;
     handleAskTurnStart();
     try {
       const intents = await autoHighlight({ kind: "paper", paperId: paper.id }, categories);
+      if (intents.length === 0) {
+        aiNotice = "The AI found no passages to mark in this document.";
+      }
       for (const intent of intents) {
@@
   function keepAi() {
     aiError = "";
+    aiNotice = "";
     aiUnresolvedCount = 0;
     keepTurnHighlights();
   }
```

```diff
-        {#if aiBusy || showTurnAffordance || aiError || aiUnresolvedCount > 0}
+        {#if aiBusy || showTurnAffordance || aiError || aiNotice || aiUnresolvedCount > 0}
@@
               <button class="ai-link" type="button" onclick={() => void undoAi()}>Undo all</button>
+            {:else if aiNotice}
+              <span>{aiNotice}</span>
+              <div class="flex1"></div>
+              <button class="ai-link" type="button" onclick={keepAi}>Dismiss</button>
             {:else}
               <span class="ai-error">Couldn't locate {aiUnresolvedCount} passage{aiUnresolvedCount === 1 ? "" : "s"} in this document.</span>
```

The new branch must sit **before** the final `{:else}` — otherwise a zero-intent
run falls through to it and renders "Couldn't locate 0 passages".

### R4 — rebuild `highlights` when the constraint is legacy

`src-tauri/src/storage/library_store.rs`, in `init()` immediately after the
existing RFC 0061 `alter table` (so the `note` column exists before the copy):

```diff
         let _ = conn.execute("alter table highlights add column note text", []);
+        // RFC 0072 R4: and relax the NOT NULL the same RFC left behind on
+        // every pre-existing vault.
+        relax_highlight_color_not_null(&conn)?;

         self.migrate_threads_to_highlights()?;
```

```rust
/// RFC 0061 made `highlights.color` nullable in `create_schema` only — which is
/// inert on a database that already has the table — so every vault created
/// before it still rejects the color-less passages Note/Ask create (RFC 0072
/// bug 2). SQLite cannot drop a NOT NULL constraint, so rebuild the table.
///
/// Idempotent and cheap: one `pragma table_info` per launch, and a no-op once
/// the column is nullable.
fn relax_highlight_color_not_null(conn: &Connection) -> StoreResult<()> {
    if !column_is_not_null(conn, "highlights", "color")? {
        return Ok(());
    }

    // Foreign keys can only be toggled outside a transaction. Nothing declares
    // a foreign key referencing `highlights`, but the rebuild drops a table, so
    // follow SQLite's documented procedure rather than relying on that.
    conn.execute_batch("pragma foreign_keys = off;")
        .map_err(|error| error.to_string())?;

    // Explicit column lists on BOTH halves of the insert: the live table has
    // `note` appended last (it arrived via `alter table`), while the canonical
    // shape carries it between `color` and `label`. `select *` would silently
    // shift every column after `color`.
    let rebuild = conn.execute_batch(
        "
        begin;
        create table highlights_new (
          id text primary key,
          paper_id text not null,
          source_id text not null,
          locator_kind text not null,
          start_offset integer,
          end_offset integer,
          page_index integer,
          rects_json text,
          excerpt text not null,
          color text,
          note text,
          label text,
          author_kind text not null,
          author_model text,
          created_at text not null,
          updated_at text not null
        );
        insert into highlights_new (
          id, paper_id, source_id, locator_kind, start_offset, end_offset,
          page_index, rects_json, excerpt, color, note, label, author_kind,
          author_model, created_at, updated_at
        )
        select
          id, paper_id, source_id, locator_kind, start_offset, end_offset,
          page_index, rects_json, excerpt, color, note, label, author_kind,
          author_model, created_at, updated_at
        from highlights;
        drop table highlights;
        alter table highlights_new rename to highlights;
        create index if not exists idx_highlights_paper
          on highlights(paper_id, created_at);
        commit;
        ",
    );

    if rebuild.is_err() {
        // `execute_batch` stops at the failing statement, leaving the
        // transaction open; close it explicitly so the pragma restore and any
        // later work see a clean connection.
        let _ = conn.execute_batch("rollback;");
    }
    conn.execute_batch("pragma foreign_keys = on;")
        .map_err(|error| error.to_string())?;
    rebuild.map_err(|error| error.to_string())
}

/// Whether `table.column` is declared NOT NULL. Companion to `column_exists`
/// (`pragma table_info` columns: 0 cid, 1 name, 2 type, 3 notnull).
fn column_is_not_null(conn: &Connection, table: &str, column: &str) -> StoreResult<bool> {
    let mut stmt = conn
        .prepare(&format!("pragma table_info({table})"))
        .map_err(|error| error.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            Ok((row.get::<_, String>(1)?, row.get::<_, i64>(3)?))
        })
        .map_err(|error| error.to_string())?;

    for row in rows {
        let (name, not_null) = row.map_err(|error| error.to_string())?;
        if name == column {
            return Ok(not_null != 0);
        }
    }

    Ok(false)
}
```

The `idx_highlights_paper` recreation is not optional — the index dies with the
dropped table, and `list_highlights` orders every reader repaint by
`(paper_id, created_at)`.

**The regression test** (in `library_store.rs`'s test module). `test_db()` calls
`init()` on a *fresh* database, whose schema is already correct — a test built on
it passes today and proves nothing. The legacy DDL has to be materialized first:

```rust
#[test]
fn init_relaxes_legacy_not_null_color() -> StoreResult<()> {
    use crate::domain::highlight::{HighlightAuthor, HighlightColor, Locator};

    // A pre-RFC-0061 vault, byte for byte: `color text not null`, no `note`
    // column. `create table if not exists` in create_schema() is inert once the
    // table exists, which is exactly why the bug survived.
    let dir = std::env::temp_dir().join(format!("i0i-legacy-color-{}", std::process::id()));
    fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    let path = dir.join("library.sqlite");
    {
        let conn = Connection::open(&path).map_err(|error| error.to_string())?;
        conn.execute_batch(
            "
            create table highlights (
              id text primary key, paper_id text not null, source_id text not null,
              locator_kind text not null, start_offset integer, end_offset integer,
              page_index integer, rects_json text, excerpt text not null,
              color text not null, label text, author_kind text not null,
              author_model text, created_at text not null, updated_at text not null
            );
            insert into highlights values (
              'hl-legacy', 'p', 's', 'pdf_rect', null, null, 0, '[]', 'kept',
              'yellow', null, 'user', null, datetime('now'), datetime('now')
            );
            ",
        )
        .map_err(|error| error.to_string())?;
    }

    let store = LibraryStore::for_test(path);
    store.init()?;
    store.init()?; // idempotent — the second launch must be a no-op

    // The color-less passage RFC 0061 promised and bug 2 could not create.
    let created = store.insert_highlight(
        "p",
        &Locator::PdfRect {
            source_id: "s".into(),
            page_index: 0,
            rects_json: "[]".into(),
        },
        "note-only passage",
        None,
        None,
        &HighlightAuthor::User,
    )?;
    assert!(created.color.is_none());

    // And the rebuild kept the existing annotation, color intact.
    let all = store.list_highlights("p")?;
    let legacy = all.iter().find(|h| h.id == "hl-legacy").expect("legacy row survived");
    assert_eq!(legacy.color, Some(HighlightColor::Yellow));
    assert_eq!(legacy.excerpt, "kept");

    fs::remove_dir_all(&dir).ok();
    Ok(())
}
```

### R5 — surface the real failure

`src/lib/features/reader/ReaderView.svelte`

```diff
     } catch (error) {
       readerLog("create-highlight-error", { error: errorDetail(error) }, "error");
-      return null;
+      // RFC 0072 R5: `null` now means only "there was nothing to mark". A
+      // genuine backend failure carries its message to the caller, which
+      // decides whether it is fatal.
+      throw error;
     } finally {
       highlightActionInFlight = false;
     }
```

`src/lib/features/reader/ReaderInspector.svelte` — `saveNote` needs no change:
the throw lands in its existing `catch`, which already does
`error = String(caught)`, so the user finally sees `NOT NULL constraint failed:
highlights.color` instead of the generic line. The generic message stays for the
real `null` case (no selection).

`ask()` must **not** become fatal — the answer still has value even if the
passage row could not be created:

```diff
+  // Marking the passage is a bonus on the ask path; failing it must not lose
+  // the user's question (RFC 0072 R5).
+  let passageWarning = $state("");
@@
-  const visibleError = $derived(error || chatError);
+  const visibleError = $derived(error || passageWarning || chatError);
@@
     isBusy = true;
     error = "";
+    passageWarning = "";
@@
       if (virtual) {
-        await onEnsureHighlight();
+        try {
+          await onEnsureHighlight();
+        } catch (caught) {
+          passageWarning = `Couldn't mark this passage: ${String(caught)}`;
+        }
       }
```

`saveNote` should clear `passageWarning` alongside `error` for the same reason.
Folding it into `visibleError` means it renders with `.note-error`
(`ReaderInspector.svelte:1099-1107`) even though it is non-fatal — acceptable to
ship, but a `.note-warning` variant is worth adding if that CSS is touched
anyway.

### R1–R3 — header title

`src/lib/features/reader/ReaderHeader.svelte`

```diff
   <div class="row title-line">
-    <h1>{document.title}</h1>
+    <h1 title={document.title}>{document.title}</h1>
     <span class="authors truncate">{document.authors.join(" / ")}</span>
   </div>
```

```diff
   .title-line {
     flex: 1 1 auto;
     min-height: 0;
     overflow: hidden;
     gap: 12px;
-    align-items: baseline;
+    /* R3: `baseline` aligns the byline to the h1's FIRST line, dropping it
+       into the clipped band of a wrapped title. */
+    align-items: flex-end;
   }

   h1 {
     margin: 0;
+    /* R1: clamp to whole lines so a too-short header can never slice a line
+       box mid-glyph. The full string stays reachable via the title tooltip
+       and the inspector's Info section. */
+    display: -webkit-box;
+    -webkit-box-orient: vertical;
+    -webkit-line-clamp: 2;
+    line-clamp: 2;
+    min-width: 0;
+    overflow: hidden;
     color: var(--amber);
     font-size: 18px;
     font-weight: 600;
   }
```

`src/lib/features/reader/ReaderView.svelte`

```diff
             panes={[
-              { id: "header", min: 64, max: 320, default: 116 },
+              // R2: min/default must fit meta + TWO clamped title lines +
+              // (focus mode) the action row. Measure the rendered header after
+              // R3 lands and set this from that number — 132 is the arithmetic
+              // estimate, not a measurement.
+              { id: "header", min: 132, max: 320, default: 132 },
               { id: "content", min: 240, default: 620 },
             ]}
```

Raising `min` is also the repair path for users whose persisted pane is already
too small: `loadSizes()` runs stored values through `clampSizes()`
(`ResizableSplit.svelte:77, 96-105`), so the next reader open lifts them to the
new floor. No localStorage reset, no migration.

## Verification

| # | Change | How it is verified |
| --- | --- | --- |
| R1–R3 | header clamp + pane min | manual: open a paper with a 3-line title in normal *and* focus mode; no partial line at any pane size down to `min`; drag-persisted small panes snap back on reload |
| R4 | schema rebuild | the legacy-schema regression test above; `verify-reader-bugs.sh` flips to GREEN on a copy of the live DB |
| R5 | error propagation | force a failure (temporarily re-add the constraint) and confirm the real message reaches the inspector |
| R6 | extract on import | import a PDF, then `verify-reader-bugs.sh` reports no cached PDF without a ready extraction — **without restarting the app** |
| R7 | reader-open safety net | open `local:eb15b46f6a8f` on the current vault; extraction runs, `includedChars` on the next ask is non-zero |
| R8 backend | guard on empty body | run Highlight with AI on a paper with no extraction; the run fails fast with the extraction message via the existing `aiError` branch, and no model call is made |
| R8 frontend | zero-intent branch | run Highlight with AI on an **extracted** paper with a category nothing matches; the bar reads "found no passages to mark" instead of unmounting. This is the only path that reaches `aiNotice` — without this case the branch ships unexercised |

Suggested sequencing: **R6 + R7 first** (they unblock every AI feature on
imported papers and repair existing data), then **R4 + R5** (one migration, one
test), then **R1–R3** (cosmetic, no data risk).

### Open question for the live run

Nothing here proves this particular 29.7 MB PDF *can* be extracted — only that
extraction was never attempted on it. `DEFAULT_TIMEOUT_MS` is 120 s and
`DEFAULT_PAGE_CAP` is 200 pages (`pdf_extraction.rs:22-24`). The discriminating
line is in the dev server's stderr on first launch:

```
[extraction …] ready  source_id=pdf:local:eb15b46f6a8f:… pages=N blocks=N
[extraction …] failed source_id=… error=PDF extraction timed out after 120000 ms
```

If it times out, the diagnosis and the fixes still stand — the mechanism is
repaired — but that one paper stays AI-blind and needs a separate follow-up
(raise the timeout, or extract incrementally). Watch for it before concluding
R6/R7 worked.

## Out of scope

The remaining `bug-logs.md` items — marks list placed below the note input, Ask
in PDF opening the Note pane, scroll slowness with marks, and the collapsible
index sidebar — are untouched here. The slowness item in particular needs its own
measurement pass, not a code read.

## Appendix — verification script

```bash
#!/bin/bash
# Feedback loop for bug-logs.md items 2 and 3. Runs against a COPY of the live
# vault DB, so it never mutates user data.  RED = bug present, GREEN = fixed.
set -u
LIVE="$HOME/Library/Application Support/com.i0i.app/library.sqlite"
WORK="$(dirname "$0")/lib-probe.sqlite"
cp "$LIVE" "$WORK"
fail=0

echo "== Bug 2: color-less passage (note/ask one-gesture) =="
sqlite3 "$WORK" "select sql from sqlite_master where name='highlights';" \
  | grep -q "color text not null" \
  && echo "  schema:  color text NOT NULL   <- legacy (pre-RFC-0061) shape" \
  || echo "  schema:  color nullable"
err=$(sqlite3 "$WORK" "insert into highlights (id,paper_id,source_id,locator_kind,page_index,rects_json,excerpt,color,label,author_kind,author_model,created_at,updated_at) values ('hl-probe','p','s','pdf_rect',0,'[]','probe',NULL,NULL,'user',NULL,datetime('now'),datetime('now'));" 2>&1)
if [ -n "$err" ]; then echo "  RED   insert with color=NULL failed: $err"; fail=1
else echo "  GREEN insert with color=NULL succeeded"; fi

echo
echo "== Bug 3: cached PDFs with no extracted text =="
missing=$(sqlite3 "$WORK" "
  select s.id || '  (imported ' || s.created_at || ', method ' || coalesce(s.acquisition_method,'?') || ')'
  from document_sources s
  where s.source_kind='pdf' and s.status='cached'
    and not exists (select 1 from document_extractions e
                    where e.source_id=s.id and e.status='ready');")
if [ -n "$missing" ]; then
  echo "  RED   these papers have an empty source_text (AI sees no paper body):"
  echo "$missing" | sed 's/^/        /'; fail=1
else echo "  GREEN every cached PDF has a ready extraction"; fi

echo
echo "  extraction latency by acquisition method (import path vs download path):"
sqlite3 -header -column "$WORK" "
  select s.acquisition_method, s.created_at as source_created,
         coalesce(e.created_at,'(never)') as extraction_created
  from document_sources s
  left join document_extractions e on e.source_id=s.id
  where s.source_kind='pdf' order by s.created_at;" | sed 's/^/        /'

rm -f "$WORK"
echo
[ "$fail" -eq 0 ] && echo "RESULT: GREEN" || echo "RESULT: RED (bugs reproduced)"
exit "$fail"
```

To check a single paper's chat context (the `includedChars: 0` evidence):

```sql
select t.scope_id, e.created_at, e.context_json
from chat_entries e join chat_threads t on e.thread_id = t.id
where e.context_json <> '' order by e.created_at;
```
