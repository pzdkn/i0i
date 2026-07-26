use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{params, Connection, OptionalExtension};
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Manager};

use crate::domain::chat::{
    ChatContextSummary, ChatEntry, ChatEntryDraft, ChatThread, ChatThreadSummary, ChatThreadView,
    PinnedHighlight, ThreadAnchor, ENTRY_ANSWER, ENTRY_NOTE, ENTRY_QUESTION,
};
use crate::domain::discovery::PaperCandidate;
use crate::domain::library::{
    DocumentAsset, DocumentBlock, DocumentExtraction, DocumentPage, DocumentSource, DocumentSpan,
    LibrarySnapshot, Paper, PaperDraft, PaperMetadataEnrichment, PaperMetadataUpdate,
    PaperSourceDraft, Vault, VaultDraft, VaultPaper, VaultRenameDraft,
};
use crate::domain::research::{
    candidate_dedup_key, RankedCandidate, Search, SearchCandidate, SearchDraft, SearchRun,
    SearchRunStatus,
};

type StoreResult<T> = Result<T, String>;

#[derive(Clone)]
pub struct LibraryStore {
    db_path: PathBuf,
}

pub struct AnchoredThreadWrite {
    pub view: ChatThreadView,
    pub created: bool,
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
    pub(crate) fn for_test(db_path: PathBuf) -> Self {
        Self { db_path }
    }

    pub fn init(&self) -> StoreResult<()> {
        let mut conn = self.open_connection()?;
        self.create_schema(&conn)?;
        // Legacy chat first (it guards on "no threads yet"), then notes — which
        // may reuse the whole-paper threads the chat migration just created.
        migrate_chat_messages_to_threads(&mut conn)?;
        migrate_paper_notes_into_threads(&mut conn)?;
        drop_legacy_chat_messages(&conn)?;

        if self.is_library_empty(&conn)? {
            self.seed_defaults(&mut conn)?;
        }

        // chat_threads.highlight_id may predate this column on an existing DB
        // (it's only in the `create table if not exists` shape for new DBs).
        // Swallow the "duplicate column" error on already-migrated DBs; that's
        // the idempotency guard.
        let _ = conn.execute("alter table chat_threads add column highlight_id text", []);

        self.migrate_threads_to_highlights()?;

        Ok(())
    }

    pub fn get_library(&self) -> StoreResult<LibrarySnapshot> {
        let conn = self.open_connection()?;
        self.read_library(&conn)
    }

    pub fn get_paper(&self, paper_id: &str) -> StoreResult<Option<Paper>> {
        let conn = self.open_connection()?;
        read_paper(&conn, paper_id)
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
        self.set_document_source_cached_with_acquisition(source_id, local_path, None, None)
    }

    pub fn set_document_source_cached_with_acquisition(
        &self,
        source_id: &str,
        local_path: &str,
        final_url: Option<&str>,
        acquisition_method: Option<&str>,
    ) -> StoreResult<DocumentSource> {
        let mut conn = self.open_connection()?;
        let tx = conn.transaction().map_err(|error| error.to_string())?;
        tx.execute(
            "
            update document_sources
            set status = 'cached',
                local_path = ?2,
                final_url = ?3,
                acquisition_method = ?4,
                error = null,
                updated_at = datetime('now')
            where id = ?1
            ",
            params![source_id, local_path, final_url, acquisition_method],
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

    pub fn resolve_cached_pdf_source(
        &self,
        paper_id: &str,
        source_id: Option<&str>,
    ) -> StoreResult<DocumentSource> {
        let conn = self.open_connection()?;
        if let Some(source_id) = source_id {
            let source = read_document_source(&conn, source_id)?;
            if source.paper_id != paper_id {
                return Err(format!(
                    "Document source {source_id} does not belong to paper {paper_id}"
                ));
            }
            if source.source_kind != "pdf" || source.status != "cached" {
                return Err(format!("Document source is not a cached PDF: {source_id}"));
            }
            if source.local_path.is_none() {
                return Err(format!("Cached PDF source has no local path: {source_id}"));
            }
            return Ok(source);
        }

        conn.query_row(
            "
            select s.id, s.paper_id, s.source_kind, s.source_url, s.landing_url,
                   s.final_url, s.acquisition_method, s.local_path, s.status, s.error, s.created_at, s.updated_at
            from document_sources s
            left join papers p on p.id = s.paper_id
            where s.paper_id = ?1
              and s.source_kind = 'pdf'
              and s.status = 'cached'
              and s.local_path is not null
            order by case when p.active_source_id = s.id then 0 else 1 end,
                     s.updated_at desc,
                     s.id
            limit 1
            ",
            params![paper_id],
            document_source_from_row,
        )
        .map_err(|error| error.to_string())
    }

    pub fn cached_pdf_sources_without_ready_extraction(
        &self,
        extractor: &str,
    ) -> StoreResult<Vec<DocumentSource>> {
        let conn = self.open_connection()?;
        let mut stmt = conn
            .prepare(
                "
                select s.id, s.paper_id, s.source_kind, s.source_url, s.landing_url,
                       s.final_url, s.acquisition_method, s.local_path, s.status, s.error, s.created_at, s.updated_at
                from document_sources s
                where s.source_kind = 'pdf'
                  and s.status = 'cached'
                  and s.local_path is not null
                  and not exists (
                    select 1 from document_extractions e
                    where e.source_id = s.id
                      and e.extractor = ?1
                      and e.status = 'ready'
                  )
                order by s.updated_at asc, s.id
                ",
            )
            .map_err(|error| error.to_string())?;

        let rows = stmt
            .query_map(params![extractor], document_source_from_row)
            .map_err(|error| error.to_string())?;

        collect_rows(rows)
    }

    pub fn stale_document_extractions(
        &self,
        extractor: &str,
    ) -> StoreResult<Vec<DocumentExtraction>> {
        let conn = self.open_connection()?;
        read_document_extractions_by_status(&conn, extractor, "extracting")
    }

    pub fn ready_document_extraction_for_source(
        &self,
        source_id: &str,
        extractor: &str,
    ) -> StoreResult<Option<DocumentExtraction>> {
        let conn = self.open_connection()?;
        read_document_extraction_for_source(&conn, source_id, extractor, "ready")
    }

    pub fn start_document_extraction(
        &self,
        source_id: &str,
        extractor: &str,
        extractor_version: &str,
        annotation_source_id: &str,
        force: bool,
    ) -> StoreResult<DocumentExtraction> {
        let mut conn = self.open_connection()?;
        let tx = conn.transaction().map_err(|error| error.to_string())?;
        let source = read_document_source(&tx, source_id)?;
        if source.source_kind != "pdf" || source.status != "cached" {
            return Err(format!("Document source is not a cached PDF: {source_id}"));
        }
        if source.local_path.is_none() {
            return Err(format!("Cached PDF source has no local path: {source_id}"));
        }

        let existing = read_document_extraction_by_annotation_source(&tx, annotation_source_id)?;
        if let Some(existing) = existing {
            if existing.status == "ready" && !force {
                return Ok(existing);
            }

            if force {
                tx.execute(
                    "delete from document_extractions where id = ?1",
                    params![existing.id],
                )
                .map_err(|error| error.to_string())?;
            } else {
                clear_extraction_children(&tx, &existing.id)?;
                tx.execute(
                    "
                    update document_extractions
                    set extractor_version = ?2,
                        status = 'extracting',
                        error = null,
                        updated_at = datetime('now')
                    where id = ?1
                    ",
                    params![existing.id, extractor_version],
                )
                .map_err(|error| error.to_string())?;
                tx.commit().map_err(|error| error.to_string())?;
                let conn = self.open_connection()?;
                return read_document_extraction(&conn, &existing.id);
            }
        }

        let extraction_id = document_extraction_id(source_id, extractor);
        tx.execute(
            "
            insert into document_extractions (
              id, paper_id, source_id, extractor, extractor_version,
              annotation_source_id, status, error, created_at, updated_at
            )
            values (?1, ?2, ?3, ?4, ?5, ?6, 'extracting', null, datetime('now'), datetime('now'))
            ",
            params![
                extraction_id,
                source.paper_id,
                source.id,
                extractor,
                extractor_version,
                annotation_source_id,
            ],
        )
        .map_err(|error| error.to_string())?;

        tx.commit().map_err(|error| error.to_string())?;
        let conn = self.open_connection()?;
        read_document_extraction(&conn, &extraction_id)
    }

    pub fn finish_document_extraction(
        &self,
        extraction_id: &str,
        pages: &[DocumentPage],
        blocks: &[DocumentBlock],
    ) -> StoreResult<DocumentExtraction> {
        let mut conn = self.open_connection()?;
        let tx = conn.transaction().map_err(|error| error.to_string())?;
        clear_extraction_children(&tx, extraction_id)?;

        for page in pages {
            tx.execute(
                "
                insert into document_pages (
                  id, paper_id, source_id, extraction_id, page_index, width, height
                )
                values (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                ",
                params![
                    page.id,
                    page.paper_id,
                    page.source_id,
                    page.extraction_id,
                    page.page_index,
                    page.width,
                    page.height,
                ],
            )
            .map_err(|error| error.to_string())?;
        }

        for block in blocks {
            tx.execute(
                "
                insert into document_blocks (
                  id, paper_id, source_id, extraction_id, page_index,
                  block_index, reading_order, kind, text, asset_id,
                  source_start, source_end, bbox_json
                )
                values (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
                ",
                params![
                    block.id,
                    block.paper_id,
                    block.source_id,
                    block.extraction_id,
                    block.page_index,
                    block.block_index,
                    block.reading_order,
                    block.kind,
                    block.text,
                    block.asset_id,
                    block.source_start,
                    block.source_end,
                    block.bbox_json,
                ],
            )
            .map_err(|error| error.to_string())?;
        }

        tx.execute(
            "
            update document_extractions
            set status = 'ready', error = null, updated_at = datetime('now')
            where id = ?1
            ",
            params![extraction_id],
        )
        .map_err(|error| error.to_string())?;
        tx.execute(
            "
            update papers
            set active_extraction_id = coalesce(active_extraction_id, ?1),
                updated_at = datetime('now')
            where id = (
              select paper_id from document_extractions where id = ?1
            )
            ",
            params![extraction_id],
        )
        .map_err(|error| error.to_string())?;

        tx.commit().map_err(|error| error.to_string())?;
        let conn = self.open_connection()?;
        read_document_extraction(&conn, extraction_id)
    }

    pub fn set_document_extraction_failed(
        &self,
        extraction_id: &str,
        error: &str,
    ) -> StoreResult<DocumentExtraction> {
        let conn = self.open_connection()?;
        let updated = conn
            .execute(
                "
                update document_extractions
                set status = 'failed', error = ?2, updated_at = datetime('now')
                where id = ?1
                ",
                params![extraction_id, error],
            )
            .map_err(|error| error.to_string())?;
        if updated == 0 {
            return Err(format!("Document extraction not found: {extraction_id}"));
        }
        read_document_extraction(&conn, extraction_id)
    }

    pub fn reset_document_extraction_to_queued(
        &self,
        extraction_id: &str,
    ) -> StoreResult<DocumentExtraction> {
        let conn = self.open_connection()?;
        conn.execute(
            "
            update document_extractions
            set status = 'queued', error = null, updated_at = datetime('now')
            where id = ?1
            ",
            params![extraction_id],
        )
        .map_err(|error| error.to_string())?;
        read_document_extraction(&conn, extraction_id)
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

    pub fn add_local_pdf_to_vault(
        &self,
        paper: &PaperDraft,
        vault_id: &str,
        source_id: &str,
        source_url: &str,
        local_path: &str,
    ) -> StoreResult<()> {
        let mut conn = self.open_connection()?;
        let tx = conn.transaction().map_err(|error| error.to_string())?;
        let authors_json = to_json(&paper.authors)?;
        let tags_json = to_json(&paper.tags)?;

        tx.execute(
            "
            insert into papers (
              id, title, authors_json, venue, year, citations, tags_json,
              note_count, annotation_count, status, abstract, active_source_id,
              created_at, updated_at
            )
            values (?1, ?2, ?3, ?4, ?5, ?6, ?7, 0, 0, ?8, ?9, ?10, datetime('now'), datetime('now'))
            on conflict(id) do update set
              title = excluded.title,
              authors_json = excluded.authors_json,
              venue = excluded.venue,
              year = excluded.year,
              citations = excluded.citations,
              tags_json = excluded.tags_json,
              status = excluded.status,
              abstract = excluded.abstract,
              active_source_id = excluded.active_source_id,
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
                source_id,
            ],
        )
        .map_err(|error| error.to_string())?;

        tx.execute(
            "
            insert into document_sources (
              id, paper_id, source_kind, source_url, landing_url, final_url, acquisition_method,
              local_path, status, error, created_at, updated_at
            )
            values (?1, ?2, 'pdf', ?3, null, ?3, 'local_import', ?4, 'cached', null, datetime('now'), datetime('now'))
            on conflict(id) do update set
              source_url = excluded.source_url,
              final_url = excluded.final_url,
              acquisition_method = excluded.acquisition_method,
              local_path = excluded.local_path,
              status = 'cached',
              error = null,
              updated_at = datetime('now')
            ",
            params![source_id, paper.id, source_url, local_path],
        )
        .map_err(|error| error.to_string())?;

        tx.execute(
            "
            insert into vault_papers (vault_id, paper_id, added_at)
            values (?1, ?2, datetime('now'))
            on conflict(vault_id, paper_id) do nothing
            ",
            params![vault_id, paper.id],
        )
        .map_err(|error| error.to_string())?;

        tx.commit().map_err(|error| error.to_string())
    }

    pub fn apply_paper_metadata_enrichment(
        &self,
        paper_id: &str,
        enrichment: &PaperMetadataEnrichment,
    ) -> StoreResult<Option<Paper>> {
        self.apply_paper_metadata_enrichment_with_policy(paper_id, enrichment, true)
    }

    /// `require_needs_review: false` is the user-approved path (Apply button /
    /// manual edit): the worker's auto-apply still requires the tag so it never
    /// silently overwrites reviewed metadata, but an explicit user action wins.
    pub fn apply_paper_metadata_enrichment_with_policy(
        &self,
        paper_id: &str,
        enrichment: &PaperMetadataEnrichment,
        require_needs_review: bool,
    ) -> StoreResult<Option<Paper>> {
        let conn = self.open_connection()?;
        let Some(current) = read_paper(&conn, paper_id)? else {
            return Ok(None);
        };
        if require_needs_review && !current.tags.iter().any(|tag| tag == "needs-review") {
            return Ok(None);
        }

        let mut tags = current.tags.clone();
        if enrichment.confident {
            tags.retain(|tag| tag != "needs-review");
        }
        if !tags.iter().any(|tag| tag == "metadata-enriched") {
            tags.push("metadata-enriched".to_string());
        }

        let title = enrichment
            .title
            .as_deref()
            .map(str::trim)
            .filter(|title| !title.is_empty())
            .unwrap_or(&current.title);
        let authors = enrichment
            .authors
            .as_ref()
            .filter(|authors| !authors.is_empty())
            .unwrap_or(&current.authors);
        let venue = enrichment
            .venue
            .as_deref()
            .map(str::trim)
            .filter(|venue| !venue.is_empty())
            .unwrap_or(&current.venue);
        let year = enrichment.year.unwrap_or(current.year);
        let citations = enrichment.citations.unwrap_or(current.citations);
        let abstract_text = enrichment
            .abstract_text
            .as_ref()
            .filter(|abstract_text| !abstract_text.trim().is_empty())
            .or(current.abstract_text.as_ref());

        conn.execute(
            "
            update papers
            set title = ?2,
                authors_json = ?3,
                venue = ?4,
                year = ?5,
                citations = ?6,
                tags_json = ?7,
                abstract = ?8,
                updated_at = datetime('now')
            where id = ?1
            ",
            params![
                paper_id,
                title,
                to_json(authors)?,
                venue,
                year,
                citations,
                to_json(&tags)?,
                abstract_text,
            ],
        )
        .map_err(|error| error.to_string())?;

        read_paper(&conn, paper_id)
    }

    /// Manual metadata edit from the right panel (RFC 0049). Only provided
    /// fields are overwritten; saving clears `needs-review`.
    pub fn update_paper_metadata(
        &self,
        paper_id: &str,
        update: &PaperMetadataUpdate,
    ) -> StoreResult<Paper> {
        let conn = self.open_connection()?;
        let current =
            read_paper(&conn, paper_id)?.ok_or_else(|| format!("Paper not found: {paper_id}"))?;

        let title = update
            .title
            .as_deref()
            .map(str::trim)
            .filter(|title| !title.is_empty())
            .unwrap_or(&current.title);
        let authors = update
            .authors
            .as_ref()
            .map(|authors| {
                authors
                    .iter()
                    .map(|author| author.trim().to_string())
                    .filter(|author| !author.is_empty())
                    .collect::<Vec<_>>()
            })
            .filter(|authors| !authors.is_empty())
            .unwrap_or_else(|| current.authors.clone());
        let venue = update
            .venue
            .as_deref()
            .map(str::trim)
            .filter(|venue| !venue.is_empty())
            .unwrap_or(&current.venue);
        let year = update.year.unwrap_or(current.year);
        let abstract_text = update
            .abstract_text
            .as_deref()
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(str::to_string)
            .or_else(|| current.abstract_text.clone());

        let mut tags = current.tags.clone();
        tags.retain(|tag| tag != "needs-review");

        conn.execute(
            "
            update papers
            set title = ?2,
                authors_json = ?3,
                venue = ?4,
                year = ?5,
                tags_json = ?6,
                abstract = ?7,
                updated_at = datetime('now')
            where id = ?1
            ",
            params![
                paper_id,
                title,
                to_json(&authors)?,
                venue,
                year,
                to_json(&tags)?,
                abstract_text,
            ],
        )
        .map_err(|error| error.to_string())?;

        read_paper(&conn, paper_id)?.ok_or_else(|| format!("Paper not found: {paper_id}"))
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

    /// Append an entry to a thread (bumping the thread's activity) and return it.
    pub fn append_chat_entry(
        &self,
        thread_id: &str,
        draft: &ChatEntryDraft,
    ) -> StoreResult<ChatEntry> {
        let id = timestamped_id("entry")?;
        let mut conn = self.open_connection()?;
        let tx = conn.transaction().map_err(|error| error.to_string())?;
        insert_chat_entry(&tx, &id, thread_id, draft)?;
        tx.commit().map_err(|error| error.to_string())?;

        let conn = self.open_connection()?;
        read_chat_entry(&conn, &id)
    }

    /// Add a self-authored note at an anchor, creating its thread lazily, and
    /// return the thread view. The thread and its (pinned) note are written in
    /// one transaction, so a passage never leaves behind an empty thread. A
    /// `document` anchor reuses the paper's single whole-paper thread; every
    /// other anchor creates a fresh thread (no de-duplication, per RFC 0034).
    #[allow(dead_code)]
    pub fn add_note_at_anchor(
        &self,
        scope_kind: &str,
        scope_id: &str,
        anchor: &ThreadAnchor,
        body: &str,
    ) -> StoreResult<ChatThreadView> {
        self.add_note_at_anchor_with_creation(scope_kind, scope_id, anchor, body)
            .map(|write| write.view)
    }

    pub fn add_note_at_anchor_with_creation(
        &self,
        scope_kind: &str,
        scope_id: &str,
        anchor: &ThreadAnchor,
        body: &str,
    ) -> StoreResult<AnchoredThreadWrite> {
        let mut conn = self.open_connection()?;
        let tx = conn.transaction().map_err(|error| error.to_string())?;
        let thread = find_or_create_thread_id(&tx, scope_kind, scope_id, anchor)?;
        let entry_id = timestamped_id("entry")?;
        insert_chat_entry(
            &tx,
            &entry_id,
            &thread.id,
            &ChatEntryDraft::note(body.to_string()),
        )?;
        tx.commit().map_err(|error| error.to_string())?;
        Ok(AnchoredThreadWrite {
            view: self.get_chat_thread(&thread.id)?,
            created: thread.created,
        })
    }

    /// Persist a completed ask turn at an anchor, creating the thread lazily.
    ///
    /// The thread (created or reused, like [`Self::add_note_at_anchor`]), the
    /// question, and the answer are written in one transaction. Because the
    /// caller invokes this only after the model reply succeeds, a failed ask
    /// persists nothing — no empty thread, no orphaned question.
    #[allow(dead_code)]
    pub fn persist_anchored_turn(
        &self,
        scope_kind: &str,
        scope_id: &str,
        anchor: &ThreadAnchor,
        question: &ChatEntryDraft,
        answer: &ChatEntryDraft,
    ) -> StoreResult<ChatThreadView> {
        self.persist_anchored_turn_with_creation(scope_kind, scope_id, anchor, question, answer)
            .map(|write| write.view)
    }

    pub fn persist_anchored_turn_with_creation(
        &self,
        scope_kind: &str,
        scope_id: &str,
        anchor: &ThreadAnchor,
        question: &ChatEntryDraft,
        answer: &ChatEntryDraft,
    ) -> StoreResult<AnchoredThreadWrite> {
        let mut conn = self.open_connection()?;
        let tx = conn.transaction().map_err(|error| error.to_string())?;
        let thread = find_or_create_thread_id(&tx, scope_kind, scope_id, anchor)?;
        // Derive both ids from one base so the question always sorts before the
        // answer even when their `created_at` second is identical.
        let base = timestamped_id("entry")?;
        insert_chat_entry(&tx, &format!("{base}_1"), &thread.id, question)?;
        insert_chat_entry(&tx, &format!("{base}_2"), &thread.id, answer)?;
        tx.commit().map_err(|error| error.to_string())?;
        Ok(AnchoredThreadWrite {
            view: self.get_chat_thread(&thread.id)?,
            created: thread.created,
        })
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

    /// Update a generated title only while the thread still has its default.
    ///
    /// This is the rename-safety guard for background title generation: if the
    /// user renamed the thread while the model was thinking, the `where title`
    /// clause prevents the generated title from clobbering their choice.
    pub fn rename_chat_thread_if_title_is(
        &self,
        thread_id: &str,
        expected_current_title: &str,
        generated_title: &str,
    ) -> StoreResult<Option<ChatThread>> {
        let generated_title = generated_title.trim();
        if generated_title.is_empty() {
            return Ok(None);
        }
        let conn = self.open_connection()?;
        let updated = conn
            .execute(
                "
                update chat_threads
                set title = ?2, updated_at = datetime('now')
                where id = ?1 and title = ?3
                ",
                params![thread_id, generated_title, expected_current_title],
            )
            .map_err(|error| error.to_string())?;
        if updated == 0 {
            return Ok(None);
        }
        read_chat_thread(&conn, thread_id).map(Some)
    }

    /// Delete a thread and its entries (entries cascade via FK).
    pub fn delete_chat_thread(&self, thread_id: &str) -> StoreResult<()> {
        let conn = self.open_connection()?;
        conn.execute("delete from chat_threads where id = ?1", params![thread_id])
            .map_err(|error| error.to_string())?;
        Ok(())
    }

    /// Create a highlight at a locator on a paper's source.
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

        let conn = self.open_connection()?;
        let id = timestamped_id("hl")?;
        let color_str = highlight_color_str(color);
        let (start, end, page, rects) = match locator {
            Locator::TextOffset {
                start_offset,
                end_offset,
                ..
            } => (Some(*start_offset), Some(*end_offset), None, None),
            Locator::PdfRect {
                page_index,
                rects_json,
                ..
            } => (None, None, Some(*page_index), Some(rects_json.clone())),
        };
        conn.execute(
            "
            insert into highlights (
              id, paper_id, source_id, locator_kind, start_offset, end_offset,
              page_index, rects_json, excerpt, color, label, author_kind,
              author_model, created_at, updated_at
            )
            values (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, datetime('now'), datetime('now'))
            ",
            params![
                id,
                paper_id,
                locator.source_id(),
                locator.kind_str(),
                start,
                end,
                page,
                rects,
                excerpt,
                color_str,
                label,
                author.kind_str(),
                author.model(),
            ],
        )
        .map_err(|error| error.to_string())?;
        read_highlight(&conn, &id)
    }

    /// Find an existing highlight at the same locator (paper, source, and
    /// matching offsets/rect), if any. Used to keep highlight creation
    /// idempotent when the same passage is targeted twice — e.g. the
    /// legacy-thread migration linking to a highlight the new-flow UI
    /// already created, instead of inserting a duplicate.
    pub fn find_highlight_by_locator(
        &self,
        paper_id: &str,
        locator: &crate::domain::highlight::Locator,
    ) -> StoreResult<Option<crate::domain::highlight::Highlight>> {
        use crate::domain::highlight::Locator;

        let conn = self.open_connection()?;
        let id: Option<String> = match locator {
            Locator::TextOffset {
                source_id,
                start_offset,
                end_offset,
            } => conn
                .query_row(
                    "select id from highlights
                     where paper_id = ?1 and source_id = ?2 and locator_kind = 'text_offset'
                       and start_offset = ?3 and end_offset = ?4
                     limit 1",
                    params![paper_id, source_id, start_offset, end_offset],
                    |row| row.get(0),
                )
                .optional()
                .map_err(|error| error.to_string())?,
            Locator::PdfRect {
                source_id,
                page_index,
                rects_json,
            } => conn
                .query_row(
                    "select id from highlights
                     where paper_id = ?1 and source_id = ?2 and locator_kind = 'pdf_rect'
                       and page_index = ?3 and rects_json = ?4
                     limit 1",
                    params![paper_id, source_id, page_index, rects_json],
                    |row| row.get(0),
                )
                .optional()
                .map_err(|error| error.to_string())?,
        };
        match id {
            Some(id) => read_highlight(&conn, &id).map(Some),
            None => Ok(None),
        }
    }

    /// List a paper's highlights, oldest first.
    pub fn list_highlights(
        &self,
        paper_id: &str,
    ) -> StoreResult<Vec<crate::domain::highlight::Highlight>> {
        let conn = self.open_connection()?;
        let mut stmt = conn
            .prepare(
                "
                select id, paper_id, source_id, locator_kind, start_offset, end_offset,
                       page_index, rects_json, excerpt, color, label, author_kind,
                       author_model, created_at, updated_at
                from highlights
                where paper_id = ?1
                order by created_at asc
                ",
            )
            .map_err(|error| error.to_string())?;
        let rows = stmt
            .query_map(params![paper_id], highlight_from_row)
            .map_err(|error| error.to_string())?;
        collect_rows(rows)
    }

    /// Change a highlight's color.
    pub fn recolor_highlight(
        &self,
        id: &str,
        color: crate::domain::highlight::HighlightColor,
    ) -> StoreResult<()> {
        let conn = self.open_connection()?;
        conn.execute(
            "update highlights set color = ?2, updated_at = datetime('now') where id = ?1",
            params![id, highlight_color_str(color)],
        )
        .map_err(|error| error.to_string())?;
        Ok(())
    }

    /// Set or clear a highlight's label.
    pub fn set_highlight_label(&self, id: &str, label: Option<&str>) -> StoreResult<()> {
        let conn = self.open_connection()?;
        conn.execute(
            "update highlights set label = ?2, updated_at = datetime('now') where id = ?1",
            params![id, label],
        )
        .map_err(|error| error.to_string())?;
        Ok(())
    }

    /// Delete a highlight.
    pub fn remove_highlight(&self, id: &str) -> StoreResult<()> {
        let conn = self.open_connection()?;
        conn.execute("delete from highlights where id = ?1", params![id])
            .map_err(|error| error.to_string())?;
        Ok(())
    }

    /// One-time backfill (RFC 0058): every paper-scoped anchored thread
    /// (text_offset / pdf_rect) with no highlight yet becomes a User
    /// highlight, and the thread points at it. Document-anchored threads and
    /// non-paper scopes (e.g. vault-level threads, where `scope_id` is not a
    /// paper id) are left with `highlight_id = NULL`. Idempotent: only rows
    /// where `highlight_id is null` are considered, so a second run migrates
    /// nothing.
    pub fn migrate_threads_to_highlights(&self) -> StoreResult<usize> {
        use crate::domain::highlight::{HighlightAuthor, HighlightColor, Locator};

        let conn = self.open_connection()?;
        let mut stmt = conn
            .prepare(
                "select t.id, t.scope_id, t.anchor_kind, t.source_id, t.start_offset,
                        t.end_offset, t.selected_text, t.page_index, t.rects_json
                 from chat_threads t
                 where t.highlight_id is null
                   and t.scope_kind = 'paper'
                   and t.anchor_kind in ('text_offset','pdf_rect')",
            )
            .map_err(|error| error.to_string())?;

        struct Legacy {
            thread_id: String,
            paper_id: String,
            kind: String,
            source: Option<String>,
            start: Option<i64>,
            end: Option<i64>,
            text: Option<String>,
            page: Option<i32>,
            rects: Option<String>,
        }

        let rows = stmt
            .query_map([], |r| {
                Ok(Legacy {
                    thread_id: r.get(0)?,
                    paper_id: r.get(1)?,
                    kind: r.get(2)?,
                    source: r.get(3)?,
                    start: r.get(4)?,
                    end: r.get(5)?,
                    text: r.get(6)?,
                    page: r.get(7)?,
                    rects: r.get(8)?,
                })
            })
            .map_err(|error| error.to_string())?;
        let rows: Vec<Legacy> = collect_rows(rows)?;

        let mut count = 0;
        for row in rows {
            let source_id = row.source.unwrap_or_default();
            let locator = if row.kind == "pdf_rect" {
                Locator::PdfRect {
                    source_id,
                    page_index: row.page.unwrap_or(0),
                    rects_json: row.rects.unwrap_or_default(),
                }
            } else {
                Locator::TextOffset {
                    source_id,
                    start_offset: row.start.unwrap_or(0),
                    end_offset: row.end.unwrap_or(0),
                }
            };
            // A highlight may already exist at this exact locator — either
            // because the new note/ask flow created one up front (leaving
            // this thread's `highlight_id` null until its own update lands),
            // or because a previous migration run inserted the highlight but
            // failed before updating the thread. Link to it instead of
            // inserting a duplicate.
            let hl = match self.find_highlight_by_locator(&row.paper_id, &locator)? {
                Some(existing) => existing,
                None => {
                    let excerpt = row.text.clone().unwrap_or_default();
                    self.insert_highlight(
                        &row.paper_id,
                        &locator,
                        &excerpt,
                        HighlightColor::default(),
                        None,
                        &HighlightAuthor::User,
                    )?
                }
            };
            conn.execute(
                "update chat_threads set highlight_id = ?2 where id = ?1",
                params![row.thread_id, hl.id],
            )
            .map_err(|error| error.to_string())?;
            count += 1;
        }
        Ok(count)
    }

    /// Read the `highlight_id` a thread now points at (set by the backfill
    /// migration, or by future writes that create the highlight up front).
    pub fn thread_highlight_id(&self, id: &str) -> StoreResult<Option<String>> {
        let conn = self.open_connection()?;
        conn.query_row(
            "select highlight_id from chat_threads where id = ?1",
            params![id],
            |row| row.get::<_, Option<String>>(0),
        )
        .optional()
        .map_err(|error| error.to_string())
        .map(|value| value.flatten())
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

            create table if not exists document_sources (
              id text primary key,
              paper_id text not null,
              source_kind text not null,
              source_url text,
              landing_url text,
              final_url text,
              acquisition_method text,
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
              highlight_id text,
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

            create table if not exists highlights (
              id text primary key,
              paper_id text not null,
              source_id text not null,
              locator_kind text not null,
              start_offset integer,
              end_offset integer,
              page_index integer,
              rects_json text,
              excerpt text not null,
              color text not null,
              label text,
              author_kind text not null,
              author_model text,
              created_at text not null,
              updated_at text not null
            );

            create index if not exists idx_highlights_paper
              on highlights(paper_id, created_at);

            create table if not exists searches (
              id text primary key,
              title text not null,
              goal text not null,
              constraints text not null,
              strategy text not null,
              schedule text,
              status text not null,
              stop_reason text,
              summary text,
              error text,
              created_at text not null,
              updated_at text not null
            );

            create table if not exists search_runs (
              id text primary key,
              search_id text not null,
              mode text not null default 'deep',
              provider_set text,
              query_expansions text,
              status text not null,
              stop_reason text,
              iteration integer not null default 0,
              added_count integer not null default 0,
              total_count integer not null default 0,
              started_at text,
              finished_at text,
              error text,
              created_at text not null,
              foreign key (search_id) references searches(id) on delete cascade
            );

            create index if not exists idx_search_runs_search_id
              on search_runs(search_id);

            create table if not exists search_provider_queries (
              id text primary key,
              run_id text not null,
              iteration integer not null,
              provider text not null,
              query_text text not null,
              filters text,
              status text not null,
              result_count integer not null default 0,
              error text,
              created_at text not null,
              foreign key (run_id) references search_runs(id) on delete cascade
            );

            create index if not exists idx_search_provider_queries_run_id
              on search_provider_queries(run_id);

            create table if not exists search_candidates (
              id text primary key,
              search_id text not null,
              first_seen_run_id text not null,
              dedup_key text not null,
              rank integer not null,
              score real,
              rationale text,
              rank_signals_json text,
              provider_hits_json text,
              doi text,
              arxiv_id text,
              title text not null,
              candidate_json text not null,
              from_seed_paper_ids text,
              already_in_library integer not null default 0,
              saved integer not null default 0,
              seen integer not null default 0,
              first_seen_at text not null,
              created_at text not null,
              unique (search_id, dedup_key),
              foreign key (search_id) references searches(id) on delete cascade
            );

            create index if not exists idx_search_candidates_search_id
              on search_candidates(search_id);
            ",
        )
        .map_err(|error| error.to_string())?;

        add_column_if_missing(conn, "papers", "active_source_id", "text")?;
        add_column_if_missing(conn, "papers", "active_extraction_id", "text")?;
        add_column_if_missing(conn, "document_sources", "landing_url", "text")?;
        add_column_if_missing(conn, "document_sources", "final_url", "text")?;
        add_column_if_missing(conn, "document_sources", "acquisition_method", "text")?;
        add_column_if_missing(conn, "search_runs", "mode", "text not null default 'deep'")?;
        add_column_if_missing(conn, "search_runs", "provider_set", "text")?;
        add_column_if_missing(conn, "search_runs", "query_expansions", "text")?;
        add_column_if_missing(conn, "search_candidates", "rank_signals_json", "text")?;
        add_column_if_missing(conn, "search_candidates", "provider_hits_json", "text")
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
    // `highlight_count` is the number of pinned chat entries across the paper's
    // threads (RFC 0034) — computed on read so it never drifts. The legacy
    // `note_count` column is left unused.
    let mut stmt = conn
        .prepare(
            "
            select id, title, authors_json, venue, year, citations, tags_json,
                   (select count(*)
                      from chat_entries e
                      join chat_threads t on e.thread_id = t.id
                     where t.scope_kind = 'paper' and t.scope_id = papers.id
                       and e.pinned = 1) as highlight_count,
                   annotation_count, status, abstract,
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
                highlight_count: row.get(7)?,
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

fn read_paper(conn: &Connection, paper_id: &str) -> StoreResult<Option<Paper>> {
    conn.query_row(
        "
        select id, title, authors_json, venue, year, citations, tags_json,
               (select count(*)
                  from chat_entries e
                  join chat_threads t on e.thread_id = t.id
                 where t.scope_kind = 'paper' and t.scope_id = papers.id
                   and e.pinned = 1) as highlight_count,
               annotation_count, status, abstract,
               active_source_id, active_extraction_id
        from papers
        where id = ?1
        ",
        params![paper_id],
        |row| {
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
                highlight_count: row.get(7)?,
                annotation_count: row.get(8)?,
                status: row.get(9)?,
                abstract_text: row.get(10)?,
                active_source_id: row.get(11)?,
                active_extraction_id: row.get(12)?,
            })
        },
    )
    .optional()
    .map_err(|error| error.to_string())
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
            select id, paper_id, source_kind, source_url, landing_url,
                   final_url, acquisition_method, local_path, status, error, created_at, updated_at
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
                landing_url: row.get(4)?,
                final_url: row.get(5)?,
                acquisition_method: row.get(6)?,
                local_path: row.get(7)?,
                status: row.get(8)?,
                error: row.get(9)?,
                created_at: row.get(10)?,
                updated_at: row.get(11)?,
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
            select id, paper_id, source_kind, source_url, landing_url,
                   final_url, acquisition_method, local_path, status, error, created_at, updated_at
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
            select id, paper_id, source_kind, source_url, landing_url,
                   final_url, acquisition_method, local_path, status, error, created_at, updated_at
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
        select id, paper_id, source_kind, source_url, landing_url,
               final_url, acquisition_method, local_path, status, error, created_at, updated_at
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
        landing_url: row.get(4)?,
        final_url: row.get(5)?,
        acquisition_method: row.get(6)?,
        local_path: row.get(7)?,
        status: row.get(8)?,
        error: row.get(9)?,
        created_at: row.get(10)?,
        updated_at: row.get(11)?,
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

fn read_document_extractions_by_status(
    conn: &Connection,
    extractor: &str,
    status: &str,
) -> StoreResult<Vec<DocumentExtraction>> {
    let mut stmt = conn
        .prepare(
            "
            select id, paper_id, source_id, extractor, extractor_version,
                   annotation_source_id, status, error, created_at, updated_at
            from document_extractions
            where extractor = ?1 and status = ?2
            order by updated_at asc, id
            ",
        )
        .map_err(|error| error.to_string())?;

    let rows = stmt
        .query_map(params![extractor, status], document_extraction_from_row)
        .map_err(|error| error.to_string())?;

    collect_rows(rows)
}

fn read_document_extraction_for_source(
    conn: &Connection,
    source_id: &str,
    extractor: &str,
    status: &str,
) -> StoreResult<Option<DocumentExtraction>> {
    conn.query_row(
        "
        select id, paper_id, source_id, extractor, extractor_version,
               annotation_source_id, status, error, created_at, updated_at
        from document_extractions
        where source_id = ?1 and extractor = ?2 and status = ?3
        order by updated_at desc, id
        limit 1
        ",
        params![source_id, extractor, status],
        document_extraction_from_row,
    )
    .optional()
    .map_err(|error| error.to_string())
}

fn read_document_extraction_by_annotation_source(
    conn: &Connection,
    annotation_source_id: &str,
) -> StoreResult<Option<DocumentExtraction>> {
    conn.query_row(
        "
        select id, paper_id, source_id, extractor, extractor_version,
               annotation_source_id, status, error, created_at, updated_at
        from document_extractions
        where annotation_source_id = ?1
        ",
        params![annotation_source_id],
        document_extraction_from_row,
    )
    .optional()
    .map_err(|error| error.to_string())
}

fn read_document_extraction(
    conn: &Connection,
    extraction_id: &str,
) -> StoreResult<DocumentExtraction> {
    conn.query_row(
        "
        select id, paper_id, source_id, extractor, extractor_version,
               annotation_source_id, status, error, created_at, updated_at
        from document_extractions
        where id = ?1
        ",
        params![extraction_id],
        document_extraction_from_row,
    )
    .map_err(|error| error.to_string())
}

fn document_extraction_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<DocumentExtraction> {
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

/// Serialize a `HighlightColor` to its lowercase storage string, e.g. `"yellow"`.
fn highlight_color_str(color: crate::domain::highlight::HighlightColor) -> String {
    serde_json::to_string(&color)
        .ok()
        .map(|s| s.trim_matches('"').to_string())
        .unwrap_or_else(|| "yellow".to_string())
}

fn read_highlight(
    conn: &Connection,
    id: &str,
) -> StoreResult<crate::domain::highlight::Highlight> {
    conn.query_row(
        "
        select id, paper_id, source_id, locator_kind, start_offset, end_offset,
               page_index, rects_json, excerpt, color, label, author_kind,
               author_model, created_at, updated_at
        from highlights
        where id = ?1
        ",
        params![id],
        highlight_from_row,
    )
    .map_err(|error| error.to_string())
}

fn highlight_from_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<crate::domain::highlight::Highlight> {
    use crate::domain::highlight::{Highlight, HighlightAuthor, HighlightColor, Locator};

    let source_id: String = row.get(2)?;
    let locator_kind: String = row.get(3)?;
    let locator = if locator_kind == "pdf_rect" {
        Locator::PdfRect {
            source_id: source_id.clone(),
            page_index: row.get::<_, Option<i32>>(6)?.unwrap_or_default(),
            rects_json: row.get::<_, Option<String>>(7)?.unwrap_or_default(),
        }
    } else {
        Locator::TextOffset {
            source_id: source_id.clone(),
            start_offset: row.get::<_, Option<i64>>(4)?.unwrap_or_default(),
            end_offset: row.get::<_, Option<i64>>(5)?.unwrap_or_default(),
        }
    };
    let color_str: String = row.get(9)?;
    let color: HighlightColor =
        serde_json::from_str(&format!("\"{color_str}\"")).unwrap_or_default();
    let author = match row.get::<_, String>(11)?.as_str() {
        "agent" => HighlightAuthor::Agent {
            model: row.get::<_, Option<String>>(12)?.unwrap_or_default(),
        },
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

/// Insert one chat entry and bump its thread's activity timestamp. Operates on
/// any `Connection` (a `&Transaction` coerces here), so it composes inside the
/// atomic anchored writes as well as the standalone `append_chat_entry`.
fn insert_chat_entry(
    conn: &Connection,
    id: &str,
    thread_id: &str,
    draft: &ChatEntryDraft,
) -> StoreResult<()> {
    let context_json = match &draft.context_summary {
        Some(summary) => Some(serde_json::to_string(summary).map_err(|e| e.to_string())?),
        None => None,
    };
    conn.execute(
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
    conn.execute(
        "update chat_threads set updated_at = datetime('now') where id = ?1",
        params![thread_id],
    )
    .map_err(|error| error.to_string())?;
    Ok(())
}

struct ThreadResolution {
    id: String,
    created: bool,
}

/// Resolve the thread id for an anchor, creating the thread if needed. A
/// `document` anchor is get-or-create-singular (one whole-paper thread per
/// paper); every other anchor always creates a fresh thread.
fn find_or_create_thread_id(
    conn: &Connection,
    scope_kind: &str,
    scope_id: &str,
    anchor: &ThreadAnchor,
) -> StoreResult<ThreadResolution> {
    if matches!(anchor, ThreadAnchor::Document) {
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
            return Ok(ThreadResolution { id, created: false });
        }
    }

    let id = timestamped_id("thread")?;
    insert_chat_thread(conn, &id, scope_kind, scope_id, anchor, None)?;
    Ok(ThreadResolution { id, created: true })
}

// Deep-research search persistence (RFC 0037). Searches are the durable unit;
// runs are a thin ledger; candidates form a stacked pool keyed by dedup_key.
// `allow(dead_code)`: consumed by the commands layer that lands later; remove then.
#[allow(dead_code)]
impl LibraryStore {
    /// Create a saved search (status `queued`, no runs yet).
    pub fn create_search(&self, draft: &SearchDraft) -> StoreResult<Search> {
        let conn = self.open_connection()?;
        let id = timestamped_id("search")?;
        let constraints = serde_json::to_string(&draft.constraints).map_err(|e| e.to_string())?;
        let strategy = serde_json::to_string(&draft.strategy).map_err(|e| e.to_string())?;
        let schedule = match &draft.schedule {
            Some(s) => Some(serde_json::to_string(s).map_err(|e| e.to_string())?),
            None => None,
        };
        conn.execute(
            "insert into searches
               (id, title, goal, constraints, strategy, schedule, status,
                created_at, updated_at)
             values (?1, ?2, ?3, ?4, ?5, ?6, 'queued', datetime('now'), datetime('now'))",
            params![id, draft.title, draft.goal, constraints, strategy, schedule],
        )
        .map_err(|e| e.to_string())?;
        read_search(&conn, &id)
    }

    /// Load a single search by id.
    pub fn get_search(&self, id: &str) -> StoreResult<Search> {
        let conn = self.open_connection()?;
        read_search(&conn, id)
    }

    /// List all searches, newest first.
    pub fn list_searches(&self) -> StoreResult<Vec<Search>> {
        let conn = self.open_connection()?;
        let mut stmt = conn
            .prepare(
                "select id, title, goal, constraints, strategy, schedule, status,
                        stop_reason, summary, created_at, updated_at
                 from searches order by created_at desc, id desc",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], search_from_row)
            .map_err(|e| e.to_string())?;
        collect_rows(rows)
    }

    /// Update a search's latest-run status fields (after a run resolves).
    pub fn set_search_status(
        &self,
        search_id: &str,
        status: SearchRunStatus,
        stop_reason: Option<&str>,
        summary: Option<&str>,
    ) -> StoreResult<()> {
        let conn = self.open_connection()?;
        conn.execute(
            "update searches
               set status = ?2, stop_reason = ?3, summary = ?4, updated_at = datetime('now')
             where id = ?1",
            params![search_id, status.as_str(), stop_reason, summary],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Start a new run for a search (status `queued`).
    pub fn create_search_run(&self, search_id: &str, mode: &str) -> StoreResult<SearchRun> {
        let conn = self.open_connection()?;
        let id = timestamped_id("run")?;
        let provider_set = search_provider_set_json(&conn, search_id)?;
        conn.execute(
            "insert into search_runs
               (id, search_id, mode, provider_set, query_expansions, status, created_at, started_at)
             values (?1, ?2, ?3, ?4, '[]', 'queued', datetime('now'), datetime('now'))",
            params![id, search_id, mode, provider_set],
        )
        .map_err(|e| e.to_string())?;
        read_search_run(&conn, &id)
    }

    /// Load a single run by id.
    pub fn get_search_run(&self, id: &str) -> StoreResult<SearchRun> {
        let conn = self.open_connection()?;
        read_search_run(&conn, id)
    }

    /// Update a run's progress/status. `finished` stamps `finished_at`.
    pub fn set_search_run_status(
        &self,
        run_id: &str,
        status: SearchRunStatus,
        iteration: i32,
        stop_reason: Option<&str>,
        error: Option<&str>,
        finished: bool,
    ) -> StoreResult<()> {
        let conn = self.open_connection()?;
        let finished_sql = if finished {
            "datetime('now')"
        } else {
            "finished_at"
        };
        conn.execute(
            &format!(
                "update search_runs
                   set status = ?2, iteration = ?3, stop_reason = ?4, error = ?5,
                       finished_at = {finished_sql}
                 where id = ?1"
            ),
            params![run_id, status.as_str(), iteration, stop_reason, error],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Persist the concrete query strings Scout attempted for this run.
    pub fn set_search_run_query_expansions(
        &self,
        run_id: &str,
        query_expansions: &str,
    ) -> StoreResult<()> {
        let conn = self.open_connection()?;
        conn.execute(
            "update search_runs set query_expansions = ?2 where id = ?1",
            params![run_id, query_expansions],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Append an audit row for one provider query within a run.
    // Consumed by the agent loop (RFC 0037 loop layer).
    #[allow(dead_code, clippy::too_many_arguments)]
    pub fn append_provider_query(
        &self,
        run_id: &str,
        iteration: i32,
        provider: &str,
        query_text: &str,
        filters: Option<&str>,
        status: &str,
        result_count: i32,
        error: Option<&str>,
    ) -> StoreResult<()> {
        let conn = self.open_connection()?;
        let id = timestamped_id("spq")?;
        conn.execute(
            "insert into search_provider_queries
               (id, run_id, iteration, provider, query_text, filters, status,
                result_count, error, created_at)
             values (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, datetime('now'))",
            params![
                id,
                run_id,
                iteration,
                provider,
                query_text,
                filters,
                status,
                result_count,
                error
            ],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Stack ranked candidates onto a search's pool: insert only those whose
    /// dedup key is not already present. Returns how many were newly added.
    pub fn append_new_candidates(
        &self,
        search_id: &str,
        run_id: &str,
        ranked: &[RankedCandidate],
    ) -> StoreResult<usize> {
        let mut conn = self.open_connection()?;
        let tx = conn.transaction().map_err(|e| e.to_string())?;
        let mut added = 0usize;
        for item in ranked {
            let candidate = &item.candidate;
            let dedup_key = candidate_dedup_key(candidate);
            let candidate_json = serde_json::to_string(candidate).map_err(|e| e.to_string())?;
            let seeds = serde_json::to_string(&candidate.match_summary.from_seed_paper_ids)
                .map_err(|e| e.to_string())?;
            let rank_signals_json = item
                .rank_signals_json
                .clone()
                .or_else(|| default_rank_signals_json(item).ok());
            let provider_hits_json = item
                .provider_hits_json
                .clone()
                .or_else(|| default_provider_hits_json(candidate).ok());
            let id = timestamped_id("sc")?;
            let changed = tx
                .execute(
                    "insert or ignore into search_candidates
                       (id, search_id, first_seen_run_id, dedup_key, rank, score, rationale,
                        rank_signals_json, provider_hits_json, doi, arxiv_id, title,
                        candidate_json, from_seed_paper_ids,
                        already_in_library, saved, seen, first_seen_at, created_at)
                     values (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15,
                             0, 0, datetime('now'), datetime('now'))",
                    params![
                        id,
                        search_id,
                        run_id,
                        dedup_key,
                        item.rank,
                        item.score,
                        item.rationale,
                        rank_signals_json,
                        provider_hits_json,
                        candidate.doi,
                        candidate.arxiv_id,
                        candidate.title,
                        candidate_json,
                        seeds,
                        candidate.already_in_library as i32,
                    ],
                )
                .map_err(|e| e.to_string())?;
            added += changed;
        }
        tx.commit().map_err(|e| e.to_string())?;
        Ok(added)
    }

    /// List a search's stacked pool, newest batch first then by rank.
    pub fn list_search_candidates(&self, search_id: &str) -> StoreResult<Vec<SearchCandidate>> {
        let conn = self.open_connection()?;
        let mut stmt = conn
            .prepare(
                "select id, search_id, first_seen_run_id, rank, score, rationale,
                        rank_signals_json, provider_hits_json,
                        candidate_json, already_in_library, saved, seen, first_seen_at
                 from search_candidates
                 where search_id = ?1
                 order by first_seen_run_id desc, rank asc",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(params![search_id], search_candidate_from_row)
            .map_err(|e| e.to_string())?;
        collect_rows(rows)
    }

    /// Mark a candidate as saved (added to a vault) or not.
    pub fn mark_search_candidate_saved(&self, candidate_id: &str, saved: bool) -> StoreResult<()> {
        let conn = self.open_connection()?;
        conn.execute(
            "update search_candidates set saved = ?2 where id = ?1",
            params![candidate_id, saved as i32],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Mark all of a search's candidates as seen (clears the unread badge).
    pub fn mark_search_candidates_seen(&self, search_id: &str) -> StoreResult<()> {
        let conn = self.open_connection()?;
        conn.execute(
            "update search_candidates set seen = 1 where search_id = ?1",
            params![search_id],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }
}

fn read_search(conn: &Connection, id: &str) -> StoreResult<Search> {
    conn.query_row(
        "select id, title, goal, constraints, strategy, schedule, status,
                stop_reason, summary, created_at, updated_at
         from searches where id = ?1",
        params![id],
        search_from_row,
    )
    .map_err(|e| e.to_string())
}

fn search_from_row(row: &rusqlite::Row) -> rusqlite::Result<Search> {
    let constraints_json: String = row.get(3)?;
    let strategy_json: String = row.get(4)?;
    let schedule_json: Option<String> = row.get(5)?;
    Ok(Search {
        id: row.get(0)?,
        title: row.get(1)?,
        goal: row.get(2)?,
        constraints: serde_json::from_str(&constraints_json).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(3, rusqlite::types::Type::Text, Box::new(e))
        })?,
        strategy: serde_json::from_str(&strategy_json).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(4, rusqlite::types::Type::Text, Box::new(e))
        })?,
        schedule: match schedule_json {
            Some(json) => Some(serde_json::from_str(&json).map_err(|e| {
                rusqlite::Error::FromSqlConversionFailure(
                    5,
                    rusqlite::types::Type::Text,
                    Box::new(e),
                )
            })?),
            None => None,
        },
        status: row.get(6)?,
        stop_reason: row.get(7)?,
        summary: row.get(8)?,
        created_at: row.get(9)?,
        updated_at: row.get(10)?,
    })
}

fn read_search_run(conn: &Connection, id: &str) -> StoreResult<SearchRun> {
    conn.query_row(
        "select id, search_id, mode, provider_set, query_expansions, status,
                stop_reason, iteration, added_count, total_count, started_at,
                finished_at, error, created_at
         from search_runs where id = ?1",
        params![id],
        search_run_from_row,
    )
    .map_err(|e| e.to_string())
}

fn search_run_from_row(row: &rusqlite::Row) -> rusqlite::Result<SearchRun> {
    Ok(SearchRun {
        id: row.get(0)?,
        search_id: row.get(1)?,
        mode: row.get(2)?,
        provider_set: row.get(3)?,
        query_expansions: row.get(4)?,
        status: row.get(5)?,
        stop_reason: row.get(6)?,
        iteration: row.get(7)?,
        added_count: row.get(8)?,
        total_count: row.get(9)?,
        started_at: row.get(10)?,
        finished_at: row.get(11)?,
        error: row.get(12)?,
        created_at: row.get(13)?,
    })
}

fn search_provider_set_json(conn: &Connection, search_id: &str) -> StoreResult<String> {
    let constraints_json: String = conn
        .query_row(
            "select constraints from searches where id = ?1",
            params![search_id],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;
    let constraints: crate::domain::research::SearchConstraints =
        serde_json::from_str(&constraints_json).map_err(|e| e.to_string())?;
    serde_json::to_string(&constraints.providers).map_err(|e| e.to_string())
}

fn search_candidate_from_row(row: &rusqlite::Row) -> rusqlite::Result<SearchCandidate> {
    let candidate_json: String = row.get(8)?;
    Ok(SearchCandidate {
        id: row.get(0)?,
        search_id: row.get(1)?,
        first_seen_run_id: row.get(2)?,
        rank: row.get(3)?,
        score: row.get(4)?,
        rationale: row.get(5)?,
        rank_signals_json: row.get(6)?,
        provider_hits_json: row.get(7)?,
        candidate: serde_json::from_str(&candidate_json).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(8, rusqlite::types::Type::Text, Box::new(e))
        })?,
        already_in_library: row.get::<_, i32>(9)? != 0,
        saved: row.get::<_, i32>(10)? != 0,
        seen: row.get::<_, i32>(11)? != 0,
        first_seen_at: row.get(12)?,
    })
}

fn default_rank_signals_json(item: &RankedCandidate) -> Result<String, serde_json::Error> {
    serde_json::to_string(&serde_json::json!({
        "score": item.score,
        "rationale": item.rationale,
        "candidateScore": item.candidate.match_summary.score,
        "reasons": item.candidate.match_summary.reasons,
    }))
}

fn default_provider_hits_json(candidate: &PaperCandidate) -> Result<String, serde_json::Error> {
    let providers = candidate
        .match_summary
        .reasons
        .iter()
        .filter_map(|reason| reason.strip_prefix("found_by:"))
        .collect::<Vec<_>>();
    serde_json::to_string(&serde_json::json!({ "providers": providers }))
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

/// A legacy `paper_notes` row used during migration: (paper_id, source_id,
/// start_offset, end_offset, selected_text, anchor_kind, page_index,
/// rects_json, body, created_at).
type LegacyPaperNote = (
    String,
    String,
    i64,
    i64,
    String,
    String,
    Option<i32>,
    Option<String>,
    String,
    String,
);

/// One-time migration of RFC 0033's `paper_notes` into anchored threads
/// (RFC 0034). Each note becomes a thread carrying a single **pinned** `note`
/// entry — so migrated notes stay highlighted, as before. Text/PDF anchors get
/// their own thread; anchorless chat notes append to the paper's whole-paper
/// thread. The `paper_notes` table is dropped afterward, which also guards
/// against re-running.
fn migrate_paper_notes_into_threads(conn: &mut Connection) -> StoreResult<()> {
    if !table_exists(conn, "paper_notes")? {
        return Ok(());
    }

    let notes: Vec<LegacyPaperNote> = {
        let mut stmt = conn
            .prepare(
                "
                select paper_id, source_id, start_offset, end_offset, selected_text,
                       anchor_kind, page_index, rects_json, body, created_at
                from paper_notes
                order by paper_id, created_at, id
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
                    row.get(7)?,
                    row.get(8)?,
                    row.get(9)?,
                ))
            })
            .map_err(|error| error.to_string())?;
        collect_rows(rows)?
    };

    let tx = conn.transaction().map_err(|error| error.to_string())?;
    let mut seq = 0u64;
    let mut doc_thread_by_paper: HashMap<String, String> = HashMap::new();
    for note in notes {
        let (
            paper_id,
            source_id,
            start_offset,
            end_offset,
            selected_text,
            anchor_kind,
            page_index,
            rects_json,
            body,
            created_at,
        ) = note;

        let thread_id = match anchor_kind.as_str() {
            "pdf_rect" => {
                let id = format!("thread_pn_{seq}");
                seq += 1;
                let anchor = ThreadAnchor::PdfRect {
                    source_id,
                    page_index: page_index.unwrap_or_default(),
                    rects_json: rects_json.unwrap_or_else(|| "[]".to_string()),
                    selected_text,
                };
                insert_migrated_thread(&tx, &id, &paper_id, &anchor, &created_at)?;
                id
            }
            "chat" => migrated_document_thread(
                &tx,
                &paper_id,
                &created_at,
                &mut doc_thread_by_paper,
                &mut seq,
            )?,
            _ => {
                // text_offset (and any unknown kind) → a text-anchored thread.
                let id = format!("thread_pn_{seq}");
                seq += 1;
                let anchor = ThreadAnchor::TextOffset {
                    source_id,
                    start_offset,
                    end_offset,
                    selected_text,
                };
                insert_migrated_thread(&tx, &id, &paper_id, &anchor, &created_at)?;
                id
            }
        };

        let entry_id = format!("entry_pn_{seq}");
        seq += 1;
        tx.execute(
            "
            insert into chat_entries
              (id, thread_id, kind, body, model, context_json, pinned, created_at)
            values (?1, ?2, ?3, ?4, null, null, 1, ?5)
            ",
            params![entry_id, thread_id, ENTRY_NOTE, body, created_at],
        )
        .map_err(|error| error.to_string())?;
        tx.execute(
            "update chat_threads set updated_at = ?2 where id = ?1",
            params![thread_id, created_at],
        )
        .map_err(|error| error.to_string())?;
    }

    tx.execute_batch("drop table paper_notes;")
        .map_err(|error| error.to_string())?;
    tx.commit().map_err(|error| error.to_string())?;
    Ok(())
}

/// Insert a migrated thread, preserving the note's original timestamps.
fn insert_migrated_thread(
    conn: &Connection,
    id: &str,
    paper_id: &str,
    anchor: &ThreadAnchor,
    created_at: &str,
) -> StoreResult<()> {
    let (kind, source_id, start_offset, end_offset, selected_text, page_index, rects_json) =
        thread_anchor_to_columns(anchor);
    conn.execute(
        "
        insert into chat_threads (
          id, scope_kind, scope_id, anchor_kind, source_id, start_offset, end_offset,
          selected_text, page_index, rects_json, title, created_at, updated_at
        )
        values (?1, 'paper', ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?11)
        ",
        params![
            id,
            paper_id,
            kind,
            source_id,
            start_offset,
            end_offset,
            selected_text,
            page_index,
            rects_json,
            anchor.default_title(),
            created_at,
        ],
    )
    .map_err(|error| error.to_string())?;
    Ok(())
}

/// The paper's whole-paper thread during migration — reusing one created by the
/// chat-message migration when present, otherwise creating it.
fn migrated_document_thread(
    conn: &Connection,
    paper_id: &str,
    created_at: &str,
    cache: &mut HashMap<String, String>,
    seq: &mut u64,
) -> StoreResult<String> {
    if let Some(id) = cache.get(paper_id) {
        return Ok(id.clone());
    }
    let existing: Option<String> = conn
        .query_row(
            "
            select id from chat_threads
            where scope_kind = 'paper' and scope_id = ?1 and anchor_kind = 'document'
            order by created_at asc
            limit 1
            ",
            params![paper_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| error.to_string())?;
    let id = match existing {
        Some(id) => id,
        None => {
            let id = format!("thread_pn_{}", *seq);
            *seq += 1;
            insert_migrated_thread(conn, &id, paper_id, &ThreadAnchor::Document, created_at)?;
            id
        }
    };
    cache.insert(paper_id.to_string(), id.clone());
    Ok(id)
}

/// Drop the legacy `chat_messages` table once it has been migrated (RFC 0034).
fn drop_legacy_chat_messages(conn: &Connection) -> StoreResult<()> {
    if table_exists(conn, "chat_messages")? {
        conn.execute_batch("drop table chat_messages;")
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn upsert_document_sources(tx: &rusqlite::Transaction<'_>, paper: &PaperDraft) -> StoreResult<()> {
    for source in &paper.sources {
        if source.source_kind != "pdf" || source.source_url.trim().is_empty() {
            continue;
        }

        let source_url = source.source_url.trim();
        let landing_url = source
            .landing_url
            .as_deref()
            .map(str::trim)
            .filter(|url| !url.is_empty());
        let source_id = document_source_id(&paper.id, source);

        tx.execute(
            "
            insert into document_sources (
              id, paper_id, source_kind, source_url, landing_url, final_url, acquisition_method, local_path, status, error,
              created_at, updated_at
            )
            values (?1, ?2, ?3, ?4, ?5, null, null, null, 'remote_available', null, datetime('now'), datetime('now'))
            on conflict(id) do update set
              source_url = excluded.source_url,
              landing_url = excluded.landing_url,
              final_url = case
                when document_sources.status = 'cached' then document_sources.final_url
                else null
              end,
              acquisition_method = case
                when document_sources.status = 'cached' then document_sources.acquisition_method
                else null
              end,
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
            params![
                source_id,
                paper.id,
                source.source_kind,
                source_url,
                landing_url
            ],
        )
        .map_err(|error| error.to_string())?;
    }

    Ok(())
}

pub fn document_source_id(paper_id: &str, source: &PaperSourceDraft) -> String {
    let hash = short_sha256(&source.source_url);
    format!("{}:{}:{hash}", source.source_kind, paper_id)
}

fn document_extraction_id(source_id: &str, extractor: &str) -> String {
    format!("extraction:{extractor}:{source_id}")
}

fn clear_extraction_children(conn: &Connection, extraction_id: &str) -> StoreResult<()> {
    conn.execute(
        "delete from document_spans where extraction_id = ?1",
        params![extraction_id],
    )
    .map_err(|error| error.to_string())?;
    conn.execute(
        "delete from document_assets where extraction_id = ?1",
        params![extraction_id],
    )
    .map_err(|error| error.to_string())?;
    conn.execute(
        "delete from document_blocks where extraction_id = ?1",
        params![extraction_id],
    )
    .map_err(|error| error.to_string())?;
    conn.execute(
        "delete from document_pages where extraction_id = ?1",
        params![extraction_id],
    )
    .map_err(|error| error.to_string())?;
    Ok(())
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
            landing_url: Some("https://example.test/landing".to_string()),
        }];
        draft
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

    fn extraction_page(extraction: &DocumentExtraction) -> DocumentPage {
        DocumentPage {
            id: format!("{}:page:0", extraction.id),
            paper_id: extraction.paper_id.clone(),
            source_id: extraction.source_id.clone(),
            extraction_id: extraction.id.clone(),
            page_index: 0,
            width: 612.0,
            height: 792.0,
        }
    }

    fn extraction_block(extraction: &DocumentExtraction, text: &str) -> DocumentBlock {
        DocumentBlock {
            id: format!("{}:block:0:0", extraction.id),
            paper_id: extraction.paper_id.clone(),
            source_id: extraction.source_id.clone(),
            extraction_id: extraction.id.clone(),
            page_index: 0,
            block_index: 0,
            reading_order: 0,
            kind: "paragraph".to_string(),
            text: Some(text.to_string()),
            asset_id: None,
            source_start: Some(0),
            source_end: Some(text.chars().count() as i64),
            bbox_json: None,
        }
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
        assert_eq!(
            source.landing_url.as_deref(),
            Some("https://example.test/landing")
        );
        assert_eq!(source.status, "remote_available");
        assert!(source.local_path.is_none());
        assert!(paper(&snapshot, "pdf-source-paper")
            .active_source_id
            .is_none());

        Ok(())
    }

    #[test]
    fn add_local_pdf_to_vault_persists_cached_active_source() -> StoreResult<()> {
        let db = test_db()?;
        let draft = paper_draft("local-pdf-paper");
        let source_id = "pdf:local-pdf-paper:abc123";

        db.store.add_local_pdf_to_vault(
            &draft,
            "attention",
            source_id,
            "local://sha256/abc123",
            "/tmp/i0i/local-paper.pdf",
        )?;
        let snapshot = db.store.get_library()?;
        let source = snapshot
            .document_sources
            .iter()
            .find(|source| source.id == source_id)
            .expect("local PDF source should exist");

        assert!(has_membership(&snapshot, "attention", "local-pdf-paper"));
        assert_eq!(source.source_kind, "pdf");
        assert_eq!(source.status, "cached");
        assert_eq!(source.acquisition_method.as_deref(), Some("local_import"));
        assert_eq!(
            source.local_path.as_deref(),
            Some("/tmp/i0i/local-paper.pdf")
        );
        assert_eq!(
            paper(&snapshot, "local-pdf-paper")
                .active_source_id
                .as_deref(),
            Some(source_id)
        );

        Ok(())
    }

    #[test]
    fn metadata_enrichment_updates_needs_review_paper() -> StoreResult<()> {
        let db = test_db()?;
        let mut draft = paper_draft("needs-metadata-paper");
        draft.tags = vec!["local".to_string(), "needs-review".to_string()];
        db.store.add_local_pdf_to_vault(
            &draft,
            "attention",
            "pdf:needs-metadata-paper:abc123",
            "local://sha256/abc123",
            "/tmp/i0i/needs-metadata-paper.pdf",
        )?;

        let updated = db
            .store
            .apply_paper_metadata_enrichment(
                "needs-metadata-paper",
                &PaperMetadataEnrichment {
                    title: Some("Recovered Paper Title".to_string()),
                    authors: Some(vec!["Ada Lovelace".to_string()]),
                    venue: Some("Journal of Useful Machines".to_string()),
                    year: Some(1843),
                    citations: Some(12),
                    abstract_text: Some("A recovered abstract.".to_string()),
                    confident: true,
                },
            )?
            .expect("paper should be updated");

        assert_eq!(updated.title, "Recovered Paper Title");
        assert_eq!(updated.authors, vec!["Ada Lovelace"]);
        assert_eq!(updated.venue, "Journal of Useful Machines");
        assert_eq!(updated.year, 1843);
        assert_eq!(updated.citations, 12);
        assert_eq!(
            updated.abstract_text.as_deref(),
            Some("A recovered abstract.")
        );
        assert!(updated.tags.contains(&"metadata-enriched".to_string()));
        assert!(!updated.tags.contains(&"needs-review".to_string()));

        Ok(())
    }

    #[test]
    fn cached_document_source_persists_acquisition_provenance() -> StoreResult<()> {
        let db = test_db()?;
        let draft = paper_draft_with_pdf("provenance-paper", "https://example.test/blocked.pdf");
        db.store
            .add_paper_to_vaults(&draft, &["attention".to_string()])?;
        let source = db
            .store
            .get_document_sources("provenance-paper")?
            .into_iter()
            .next()
            .expect("PDF source should exist");

        let source = db.store.set_document_source_cached_with_acquisition(
            &source.id,
            "/tmp/provenance.pdf",
            Some("https://cdn.example.test/provenance.pdf"),
            Some("obscura_browser_stealth"),
        )?;

        assert_eq!(source.status, "cached");
        assert_eq!(
            source.final_url.as_deref(),
            Some("https://cdn.example.test/provenance.pdf")
        );
        assert_eq!(
            source.acquisition_method.as_deref(),
            Some("obscura_browser_stealth")
        );

        Ok(())
    }

    #[test]
    fn finish_document_extraction_persists_rows_and_sets_active_extraction() -> StoreResult<()> {
        let db = test_db()?;
        let draft = paper_draft_with_pdf("extracted-paper", "https://example.test/extracted.pdf");
        db.store
            .add_paper_to_vaults(&draft, &["attention".to_string()])?;
        let source = db
            .store
            .get_document_sources("extracted-paper")?
            .into_iter()
            .next()
            .expect("PDF source should exist");
        let source = db
            .store
            .set_document_source_cached(&source.id, "/tmp/extracted.pdf")?;
        let annotation_source_id = format!("pdfium_basic:{}", source.id);

        let extraction = db.store.start_document_extraction(
            &source.id,
            "pdfium_basic",
            "0.1.0",
            &annotation_source_id,
            false,
        )?;
        assert_eq!(extraction.status, "extracting");

        let page = extraction_page(&extraction);
        let block = extraction_block(&extraction, "Real extracted page text.");
        let extraction = db.store.finish_document_extraction(
            &extraction.id,
            &[page.clone()],
            &[block.clone()],
        )?;
        let snapshot = db.store.get_library()?;

        assert_eq!(extraction.status, "ready");
        assert_eq!(
            paper(&snapshot, "extracted-paper")
                .active_extraction_id
                .as_deref(),
            Some(extraction.id.as_str())
        );
        assert!(snapshot
            .document_pages
            .iter()
            .any(|row| row.id == page.id && row.extraction_id == extraction.id));
        assert!(snapshot
            .document_blocks
            .iter()
            .any(|row| row.id == block.id
                && row.text.as_deref() == Some("Real extracted page text.")));

        Ok(())
    }

    #[test]
    fn ready_document_extraction_is_idempotent_until_forced() -> StoreResult<()> {
        let db = test_db()?;
        let draft = paper_draft_with_pdf("force-paper", "https://example.test/force.pdf");
        db.store
            .add_paper_to_vaults(&draft, &["attention".to_string()])?;
        let source = db
            .store
            .get_document_sources("force-paper")?
            .into_iter()
            .next()
            .expect("PDF source should exist");
        let source = db
            .store
            .set_document_source_cached(&source.id, "/tmp/force.pdf")?;
        let annotation_source_id = format!("pdfium_basic:{}", source.id);

        let extraction = db.store.start_document_extraction(
            &source.id,
            "pdfium_basic",
            "0.1.0",
            &annotation_source_id,
            false,
        )?;
        let block = extraction_block(&extraction, "Original text.");
        db.store.finish_document_extraction(
            &extraction.id,
            &[extraction_page(&extraction)],
            &[block],
        )?;

        let existing = db.store.start_document_extraction(
            &source.id,
            "pdfium_basic",
            "0.1.0",
            &annotation_source_id,
            false,
        )?;
        assert_eq!(existing.status, "ready");
        assert_eq!(
            db.store.get_library()?.document_blocks[0].text.as_deref(),
            Some("Original text.")
        );

        let forced = db.store.start_document_extraction(
            &source.id,
            "pdfium_basic",
            "0.1.0",
            &annotation_source_id,
            true,
        )?;
        assert_eq!(forced.status, "extracting");
        assert!(db.store.get_library()?.document_blocks.is_empty());

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
    fn append_entries_round_trip_oldest_first() -> StoreResult<()> {
        let db = test_db()?;
        // The first note creates the thread lazily; later turns append to it.
        let thread = db
            .store
            .add_note_at_anchor("paper", "vaswani2017", &text_anchor(), "my note")?
            .thread;

        assert_eq!(thread.title, "scaled dot-product");
        assert!(matches!(thread.anchor, ThreadAnchor::TextOffset { .. }));

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
    fn pinning_drives_highlights_and_thread_counts() -> StoreResult<()> {
        let db = test_db()?;
        let thread = db
            .store
            .add_note_at_anchor("paper", "vaswani2017", &text_anchor(), "kept")?
            .thread;
        let note = db.store.get_chat_thread(&thread.id)?.entries[0].clone();
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
    fn highlight_crud_round_trips() -> StoreResult<()> {
        let db = test_db()?;
        let paper_id = "vaswani2017";
        let loc = crate::domain::highlight::Locator::TextOffset {
            source_id: "src-1".into(),
            start_offset: 5,
            end_offset: 25,
        };
        let hl = db.store.insert_highlight(
            paper_id,
            &loc,
            "quoted text",
            crate::domain::highlight::HighlightColor::Yellow,
            Some("important"),
            &crate::domain::highlight::HighlightAuthor::User,
        )?;
        assert_eq!(hl.color, crate::domain::highlight::HighlightColor::Yellow);
        assert_eq!(hl.excerpt, "quoted text");

        db.store
            .recolor_highlight(&hl.id, crate::domain::highlight::HighlightColor::Red)?;
        let listed = db.store.list_highlights(paper_id)?;
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].color, crate::domain::highlight::HighlightColor::Red);

        db.store.remove_highlight(&hl.id)?;
        assert!(db.store.list_highlights(paper_id)?.is_empty());

        Ok(())
    }

    #[test]
    fn backfill_makes_anchored_threads_into_visible_highlights() -> StoreResult<()> {
        let db = test_db()?;
        let paper_id = "vaswani2017";
        // Create a legacy-style anchored thread via the existing note path.
        let anchor = ThreadAnchor::TextOffset {
            source_id: "src-1".into(),
            start_offset: 3,
            end_offset: 12,
            selected_text: "abc".into(),
        };
        let write =
            db.store
                .add_note_at_anchor_with_creation("paper", paper_id, &anchor, "note body")?;

        // init() already ran the migration once (against an empty table), so
        // this thread — created after init — still needs its own backfill.
        let migrated = db.store.migrate_threads_to_highlights()?;
        assert_eq!(migrated, 1);

        let highlights = db.store.list_highlights(paper_id)?;
        assert_eq!(highlights.len(), 1);
        assert_eq!(highlights[0].excerpt, "abc");

        // The thread now references the new highlight.
        assert_eq!(
            db.store.thread_highlight_id(&write.view.thread.id)?,
            Some(highlights[0].id.clone())
        );

        // Idempotent: second run migrates nothing.
        assert_eq!(db.store.migrate_threads_to_highlights()?, 0);

        Ok(())
    }

    #[test]
    fn migration_links_instead_of_duplicating_when_highlight_already_exists() -> StoreResult<()> {
        use crate::domain::highlight::{HighlightAuthor, HighlightColor, Locator};

        let db = test_db()?;
        let paper_id = "vaswani2017";

        // Simulate the new note/ask UI flow: it creates the thread with an
        // anchor (highlight_id still null, since the thread write and the
        // highlight write are separate calls) and separately inserts a
        // highlight at the same locator up front.
        let anchor = ThreadAnchor::TextOffset {
            source_id: "src-1".into(),
            start_offset: 3,
            end_offset: 12,
            selected_text: "abc".into(),
        };
        let write =
            db.store
                .add_note_at_anchor_with_creation("paper", paper_id, &anchor, "note body")?;

        let locator = Locator::TextOffset {
            source_id: "src-1".into(),
            start_offset: 3,
            end_offset: 12,
        };
        let created = db.store.insert_highlight(
            paper_id,
            &locator,
            "abc",
            HighlightColor::Green,
            None,
            &HighlightAuthor::User,
        )?;

        // The migration must find the existing highlight at this locator and
        // link the thread to it, rather than inserting a second (default
        // yellow) highlight at the same passage.
        let migrated = db.store.migrate_threads_to_highlights()?;
        assert_eq!(migrated, 1);

        let highlights = db.store.list_highlights(paper_id)?;
        assert_eq!(highlights.len(), 1);
        assert_eq!(highlights[0].id, created.id);
        assert_eq!(highlights[0].color, HighlightColor::Green);

        assert_eq!(
            db.store.thread_highlight_id(&write.view.thread.id)?,
            Some(created.id)
        );

        // Idempotent: second run migrates nothing further.
        assert_eq!(db.store.migrate_threads_to_highlights()?, 0);

        Ok(())
    }

    #[test]
    fn rename_thread_changes_title() -> StoreResult<()> {
        let db = test_db()?;
        let thread = db
            .store
            .add_note_at_anchor("paper", "vaswani2017", &ThreadAnchor::Document, "n")?
            .thread;
        db.store.rename_chat_thread(&thread.id, "  My title  ")?;
        assert_eq!(
            db.store.get_chat_thread(&thread.id)?.thread.title,
            "My title"
        );

        Ok(())
    }

    #[test]
    fn anchored_write_reports_whether_thread_was_created() -> StoreResult<()> {
        let db = test_db()?;
        let first = db.store.add_note_at_anchor_with_creation(
            "paper",
            "vaswani2017",
            &ThreadAnchor::Document,
            "first note",
        )?;
        let second = db.store.add_note_at_anchor_with_creation(
            "paper",
            "vaswani2017",
            &ThreadAnchor::Document,
            "second note",
        )?;

        assert!(first.created);
        assert!(!second.created);
        assert_eq!(first.view.thread.id, second.view.thread.id);
        assert_eq!(second.view.entries.len(), 2);

        Ok(())
    }

    #[test]
    fn generated_thread_title_only_applies_while_default_is_unchanged() -> StoreResult<()> {
        let db = test_db()?;
        let view = db
            .store
            .add_note_at_anchor("paper", "vaswani2017", &text_anchor(), "n")?;

        let renamed = db.store.rename_chat_thread_if_title_is(
            &view.thread.id,
            "scaled dot-product",
            "Attention Scaling",
        )?;
        assert_eq!(
            renamed.expect("default title should update").title,
            "Attention Scaling"
        );

        let skipped = db.store.rename_chat_thread_if_title_is(
            &view.thread.id,
            "scaled dot-product",
            "Model Generated Late",
        )?;
        assert!(skipped.is_none());
        assert_eq!(
            db.store.get_chat_thread(&view.thread.id)?.thread.title,
            "Attention Scaling"
        );

        Ok(())
    }

    #[test]
    fn delete_thread_removes_its_entries() -> StoreResult<()> {
        let db = test_db()?;
        let thread = db
            .store
            .add_note_at_anchor("paper", "vaswani2017", &ThreadAnchor::Document, "n")?
            .thread;

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
        db.store
            .add_note_at_anchor("paper", "caron2021", &ThreadAnchor::Document, "n")?;

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

    fn other_text_anchor() -> ThreadAnchor {
        ThreadAnchor::TextOffset {
            source_id: "reader-text-v1:vaswani2017".to_string(),
            start_offset: 40,
            end_offset: 55,
            selected_text: "multi-head".to_string(),
        }
    }

    fn highlight_count(db: &TestDb, paper_id: &str) -> StoreResult<i32> {
        Ok(db
            .store
            .get_library()?
            .papers
            .iter()
            .find(|paper| paper.id == paper_id)
            .expect("paper should exist")
            .highlight_count)
    }

    #[test]
    fn add_note_at_anchor_creates_thread_lazily_with_one_pinned_note() -> StoreResult<()> {
        let db = test_db()?;
        assert!(db
            .store
            .list_chat_threads("paper", "vaswani2017")?
            .is_empty());

        let view = db.store.add_note_at_anchor(
            "paper",
            "vaswani2017",
            &text_anchor(),
            "worth revisiting",
        )?;

        assert!(matches!(
            view.thread.anchor,
            ThreadAnchor::TextOffset { .. }
        ));
        assert_eq!(view.thread.title, "scaled dot-product");
        assert_eq!(view.entries.len(), 1);
        assert_eq!(view.entries[0].kind, "note");
        assert_eq!(view.entries[0].body, "worth revisiting");
        assert!(view.entries[0].pinned, "notes pin by default");

        let threads = db.store.list_chat_threads("paper", "vaswani2017")?;
        assert_eq!(threads.len(), 1);
        assert_eq!(threads[0].pinned_count, 1);

        Ok(())
    }

    #[test]
    fn add_note_at_anchor_is_new_per_selection_but_singular_for_document() -> StoreResult<()> {
        let db = test_db()?;
        db.store
            .add_note_at_anchor("paper", "vaswani2017", &text_anchor(), "a")?;
        db.store
            .add_note_at_anchor("paper", "vaswani2017", &other_text_anchor(), "b")?;
        assert_eq!(db.store.list_chat_threads("paper", "vaswani2017")?.len(), 2);

        // Two whole-paper notes share one document thread.
        db.store
            .add_note_at_anchor("paper", "vaswani2017", &ThreadAnchor::Document, "c")?;
        db.store
            .add_note_at_anchor("paper", "vaswani2017", &ThreadAnchor::Document, "d")?;
        let doc_threads: Vec<_> = db
            .store
            .list_chat_threads("paper", "vaswani2017")?
            .into_iter()
            .filter(|thread| matches!(thread.anchor, ThreadAnchor::Document))
            .collect();
        assert_eq!(doc_threads.len(), 1);
        assert_eq!(doc_threads[0].title, "Whole paper");
        assert_eq!(doc_threads[0].entry_count, 2);
        assert_eq!(doc_threads[0].pinned_count, 2);

        Ok(())
    }

    #[test]
    fn persist_anchored_turn_creates_thread_with_question_then_answer() -> StoreResult<()> {
        let db = test_db()?;
        let view = db.store.persist_anchored_turn(
            "paper",
            "vaswani2017",
            &text_anchor(),
            &ChatEntryDraft::question("why scale?".to_string()),
            &ChatEntryDraft::answer(
                "because gradients".to_string(),
                "model-x".to_string(),
                summary(),
            ),
        )?;

        assert_eq!(view.entries.len(), 2);
        assert_eq!(view.entries[0].kind, "question");
        assert_eq!(view.entries[0].body, "why scale?");
        assert!(!view.entries[0].pinned);
        assert_eq!(view.entries[1].kind, "answer");
        assert_eq!(view.entries[1].model.as_deref(), Some("model-x"));

        // Follow-up turns on the document anchor continue one thread.
        for (q, a) in [("q2", "a2"), ("q3", "a3")] {
            db.store.persist_anchored_turn(
                "paper",
                "vaswani2017",
                &ThreadAnchor::Document,
                &ChatEntryDraft::question(q.to_string()),
                &ChatEntryDraft::answer(a.to_string(), "m".to_string(), summary()),
            )?;
        }
        let doc: Vec<_> = db
            .store
            .list_chat_threads("paper", "vaswani2017")?
            .into_iter()
            .filter(|thread| matches!(thread.anchor, ThreadAnchor::Document))
            .collect();
        assert_eq!(doc.len(), 1);
        assert_eq!(doc[0].entry_count, 4);

        Ok(())
    }

    #[test]
    fn highlight_count_counts_pinned_entries_and_tracks_pinning() -> StoreResult<()> {
        let db = test_db()?;
        assert_eq!(highlight_count(&db, "vaswani2017")?, 0);

        let thread = db
            .store
            .add_note_at_anchor("paper", "vaswani2017", &text_anchor(), "kept")?
            .thread;
        let note = db.store.get_chat_thread(&thread.id)?.entries[0].clone();
        let answer = db.store.append_chat_entry(
            &thread.id,
            &ChatEntryDraft::answer("a".to_string(), "m".to_string(), summary()),
        )?;

        // The note pins by default → one highlight.
        assert_eq!(highlight_count(&db, "vaswani2017")?, 1);

        // Star the answer → two; unpin the note → one.
        db.store.set_chat_entry_pinned(&answer.id, true)?;
        assert_eq!(highlight_count(&db, "vaswani2017")?, 2);
        db.store.set_chat_entry_pinned(&note.id, false)?;
        assert_eq!(highlight_count(&db, "vaswani2017")?, 1);

        Ok(())
    }

    #[test]
    fn migrates_paper_notes_into_pinned_threads() -> StoreResult<()> {
        let db = test_db()?;
        {
            let conn = Connection::open(&db.store.db_path).map_err(|e| e.to_string())?;
            conn.execute_batch(
                "create table paper_notes (id text primary key, paper_id text not null, \
                 source_id text not null, start_offset integer not null, \
                 end_offset integer not null, selected_text text not null, \
                 anchor_kind text not null default 'text_offset', page_index integer, \
                 rects_json text, quote_context text, body text not null, \
                 created_at text not null, updated_at text not null);",
            )
            .map_err(|e| e.to_string())?;
            conn.execute(
                "insert into paper_notes values ('n1','vaswani2017','src',4,16,\
                 'scaled dot-product','text_offset',null,null,null,'text note',\
                 '2026-01-01 00:00:00','2026-01-01 00:00:00')",
                [],
            )
            .map_err(|e| e.to_string())?;
            conn.execute(
                "insert into paper_notes values ('n2','vaswani2017','pdf:src',0,0,'',\
                 'pdf_rect',2,'[{\"x\":0.1,\"y\":0.2,\"width\":0.3,\"height\":0.05}]',\
                 null,'pdf note','2026-01-01 00:00:01','2026-01-01 00:00:01')",
                [],
            )
            .map_err(|e| e.to_string())?;
            conn.execute(
                "insert into paper_notes values ('n3','vaswani2017','',0,0,'','chat',\
                 null,null,'why?','chat note','2026-01-01 00:00:02','2026-01-01 00:00:02')",
                [],
            )
            .map_err(|e| e.to_string())?;
        }

        let mut conn = Connection::open(&db.store.db_path).map_err(|e| e.to_string())?;
        migrate_paper_notes_into_threads(&mut conn)?;

        // text_offset + pdf_rect + one shared document thread (the chat note) = 3.
        let threads = db.store.list_chat_threads("paper", "vaswani2017")?;
        assert_eq!(threads.len(), 3);

        // Every migrated note is one pinned `note` entry → still highlighted.
        let pinned = db.store.list_pinned_chat_entries("paper", "vaswani2017")?;
        assert_eq!(pinned.len(), 3);
        assert!(pinned
            .iter()
            .all(|pin| pin.entry.kind == "note" && pin.entry.pinned));
        assert_eq!(highlight_count(&db, "vaswani2017")?, 3);

        let doc_threads: Vec<_> = threads
            .iter()
            .filter(|thread| matches!(thread.anchor, ThreadAnchor::Document))
            .collect();
        assert_eq!(doc_threads.len(), 1);
        assert_eq!(doc_threads[0].entry_count, 1);

        // The table is dropped, so re-running is a guarded no-op.
        assert!(!table_exists(&conn, "paper_notes")?);
        migrate_paper_notes_into_threads(&mut conn)?;
        assert_eq!(db.store.list_chat_threads("paper", "vaswani2017")?.len(), 3);

        Ok(())
    }

    // --- Deep-research search persistence (RFC 0037) ---

    use crate::domain::discovery::{CandidateMatch, PaperCandidate};
    use crate::domain::research::{
        Depth, RankedCandidate, SearchConstraints, SearchDraft, SearchRunStatus,
    };

    fn sample_constraints() -> SearchConstraints {
        SearchConstraints {
            year_from: Some(2023),
            year_to: None,
            providers: vec![Default::default()],
            open_access: true,
            target_count: 20,
            venues: Vec::new(),
            authors: Vec::new(),
            fields_of_study: Vec::new(),
            seed_paper_ids: Vec::new(),
        }
    }

    fn sample_search_draft() -> SearchDraft {
        SearchDraft {
            title: "XAI methods".to_string(),
            goal: "Recent explainable-AI papers, methods not surveys".to_string(),
            constraints: sample_constraints(),
            strategy: Depth::Standard.budget(),
            schedule: None,
        }
    }

    fn sample_candidate(title: &str, doi: Option<&str>) -> PaperCandidate {
        PaperCandidate {
            id: format!("cand-{title}"),
            source_provider: "openalex".to_string(),
            source_id: format!("S-{title}"),
            title: title.to_string(),
            authors: vec!["A. Tester".to_string()],
            abstract_text: Some("An abstract.".to_string()),
            year: Some(2024),
            publication_date: None,
            venue: Some("NeurIPS".to_string()),
            citation_count: Some(3),
            doi: doi.map(ToString::to_string),
            openalex_id: None,
            arxiv_id: None,
            external_url: None,
            pdf_url: None,
            open_access: None,
            match_summary: CandidateMatch {
                score: None,
                reasons: Vec::new(),
                matched_keywords: Vec::new(),
                from_seed_paper_ids: Vec::new(),
            },
            already_in_library: false,
        }
    }

    fn ranked(title: &str, doi: Option<&str>, rank: i32) -> RankedCandidate {
        RankedCandidate {
            candidate: sample_candidate(title, doi),
            rank,
            score: Some(1.0 / rank as f64),
            rationale: Some(format!("rationale for {title}")),
            rank_signals_json: None,
            provider_hits_json: None,
        }
    }

    #[test]
    fn create_and_get_search_round_trips() -> StoreResult<()> {
        let db = test_db()?;
        let created = db.store.create_search(&sample_search_draft())?;
        assert!(created.id.starts_with("search_"));
        assert_eq!(created.status, "queued");

        let fetched = db.store.get_search(&created.id)?;
        assert_eq!(
            fetched.goal,
            "Recent explainable-AI papers, methods not surveys"
        );
        assert_eq!(fetched.constraints.target_count, 20);
        assert_eq!(fetched.strategy.depth, Depth::Standard);
        assert_eq!(fetched.strategy.max_iterations, 3);
        Ok(())
    }

    #[test]
    fn run_lifecycle_sets_status_and_finish() -> StoreResult<()> {
        let db = test_db()?;
        let search = db.store.create_search(&sample_search_draft())?;
        let run = db.store.create_search_run(&search.id, "deep")?;
        assert_eq!(run.status, "queued");
        assert_eq!(run.mode, "deep");
        assert!(run.provider_set.is_some());
        assert_eq!(run.query_expansions.as_deref(), Some("[]"));

        db.store
            .set_search_run_query_expansions(&run.id, "[\"query one\"]")?;

        db.store.set_search_run_status(
            &run.id,
            SearchRunStatus::Ready,
            2,
            Some("target_reached"),
            None,
            true,
        )?;
        let updated = db.store.get_search_run(&run.id)?;
        assert_eq!(updated.status, "ready");
        assert_eq!(updated.iteration, 2);
        assert_eq!(updated.stop_reason.as_deref(), Some("target_reached"));
        assert_eq!(updated.query_expansions.as_deref(), Some("[\"query one\"]"));
        assert!(updated.finished_at.is_some());
        Ok(())
    }

    #[test]
    fn appending_candidates_stacks_only_new_ones() -> StoreResult<()> {
        let db = test_db()?;
        let search = db.store.create_search(&sample_search_draft())?;
        let run1 = db.store.create_search_run(&search.id, "deep")?;

        let added = db.store.append_new_candidates(
            &search.id,
            &run1.id,
            &[
                ranked("Alpha", Some("10.1/a"), 1),
                ranked("Beta", Some("10.1/b"), 2),
            ],
        )?;
        assert_eq!(added, 2);

        // A second run that re-finds Alpha (same DOI) and a new Gamma adds only Gamma.
        let run2 = db.store.create_search_run(&search.id, "improve")?;
        let added2 = db.store.append_new_candidates(
            &search.id,
            &run2.id,
            &[
                ranked("Alpha again", Some("10.1/a"), 1),
                ranked("Gamma", Some("10.1/g"), 2),
            ],
        )?;
        assert_eq!(added2, 1, "duplicate DOI must not stack again");

        let pool = db.store.list_search_candidates(&search.id)?;
        assert_eq!(pool.len(), 3);
        // Newest batch (run2) sorts first.
        assert_eq!(pool[0].first_seen_run_id, run2.id);
        assert_eq!(pool[0].candidate.title, "Gamma");
        assert!(pool[0].rank_signals_json.is_some());
        assert!(pool[0].provider_hits_json.is_some());
        Ok(())
    }

    #[test]
    fn list_searches_newest_first_and_status_update() -> StoreResult<()> {
        let db = test_db()?;
        let first = db.store.create_search(&sample_search_draft())?;
        let mut second_draft = sample_search_draft();
        second_draft.title = "sparse attention".to_string();
        let second = db.store.create_search(&second_draft)?;

        let listed = db.store.list_searches()?;
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].id, second.id, "newest first");

        db.store.set_search_status(
            &first.id,
            SearchRunStatus::Ready,
            Some("target_reached"),
            Some("{\"new\":3}"),
        )?;
        let reloaded = db.store.get_search(&first.id)?;
        assert_eq!(reloaded.status, "ready");
        assert_eq!(reloaded.summary.as_deref(), Some("{\"new\":3}"));
        Ok(())
    }

    #[test]
    fn marking_candidates_seen_and_saved() -> StoreResult<()> {
        let db = test_db()?;
        let search = db.store.create_search(&sample_search_draft())?;
        let run = db.store.create_search_run(&search.id, "deep")?;
        db.store.append_new_candidates(
            &search.id,
            &run.id,
            &[ranked("Alpha", Some("10.1/a"), 1)],
        )?;

        let pool = db.store.list_search_candidates(&search.id)?;
        assert!(!pool[0].seen);
        assert!(!pool[0].saved);

        db.store.mark_search_candidate_saved(&pool[0].id, true)?;
        db.store.mark_search_candidates_seen(&search.id)?;

        let pool = db.store.list_search_candidates(&search.id)?;
        assert!(pool[0].seen);
        assert!(pool[0].saved);
        Ok(())
    }
}
