use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{params, Connection, OptionalExtension};
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Manager};

use crate::domain::chat::{
    ChatContextSummary, ChatEntry, ChatEntryDraft, ChatThread, ChatThreadSummary, ChatThreadView,
    PinnedHighlight, ThreadAnchor, ENTRY_ANSWER, ENTRY_QUESTION,
};
use crate::domain::library::{
    DocumentAsset, DocumentBlock, DocumentExtraction, DocumentPage, DocumentSource, DocumentSpan,
    LibrarySnapshot, Paper, PaperDraft, PaperNote, PaperNoteDraft, PaperSourceDraft, Vault,
    VaultDraft, VaultPaper, VaultRenameDraft,
};

type StoreResult<T> = Result<T, String>;

#[derive(Clone)]
pub struct LibraryStore {
    db_path: PathBuf,
}

#[derive(Clone)]
struct SeedVault {
    id: &'static str,
    title: &'static str,
    path: &'static str,
}

#[derive(Clone)]
struct SeedPaper {
    id: &'static str,
    title: &'static str,
    authors: &'static [&'static str],
    venue: &'static str,
    year: i32,
    citations: i32,
    tags: &'static [&'static str],
    note_count: i32,
    annotation_count: i32,
    status: &'static str,
    abstract_text: Option<&'static str>,
}

impl LibraryStore {
    pub fn new(app: &AppHandle) -> StoreResult<Self> {
        let app_data_dir = app
            .path()
            .app_data_dir()
            .map_err(|error| error.to_string())?;
        fs::create_dir_all(&app_data_dir).map_err(|error| error.to_string())?;

        Ok(Self {
            db_path: app_data_dir.join("library.sqlite"),
        })
    }

    #[cfg(test)]
    fn for_test(db_path: PathBuf) -> Self {
        Self { db_path }
    }

    pub fn init(&self) -> StoreResult<()> {
        let mut conn = self.open_connection()?;
        self.create_schema(&conn)?;
        migrate_chat_messages_to_threads(&mut conn)?;

        if self.is_library_empty(&conn)? {
            self.seed_defaults(&mut conn)?;
        }

        Ok(())
    }

    pub fn get_library(&self) -> StoreResult<LibrarySnapshot> {
        let conn = self.open_connection()?;
        self.read_library(&conn)
    }

    pub fn get_document_sources(&self, paper_id: &str) -> StoreResult<Vec<DocumentSource>> {
        let conn = self.open_connection()?;
        read_document_sources_for_paper(&conn, paper_id)
    }

    pub fn get_document_source(&self, source_id: &str) -> StoreResult<DocumentSource> {
        let conn = self.open_connection()?;
        read_document_source(&conn, source_id)
    }

    pub fn remote_available_pdf_sources(&self) -> StoreResult<Vec<DocumentSource>> {
        let conn = self.open_connection()?;
        read_document_sources_by_status(&conn, "remote_available")
    }

    pub fn stale_downloading_pdf_sources(&self) -> StoreResult<Vec<DocumentSource>> {
        let conn = self.open_connection()?;
        read_document_sources_by_status(&conn, "downloading")
    }

    pub fn set_document_source_downloading(&self, source_id: &str) -> StoreResult<DocumentSource> {
        let conn = self.open_connection()?;
        conn.execute(
            "
            update document_sources
            set status = 'downloading', error = null, updated_at = datetime('now')
            where id = ?1
            ",
            params![source_id],
        )
        .map_err(|error| error.to_string())?;
        read_document_source(&conn, source_id)
    }

    pub fn set_document_source_cached(
        &self,
        source_id: &str,
        local_path: &str,
    ) -> StoreResult<DocumentSource> {
        let mut conn = self.open_connection()?;
        let tx = conn.transaction().map_err(|error| error.to_string())?;
        tx.execute(
            "
            update document_sources
            set status = 'cached', local_path = ?2, error = null, updated_at = datetime('now')
            where id = ?1
            ",
            params![source_id, local_path],
        )
        .map_err(|error| error.to_string())?;
        tx.execute(
            "
            update papers
            set active_source_id = coalesce(active_source_id, ?1),
                updated_at = datetime('now')
            where id = (
              select paper_id from document_sources where id = ?1
            )
            ",
            params![source_id],
        )
        .map_err(|error| error.to_string())?;
        tx.commit().map_err(|error| error.to_string())?;
        let conn = self.open_connection()?;
        read_document_source(&conn, source_id)
    }

    pub fn set_document_source_failed(
        &self,
        source_id: &str,
        error: &str,
    ) -> StoreResult<DocumentSource> {
        let conn = self.open_connection()?;
        let updated = conn
            .execute(
                "
            update document_sources
            set status = 'failed', error = ?2, updated_at = datetime('now')
            where id = ?1
            ",
                params![source_id, error],
            )
            .map_err(|error| error.to_string())?;
        if updated == 0 {
            return Err(format!(
                "Document source disappeared before marking download failed: {source_id}"
            ));
        }
        read_document_source(&conn, source_id)
    }

    pub fn reset_document_source_to_remote_available(
        &self,
        source_id: &str,
    ) -> StoreResult<DocumentSource> {
        let conn = self.open_connection()?;
        conn.execute(
            "
            update document_sources
            set status = 'remote_available', error = null, updated_at = datetime('now')
            where id = ?1
            ",
            params![source_id],
        )
        .map_err(|error| error.to_string())?;
        read_document_source(&conn, source_id)
    }

    pub fn add_paper_to_vaults(
        &self,
        paper: &PaperDraft,
        vault_ids: &[String],
    ) -> StoreResult<LibrarySnapshot> {
        let mut conn = self.open_connection()?;
        let tx = conn.transaction().map_err(|error| error.to_string())?;
        let authors_json = to_json(&paper.authors)?;
        let tags_json = to_json(&paper.tags)?;

        tx.execute(
            "
            insert into papers (
              id, title, authors_json, venue, year, citations, tags_json,
              note_count, annotation_count, status, abstract, created_at, updated_at
            )
            values (?1, ?2, ?3, ?4, ?5, ?6, ?7, 0, 0, ?8, ?9, datetime('now'), datetime('now'))
            on conflict(id) do update set
              title = excluded.title,
              authors_json = excluded.authors_json,
              venue = excluded.venue,
              year = excluded.year,
              citations = excluded.citations,
              tags_json = excluded.tags_json,
              status = excluded.status,
              abstract = excluded.abstract,
              updated_at = datetime('now')
            ",
            params![
                paper.id,
                paper.title,
                authors_json,
                paper.venue,
                paper.year,
                paper.citations,
                tags_json,
                paper.status,
                paper.abstract_text,
            ],
        )
        .map_err(|error| error.to_string())?;

        upsert_document_sources(&tx, paper)?;

        for vault_id in vault_ids {
            tx.execute(
                "
                insert into vault_papers (vault_id, paper_id, added_at)
                values (?1, ?2, datetime('now'))
                on conflict(vault_id, paper_id) do nothing
                ",
                params![vault_id, paper.id],
            )
            .map_err(|error| error.to_string())?;
        }

        tx.commit().map_err(|error| error.to_string())?;
        self.get_library()
    }

    pub fn create_vault(&self, draft: &VaultDraft) -> StoreResult<LibrarySnapshot> {
        let normalized = normalize_vault_path(&draft.path)?;
        let title = vault_title_from_path(&normalized)?;
        let id = vault_id_from_path(&normalized)?;
        let conn = self.open_connection()?;

        conn.execute(
            "
            insert into vaults (id, title, path, created_at, updated_at)
            values (?1, ?2, ?3, datetime('now'), datetime('now'))
            ",
            params![id, title, normalized],
        )
        .map_err(|error| {
            let message = error.to_string();
            if message.contains("UNIQUE constraint failed: vaults.path") {
                format!("Vault path already exists: {normalized}")
            } else if message.contains("UNIQUE constraint failed: vaults.id") {
                format!("Vault id already exists: {id}")
            } else {
                message
            }
        })?;

        self.get_library()
    }

    pub fn rename_vault(&self, draft: &VaultRenameDraft) -> StoreResult<LibrarySnapshot> {
        let normalized = normalize_vault_path(&draft.path)?;
        let title = vault_title_from_path(&normalized)?;
        let conn = self.open_connection()?;
        let updated = conn
            .execute(
                "
                update vaults
                set title = ?1, path = ?2, updated_at = datetime('now')
                where id = ?3
                ",
                params![title, normalized, draft.id],
            )
            .map_err(|error| {
                let message = error.to_string();
                if message.contains("UNIQUE constraint failed: vaults.path") {
                    format!("Vault path already exists: {normalized}")
                } else {
                    message
                }
            })?;

        if updated == 0 {
            return Err(format!("Vault not found: {}", draft.id));
        }

        self.get_library()
    }

    pub fn delete_vault(&self, vault_id: &str) -> StoreResult<LibrarySnapshot> {
        if vault_id.trim().is_empty() {
            return Err("Vault id cannot be empty".to_string());
        }

        let mut conn = self.open_connection()?;
        let tx = conn.transaction().map_err(|error| error.to_string())?;
        let deleted = tx
            .execute("delete from vaults where id = ?1", params![vault_id])
            .map_err(|error| error.to_string())?;

        if deleted == 0 {
            return Err(format!("Vault not found: {vault_id}"));
        }

        // Deleting a Vault cascades its membership rows. Papers are shared
        // entities, so remove only the ones that lost their final membership.
        tx.execute(
            "
            delete from papers
            where not exists (
              select 1 from vault_papers
              where vault_papers.paper_id = papers.id
            )
            ",
            [],
        )
        .map_err(|error| error.to_string())?;

        tx.commit().map_err(|error| error.to_string())?;
        self.get_library()
    }

    pub fn remove_paper_from_vault(
        &self,
        vault_id: &str,
        paper_id: &str,
    ) -> StoreResult<LibrarySnapshot> {
        if vault_id.trim().is_empty() {
            return Err("Vault id cannot be empty".to_string());
        }

        if paper_id.trim().is_empty() {
            return Err("Paper id cannot be empty".to_string());
        }

        let mut conn = self.open_connection()?;
        let tx = conn.transaction().map_err(|error| error.to_string())?;
        let deleted = tx
            .execute(
                "delete from vault_papers where vault_id = ?1 and paper_id = ?2",
                params![vault_id, paper_id],
            )
            .map_err(|error| error.to_string())?;

        if deleted == 0 {
            return Err(format!(
                "Paper {paper_id} is not linked to Vault {vault_id}"
            ));
        }

        let remaining_memberships: i64 = tx
            .query_row(
                "select count(*) from vault_papers where paper_id = ?1",
                params![paper_id],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;

        if remaining_memberships == 0 {
            tx.execute("delete from papers where id = ?1", params![paper_id])
                .map_err(|error| error.to_string())?;
        }

        tx.commit().map_err(|error| error.to_string())?;
        self.get_library()
    }

    pub fn delete_paper_globally(&self, paper_id: &str) -> StoreResult<LibrarySnapshot> {
        if paper_id.trim().is_empty() {
            return Err("Paper id cannot be empty".to_string());
        }

        let mut conn = self.open_connection()?;
        let tx = conn.transaction().map_err(|error| error.to_string())?;
        let deleted = tx
            .execute("delete from papers where id = ?1", params![paper_id])
            .map_err(|error| error.to_string())?;

        if deleted == 0 {
            return Err(format!("Paper not found: {paper_id}"));
        }

        // chat_threads has no FK to papers (scope_id is polymorphic), so its
        // paper-scoped rows are cleaned up explicitly alongside the paper; their
        // entries cascade via the chat_entries FK.
        tx.execute(
            "delete from chat_threads where scope_kind = 'paper' and scope_id = ?1",
            params![paper_id],
        )
        .map_err(|error| error.to_string())?;

        tx.commit().map_err(|error| error.to_string())?;
        self.get_library()
    }

    pub fn get_paper_notes(&self, paper_id: &str) -> StoreResult<Vec<PaperNote>> {
        if paper_id.trim().is_empty() {
            return Err("Paper id cannot be empty".to_string());
        }

        let conn = self.open_connection()?;
        read_paper_notes(&conn, paper_id)
    }

    pub fn create_paper_note(&self, draft: &PaperNoteDraft) -> StoreResult<Vec<PaperNote>> {
        let body = draft.body.trim();
        let anchor_kind = draft.anchor_kind.as_deref().unwrap_or("text_offset").trim();

        if draft.paper_id.trim().is_empty() {
            return Err("Paper id cannot be empty".to_string());
        }

        if draft.source_id.trim().is_empty() {
            return Err("Source id cannot be empty".to_string());
        }

        if body.is_empty() {
            return Err("Note body cannot be empty".to_string());
        }

        match anchor_kind {
            "text_offset" => {
                if draft.selected_text.trim().is_empty() {
                    return Err("Selected text cannot be empty".to_string());
                }

                if draft.start_offset < 0 || draft.end_offset <= draft.start_offset {
                    return Err("Note offsets must define a non-empty range".to_string());
                }
            }
            "pdf_rect" => {
                if draft.page_index.is_none() {
                    return Err("PDF note page index is required".to_string());
                }

                let rects_json = draft
                    .rects_json
                    .as_deref()
                    .ok_or_else(|| "PDF note rectangles are required".to_string())?;
                validate_note_rects_json(rects_json)?;
            }
            "chat" => {
                // Chat-born notes have no document anchor: no selected text,
                // offsets, page, or rects. The body (validated above) is the
                // saved answer; quote_context carries the originating question.
            }
            _ => {
                return Err(format!("Unsupported note anchor kind: {anchor_kind}"));
            }
        }

        let note_id = generate_note_id()?;
        let mut conn = self.open_connection()?;
        let tx = conn.transaction().map_err(|error| error.to_string())?;

        tx.execute(
            "
            insert into paper_notes (
              id, paper_id, source_id, start_offset, end_offset,
              selected_text, anchor_kind, page_index, rects_json, quote_context,
              body, created_at, updated_at
            )
            values (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, datetime('now'), datetime('now'))
            ",
            params![
                note_id,
                draft.paper_id,
                draft.source_id,
                draft.start_offset,
                draft.end_offset,
                draft.selected_text,
                anchor_kind,
                draft.page_index,
                draft.rects_json.as_deref(),
                draft.quote_context.as_deref(),
                body,
            ],
        )
        .map_err(|error| error.to_string())?;

        tx.execute(
            "
            update papers
            set note_count = note_count + 1,
                updated_at = datetime('now')
            where id = ?1
            ",
            params![draft.paper_id],
        )
        .map_err(|error| error.to_string())?;

        tx.commit().map_err(|error| error.to_string())?;
        self.get_paper_notes(&draft.paper_id)
    }

    pub fn delete_paper_note(&self, note_id: &str) -> StoreResult<()> {
        if note_id.trim().is_empty() {
            return Err("Note id cannot be empty".to_string());
        }

        let mut conn = self.open_connection()?;
        let tx = conn.transaction().map_err(|error| error.to_string())?;

        tx.execute(
            "
            update papers
            set note_count = max(note_count - 1, 0),
                updated_at = datetime('now')
            where id = (
              select paper_id from paper_notes where id = ?1
            )
            ",
            params![note_id],
        )
        .map_err(|error| error.to_string())?;

        tx.execute("delete from paper_notes where id = ?1", params![note_id])
            .map_err(|error| error.to_string())?;

        tx.commit().map_err(|error| error.to_string())
    }

    pub fn update_paper_note(&self, note_id: &str, body: &str) -> StoreResult<()> {
        let body = body.trim();

        if note_id.trim().is_empty() {
            return Err("Note id cannot be empty".to_string());
        }

        if body.is_empty() {
            return Err("Note body cannot be empty".to_string());
        }

        let conn = self.open_connection()?;
        let updated = conn
            .execute(
                "
                update paper_notes
                set body = ?1,
                    updated_at = datetime('now')
                where id = ?2
                ",
                params![body, note_id],
            )
            .map_err(|error| error.to_string())?;

        if updated == 0 {
            return Err(format!("Note not found: {note_id}"));
        }

        Ok(())
    }

    /// List a scope's threads, newest-activity first, with entry/pin counts.
    pub fn list_chat_threads(
        &self,
        scope_kind: &str,
        scope_id: &str,
    ) -> StoreResult<Vec<ChatThreadSummary>> {
        let conn = self.open_connection()?;
        let mut stmt = conn
            .prepare(
                "
                select t.id, t.anchor_kind, t.source_id, t.start_offset, t.end_offset,
                       t.selected_text, t.page_index, t.rects_json, t.title, t.updated_at,
                       (select count(*) from chat_entries e where e.thread_id = t.id),
                       (select count(*) from chat_entries e where e.thread_id = t.id and e.pinned = 1)
                from chat_threads t
                where t.scope_kind = ?1 and t.scope_id = ?2
                order by t.updated_at desc, t.id desc
                ",
            )
            .map_err(|error| error.to_string())?;

        let rows = stmt
            .query_map(params![scope_kind, scope_id], |row| {
                Ok(ChatThreadSummary {
                    id: row.get(0)?,
                    anchor: thread_anchor_from_row(row, 1)?,
                    title: row.get(8)?,
                    updated_at: row.get(9)?,
                    entry_count: row.get(10)?,
                    pinned_count: row.get(11)?,
                })
            })
            .map_err(|error| error.to_string())?;

        collect_rows(rows)
    }

    /// Load a thread with its entries, oldest-first.
    pub fn get_chat_thread(&self, thread_id: &str) -> StoreResult<ChatThreadView> {
        let conn = self.open_connection()?;
        let thread = read_chat_thread(&conn, thread_id)?;
        let entries = read_chat_entries(&conn, thread_id)?;
        Ok(ChatThreadView { thread, entries })
    }

    /// Return a thread's `(scope_kind, scope_id)`, e.g. `("paper", paper_id)`.
    pub fn chat_thread_scope(&self, thread_id: &str) -> StoreResult<(String, String)> {
        let conn = self.open_connection()?;
        conn.query_row(
            "select scope_kind, scope_id from chat_threads where id = ?1",
            params![thread_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|error| error.to_string())
    }

    /// Create a thread for an anchor and return it.
    pub fn create_chat_thread(
        &self,
        scope_kind: &str,
        scope_id: &str,
        anchor: &ThreadAnchor,
        title: Option<&str>,
    ) -> StoreResult<ChatThread> {
        let id = timestamped_id("thread")?;
        let conn = self.open_connection()?;
        insert_chat_thread(&conn, &id, scope_kind, scope_id, anchor, title)?;
        read_chat_thread(&conn, &id)
    }

    /// Return the scope's whole-paper (document) thread, creating it if absent.
    pub fn ensure_document_thread(
        &self,
        scope_kind: &str,
        scope_id: &str,
    ) -> StoreResult<ChatThread> {
        let conn = self.open_connection()?;
        let existing: Option<String> = conn
            .query_row(
                "
                select id from chat_threads
                where scope_kind = ?1 and scope_id = ?2 and anchor_kind = 'document'
                order by created_at asc
                limit 1
                ",
                params![scope_kind, scope_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|error| error.to_string())?;

        if let Some(id) = existing {
            return read_chat_thread(&conn, &id);
        }

        drop(conn);
        self.create_chat_thread(scope_kind, scope_id, &ThreadAnchor::Document, None)
    }

    /// Append an entry to a thread (bumping the thread's activity) and return it.
    pub fn append_chat_entry(
        &self,
        thread_id: &str,
        draft: &ChatEntryDraft,
    ) -> StoreResult<ChatEntry> {
        let id = timestamped_id("entry")?;
        let context_json = match &draft.context_summary {
            Some(summary) => Some(serde_json::to_string(summary).map_err(|e| e.to_string())?),
            None => None,
        };

        let mut conn = self.open_connection()?;
        let tx = conn.transaction().map_err(|error| error.to_string())?;
        tx.execute(
            "
            insert into chat_entries (
              id, thread_id, kind, body, model, context_json, pinned, created_at
            )
            values (?1, ?2, ?3, ?4, ?5, ?6, ?7, datetime('now'))
            ",
            params![
                id,
                thread_id,
                draft.kind,
                draft.body,
                draft.model,
                context_json,
                draft.pinned as i64,
            ],
        )
        .map_err(|error| error.to_string())?;
        tx.execute(
            "update chat_threads set updated_at = datetime('now') where id = ?1",
            params![thread_id],
        )
        .map_err(|error| error.to_string())?;
        tx.commit().map_err(|error| error.to_string())?;

        let conn = self.open_connection()?;
        read_chat_entry(&conn, &id)
    }

    /// Set or clear an entry's pin.
    pub fn set_chat_entry_pinned(&self, entry_id: &str, pinned: bool) -> StoreResult<()> {
        let conn = self.open_connection()?;
        let updated = conn
            .execute(
                "update chat_entries set pinned = ?2 where id = ?1",
                params![entry_id, pinned as i64],
            )
            .map_err(|error| error.to_string())?;
        if updated == 0 {
            return Err(format!("Chat entry not found: {entry_id}"));
        }
        Ok(())
    }

    /// List a scope's pinned entries across all threads, newest-first.
    pub fn list_pinned_chat_entries(
        &self,
        scope_kind: &str,
        scope_id: &str,
    ) -> StoreResult<Vec<PinnedHighlight>> {
        let conn = self.open_connection()?;
        let mut stmt = conn
            .prepare(
                "
                select e.id, e.thread_id, e.kind, e.body, e.model, e.context_json, e.pinned,
                       e.created_at, t.title, t.anchor_kind, t.source_id, t.start_offset,
                       t.end_offset, t.selected_text, t.page_index, t.rects_json
                from chat_entries e
                join chat_threads t on e.thread_id = t.id
                where t.scope_kind = ?1 and t.scope_id = ?2 and e.pinned = 1
                order by e.created_at desc, e.id desc
                ",
            )
            .map_err(|error| error.to_string())?;

        let rows = stmt
            .query_map(params![scope_kind, scope_id], |row| {
                Ok(PinnedHighlight {
                    entry: chat_entry_from_row(row)?,
                    thread_title: row.get(8)?,
                    anchor: thread_anchor_from_row(row, 9)?,
                })
            })
            .map_err(|error| error.to_string())?;

        collect_rows(rows)
    }

    /// Rename a thread.
    pub fn rename_chat_thread(&self, thread_id: &str, title: &str) -> StoreResult<()> {
        let title = title.trim();
        if title.is_empty() {
            return Err("Thread title cannot be empty".to_string());
        }
        let conn = self.open_connection()?;
        let updated = conn
            .execute(
                "update chat_threads set title = ?2, updated_at = datetime('now') where id = ?1",
                params![thread_id, title],
            )
            .map_err(|error| error.to_string())?;
        if updated == 0 {
            return Err(format!("Thread not found: {thread_id}"));
        }
        Ok(())
    }

    /// Delete a thread and its entries (entries cascade via FK).
    pub fn delete_chat_thread(&self, thread_id: &str) -> StoreResult<()> {
        let conn = self.open_connection()?;
        conn.execute("delete from chat_threads where id = ?1", params![thread_id])
            .map_err(|error| error.to_string())?;
        Ok(())
    }

    fn open_connection(&self) -> StoreResult<Connection> {
        let conn = Connection::open(&self.db_path).map_err(|error| error.to_string())?;
        conn.execute_batch("pragma foreign_keys = on;")
            .map_err(|error| error.to_string())?;
        Ok(conn)
    }

    fn create_schema(&self, conn: &Connection) -> StoreResult<()> {
        conn.execute_batch(
            "
            create table if not exists vaults (
              id text primary key,
              title text not null,
              path text not null unique,
              created_at text not null,
              updated_at text not null
            );

            create table if not exists papers (
              id text primary key,
              title text not null,
              authors_json text not null,
              venue text not null,
              year integer not null,
              citations integer not null default 0,
              tags_json text not null,
              note_count integer not null default 0,
              annotation_count integer not null default 0,
              status text not null,
              abstract text,
              active_source_id text,
              active_extraction_id text,
              created_at text not null,
              updated_at text not null
            );

            create table if not exists vault_papers (
              vault_id text not null,
              paper_id text not null,
              added_at text not null,
              primary key (vault_id, paper_id),
              foreign key (vault_id) references vaults(id) on delete cascade,
              foreign key (paper_id) references papers(id) on delete cascade
            );

            create table if not exists paper_notes (
              id text primary key,
              paper_id text not null,
              source_id text not null,
              start_offset integer not null,
              end_offset integer not null,
              selected_text text not null,
              anchor_kind text not null default 'text_offset',
              page_index integer,
              rects_json text,
              quote_context text,
              body text not null,
              created_at text not null,
              updated_at text not null,
              foreign key (paper_id) references papers(id) on delete cascade
            );

            create table if not exists document_sources (
              id text primary key,
              paper_id text not null,
              source_kind text not null,
              source_url text,
              local_path text,
              status text not null,
              error text,
              created_at text not null,
              updated_at text not null,
              foreign key (paper_id) references papers(id) on delete cascade
            );

            create index if not exists idx_document_sources_paper_id
              on document_sources(paper_id);

            create table if not exists document_extractions (
              id text primary key,
              paper_id text not null,
              source_id text not null,
              extractor text not null,
              extractor_version text not null,
              annotation_source_id text not null unique,
              status text not null,
              error text,
              created_at text not null,
              updated_at text not null,
              foreign key (paper_id) references papers(id) on delete cascade,
              foreign key (source_id) references document_sources(id) on delete cascade
            );

            create index if not exists idx_document_extractions_paper_id
              on document_extractions(paper_id);

            create index if not exists idx_document_extractions_source_id
              on document_extractions(source_id);

            create table if not exists document_pages (
              id text primary key,
              paper_id text not null,
              source_id text not null,
              extraction_id text not null,
              page_index integer not null,
              width real not null,
              height real not null,
              foreign key (paper_id) references papers(id) on delete cascade,
              foreign key (source_id) references document_sources(id) on delete cascade,
              foreign key (extraction_id) references document_extractions(id) on delete cascade
            );

            create index if not exists idx_document_pages_extraction_id
              on document_pages(extraction_id);

            create table if not exists document_blocks (
              id text primary key,
              paper_id text not null,
              source_id text not null,
              extraction_id text not null,
              page_index integer not null,
              block_index integer not null,
              reading_order integer not null,
              kind text not null,
              text text,
              asset_id text,
              source_start integer,
              source_end integer,
              bbox_json text,
              foreign key (paper_id) references papers(id) on delete cascade,
              foreign key (source_id) references document_sources(id) on delete cascade,
              foreign key (extraction_id) references document_extractions(id) on delete cascade
            );

            create index if not exists idx_document_blocks_extraction_id
              on document_blocks(extraction_id);

            create table if not exists document_spans (
              id text primary key,
              paper_id text not null,
              source_id text not null,
              extraction_id text not null,
              block_id text not null,
              page_index integer not null,
              text text not null,
              source_start integer not null,
              source_end integer not null,
              bbox_json text not null,
              foreign key (paper_id) references papers(id) on delete cascade,
              foreign key (source_id) references document_sources(id) on delete cascade,
              foreign key (extraction_id) references document_extractions(id) on delete cascade,
              foreign key (block_id) references document_blocks(id) on delete cascade
            );

            create index if not exists idx_document_spans_extraction_id
              on document_spans(extraction_id);

            create table if not exists document_assets (
              id text primary key,
              paper_id text not null,
              source_id text not null,
              extraction_id text not null,
              asset_kind text not null,
              page_index integer not null,
              bbox_json text,
              local_path text not null,
              caption text,
              created_at text not null,
              updated_at text not null,
              foreign key (paper_id) references papers(id) on delete cascade,
              foreign key (source_id) references document_sources(id) on delete cascade,
              foreign key (extraction_id) references document_extractions(id) on delete cascade
            );

            create index if not exists idx_document_assets_extraction_id
              on document_assets(extraction_id);

            create table if not exists chat_threads (
              id text primary key,
              scope_kind text not null,
              scope_id text not null,
              anchor_kind text not null,
              source_id text,
              start_offset integer,
              end_offset integer,
              selected_text text,
              page_index integer,
              rects_json text,
              title text not null,
              created_at text not null,
              updated_at text not null
            );

            create index if not exists idx_chat_threads_scope
              on chat_threads(scope_kind, scope_id, updated_at);

            create table if not exists chat_entries (
              id text primary key,
              thread_id text not null,
              kind text not null,
              body text not null,
              model text,
              context_json text,
              pinned integer not null default 0,
              created_at text not null,
              foreign key (thread_id) references chat_threads(id) on delete cascade
            );

            create index if not exists idx_chat_entries_thread
              on chat_entries(thread_id, created_at);

            create index if not exists idx_chat_entries_pinned
              on chat_entries(thread_id, pinned);
            ",
        )
        .map_err(|error| error.to_string())?;

        add_column_if_missing(conn, "papers", "active_source_id", "text")?;
        add_column_if_missing(conn, "papers", "active_extraction_id", "text")?;
        add_column_if_missing(
            conn,
            "paper_notes",
            "anchor_kind",
            "text not null default 'text_offset'",
        )?;
        add_column_if_missing(conn, "paper_notes", "page_index", "integer")?;
        add_column_if_missing(conn, "paper_notes", "rects_json", "text")?;
        add_column_if_missing(conn, "paper_notes", "quote_context", "text")
    }

    fn is_library_empty(&self, conn: &Connection) -> StoreResult<bool> {
        let count: i64 = conn
            .query_row("select count(*) from vaults", [], |row| row.get(0))
            .map_err(|error| error.to_string())?;

        Ok(count == 0)
    }

    fn seed_defaults(&self, conn: &mut Connection) -> StoreResult<()> {
        let tx = conn.transaction().map_err(|error| error.to_string())?;

        for vault in default_vaults() {
            tx.execute(
                "
                insert into vaults (id, title, path, created_at, updated_at)
                values (?1, ?2, ?3, datetime('now'), datetime('now'))
                ",
                params![vault.id, vault.title, vault.path],
            )
            .map_err(|error| error.to_string())?;
        }

        for paper in default_papers() {
            tx.execute(
                "
                insert into papers (
                  id, title, authors_json, venue, year, citations, tags_json,
                  note_count, annotation_count, status, abstract, created_at, updated_at
                )
                values (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, datetime('now'), datetime('now'))
                ",
                params![
                    paper.id,
                    paper.title,
                    to_json_slice(paper.authors)?,
                    paper.venue,
                    paper.year,
                    paper.citations,
                    to_json_slice(paper.tags)?,
                    paper.note_count,
                    paper.annotation_count,
                    paper.status,
                    paper.abstract_text,
                ],
            )
            .map_err(|error| error.to_string())?;
        }

        for (vault_id, paper_id) in default_memberships() {
            tx.execute(
                "
                insert into vault_papers (vault_id, paper_id, added_at)
                values (?1, ?2, datetime('now'))
                on conflict(vault_id, paper_id) do nothing
                ",
                params![vault_id, paper_id],
            )
            .map_err(|error| error.to_string())?;
        }

        tx.commit().map_err(|error| error.to_string())
    }

    fn read_library(&self, conn: &Connection) -> StoreResult<LibrarySnapshot> {
        Ok(LibrarySnapshot {
            vaults: read_vaults(conn)?,
            papers: read_papers(conn)?,
            vault_papers: read_vault_papers(conn)?,
            document_sources: read_document_sources(conn)?,
            document_extractions: read_document_extractions(conn)?,
            document_pages: read_document_pages(conn)?,
            document_blocks: read_document_blocks(conn)?,
            document_spans: read_document_spans(conn)?,
            document_assets: read_document_assets(conn)?,
        })
    }
}

fn read_vaults(conn: &Connection) -> StoreResult<Vec<Vault>> {
    let mut stmt = conn
        .prepare("select id, title, path from vaults order by path")
        .map_err(|error| error.to_string())?;

    let rows = stmt
        .query_map([], |row| {
            Ok(Vault {
                id: row.get(0)?,
                title: row.get(1)?,
                path: row.get(2)?,
            })
        })
        .map_err(|error| error.to_string())?;

    collect_rows(rows)
}

fn read_papers(conn: &Connection) -> StoreResult<Vec<Paper>> {
    let mut stmt = conn
        .prepare(
            "
            select id, title, authors_json, venue, year, citations, tags_json,
                   note_count, annotation_count, status, abstract,
                   active_source_id, active_extraction_id
            from papers
            order by updated_at desc, title
            ",
        )
        .map_err(|error| error.to_string())?;

    let rows = stmt
        .query_map([], |row| {
            let authors_json: String = row.get(2)?;
            let tags_json: String = row.get(6)?;

            Ok(Paper {
                id: row.get(0)?,
                title: row.get(1)?,
                authors: from_json(&authors_json),
                venue: row.get(3)?,
                year: row.get(4)?,
                citations: row.get(5)?,
                tags: from_json(&tags_json),
                note_count: row.get(7)?,
                annotation_count: row.get(8)?,
                status: row.get(9)?,
                abstract_text: row.get(10)?,
                active_source_id: row.get(11)?,
                active_extraction_id: row.get(12)?,
            })
        })
        .map_err(|error| error.to_string())?;

    collect_rows(rows)
}

fn read_vault_papers(conn: &Connection) -> StoreResult<Vec<VaultPaper>> {
    let mut stmt = conn
        .prepare("select vault_id, paper_id from vault_papers order by added_at desc")
        .map_err(|error| error.to_string())?;

    let rows = stmt
        .query_map([], |row| {
            Ok(VaultPaper {
                vault_id: row.get(0)?,
                paper_id: row.get(1)?,
            })
        })
        .map_err(|error| error.to_string())?;

    collect_rows(rows)
}

fn read_document_sources(conn: &Connection) -> StoreResult<Vec<DocumentSource>> {
    let mut stmt = conn
        .prepare(
            "
            select id, paper_id, source_kind, source_url, local_path,
                   status, error, created_at, updated_at
            from document_sources
            order by updated_at desc, id
            ",
        )
        .map_err(|error| error.to_string())?;

    let rows = stmt
        .query_map([], |row| {
            Ok(DocumentSource {
                id: row.get(0)?,
                paper_id: row.get(1)?,
                source_kind: row.get(2)?,
                source_url: row.get(3)?,
                local_path: row.get(4)?,
                status: row.get(5)?,
                error: row.get(6)?,
                created_at: row.get(7)?,
                updated_at: row.get(8)?,
            })
        })
        .map_err(|error| error.to_string())?;

    collect_rows(rows)
}

fn read_document_sources_for_paper(
    conn: &Connection,
    paper_id: &str,
) -> StoreResult<Vec<DocumentSource>> {
    let mut stmt = conn
        .prepare(
            "
            select id, paper_id, source_kind, source_url, local_path,
                   status, error, created_at, updated_at
            from document_sources
            where paper_id = ?1
            order by updated_at desc, id
            ",
        )
        .map_err(|error| error.to_string())?;

    let rows = stmt
        .query_map(params![paper_id], document_source_from_row)
        .map_err(|error| error.to_string())?;

    collect_rows(rows)
}

fn read_document_sources_by_status(
    conn: &Connection,
    status: &str,
) -> StoreResult<Vec<DocumentSource>> {
    let mut stmt = conn
        .prepare(
            "
            select id, paper_id, source_kind, source_url, local_path,
                   status, error, created_at, updated_at
            from document_sources
            where source_kind = 'pdf' and status = ?1 and source_url is not null
            order by updated_at asc, id
            ",
        )
        .map_err(|error| error.to_string())?;

    let rows = stmt
        .query_map(params![status], document_source_from_row)
        .map_err(|error| error.to_string())?;

    collect_rows(rows)
}

fn read_document_source(conn: &Connection, source_id: &str) -> StoreResult<DocumentSource> {
    conn.query_row(
        "
        select id, paper_id, source_kind, source_url, local_path,
               status, error, created_at, updated_at
        from document_sources
        where id = ?1
        ",
        params![source_id],
        document_source_from_row,
    )
    .map_err(|error| error.to_string())
}

fn document_source_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<DocumentSource> {
    Ok(DocumentSource {
        id: row.get(0)?,
        paper_id: row.get(1)?,
        source_kind: row.get(2)?,
        source_url: row.get(3)?,
        local_path: row.get(4)?,
        status: row.get(5)?,
        error: row.get(6)?,
        created_at: row.get(7)?,
        updated_at: row.get(8)?,
    })
}

fn read_document_extractions(conn: &Connection) -> StoreResult<Vec<DocumentExtraction>> {
    let mut stmt = conn
        .prepare(
            "
            select id, paper_id, source_id, extractor, extractor_version,
                   annotation_source_id, status, error, created_at, updated_at
            from document_extractions
            order by updated_at desc, id
            ",
        )
        .map_err(|error| error.to_string())?;

    let rows = stmt
        .query_map([], |row| {
            Ok(DocumentExtraction {
                id: row.get(0)?,
                paper_id: row.get(1)?,
                source_id: row.get(2)?,
                extractor: row.get(3)?,
                extractor_version: row.get(4)?,
                annotation_source_id: row.get(5)?,
                status: row.get(6)?,
                error: row.get(7)?,
                created_at: row.get(8)?,
                updated_at: row.get(9)?,
            })
        })
        .map_err(|error| error.to_string())?;

    collect_rows(rows)
}

fn read_document_pages(conn: &Connection) -> StoreResult<Vec<DocumentPage>> {
    let mut stmt = conn
        .prepare(
            "
            select id, paper_id, source_id, extraction_id, page_index, width, height
            from document_pages
            order by extraction_id, page_index
            ",
        )
        .map_err(|error| error.to_string())?;

    let rows = stmt
        .query_map([], |row| {
            Ok(DocumentPage {
                id: row.get(0)?,
                paper_id: row.get(1)?,
                source_id: row.get(2)?,
                extraction_id: row.get(3)?,
                page_index: row.get(4)?,
                width: row.get(5)?,
                height: row.get(6)?,
            })
        })
        .map_err(|error| error.to_string())?;

    collect_rows(rows)
}

fn read_document_blocks(conn: &Connection) -> StoreResult<Vec<DocumentBlock>> {
    let mut stmt = conn
        .prepare(
            "
            select id, paper_id, source_id, extraction_id, page_index,
                   block_index, reading_order, kind, text, asset_id,
                   source_start, source_end, bbox_json
            from document_blocks
            order by extraction_id, reading_order, block_index
            ",
        )
        .map_err(|error| error.to_string())?;

    let rows = stmt
        .query_map([], |row| {
            Ok(DocumentBlock {
                id: row.get(0)?,
                paper_id: row.get(1)?,
                source_id: row.get(2)?,
                extraction_id: row.get(3)?,
                page_index: row.get(4)?,
                block_index: row.get(5)?,
                reading_order: row.get(6)?,
                kind: row.get(7)?,
                text: row.get(8)?,
                asset_id: row.get(9)?,
                source_start: row.get(10)?,
                source_end: row.get(11)?,
                bbox_json: row.get(12)?,
            })
        })
        .map_err(|error| error.to_string())?;

    collect_rows(rows)
}

fn read_document_spans(conn: &Connection) -> StoreResult<Vec<DocumentSpan>> {
    let mut stmt = conn
        .prepare(
            "
            select id, paper_id, source_id, extraction_id, block_id, page_index,
                   text, source_start, source_end, bbox_json
            from document_spans
            order by extraction_id, source_start, id
            ",
        )
        .map_err(|error| error.to_string())?;

    let rows = stmt
        .query_map([], |row| {
            Ok(DocumentSpan {
                id: row.get(0)?,
                paper_id: row.get(1)?,
                source_id: row.get(2)?,
                extraction_id: row.get(3)?,
                block_id: row.get(4)?,
                page_index: row.get(5)?,
                text: row.get(6)?,
                source_start: row.get(7)?,
                source_end: row.get(8)?,
                bbox_json: row.get(9)?,
            })
        })
        .map_err(|error| error.to_string())?;

    collect_rows(rows)
}

fn read_document_assets(conn: &Connection) -> StoreResult<Vec<DocumentAsset>> {
    let mut stmt = conn
        .prepare(
            "
            select id, paper_id, source_id, extraction_id, asset_kind, page_index,
                   bbox_json, local_path, caption, created_at, updated_at
            from document_assets
            order by extraction_id, page_index, id
            ",
        )
        .map_err(|error| error.to_string())?;

    let rows = stmt
        .query_map([], |row| {
            Ok(DocumentAsset {
                id: row.get(0)?,
                paper_id: row.get(1)?,
                source_id: row.get(2)?,
                extraction_id: row.get(3)?,
                asset_kind: row.get(4)?,
                page_index: row.get(5)?,
                bbox_json: row.get(6)?,
                local_path: row.get(7)?,
                caption: row.get(8)?,
                created_at: row.get(9)?,
                updated_at: row.get(10)?,
            })
        })
        .map_err(|error| error.to_string())?;

    collect_rows(rows)
}

fn read_paper_notes(conn: &Connection, paper_id: &str) -> StoreResult<Vec<PaperNote>> {
    let mut stmt = conn
        .prepare(
            "
            select id, paper_id, source_id, start_offset, end_offset,
                   selected_text, anchor_kind, page_index, rects_json, quote_context,
                   body, created_at, updated_at
            from paper_notes
            where paper_id = ?1
            order by updated_at desc, id desc
            ",
        )
        .map_err(|error| error.to_string())?;

    let rows = stmt
        .query_map(params![paper_id], |row| {
            Ok(PaperNote {
                id: row.get(0)?,
                paper_id: row.get(1)?,
                source_id: row.get(2)?,
                start_offset: row.get(3)?,
                end_offset: row.get(4)?,
                selected_text: row.get(5)?,
                anchor_kind: row.get(6)?,
                page_index: row.get(7)?,
                rects_json: row.get(8)?,
                quote_context: row.get(9)?,
                body: row.get(10)?,
                created_at: row.get(11)?,
                updated_at: row.get(12)?,
            })
        })
        .map_err(|error| error.to_string())?;

    collect_rows(rows)
}

fn read_chat_thread(conn: &Connection, thread_id: &str) -> StoreResult<ChatThread> {
    conn.query_row(
        "
        select id, anchor_kind, source_id, start_offset, end_offset, selected_text,
               page_index, rects_json, title, created_at, updated_at
        from chat_threads
        where id = ?1
        ",
        params![thread_id],
        |row| {
            Ok(ChatThread {
                id: row.get(0)?,
                anchor: thread_anchor_from_row(row, 1)?,
                title: row.get(8)?,
                created_at: row.get(9)?,
                updated_at: row.get(10)?,
            })
        },
    )
    .map_err(|error| error.to_string())
}

fn read_chat_entries(conn: &Connection, thread_id: &str) -> StoreResult<Vec<ChatEntry>> {
    let mut stmt = conn
        .prepare(
            "
            select id, thread_id, kind, body, model, context_json, pinned, created_at
            from chat_entries
            where thread_id = ?1
            order by created_at asc, id asc
            ",
        )
        .map_err(|error| error.to_string())?;

    let rows = stmt
        .query_map(params![thread_id], chat_entry_from_row)
        .map_err(|error| error.to_string())?;

    collect_rows(rows)
}

fn read_chat_entry(conn: &Connection, id: &str) -> StoreResult<ChatEntry> {
    conn.query_row(
        "
        select id, thread_id, kind, body, model, context_json, pinned, created_at
        from chat_entries
        where id = ?1
        ",
        params![id],
        chat_entry_from_row,
    )
    .map_err(|error| error.to_string())
}

fn chat_entry_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ChatEntry> {
    let context_json: Option<String> = row.get(5)?;
    let context_summary = context_json
        .as_deref()
        .and_then(|json| serde_json::from_str::<ChatContextSummary>(json).ok());
    let pinned: i64 = row.get(6)?;

    Ok(ChatEntry {
        id: row.get(0)?,
        thread_id: row.get(1)?,
        kind: row.get(2)?,
        body: row.get(3)?,
        model: row.get(4)?,
        context_summary,
        pinned: pinned != 0,
        created_at: row.get(7)?,
    })
}

/// Read a `ThreadAnchor` from seven consecutive columns starting at `base`:
/// anchor_kind, source_id, start_offset, end_offset, selected_text, page_index,
/// rects_json.
fn thread_anchor_from_row(row: &rusqlite::Row<'_>, base: usize) -> rusqlite::Result<ThreadAnchor> {
    let anchor_kind: String = row.get(base)?;
    let source_id: Option<String> = row.get(base + 1)?;
    let start_offset: Option<i64> = row.get(base + 2)?;
    let end_offset: Option<i64> = row.get(base + 3)?;
    let selected_text: Option<String> = row.get(base + 4)?;
    let page_index: Option<i32> = row.get(base + 5)?;
    let rects_json: Option<String> = row.get(base + 6)?;

    Ok(match anchor_kind.as_str() {
        "text_offset" => ThreadAnchor::TextOffset {
            source_id: source_id.unwrap_or_default(),
            start_offset: start_offset.unwrap_or_default(),
            end_offset: end_offset.unwrap_or_default(),
            selected_text: selected_text.unwrap_or_default(),
        },
        "pdf_rect" => ThreadAnchor::PdfRect {
            source_id: source_id.unwrap_or_default(),
            page_index: page_index.unwrap_or_default(),
            rects_json: rects_json.unwrap_or_default(),
            selected_text: selected_text.unwrap_or_default(),
        },
        _ => ThreadAnchor::Document,
    })
}

/// Flatten a `ThreadAnchor` into nullable storage columns.
type AnchorColumns = (
    &'static str,
    Option<String>,
    Option<i64>,
    Option<i64>,
    Option<String>,
    Option<i32>,
    Option<String>,
);

fn thread_anchor_to_columns(anchor: &ThreadAnchor) -> AnchorColumns {
    match anchor {
        ThreadAnchor::Document => ("document", None, None, None, None, None, None),
        ThreadAnchor::TextOffset {
            source_id,
            start_offset,
            end_offset,
            selected_text,
        } => (
            "text_offset",
            Some(source_id.clone()),
            Some(*start_offset),
            Some(*end_offset),
            Some(selected_text.clone()),
            None,
            None,
        ),
        ThreadAnchor::PdfRect {
            source_id,
            page_index,
            rects_json,
            selected_text,
        } => (
            "pdf_rect",
            Some(source_id.clone()),
            None,
            None,
            Some(selected_text.clone()),
            Some(*page_index),
            Some(rects_json.clone()),
        ),
    }
}

fn insert_chat_thread(
    conn: &Connection,
    id: &str,
    scope_kind: &str,
    scope_id: &str,
    anchor: &ThreadAnchor,
    title: Option<&str>,
) -> StoreResult<()> {
    let (kind, source_id, start_offset, end_offset, selected_text, page_index, rects_json) =
        thread_anchor_to_columns(anchor);
    let title = title
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| anchor.default_title());

    conn.execute(
        "
        insert into chat_threads (
          id, scope_kind, scope_id, anchor_kind, source_id, start_offset, end_offset,
          selected_text, page_index, rects_json, title, created_at, updated_at
        )
        values (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, datetime('now'), datetime('now'))
        ",
        params![
            id,
            scope_kind,
            scope_id,
            kind,
            source_id,
            start_offset,
            end_offset,
            selected_text,
            page_index,
            rects_json,
            title,
        ],
    )
    .map_err(|error| error.to_string())?;
    Ok(())
}

fn table_exists(conn: &Connection, name: &str) -> StoreResult<bool> {
    let count: i64 = conn
        .query_row(
            "select count(*) from sqlite_master where type = 'table' and name = ?1",
            params![name],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    Ok(count > 0)
}

/// A legacy `chat_messages` row: (scope_kind, scope_id, role, body, model,
/// context_json, created_at).
type LegacyChatMessage = (
    String,
    String,
    String,
    String,
    Option<String>,
    Option<String>,
    String,
);

/// One-time migration of RFC 0032 `chat_messages` into document threads.
///
/// Runs only when no threads exist yet and a legacy `chat_messages` table is
/// present. Each scope's messages become one whole-paper thread of entries
/// (`user` → question, `assistant` → answer), preserving timestamps.
fn migrate_chat_messages_to_threads(conn: &mut Connection) -> StoreResult<()> {
    let thread_count: i64 = conn
        .query_row("select count(*) from chat_threads", [], |row| row.get(0))
        .map_err(|error| error.to_string())?;
    if thread_count > 0 || !table_exists(conn, "chat_messages")? {
        return Ok(());
    }

    let messages: Vec<LegacyChatMessage> = {
        let mut stmt = conn
            .prepare(
                "
                select scope_kind, scope_id, role, body, model, context_json, created_at
                from chat_messages
                order by scope_kind, scope_id, created_at, id
                ",
            )
            .map_err(|error| error.to_string())?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                ))
            })
            .map_err(|error| error.to_string())?;
        collect_rows(rows)?
    };
    if messages.is_empty() {
        return Ok(());
    }

    let tx = conn.transaction().map_err(|error| error.to_string())?;
    let mut seq = 0u64;
    let mut current_key: Option<(String, String)> = None;
    let mut thread_id = String::new();
    for (scope_kind, scope_id, role, body, model, context_json, created_at) in messages {
        let key = (scope_kind.clone(), scope_id.clone());
        if current_key.as_ref() != Some(&key) {
            thread_id = format!("thread_mig_{seq}");
            seq += 1;
            tx.execute(
                "
                insert into chat_threads
                  (id, scope_kind, scope_id, anchor_kind, title, created_at, updated_at)
                values (?1, ?2, ?3, 'document', 'Whole paper', ?4, ?4)
                ",
                params![thread_id, scope_kind, scope_id, created_at],
            )
            .map_err(|error| error.to_string())?;
            current_key = Some(key);
        }

        let kind = if role == "assistant" {
            ENTRY_ANSWER
        } else {
            ENTRY_QUESTION
        };
        let entry_id = format!("entry_mig_{seq}");
        seq += 1;
        tx.execute(
            "
            insert into chat_entries
              (id, thread_id, kind, body, model, context_json, pinned, created_at)
            values (?1, ?2, ?3, ?4, ?5, ?6, 0, ?7)
            ",
            params![
                entry_id,
                thread_id,
                kind,
                body,
                model,
                context_json,
                created_at
            ],
        )
        .map_err(|error| error.to_string())?;
        tx.execute(
            "update chat_threads set updated_at = ?2 where id = ?1",
            params![thread_id, created_at],
        )
        .map_err(|error| error.to_string())?;
    }
    tx.commit().map_err(|error| error.to_string())?;
    Ok(())
}

fn upsert_document_sources(tx: &rusqlite::Transaction<'_>, paper: &PaperDraft) -> StoreResult<()> {
    for source in &paper.sources {
        if source.source_kind != "pdf" || source.source_url.trim().is_empty() {
            continue;
        }

        let source_url = source.source_url.trim();
        let source_id = document_source_id(&paper.id, source);

        tx.execute(
            "
            insert into document_sources (
              id, paper_id, source_kind, source_url, local_path, status, error,
              created_at, updated_at
            )
            values (?1, ?2, ?3, ?4, null, 'remote_available', null, datetime('now'), datetime('now'))
            on conflict(id) do update set
              source_url = excluded.source_url,
              status = case
                when document_sources.status = 'cached' then document_sources.status
                else 'remote_available'
              end,
              local_path = case
                when document_sources.status = 'cached' then document_sources.local_path
                else null
              end,
              error = case
                when document_sources.status = 'cached' then document_sources.error
                else null
              end,
              updated_at = datetime('now')
            ",
            params![source_id, paper.id, source.source_kind, source_url],
        )
        .map_err(|error| error.to_string())?;
    }

    Ok(())
}

pub fn document_source_id(paper_id: &str, source: &PaperSourceDraft) -> String {
    let hash = short_sha256(&source.source_url);
    format!("{}:{}:{hash}", source.source_kind, paper_id)
}

fn short_sha256(input: &str) -> String {
    let digest = Sha256::digest(input.as_bytes());
    digest[..6]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>()
}

fn collect_rows<T>(
    rows: rusqlite::MappedRows<'_, impl FnMut(&rusqlite::Row<'_>) -> rusqlite::Result<T>>,
) -> StoreResult<Vec<T>> {
    let mut values = Vec::new();
    for row in rows {
        values.push(row.map_err(|error| error.to_string())?);
    }
    Ok(values)
}

fn validate_note_rects_json(rects_json: &str) -> StoreResult<()> {
    let value: serde_json::Value = serde_json::from_str(rects_json)
        .map_err(|_| "PDF note rectangles must be valid JSON".to_string())?;

    match value {
        serde_json::Value::Array(rects) if !rects.is_empty() => Ok(()),
        _ => Err("PDF note rectangles must be a non-empty array".to_string()),
    }
}

fn add_column_if_missing(
    conn: &Connection,
    table: &str,
    column: &str,
    definition: &str,
) -> StoreResult<()> {
    if column_exists(conn, table, column)? {
        return Ok(());
    }

    conn.execute(
        &format!("alter table {table} add column {column} {definition}"),
        [],
    )
    .map_err(|error| error.to_string())?;

    Ok(())
}

fn column_exists(conn: &Connection, table: &str, column: &str) -> StoreResult<bool> {
    let mut stmt = conn
        .prepare(&format!("pragma table_info({table})"))
        .map_err(|error| error.to_string())?;
    let rows = stmt
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|error| error.to_string())?;

    for row in rows {
        if row.map_err(|error| error.to_string())? == column {
            return Ok(true);
        }
    }

    Ok(false)
}

fn to_json(values: &[String]) -> StoreResult<String> {
    serde_json::to_string(values).map_err(|error| error.to_string())
}

fn to_json_slice(values: &[&str]) -> StoreResult<String> {
    serde_json::to_string(values).map_err(|error| error.to_string())
}

fn from_json(value: &str) -> Vec<String> {
    serde_json::from_str(value).unwrap_or_default()
}

fn generate_note_id() -> StoreResult<String> {
    timestamped_id("note")
}

/// Build a monotonic, lexicographically-sortable id with a feature prefix.
///
/// `created_at` is only second-resolution, so transcript ordering leans on the
/// nanosecond suffix here to break ties within the same second.
fn timestamped_id(prefix: &str) -> StoreResult<String> {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| error.to_string())?
        .as_nanos();

    Ok(format!("{prefix}_{nanos}"))
}

fn normalize_vault_path(input: &str) -> StoreResult<String> {
    let mut parts = Vec::new();

    for part in input.trim().split('/') {
        let trimmed = part.trim();
        if !trimmed.is_empty() {
            parts.push(trimmed);
        }
    }

    if parts.is_empty() {
        return Err("Vault path cannot be empty".to_string());
    }

    Ok(format!("/{}", parts.join("/")))
}

fn vault_title_from_path(path: &str) -> StoreResult<String> {
    path.rsplit('/')
        .find(|part| !part.is_empty())
        .map(ToString::to_string)
        .ok_or_else(|| "Vault path must include a name".to_string())
}

fn vault_id_from_path(path: &str) -> StoreResult<String> {
    let slug = path
        .trim_matches('/')
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-");

    if slug.is_empty() {
        return Err("Vault path must include a usable name".to_string());
    }

    Ok(slug)
}

fn default_vaults() -> Vec<SeedVault> {
    vec![
        SeedVault {
            id: "attention",
            title: "attention",
            path: "/transformers/attention",
        },
        SeedVault {
            id: "self-supervised",
            title: "self-supervised",
            path: "/self-supervised",
        },
        SeedVault {
            id: "vision-transformers",
            title: "vision-transformers",
            path: "/vision-transformers",
        },
        SeedVault {
            id: "interpretability",
            title: "interpretability",
            path: "/interpretability",
        },
        SeedVault {
            id: "scaling",
            title: "scaling-laws",
            path: "/transformers/scaling-laws",
        },
    ]
}

fn default_papers() -> Vec<SeedPaper> {
    vec![
        SeedPaper {
            id: "vaswani2017",
            title: "Attention Is All You Need",
            authors: &["A. Vaswani", "N. Shazeer", "N. Parmar", "J. Uszkoreit", "L. Jones", "A. Gomez"],
            venue: "NeurIPS",
            year: 2017,
            citations: 134821,
            tags: &["foundational", "transformer", "attention"],
            note_count: 4,
            annotation_count: 27,
            status: "READ",
            abstract_text: Some("The Transformer replaces recurrence with attention, making sequence modeling more parallelizable and establishing the architecture behind modern language and vision models."),
        },
        SeedPaper {
            id: "caron2021",
            title: "Emerging Properties in Self-Supervised Vision Transformers",
            authors: &["M. Caron", "H. Touvron", "I. Misra", "H. Jegou", "J. Mairal"],
            venue: "ICCV",
            year: 2021,
            citations: 8842,
            tags: &["frontier", "ssl", "vit", "dino"],
            note_count: 2,
            annotation_count: 14,
            status: "READING",
            abstract_text: Some("Self-supervised ViT features show segmentation-like structure and strong nearest-neighbor classification behavior without labels."),
        },
        SeedPaper {
            id: "dosovitskiy2020",
            title: "An Image is Worth 16x16 Words: Transformers for Image Recognition at Scale",
            authors: &["A. Dosovitskiy", "L. Beyer", "A. Kolesnikov"],
            venue: "ICLR",
            year: 2021,
            citations: 41203,
            tags: &["vit", "foundational"],
            note_count: 3,
            annotation_count: 11,
            status: "READ",
            abstract_text: None,
        },
        SeedPaper {
            id: "devlin2018",
            title: "BERT: Pre-training of Deep Bidirectional Transformers for Language Understanding",
            authors: &["J. Devlin", "M. Chang", "K. Lee", "K. Toutanova"],
            venue: "NAACL",
            year: 2019,
            citations: 102045,
            tags: &["foundational", "transformer"],
            note_count: 1,
            annotation_count: 6,
            status: "READ",
            abstract_text: None,
        },
        SeedPaper {
            id: "radford2019",
            title: "Language Models are Unsupervised Multitask Learners",
            authors: &["A. Radford", "J. Wu", "R. Child", "D. Luan"],
            venue: "OpenAI",
            year: 2019,
            citations: 14820,
            tags: &["gpt", "foundational"],
            note_count: 0,
            annotation_count: 0,
            status: "READ",
            abstract_text: None,
        },
        SeedPaper {
            id: "he2022",
            title: "Masked Autoencoders Are Scalable Vision Learners",
            authors: &["K. He", "X. Chen", "S. Xie"],
            venue: "CVPR",
            year: 2022,
            citations: 7041,
            tags: &["ssl", "vit", "mae"],
            note_count: 2,
            annotation_count: 8,
            status: "READ",
            abstract_text: None,
        },
        SeedPaper {
            id: "chen2020",
            title: "A Simple Framework for Contrastive Learning of Visual Representations",
            authors: &["T. Chen", "S. Kornblith", "M. Norouzi", "G. Hinton"],
            venue: "ICML",
            year: 2020,
            citations: 18472,
            tags: &["ssl", "contrastive", "simclr"],
            note_count: 0,
            annotation_count: 0,
            status: "READ",
            abstract_text: None,
        },
        SeedPaper {
            id: "grill2020",
            title: "Bootstrap Your Own Latent: A New Approach to Self-Supervised Learning",
            authors: &["J. Grill", "F. Strub", "F. Altche"],
            venue: "NeurIPS",
            year: 2020,
            citations: 6293,
            tags: &["ssl", "byol"],
            note_count: 0,
            annotation_count: 0,
            status: "READ",
            abstract_text: None,
        },
        SeedPaper {
            id: "oquab2023",
            title: "DINOv2: Learning Robust Visual Features without Supervision",
            authors: &["M. Oquab", "T. Darcet", "T. Moutakanni"],
            venue: "TMLR",
            year: 2024,
            citations: 1832,
            tags: &["ssl", "dino", "frontier"],
            note_count: 1,
            annotation_count: 0,
            status: "READING",
            abstract_text: None,
        },
        SeedPaper {
            id: "assran2023",
            title: "Self-Supervised Learning from Images with a Joint-Embedding Predictive Architecture",
            authors: &["M. Assran", "Q. Duval", "I. Misra"],
            venue: "CVPR",
            year: 2023,
            citations: 612,
            tags: &["ssl", "jepa", "frontier"],
            note_count: 0,
            annotation_count: 0,
            status: "UNREAD",
            abstract_text: None,
        },
        SeedPaper {
            id: "tay2022",
            title: "Efficient Transformers: A Survey",
            authors: &["Y. Tay", "M. Dehghani", "D. Bahri", "D. Metzler"],
            venue: "ACM CSUR",
            year: 2022,
            citations: 1480,
            tags: &["survey", "efficient"],
            note_count: 0,
            annotation_count: 0,
            status: "UNREAD",
            abstract_text: None,
        },
        SeedPaper {
            id: "kaplan2020",
            title: "Scaling Laws for Neural Language Models",
            authors: &["J. Kaplan", "S. McCandlish", "T. Henighan"],
            venue: "arXiv",
            year: 2020,
            citations: 5821,
            tags: &["scaling", "foundational"],
            note_count: 0,
            annotation_count: 0,
            status: "READ",
            abstract_text: None,
        },
    ]
}

fn default_memberships() -> Vec<(&'static str, &'static str)> {
    vec![
        ("attention", "vaswani2017"),
        ("attention", "caron2021"),
        ("attention", "dosovitskiy2020"),
        ("attention", "devlin2018"),
        ("attention", "radford2019"),
        ("attention", "he2022"),
        ("attention", "chen2020"),
        ("attention", "grill2020"),
        ("attention", "oquab2023"),
        ("attention", "assran2023"),
        ("attention", "tay2022"),
        ("attention", "kaplan2020"),
        ("self-supervised", "caron2021"),
        ("self-supervised", "he2022"),
        ("self-supervised", "chen2020"),
        ("self-supervised", "grill2020"),
        ("self-supervised", "oquab2023"),
        ("self-supervised", "assran2023"),
        ("self-supervised", "dosovitskiy2020"),
        ("vision-transformers", "dosovitskiy2020"),
        ("vision-transformers", "caron2021"),
        ("vision-transformers", "he2022"),
        ("vision-transformers", "oquab2023"),
        ("vision-transformers", "assran2023"),
        ("interpretability", "vaswani2017"),
        ("interpretability", "devlin2018"),
        ("interpretability", "radford2019"),
        ("interpretability", "kaplan2020"),
        ("interpretability", "tay2022"),
        ("scaling", "kaplan2020"),
        ("scaling", "radford2019"),
        ("scaling", "devlin2018"),
        ("scaling", "tay2022"),
        ("scaling", "vaswani2017"),
    ]
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    static TEST_DB_SEQUENCE: AtomicU64 = AtomicU64::new(0);

    struct TestDb {
        store: LibraryStore,
        dir: PathBuf,
    }

    impl Drop for TestDb {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.dir);
        }
    }

    fn test_db() -> StoreResult<TestDb> {
        let unique_id = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| error.to_string())?
            .as_nanos();
        let sequence = TEST_DB_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "i0i-store-test-{}-{unique_id}-{sequence}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).map_err(|error| error.to_string())?;

        let store = LibraryStore::for_test(dir.join("library.sqlite"));
        store.init()?;

        Ok(TestDb { store, dir })
    }

    fn paper_draft(id: &str) -> PaperDraft {
        PaperDraft {
            id: id.to_string(),
            title: format!("Test Paper {id}"),
            authors: vec!["A. Tester".to_string()],
            venue: "TEST".to_string(),
            year: 2026,
            citations: 0,
            tags: vec!["test".to_string()],
            status: "UNREAD".to_string(),
            abstract_text: Some("Test abstract".to_string()),
            sources: vec![],
        }
    }

    fn paper_draft_with_pdf(id: &str, pdf_url: &str) -> PaperDraft {
        let mut draft = paper_draft(id);
        draft.sources = vec![PaperSourceDraft {
            source_kind: "pdf".to_string(),
            source_url: pdf_url.to_string(),
        }];
        draft
    }

    fn note_draft(paper_id: &str, body: &str) -> PaperNoteDraft {
        PaperNoteDraft {
            paper_id: paper_id.to_string(),
            source_id: format!("reader-text-v1:{paper_id}"),
            start_offset: 4,
            end_offset: 16,
            selected_text: "selected text".to_string(),
            anchor_kind: None,
            page_index: None,
            rects_json: None,
            quote_context: None,
            body: body.to_string(),
        }
    }

    fn has_vault(snapshot: &LibrarySnapshot, vault_id: &str) -> bool {
        snapshot.vaults.iter().any(|vault| vault.id == vault_id)
    }

    fn paper<'a>(snapshot: &'a LibrarySnapshot, paper_id: &str) -> &'a Paper {
        snapshot
            .papers
            .iter()
            .find(|paper| paper.id == paper_id)
            .expect("paper should exist")
    }

    fn has_paper(snapshot: &LibrarySnapshot, paper_id: &str) -> bool {
        snapshot.papers.iter().any(|paper| paper.id == paper_id)
    }

    fn paper_count(snapshot: &LibrarySnapshot, paper_id: &str) -> usize {
        snapshot
            .papers
            .iter()
            .filter(|paper| paper.id == paper_id)
            .count()
    }

    fn has_membership(snapshot: &LibrarySnapshot, vault_id: &str, paper_id: &str) -> bool {
        snapshot
            .vault_papers
            .iter()
            .any(|link| link.vault_id == vault_id && link.paper_id == paper_id)
    }

    fn paper_membership_count(snapshot: &LibrarySnapshot, paper_id: &str) -> usize {
        snapshot
            .vault_papers
            .iter()
            .filter(|link| link.paper_id == paper_id)
            .count()
    }

    fn document_source_count(snapshot: &LibrarySnapshot, paper_id: &str) -> usize {
        snapshot
            .document_sources
            .iter()
            .filter(|source| source.paper_id == paper_id)
            .count()
    }

    #[test]
    fn init_seeds_default_library_when_empty() -> StoreResult<()> {
        let db = test_db()?;
        let snapshot = db.store.get_library()?;

        assert_eq!(snapshot.vaults.len(), default_vaults().len());
        assert_eq!(snapshot.papers.len(), default_papers().len());
        assert_eq!(snapshot.vault_papers.len(), default_memberships().len());
        assert!(snapshot.document_sources.is_empty());
        assert!(snapshot.document_extractions.is_empty());
        assert!(snapshot.document_pages.is_empty());
        assert!(snapshot.document_blocks.is_empty());
        assert!(snapshot.document_spans.is_empty());
        assert!(snapshot.document_assets.is_empty());
        assert!(has_vault(&snapshot, "attention"));
        assert!(has_paper(&snapshot, "vaswani2017"));
        assert!(has_membership(&snapshot, "attention", "vaswani2017"));
        assert!(paper(&snapshot, "vaswani2017").active_source_id.is_none());
        assert!(paper(&snapshot, "vaswani2017")
            .active_extraction_id
            .is_none());

        Ok(())
    }

    #[test]
    fn create_vault_normalizes_path_and_persists_it() -> StoreResult<()> {
        let db = test_db()?;
        let snapshot = db.store.create_vault(&VaultDraft {
            path: "  new / topic  ".to_string(),
        })?;
        let vault = snapshot
            .vaults
            .iter()
            .find(|vault| vault.id == "new-topic")
            .expect("created Vault should exist");

        assert_eq!(vault.path, "/new/topic");
        assert_eq!(vault.title, "topic");

        Ok(())
    }

    #[test]
    fn create_vault_rejects_duplicate_paths() -> StoreResult<()> {
        let db = test_db()?;
        let initial_count = db.store.get_library()?.vaults.len();

        db.store.create_vault(&VaultDraft {
            path: "/new/topic".to_string(),
        })?;

        let error = db
            .store
            .create_vault(&VaultDraft {
                path: "new/topic".to_string(),
            })
            .expect_err("duplicate Vault path should fail");
        let snapshot = db.store.get_library()?;

        assert!(error.contains("Vault path already exists"));
        assert_eq!(snapshot.vaults.len(), initial_count + 1);

        Ok(())
    }

    #[test]
    fn rename_vault_changes_path_and_title_but_keeps_id() -> StoreResult<()> {
        let db = test_db()?;
        let snapshot = db.store.rename_vault(&VaultRenameDraft {
            id: "attention".to_string(),
            path: "/transformers/core-attention".to_string(),
        })?;
        let vault = snapshot
            .vaults
            .iter()
            .find(|vault| vault.id == "attention")
            .expect("renamed Vault should keep its id");

        assert_eq!(vault.path, "/transformers/core-attention");
        assert_eq!(vault.title, "core-attention");
        assert!(has_membership(&snapshot, "attention", "vaswani2017"));

        Ok(())
    }

    #[test]
    fn add_paper_to_multiple_vaults_does_not_duplicate_memberships() -> StoreResult<()> {
        let db = test_db()?;
        let paper = paper_draft("multi-vault-paper");
        let vault_ids = vec!["attention".to_string(), "scaling".to_string()];

        db.store.add_paper_to_vaults(&paper, &vault_ids)?;
        let snapshot = db.store.add_paper_to_vaults(&paper, &vault_ids)?;

        assert_eq!(paper_count(&snapshot, "multi-vault-paper"), 1);
        assert!(has_membership(&snapshot, "attention", "multi-vault-paper"));
        assert!(has_membership(&snapshot, "scaling", "multi-vault-paper"));
        assert_eq!(paper_membership_count(&snapshot, "multi-vault-paper"), 2);

        Ok(())
    }

    #[test]
    fn add_paper_to_vault_persists_pdf_source_without_duplicates() -> StoreResult<()> {
        let db = test_db()?;
        let draft = paper_draft_with_pdf("pdf-source-paper", "https://example.test/paper.pdf");
        let vault_ids = vec!["attention".to_string()];

        db.store.add_paper_to_vaults(&draft, &vault_ids)?;
        let snapshot = db.store.add_paper_to_vaults(&draft, &vault_ids)?;

        assert_eq!(document_source_count(&snapshot, "pdf-source-paper"), 1);
        let source = snapshot
            .document_sources
            .iter()
            .find(|source| source.paper_id == "pdf-source-paper")
            .expect("PDF source should exist");

        assert!(source.id.starts_with("pdf:pdf-source-paper:"));
        assert_eq!(source.source_kind, "pdf");
        assert_eq!(
            source.source_url.as_deref(),
            Some("https://example.test/paper.pdf")
        );
        assert_eq!(source.status, "remote_available");
        assert!(source.local_path.is_none());
        assert!(paper(&snapshot, "pdf-source-paper")
            .active_source_id
            .is_none());

        Ok(())
    }

    #[test]
    fn remove_paper_from_one_vault_keeps_shared_paper() -> StoreResult<()> {
        let db = test_db()?;
        let snapshot = db
            .store
            .remove_paper_from_vault("self-supervised", "caron2021")?;

        assert!(!has_membership(&snapshot, "self-supervised", "caron2021"));
        assert!(has_membership(&snapshot, "attention", "caron2021"));
        assert!(has_paper(&snapshot, "caron2021"));

        Ok(())
    }

    #[test]
    fn remove_paper_from_final_vault_deletes_paper() -> StoreResult<()> {
        let db = test_db()?;
        let paper = paper_draft("single-vault-paper");

        db.store
            .add_paper_to_vaults(&paper, &["attention".to_string()])?;
        let snapshot = db
            .store
            .remove_paper_from_vault("attention", "single-vault-paper")?;

        assert!(!has_membership(
            &snapshot,
            "attention",
            "single-vault-paper"
        ));
        assert!(!has_paper(&snapshot, "single-vault-paper"));

        Ok(())
    }

    #[test]
    fn delete_paper_globally_removes_paper_and_memberships() -> StoreResult<()> {
        let db = test_db()?;
        let snapshot = db.store.delete_paper_globally("caron2021")?;

        assert!(!has_paper(&snapshot, "caron2021"));
        assert_eq!(paper_membership_count(&snapshot, "caron2021"), 0);

        Ok(())
    }

    #[test]
    fn delete_vault_preserves_shared_papers() -> StoreResult<()> {
        let db = test_db()?;
        let snapshot = db.store.delete_vault("self-supervised")?;

        assert!(!has_vault(&snapshot, "self-supervised"));
        assert!(!snapshot
            .vault_papers
            .iter()
            .any(|link| link.vault_id == "self-supervised"));
        assert!(has_paper(&snapshot, "caron2021"));
        assert!(has_membership(&snapshot, "attention", "caron2021"));

        Ok(())
    }

    #[test]
    fn delete_vault_deletes_papers_that_lose_final_membership() -> StoreResult<()> {
        let db = test_db()?;
        let paper = paper_draft("unique-vault-paper");

        let snapshot = db.store.create_vault(&VaultDraft {
            path: "/unique".to_string(),
        })?;
        assert!(has_vault(&snapshot, "unique"));

        db.store
            .add_paper_to_vaults(&paper, &["unique".to_string()])?;
        let snapshot = db.store.delete_vault("unique")?;

        assert!(!has_vault(&snapshot, "unique"));
        assert!(!has_paper(&snapshot, "unique-vault-paper"));
        assert!(has_paper(&snapshot, "vaswani2017"));

        Ok(())
    }

    #[test]
    fn create_paper_note_persists_note_and_increments_note_count() -> StoreResult<()> {
        let db = test_db()?;
        let before = db.store.get_library()?;
        let initial_note_count = paper(&before, "vaswani2017").note_count;
        let notes = db
            .store
            .create_paper_note(&note_draft("vaswani2017", "This is worth revisiting."))?;
        let after = db.store.get_library()?;

        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].paper_id, "vaswani2017");
        assert_eq!(notes[0].source_id, "reader-text-v1:vaswani2017");
        assert_eq!(notes[0].start_offset, 4);
        assert_eq!(notes[0].end_offset, 16);
        assert_eq!(notes[0].selected_text, "selected text");
        assert_eq!(notes[0].anchor_kind, "text_offset");
        assert!(notes[0].page_index.is_none());
        assert!(notes[0].rects_json.is_none());
        assert_eq!(notes[0].body, "This is worth revisiting.");
        assert_eq!(
            paper(&after, "vaswani2017").note_count,
            initial_note_count + 1
        );

        Ok(())
    }

    #[test]
    fn create_chat_note_allows_anchorless_note_with_question_context() -> StoreResult<()> {
        let db = test_db()?;
        let mut draft = note_draft("vaswani2017", "Saved from the chat answer.");
        draft.start_offset = 0;
        draft.end_offset = 0;
        draft.selected_text = String::new();
        draft.anchor_kind = Some("chat".to_string());
        draft.page_index = None;
        draft.rects_json = None;
        draft.quote_context = Some("What is scaled dot-product attention?".to_string());

        let notes = db.store.create_paper_note(&draft)?;

        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].anchor_kind, "chat");
        assert_eq!(notes[0].selected_text, "");
        assert_eq!(notes[0].body, "Saved from the chat answer.");
        assert_eq!(
            notes[0].quote_context.as_deref(),
            Some("What is scaled dot-product attention?")
        );

        Ok(())
    }

    #[test]
    fn create_pdf_note_allows_location_anchor_without_selected_text() -> StoreResult<()> {
        let db = test_db()?;
        let mut draft = note_draft("vaswani2017", "This figure matters.");
        draft.source_id = "pdf:vaswani2017:test".to_string();
        draft.start_offset = 0;
        draft.end_offset = 0;
        draft.selected_text = String::new();
        draft.anchor_kind = Some("pdf_rect".to_string());
        draft.page_index = Some(2);
        draft.rects_json = Some(r#"[{"x":0.2,"y":0.3,"width":0.1,"height":0.08}]"#.to_string());

        let notes = db.store.create_paper_note(&draft)?;

        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].anchor_kind, "pdf_rect");
        assert_eq!(notes[0].page_index, Some(2));
        assert_eq!(notes[0].selected_text, "");
        assert_eq!(notes[0].rects_json, draft.rects_json);

        Ok(())
    }

    #[test]
    fn delete_paper_note_removes_note_and_decrements_note_count() -> StoreResult<()> {
        let db = test_db()?;
        let before = db.store.get_library()?;
        let initial_note_count = paper(&before, "vaswani2017").note_count;
        let notes = db
            .store
            .create_paper_note(&note_draft("vaswani2017", "Delete this note"))?;

        db.store.delete_paper_note(&notes[0].id)?;
        let remaining_notes = db.store.get_paper_notes("vaswani2017")?;
        let after = db.store.get_library()?;

        assert!(remaining_notes.is_empty());
        assert_eq!(paper(&after, "vaswani2017").note_count, initial_note_count);

        Ok(())
    }

    #[test]
    fn delete_paper_note_ignores_missing_note_id() -> StoreResult<()> {
        let db = test_db()?;
        let before = db.store.get_library()?;
        let initial_note_count = paper(&before, "vaswani2017").note_count;

        db.store.delete_paper_note("missing-note")?;
        let after = db.store.get_library()?;

        assert_eq!(paper(&after, "vaswani2017").note_count, initial_note_count);

        Ok(())
    }

    #[test]
    fn update_paper_note_changes_body_without_changing_note_count() -> StoreResult<()> {
        let db = test_db()?;
        let before = db.store.get_library()?;
        let initial_note_count = paper(&before, "vaswani2017").note_count;
        let notes = db
            .store
            .create_paper_note(&note_draft("vaswani2017", "Original body"))?;

        db.store
            .update_paper_note(&notes[0].id, "  Updated body  ")?;
        let updated_notes = db.store.get_paper_notes("vaswani2017")?;
        let after = db.store.get_library()?;

        assert_eq!(updated_notes.len(), 1);
        assert_eq!(updated_notes[0].body, "Updated body");
        assert_eq!(
            paper(&after, "vaswani2017").note_count,
            initial_note_count + 1
        );

        Ok(())
    }

    #[test]
    fn update_paper_note_rejects_empty_body_and_missing_note_id() -> StoreResult<()> {
        let db = test_db()?;
        let notes = db
            .store
            .create_paper_note(&note_draft("vaswani2017", "Original body"))?;

        let empty_body_error = db
            .store
            .update_paper_note(&notes[0].id, "   ")
            .expect_err("empty note body should fail");
        let missing_note_error = db
            .store
            .update_paper_note("missing-note", "Updated body")
            .expect_err("missing note should fail");

        assert_eq!(empty_body_error, "Note body cannot be empty");
        assert_eq!(missing_note_error, "Note not found: missing-note");

        Ok(())
    }

    #[test]
    fn get_paper_notes_returns_newest_notes_first() -> StoreResult<()> {
        let db = test_db()?;

        db.store
            .create_paper_note(&note_draft("vaswani2017", "First note"))?;
        let notes = db
            .store
            .create_paper_note(&note_draft("vaswani2017", "Second note"))?;

        assert_eq!(notes.len(), 2);
        assert_eq!(notes[0].body, "Second note");
        assert_eq!(notes[1].body, "First note");

        Ok(())
    }

    #[test]
    fn create_paper_note_rejects_empty_body_and_invalid_offsets() -> StoreResult<()> {
        let db = test_db()?;
        let empty_body_error = db
            .store
            .create_paper_note(&note_draft("vaswani2017", "   "))
            .expect_err("empty note body should fail");
        let mut invalid_offsets = note_draft("vaswani2017", "Body");
        invalid_offsets.end_offset = invalid_offsets.start_offset;
        let offset_error = db
            .store
            .create_paper_note(&invalid_offsets)
            .expect_err("empty offset range should fail");

        assert_eq!(empty_body_error, "Note body cannot be empty");
        assert_eq!(offset_error, "Note offsets must define a non-empty range");

        Ok(())
    }

    #[test]
    fn deleting_paper_deletes_its_notes() -> StoreResult<()> {
        let db = test_db()?;

        db.store
            .create_paper_note(&note_draft("caron2021", "Remove with paper"))?;
        db.store.delete_paper_globally("caron2021")?;
        let notes = db.store.get_paper_notes("caron2021")?;

        assert!(notes.is_empty());

        Ok(())
    }

    fn text_anchor() -> ThreadAnchor {
        ThreadAnchor::TextOffset {
            source_id: "reader-text-v1:vaswani2017".to_string(),
            start_offset: 4,
            end_offset: 22,
            selected_text: "scaled dot-product".to_string(),
        }
    }

    fn summary() -> ChatContextSummary {
        ChatContextSummary {
            paper_title: "Attention Is All You Need".to_string(),
            included_chars: 1234,
            truncated: true,
        }
    }

    #[test]
    fn create_thread_and_append_entries_round_trip_oldest_first() -> StoreResult<()> {
        let db = test_db()?;
        let thread = db
            .store
            .create_chat_thread("paper", "vaswani2017", &text_anchor(), None)?;

        assert_eq!(thread.title, "scaled dot-product");
        assert!(matches!(thread.anchor, ThreadAnchor::TextOffset { .. }));

        db.store
            .append_chat_entry(&thread.id, &ChatEntryDraft::note("my note".to_string()))?;
        db.store.append_chat_entry(
            &thread.id,
            &ChatEntryDraft::question("why scale?".to_string()),
        )?;
        db.store.append_chat_entry(
            &thread.id,
            &ChatEntryDraft::answer(
                "because gradients".to_string(),
                "model-x".to_string(),
                summary(),
            ),
        )?;

        let view = db.store.get_chat_thread(&thread.id)?;
        assert_eq!(view.entries.len(), 3);

        assert_eq!(view.entries[0].kind, "note");
        assert!(view.entries[0].pinned, "notes are pinned by default");
        assert_eq!(view.entries[1].kind, "question");
        assert!(!view.entries[1].pinned);
        assert_eq!(view.entries[2].kind, "answer");
        assert!(!view.entries[2].pinned);
        assert_eq!(view.entries[2].model.as_deref(), Some("model-x"));
        assert!(view.entries[2].context_summary.is_some());

        Ok(())
    }

    #[test]
    fn ensure_document_thread_is_idempotent() -> StoreResult<()> {
        let db = test_db()?;
        let first = db.store.ensure_document_thread("paper", "vaswani2017")?;
        let second = db.store.ensure_document_thread("paper", "vaswani2017")?;

        assert_eq!(first.id, second.id);
        assert!(matches!(first.anchor, ThreadAnchor::Document));
        assert_eq!(first.title, "Whole paper");
        assert_eq!(db.store.list_chat_threads("paper", "vaswani2017")?.len(), 1);

        Ok(())
    }

    #[test]
    fn pinning_drives_highlights_and_thread_counts() -> StoreResult<()> {
        let db = test_db()?;
        let thread = db
            .store
            .create_chat_thread("paper", "vaswani2017", &text_anchor(), None)?;
        let note = db
            .store
            .append_chat_entry(&thread.id, &ChatEntryDraft::note("kept".to_string()))?;
        let answer = db.store.append_chat_entry(
            &thread.id,
            &ChatEntryDraft::answer("ans".to_string(), "m".to_string(), summary()),
        )?;

        // The note is pinned by default; the answer is not.
        let pinned = db.store.list_pinned_chat_entries("paper", "vaswani2017")?;
        assert_eq!(pinned.len(), 1);
        assert_eq!(pinned[0].entry.id, note.id);
        assert_eq!(pinned[0].thread_title, "scaled dot-product");
        assert!(matches!(pinned[0].anchor, ThreadAnchor::TextOffset { .. }));

        // Star the answer, unpin the note.
        db.store.set_chat_entry_pinned(&answer.id, true)?;
        db.store.set_chat_entry_pinned(&note.id, false)?;
        let pinned = db.store.list_pinned_chat_entries("paper", "vaswani2017")?;
        assert_eq!(pinned.len(), 1);
        assert_eq!(pinned[0].entry.id, answer.id);

        let threads = db.store.list_chat_threads("paper", "vaswani2017")?;
        assert_eq!(threads.len(), 1);
        assert_eq!(threads[0].entry_count, 2);
        assert_eq!(threads[0].pinned_count, 1);

        Ok(())
    }

    #[test]
    fn rename_thread_changes_title() -> StoreResult<()> {
        let db = test_db()?;
        let thread =
            db.store
                .create_chat_thread("paper", "vaswani2017", &ThreadAnchor::Document, None)?;
        db.store.rename_chat_thread(&thread.id, "  My title  ")?;
        assert_eq!(
            db.store.get_chat_thread(&thread.id)?.thread.title,
            "My title"
        );

        Ok(())
    }

    #[test]
    fn delete_thread_removes_its_entries() -> StoreResult<()> {
        let db = test_db()?;
        let thread =
            db.store
                .create_chat_thread("paper", "vaswani2017", &ThreadAnchor::Document, None)?;
        db.store
            .append_chat_entry(&thread.id, &ChatEntryDraft::note("n".to_string()))?;

        db.store.delete_chat_thread(&thread.id)?;

        assert!(db
            .store
            .list_chat_threads("paper", "vaswani2017")?
            .is_empty());
        assert!(db
            .store
            .list_pinned_chat_entries("paper", "vaswani2017")?
            .is_empty());

        Ok(())
    }

    #[test]
    fn delete_paper_globally_removes_its_threads() -> StoreResult<()> {
        let db = test_db()?;
        let thread =
            db.store
                .create_chat_thread("paper", "caron2021", &ThreadAnchor::Document, None)?;
        db.store
            .append_chat_entry(&thread.id, &ChatEntryDraft::note("n".to_string()))?;

        db.store.delete_paper_globally("caron2021")?;

        assert!(db.store.list_chat_threads("paper", "caron2021")?.is_empty());

        Ok(())
    }

    #[test]
    fn migrates_legacy_chat_messages_into_document_threads() -> StoreResult<()> {
        let db = test_db()?;
        {
            let conn = Connection::open(&db.store.db_path).map_err(|e| e.to_string())?;
            conn.execute_batch(
                "create table chat_messages (id text primary key, scope_kind text, \
                 scope_id text, role text, body text, model text, context_json text, \
                 created_at text);",
            )
            .map_err(|e| e.to_string())?;
            conn.execute(
                "insert into chat_messages values \
                 ('m1','paper','vaswani2017','user','q1',null,null,'2026-01-01 00:00:00')",
                [],
            )
            .map_err(|e| e.to_string())?;
            conn.execute(
                "insert into chat_messages values \
                 ('m2','paper','vaswani2017','assistant','a1','model-x',null,'2026-01-01 00:00:01')",
                [],
            )
            .map_err(|e| e.to_string())?;
        }

        let mut conn = Connection::open(&db.store.db_path).map_err(|e| e.to_string())?;
        migrate_chat_messages_to_threads(&mut conn)?;

        let threads = db.store.list_chat_threads("paper", "vaswani2017")?;
        assert_eq!(threads.len(), 1);
        assert!(matches!(threads[0].anchor, ThreadAnchor::Document));
        assert_eq!(threads[0].entry_count, 2);

        let view = db.store.get_chat_thread(&threads[0].id)?;
        assert_eq!(view.entries[0].kind, "question");
        assert_eq!(view.entries[0].body, "q1");
        assert_eq!(view.entries[1].kind, "answer");
        assert_eq!(view.entries[1].model.as_deref(), Some("model-x"));

        Ok(())
    }
}
