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
use crate::domain::chunking::{chunk_blocks, CHUNK_VERSION};
use crate::domain::context::{ContextItem, ContextItemDraft, ContextKey, PageRects};
use crate::domain::discovery::PaperCandidate;
use crate::domain::library::{
    CiteRecord, DocumentAsset, DocumentBlock, DocumentChunk, DocumentExtraction, DocumentPage,
    DocumentSource, DocumentSpan, EmbeddingCoverage, ExtractionStructure, LibrarySnapshot, Paper,
    PaperDraft, PaperMetadataEnrichment, PaperMetadataUpdate, PaperSourceDraft, Project,
    ProjectDocument, ProjectDocumentDraft, ProjectDocumentSummary, ProjectDocumentUpdate,
    ProjectDraft, ProjectRenameDraft, Vault, VaultDraft, VaultPaper, VaultRenameDraft,
};
use crate::domain::research::{
    candidate_dedup_key, RankedCandidate, Search, SearchCandidate, SearchDraft, SearchRun,
    SearchRunStatus,
};
use crate::domain::vault_suggestion::{
    VaultSuggestion, VaultSuggestionOptions, VaultSuggestionQueryPath, VaultSuggestionRun,
    VaultSuggestionSnapshot,
};
use crate::pdf_layout::NormRect;

type StoreResult<T> = Result<T, String>;

/// Width of the `vec0` embedding column, tied to the active model rather than
/// restated.
///
/// If these drifted apart, `index_chunk_vector` would skip every chunk on a
/// dimension mismatch and semantic search would return nothing forever, with
/// only a log line to explain it — so the two are the same constant, not two
/// constants that agree today.
const VECTOR_DIMENSIONS: usize = crate::services::embedding::MODEL_DIMENSIONS;

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
        migrate_vaults_to_projects(&mut conn)?;
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
        // RFC 0061: color became nullable and a note field was added. Existing
        // DBs predate the `note` column; add it if missing (the `let _` swallows
        // the "duplicate column" error on already-migrated DBs).
        let _ = conn.execute("alter table highlights add column note text", []);
        // RFC 0072: ...and relax the NOT NULL that same RFC left behind on every
        // pre-existing vault. Must run after the `note` column exists — the
        // rebuild copies it by name.
        relax_highlight_color_not_null(&conn)?;
        // RFC 0078 on a database created by RFC 0077, where the table exists
        // without `origin`. Existing rows are the user's by definition.
        let _ = conn.execute(
            "alter table chat_context_items add column origin text not null default 'user'",
            [],
        );

        self.migrate_threads_to_highlights()?;
        self.migrate_notes_into_highlight_field()?;
        realign_chunk_fts_rowids(&conn)?;

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

    /// Citation fields for every paper in a vault, for BibTeX export (RFC 0070).
    /// Reads only the columns an entry needs, so it never touches the chat tables
    /// that `read_papers` consults for computed counts. Order is unspecified;
    /// the exporter imposes its own stable ordering.
    pub fn cite_records_for_vault(&self, vault_id: &str) -> StoreResult<Vec<CiteRecord>> {
        let conn = self.open_connection()?;
        read_cite_records_for_vault(&conn, vault_id)
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

    /// Extractions left mid-flight by a crash. Status-based, *not* version-based
    /// — see `outdated_document_extractions` for the version sweep.
    pub fn stale_document_extractions(
        &self,
        extractor: &str,
    ) -> StoreResult<Vec<DocumentExtraction>> {
        let conn = self.open_connection()?;
        read_document_extractions_by_status(&conn, extractor, "extracting")
    }

    /// Ready extractions produced by an older version of the extractor.
    ///
    /// Without this, bumping `EXTRACTOR_VERSION` does nothing to a library that
    /// already has extractions: `stale_document_extractions` only looks at
    /// status, and `cached_pdf_sources_without_ready_extraction` skips any
    /// source that has *a* ready extraction regardless of which version made
    /// it. Only brand-new papers would get the new extractor, and the old ones
    /// would silently keep their old structure forever (RFC 0075 R1).
    pub fn outdated_document_extractions(
        &self,
        extractor: &str,
        current_version: &str,
    ) -> StoreResult<Vec<DocumentExtraction>> {
        let conn = self.open_connection()?;
        let mut stmt = conn
            .prepare(
                "
                select id, paper_id, source_id, extractor, extractor_version,
                       annotation_source_id, status, error, created_at, updated_at
                from document_extractions
                where extractor = ?1
                  and status = 'ready'
                  and extractor_version <> ?2
                order by updated_at asc, id
                ",
            )
            .map_err(|error| error.to_string())?;
        let rows = stmt
            .query_map(params![extractor, current_version], |row| {
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

    /// Persist an extraction's structure and mark it ready.
    ///
    /// Chunking happens here, inside the same transaction (RFC 0075 R3): an
    /// extraction is therefore never `ready` without its chunks. The
    /// alternative — a second queue with its own status column and recovery
    /// path — buys nothing, because chunking needs no model and no network.
    pub fn finish_document_extraction(
        &self,
        extraction_id: &str,
        pages: &[DocumentPage],
        blocks: &[DocumentBlock],
        spans: &[DocumentSpan],
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

        for span in spans {
            tx.execute(
                "
                insert into document_spans (
                  id, paper_id, source_id, extraction_id, block_id, page_index,
                  text, source_start, source_end, bbox_json
                )
                values (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
                ",
                params![
                    span.id,
                    span.paper_id,
                    span.source_id,
                    span.extraction_id,
                    span.block_id,
                    span.page_index,
                    span.text,
                    span.source_start,
                    span.source_end,
                    span.bbox_json,
                ],
            )
            .map_err(|error| error.to_string())?;
        }

        insert_chunks(&tx, &chunk_blocks(blocks))?;

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

    /// Blocks and spans for one extraction (RFC 0075 R2).
    pub fn extraction_structure(&self, extraction_id: &str) -> StoreResult<ExtractionStructure> {
        let conn = self.open_connection()?;
        Ok(ExtractionStructure {
            blocks: read_document_blocks_for_extraction(&conn, extraction_id)?,
            spans: read_document_spans_for_extraction(&conn, extraction_id)?,
        })
    }

    pub fn chunks_for_extraction(&self, extraction_id: &str) -> StoreResult<Vec<DocumentChunk>> {
        let conn = self.open_connection()?;
        read_chunks(&conn, "where c.extraction_id = ?1", params![extraction_id])
    }

    /// Extractions that are ready but whose chunks predate the current chunker
    /// (RFC 0075 R6). Covers a `CHUNK_VERSION` bump without forcing a full
    /// re-extraction, and heals anything a crash left half-written.
    pub fn extractions_needing_rechunk(&self) -> StoreResult<Vec<String>> {
        let conn = self.open_connection()?;
        let mut stmt = conn
            .prepare(
                "
                select e.id
                from document_extractions e
                where e.status = 'ready'
                  and not exists (
                    select 1 from document_chunks c
                    where c.extraction_id = e.id and c.chunk_version = ?1
                  )
                order by e.id
                ",
            )
            .map_err(|error| error.to_string())?;
        let rows = stmt
            .query_map(params![CHUNK_VERSION], |row| row.get::<_, String>(0))
            .map_err(|error| error.to_string())?;
        collect_rows(rows)
    }

    /// Re-chunk one extraction from its stored blocks, replacing whatever
    /// chunks it had. Blocks are canonical; chunks are disposable.
    pub fn rechunk_extraction(&self, extraction_id: &str) -> StoreResult<usize> {
        let mut conn = self.open_connection()?;
        let blocks = read_document_blocks_for_extraction(&conn, extraction_id)?;
        let chunks = chunk_blocks(&blocks);

        let tx = conn.transaction().map_err(|error| error.to_string())?;
        clear_extraction_chunks(&tx, extraction_id)?;
        insert_chunks(&tx, &chunks)?;
        tx.commit().map_err(|error| error.to_string())?;
        Ok(chunks.len())
    }

    /// Chunks with no embedding at the current model and chunk version.
    ///
    /// Absence of a row *is* the "not yet embedded" state — there is no status
    /// column and nothing to recover, so this query is the entire work queue.
    pub fn chunks_missing_embedding(
        &self,
        model: &str,
        model_version: &str,
        limit: i64,
    ) -> StoreResult<Vec<DocumentChunk>> {
        let conn = self.open_connection()?;
        read_chunks(
            &conn,
            "
            where c.chunk_version = ?1
              and not exists (
                select 1 from document_chunk_embeddings e
                where e.chunk_id = c.id
                  and e.model = ?2
                  and e.model_version = ?3
                  and e.chunk_version = c.chunk_version
              )
            limit ?4
            ",
            params![CHUNK_VERSION, model, model_version, limit],
        )
    }

    pub fn save_chunk_embedding(
        &self,
        chunk_id: &str,
        model: &str,
        model_version: &str,
        chunk_version: i32,
        embedding: &[f32],
    ) -> StoreResult<()> {
        // Little-endian f32, which is byte-for-byte what sqlite-vec's vec0
        // consumes. Phase 2 builds its index straight from these blobs rather
        // than re-embedding anything (RFC 0075).
        let mut blob = Vec::with_capacity(embedding.len() * 4);
        for value in embedding {
            blob.extend_from_slice(&value.to_le_bytes());
        }

        let conn = self.open_connection()?;
        conn.execute(
            "
            insert or replace into document_chunk_embeddings (
              chunk_id, model, model_version, dimensions, chunk_version,
              embedding, created_at
            )
            values (?1, ?2, ?3, ?4, ?5, ?6, datetime('now'))
            ",
            params![
                chunk_id,
                model,
                model_version,
                embedding.len() as i64,
                chunk_version,
                blob,
            ],
        )
        .map_err(|error| error.to_string())?;

        index_chunk_vector(&conn, chunk_id, &blob, embedding.len())?;
        Ok(())
    }

    /// Index every embedding that has no vector yet (RFC 0076).
    ///
    /// The startup backfill, and the repair path if the index is ever dropped.
    /// Mirrors RFC 0075 R6: the work list is a query, so running it twice is a
    /// no-op and a crash needs no recovery. Costs no re-embedding — the vectors
    /// already exist, this only inserts them into `vec0`.
    pub fn index_missing_chunk_vectors(&self) -> StoreResult<usize> {
        let conn = self.open_connection()?;
        if !sqlite_vec_available(&conn) {
            return Ok(0);
        }

        // Scoped so the statement — and with it SQLite's read transaction — is
        // dropped before the writes below. A live statement on the same
        // connection keeps the read open, and the first insert then fails with
        // "database is locked".
        let pending: Vec<(String, Vec<u8>, i64)> = {
            let mut stmt = conn
                .prepare(
                    "
                    select e.chunk_id, e.embedding, e.dimensions
                    from document_chunk_embeddings e
                    where not exists (
                      select 1 from document_chunk_vectors v where v.chunk_id = e.chunk_id
                    )
                    ",
                )
                .map_err(|error| error.to_string())?;
            let rows = stmt
                .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
                .map_err(|error| error.to_string())?;
            collect_rows(rows)?
        };

        let mut indexed = 0;
        for (chunk_id, blob, dimensions) in pending {
            index_chunk_vector(&conn, &chunk_id, &blob, dimensions as usize)?;
            indexed += 1;
        }
        Ok(indexed)
    }

    /// KNN over `paper_ids`, nearest first. Returns chunk ids and L2 distance.
    ///
    /// Empty when `vec0` is unavailable or nothing is indexed — the caller
    /// distinguishes those cases through `embedding_coverage`, because "no
    /// index" and "no matches" mean very different things to a user.
    pub fn semantic_chunk_ranking(
        &self,
        paper_ids: &[String],
        query_vector: &[f32],
        limit: i64,
    ) -> StoreResult<Vec<(String, f64)>> {
        if paper_ids.is_empty() || query_vector.is_empty() {
            return Ok(Vec::new());
        }

        let conn = self.open_connection()?;
        if !sqlite_vec_available(&conn) {
            return Ok(Vec::new());
        }

        let blob: Vec<u8> = query_vector.iter().flat_map(|v| v.to_le_bytes()).collect();

        // `k` in a partitioned vec0 query is **per-partition**, not a total —
        // see `vec0_partition_filter_accepts_a_set_of_papers`. Passing the
        // caller's budget straight through would fetch it once per paper, so a
        // whole-library search would pull tens of thousands of rows and still
        // look correct while missing its latency budget by an order of
        // magnitude.
        //
        // Dividing keeps the row count near the budget. Once the scope has more
        // papers than the budget has slots, every paper contributes its single
        // best chunk — which is the right degradation: broad-but-shallow beats
        // deep-in-the-first-few-papers when someone searches their whole
        // library.
        let per_partition_k = (limit as usize).div_ceil(paper_ids.len()).max(1) as i64;

        // The partition filter takes a set, which is what lets one query serve
        // any scope — verified by `vec0_partition_filter_accepts_a_set_of_papers`.
        let sql = format!(
            "
            select chunk_id, distance
            from document_chunk_vectors
            where embedding match ?
              and k = ?
              and paper_id in ({})
            order by distance
            ",
            placeholders(paper_ids.len())
        );

        let mut bindings: Vec<Box<dyn rusqlite::ToSql>> = vec![
            Box::new(blob) as Box<dyn rusqlite::ToSql>,
            Box::new(per_partition_k),
        ];
        for paper_id in paper_ids {
            bindings.push(Box::new(paper_id.clone()));
        }

        let mut stmt = conn.prepare(&sql).map_err(|error| error.to_string())?;
        let rows = stmt
            .query_map(rusqlite::params_from_iter(bindings.iter()), |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, f64>(1)?))
            })
            .map_err(|error| error.to_string())?;

        // Per-partition k means the union can exceed the caller's budget; the
        // ORDER BY is within each partition's result set, so re-sort globally
        // and truncate to what was actually asked for.
        let mut ranked: Vec<(String, f64)> = collect_rows(rows)?;
        ranked.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        ranked.truncate(limit as usize);
        Ok(ranked)
    }

    /// How much of a paper is embedded. A missing embedding is a retrieval hole
    /// the user cannot see on their own, so it is counted rather than swallowed
    /// (RFC 0075 R5).
    pub fn embedding_coverage(
        &self,
        paper_id: &str,
        model: &str,
        model_version: &str,
    ) -> StoreResult<EmbeddingCoverage> {
        let conn = self.open_connection()?;
        conn.query_row(
            "
            select
              count(*),
              coalesce(sum(case when e.chunk_id is null then 0 else 1 end), 0)
            from document_chunks c
            left join document_chunk_embeddings e
              on e.chunk_id = c.id
             and e.model = ?2
             and e.model_version = ?3
             and e.chunk_version = c.chunk_version
            where c.paper_id = ?1
            ",
            params![paper_id, model, model_version],
            |row| {
                Ok(EmbeddingCoverage {
                    chunks: row.get(0)?,
                    embedded: row.get(1)?,
                })
            },
        )
        .map_err(|error| error.to_string())
    }

    /// Resolve a search scope: `paper_ids` **AND** `vault_ids` (RFC 0076).
    ///
    /// Each list is OR-ed within itself and intersected across the two, with an
    /// empty list meaning "unconstrained on this dimension":
    ///
    /// | papers | vaults | result |
    /// |--------|--------|--------|
    /// | `[]`   | `[]`   | the whole library — global search |
    /// | `[p]`  | `[]`   | paper `p` |
    /// | `[]`   | `[v]`  | every paper in `v` |
    /// | `[p]`  | `[v]`  | `p` if `p ∈ v`, otherwise nothing |
    ///
    /// This composes: the reader passes a paper and its vault, the vault view
    /// passes a vault, global search passes neither, and an agent can express
    /// any subset — all through one call with no new endpoint.
    ///
    /// An empty result is a legitimate answer, not an error. Everything goes
    /// through the `papers` table, so an id that does not exist drops out here
    /// rather than producing a query against nothing.
    pub fn resolve_search_scope(
        &self,
        paper_ids: &[String],
        vault_ids: &[String],
    ) -> StoreResult<Vec<String>> {
        let conn = self.open_connection()?;

        let mut clauses: Vec<String> = Vec::new();
        let mut bindings: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

        if !paper_ids.is_empty() {
            clauses.push(format!("p.id in ({})", placeholders(paper_ids.len())));
            for paper_id in paper_ids {
                bindings.push(Box::new(paper_id.clone()));
            }
        }
        if !vault_ids.is_empty() {
            clauses.push(format!(
                "exists (select 1 from vault_papers vp
                         where vp.paper_id = p.id and vp.vault_id in ({}))",
                placeholders(vault_ids.len())
            ));
            for vault_id in vault_ids {
                bindings.push(Box::new(vault_id.clone()));
            }
        }

        let where_clause = if clauses.is_empty() {
            String::new()
        } else {
            format!("where {}", clauses.join(" and "))
        };
        let sql = format!("select p.id from papers p {where_clause} order by p.id");

        let mut stmt = conn.prepare(&sql).map_err(|error| error.to_string())?;
        let rows = stmt
            .query_map(rusqlite::params_from_iter(bindings.iter()), |row| {
                row.get::<_, String>(0)
            })
            .map_err(|error| error.to_string())?;
        collect_rows(rows)
    }

    /// BM25-ranked chunk ids for a query, scoped to `paper_ids`.
    ///
    /// Returns ids and scores rather than whole chunks so the caller can fuse
    /// this ranking with a semantic one before paying to load any text. FTS5
    /// returns BM25 as a *negative* number where more negative is better; it is
    /// negated here so every score in this module means "higher is better".
    pub fn lexical_chunk_ranking(
        &self,
        paper_ids: &[String],
        query: &str,
        limit: i64,
    ) -> StoreResult<Vec<(String, f64)>> {
        let trimmed = query.trim();
        if trimmed.is_empty() || paper_ids.is_empty() {
            return Ok(Vec::new());
        }

        let conn = self.open_connection()?;
        let sql = format!(
            "
            select c.id, bm25(document_chunks_fts)
            from document_chunks c
            join document_chunks_fts f on f.chunk_id = c.id
            where c.paper_id in ({})
              and document_chunks_fts match ?
            order by bm25(document_chunks_fts)
            limit ?
            ",
            placeholders(paper_ids.len())
        );

        let mut bindings: Vec<Box<dyn rusqlite::ToSql>> = paper_ids
            .iter()
            .map(|id| Box::new(id.clone()) as Box<dyn rusqlite::ToSql>)
            .collect();
        bindings.push(Box::new(fts_match_query(trimmed)));
        bindings.push(Box::new(limit));

        let mut stmt = conn.prepare(&sql).map_err(|error| error.to_string())?;
        let rows = stmt
            .query_map(rusqlite::params_from_iter(bindings.iter()), |row| {
                Ok((row.get::<_, String>(0)?, -row.get::<_, f64>(1)?))
            })
            .map_err(|error| error.to_string())?;
        collect_rows(rows)
    }

    pub fn chunks_by_ids(&self, ids: &[String]) -> StoreResult<Vec<DocumentChunk>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let conn = self.open_connection()?;
        let tail = format!("where c.id in ({})", placeholders(ids.len()));
        read_chunks(&conn, &tail, rusqlite::params_from_iter(ids))
    }

    /// Lexical retrieval returning whole chunks. A convenience over
    /// `lexical_chunk_ranking` for callers that want text and not scores.
    pub fn search_chunks_lexical(
        &self,
        paper_id: &str,
        query: &str,
        limit: i64,
    ) -> StoreResult<Vec<DocumentChunk>> {
        let ranking = self.lexical_chunk_ranking(&[paper_id.to_string()], query, limit)?;
        let ids: Vec<String> = ranking.iter().map(|(id, _)| id.clone()).collect();
        let mut chunks = self.chunks_by_ids(&ids)?;
        // `chunks_by_ids` returns rows in table order; restore rank order.
        chunks.sort_by_key(|chunk| {
            ids.iter()
                .position(|id| *id == chunk.id)
                .unwrap_or(usize::MAX)
        });
        Ok(chunks)
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
        self.add_local_source_to_vault(
            paper,
            vault_id,
            source_id,
            source_url,
            local_path,
            "pdf",
            "local_import",
        )
    }

    /// Persist a URL-imported web page as a durable vault source (RFC 0065). The
    /// `local_path` is the sanitized `source.html` snapshot; the reader serves it
    /// back via [`Self::read_html_snapshot`].
    pub fn add_local_html_to_vault(
        &self,
        paper: &PaperDraft,
        vault_id: &str,
        source_id: &str,
        source_url: &str,
        local_path: &str,
    ) -> StoreResult<()> {
        self.add_local_source_to_vault(
            paper,
            vault_id,
            source_id,
            source_url,
            local_path,
            "html",
            "html_import",
        )
    }

    /// Shared upsert for a locally-imported source (PDF file or saved web page):
    /// create/refresh the paper, register a `cached` document source of the given
    /// kind, and add the paper to the vault. Idempotent on all three (conflict
    /// clauses), so a re-import is a no-op.
    fn add_local_source_to_vault(
        &self,
        paper: &PaperDraft,
        vault_id: &str,
        source_id: &str,
        source_url: &str,
        local_path: &str,
        source_kind: &str,
        acquisition_method: &str,
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
            values (?1, ?2, ?5, ?3, null, ?3, ?6, ?4, 'cached', null, datetime('now'), datetime('now'))
            on conflict(id) do update set
              source_url = excluded.source_url,
              final_url = excluded.final_url,
              acquisition_method = excluded.acquisition_method,
              local_path = excluded.local_path,
              status = 'cached',
              error = null,
              updated_at = datetime('now')
            ",
            params![source_id, paper.id, source_url, local_path, source_kind, acquisition_method],
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

    /// Read a persisted web-page snapshot's sanitized HTML by source id (RFC
    /// 0065): resolve the source's stored `local_path` and read it. The reader's
    /// `get_reader_html` delegates here for durable `html:` vault sources.
    pub fn read_html_snapshot(&self, source_id: &str) -> StoreResult<String> {
        // `get_document_source` surfaces a raw "no rows" error for an unknown id;
        // give the reader a friendly message instead.
        let source = self
            .get_document_source(source_id)
            .map_err(|_| format!("Saved web page not found: {source_id}"))?;
        let path = source
            .local_path
            .ok_or_else(|| format!("Saved web page has no snapshot: {source_id}"))?;
        std::fs::read_to_string(&path)
            .map_err(|error| format!("Saved web page is missing: {source_id} ({error})"))
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
        self.create_project_with_vault(&ProjectDraft { title, goal: None }, Some(&normalized))
            .map_err(|error| {
                if error.starts_with("Project already exists:") {
                    format!("Vault path already exists: {normalized}")
                } else {
                    error
                }
            })
    }

    /// Atomically creates a Project and its one owned Vault.
    pub fn create_project(&self, draft: &ProjectDraft) -> StoreResult<LibrarySnapshot> {
        self.create_project_with_vault(draft, None)
    }

    fn create_project_with_vault(
        &self,
        draft: &ProjectDraft,
        requested_path: Option<&str>,
    ) -> StoreResult<LibrarySnapshot> {
        let title = normalize_project_title(&draft.title)?;
        let normalized = match requested_path {
            Some(path) => normalize_vault_path(path)?,
            None => normalize_vault_path(&title)?,
        };
        let vault_title = vault_title_from_path(&normalized)?;
        let vault_id = vault_id_from_path(&normalized)?;
        let project_id = project_id_for_vault(&vault_id);
        let mut conn = self.open_connection()?;
        let tx = conn.transaction().map_err(|error| error.to_string())?;

        tx.execute(
            "insert into projects (id, title, goal, created_at, updated_at)
             values (?1, ?2, ?3, datetime('now'), datetime('now'))",
            params![project_id, title, draft.goal],
        )
        .map_err(|error| project_create_error(error, &title, &normalized))?;
        tx.execute(
            "insert into vaults (id, project_id, title, path, created_at, updated_at)
             values (?1, ?2, ?3, ?4, datetime('now'), datetime('now'))",
            params![vault_id, project_id, vault_title, normalized],
        )
        .map_err(|error| project_create_error(error, &title, &normalized))?;

        tx.commit().map_err(|error| error.to_string())?;
        self.get_library()
    }

    /// Renames only the Project; its bibliography path remains stable.
    pub fn rename_project(&self, draft: &ProjectRenameDraft) -> StoreResult<LibrarySnapshot> {
        let title = normalize_project_title(&draft.title)?;
        let conn = self.open_connection()?;
        let updated = conn
            .execute(
                "update projects set title = ?1, updated_at = datetime('now') where id = ?2",
                params![title, draft.id],
            )
            .map_err(|error| error.to_string())?;
        if updated == 0 {
            return Err(format!("Project not found: {}", draft.id));
        }
        self.get_library()
    }

    /// Deletes a Project, its Vault, and Papers with no remaining membership.
    pub fn delete_project(&self, project_id: &str) -> StoreResult<LibrarySnapshot> {
        if project_id.trim().is_empty() {
            return Err("Project id cannot be empty".to_string());
        }

        let mut conn = self.open_connection()?;
        let tx = conn.transaction().map_err(|error| error.to_string())?;
        let deleted = tx
            .execute("delete from projects where id = ?1", params![project_id])
            .map_err(|error| error.to_string())?;
        if deleted == 0 {
            return Err(format!("Project not found: {project_id}"));
        }
        delete_unowned_papers(&tx)?;
        tx.commit().map_err(|error| error.to_string())?;
        self.get_library()
    }

    /// Creates a Markdown document inside an existing Project.
    pub fn create_project_document(
        &self,
        draft: &ProjectDocumentDraft,
    ) -> StoreResult<ProjectDocument> {
        let title = normalize_document_title(&draft.title)?;
        let id = timestamped_id("project_document")?;
        let conn = self.open_connection()?;
        let inserted = conn
            .execute(
                "insert into project_documents (
                   id, project_id, title, format, content, harness_writable,
                   created_from_run_id, created_from_state_revision, created_at, updated_at
                 )
                 select ?1, id, ?3, 'markdown', ?4, 0, null, null,
                        datetime('now'), datetime('now')
                 from projects where id = ?2",
                params![id, draft.project_id, title, draft.content],
            )
            .map_err(|error| error.to_string())?;
        if inserted == 0 {
            return Err(format!("Project not found: {}", draft.project_id));
        }
        read_project_document(&conn, &id)?
            .ok_or_else(|| format!("Project document disappeared after creation: {id}"))
    }

    /// Loads one Project document including its Markdown content.
    pub fn get_project_document(&self, document_id: &str) -> StoreResult<ProjectDocument> {
        let conn = self.open_connection()?;
        read_project_document(&conn, document_id)?
            .ok_or_else(|| format!("Project document not found: {document_id}"))
    }

    /// Replaces the editable fields of one Project document.
    pub fn update_project_document(
        &self,
        update: &ProjectDocumentUpdate,
    ) -> StoreResult<ProjectDocument> {
        let title = normalize_document_title(&update.title)?;
        let conn = self.open_connection()?;
        let updated = conn
            .execute(
                "update project_documents
                 set title = ?1, content = ?2, harness_writable = ?3,
                     updated_at = datetime('now')
                 where id = ?4",
                params![title, update.content, update.harness_writable, update.id],
            )
            .map_err(|error| error.to_string())?;
        if updated == 0 {
            return Err(format!("Project document not found: {}", update.id));
        }
        read_project_document(&conn, &update.id)?
            .ok_or_else(|| format!("Project document disappeared after update: {}", update.id))
    }

    /// Deletes one Project document without affecting its Project.
    pub fn delete_project_document(&self, document_id: &str) -> StoreResult<()> {
        let conn = self.open_connection()?;
        let deleted = conn
            .execute(
                "delete from project_documents where id = ?1",
                params![document_id],
            )
            .map_err(|error| error.to_string())?;
        if deleted == 0 {
            return Err(format!("Project document not found: {document_id}"));
        }
        Ok(())
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

        let conn = self.open_connection()?;
        let project_id = conn
            .query_row(
                "select project_id from vaults where id = ?1",
                params![vault_id],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|error| error.to_string())?
            .ok_or_else(|| format!("Vault not found: {vault_id}"))?;
        self.delete_project(&project_id)
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

        // RFC 0079 R6.2: `highlights` declares no FK to papers at all, so its
        // rows survived every paper deletion. Explicit here rather than a table
        // rebuild — the FK is the better fix, this is the safe one.
        tx.execute(
            "delete from highlights where paper_id = ?1",
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

    // ---- Chat context items (RFC 0077) ----

    /// Append a persistent context item, assigning the next `position`.
    ///
    /// Position is derived inside the transaction so two concurrent adds cannot
    /// collide on the unique `(thread_id, position)` index.
    pub fn insert_context_item(
        &self,
        thread_id: &str,
        draft: &ContextItemDraft,
    ) -> StoreResult<ContextItem> {
        let id = timestamped_id("ctx")?;
        let mut conn = self.open_connection()?;
        let tx = conn.transaction().map_err(|error| error.to_string())?;
        insert_context_item_tx(&tx, &id, thread_id, draft)?;
        tx.commit().map_err(|error| error.to_string())?;

        let conn = self.open_connection()?;
        read_context_item(&conn, &id)
    }

    /// Every persistent item for a thread, in emission order.
    pub fn context_items(&self, thread_id: &str) -> StoreResult<Vec<ContextItem>> {
        let conn = self.open_connection()?;
        read_context_items(&conn, thread_id)
    }

    /// Delete one item by whichever key the caller holds. Returns whether a row
    /// was removed — a delete of something already gone is not an error.
    pub fn delete_context_item(&self, thread_id: &str, key: &ContextKey) -> StoreResult<bool> {
        let conn = self.open_connection()?;
        let removed = match key {
            ContextKey::Item(id) => conn.execute(
                "delete from chat_context_items where thread_id = ?1 and id = ?2",
                params![thread_id, id],
            ),
            ContextKey::Chunk(chunk_id) => conn.execute(
                "delete from chat_context_items where thread_id = ?1 and chunk_id = ?2",
                params![thread_id, chunk_id],
            ),
        }
        .map_err(|error| error.to_string())?;
        Ok(removed > 0)
    }

    /// Replace the chunk items listed in `superseded` with one summary item.
    ///
    /// One transaction: a compaction either lands whole or leaves the thread
    /// untouched. `superseded` is captured *before* the summarization call, so
    /// items added while the model was working survive.
    pub fn compact_context_items(
        &self,
        thread_id: &str,
        summary: &ContextItemDraft,
        superseded: &[String],
    ) -> StoreResult<ContextItem> {
        let id = timestamped_id("ctx")?;
        let mut conn = self.open_connection()?;
        let tx = conn.transaction().map_err(|error| error.to_string())?;
        for item_id in superseded {
            tx.execute(
                "delete from chat_context_items where thread_id = ?1 and id = ?2",
                params![thread_id, item_id],
            )
            .map_err(|error| error.to_string())?;
        }
        insert_context_item_tx(&tx, &id, thread_id, summary)?;
        tx.commit().map_err(|error| error.to_string())?;

        let conn = self.open_connection()?;
        read_context_item(&conn, &id)
    }

    /// Chunks of `paper_id` overlapping `[source_start, source_end)`.
    ///
    /// The durable-anchor path: after a rechunk the original `chunk_id` is gone,
    /// but the character range into the extraction still names the same passage.
    /// May return more text than was originally added if boundaries moved — the
    /// passage is preserved, its packaging is not.
    pub fn chunks_overlapping(
        &self,
        paper_id: &str,
        source_start: i64,
        source_end: i64,
    ) -> StoreResult<Vec<DocumentChunk>> {
        let conn = self.open_connection()?;
        read_chunks(
            &conn,
            "where c.paper_id = ?1 and c.source_start < ?2 and c.source_end > ?3
             order by c.chunk_index",
            params![paper_id, source_end, source_start],
        )
    }

    /// Whether a paper has any retrievable chunks (RFC 0079 R5.4).
    ///
    /// The discriminator between "nothing matched the question" and "this paper
    /// was never indexed" — two very different things to tell a reader whose
    /// answer came back without a citation.
    pub fn paper_has_chunks(&self, paper_id: &str) -> StoreResult<bool> {
        let conn = self.open_connection()?;
        conn.query_row(
            "select exists(select 1 from document_chunks where paper_id = ?1)",
            params![paper_id],
            |row| row.get::<_, i64>(0),
        )
        .map(|exists| exists == 1)
        .map_err(|error| error.to_string())
    }

    /// Rectangles covering a chunk, grouped by page, in reading order.
    ///
    /// Block-level: a chunk resolves to whole blocks, so a citation jump lands
    /// on the paragraph rather than the sentence. Blocks without geometry
    /// (`bbox_json` null, or written before RFC 0075) contribute nothing rather
    /// than failing the lookup.
    pub fn chunk_rects(&self, chunk_id: &str) -> StoreResult<Vec<PageRects>> {
        let conn = self.open_connection()?;
        let mut statement = conn
            .prepare(
                "select b.page_index, b.bbox_json
                 from document_chunk_blocks cb
                 join document_blocks b on b.id = cb.block_id
                 where cb.chunk_id = ?1
                 order by cb.ordinal",
            )
            .map_err(|error| error.to_string())?;

        let rows = statement
            .query_map(params![chunk_id], |row| {
                Ok((row.get::<_, i32>(0)?, row.get::<_, Option<String>>(1)?))
            })
            .map_err(|error| error.to_string())?;

        // Grouped in first-seen page order, which for a chunk is reading order.
        let mut pages: Vec<PageRects> = Vec::new();
        for row in rows {
            let (page_index, bbox_json) = row.map_err(|error| error.to_string())?;
            let Some(bbox_json) = bbox_json else { continue };
            let Ok(rect) = serde_json::from_str::<NormRect>(&bbox_json) else {
                continue;
            };
            match pages.iter_mut().find(|page| page.page_index == page_index) {
                Some(page) => page.rects.push(rect),
                None => pages.push(PageRects {
                    page_index,
                    rects: vec![rect],
                }),
            }
        }
        Ok(pages)
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
        self.persist_anchored_turn_with_creation(
            scope_kind, scope_id, anchor, question, answer, false,
        )
            .map(|write| write.view)
    }

    pub fn persist_anchored_turn_with_creation(
        &self,
        scope_kind: &str,
        scope_id: &str,
        anchor: &ThreadAnchor,
        question: &ChatEntryDraft,
        answer: &ChatEntryDraft,
        force_new_thread: bool,
    ) -> StoreResult<AnchoredThreadWrite> {
        let mut conn = self.open_connection()?;
        let tx = conn.transaction().map_err(|error| error.to_string())?;
        let thread =
            find_or_create_thread_id_with(&tx, scope_kind, scope_id, anchor, force_new_thread)?;
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
        color: Option<crate::domain::highlight::HighlightColor>,
        label: Option<&str>,
        author: &crate::domain::highlight::HighlightAuthor,
    ) -> StoreResult<crate::domain::highlight::Highlight> {
        use crate::domain::highlight::Locator;

        let conn = self.open_connection()?;
        let id = timestamped_id("hl")?;
        // A null color = an annotated passage with no color highlight (RFC 0061).
        let color_str: Option<String> = color.map(highlight_color_str);
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
            // RFC 0074: a point rides in the existing columns — a zero-size rect
            // carries x/y — so sticky notes need no schema migration.
            Locator::PdfPoint {
                page_index, x, y, ..
            } => (
                None,
                None,
                Some(*page_index),
                Some(point_rects_json(*x, *y)),
            ),
            Locator::TextPoint { offset, .. } => (Some(*offset), Some(*offset), None, None),
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
            // A sticky note is idempotent on its exact placement (RFC 0074);
            // two notes a pixel apart are deliberately two notes.
            Locator::PdfPoint {
                source_id,
                page_index,
                x,
                y,
            } => conn
                .query_row(
                    "select id from highlights
                     where paper_id = ?1 and source_id = ?2 and locator_kind = 'pdf_point'
                       and page_index = ?3 and rects_json = ?4
                     limit 1",
                    params![paper_id, source_id, page_index, point_rects_json(*x, *y)],
                    |row| row.get(0),
                )
                .optional()
                .map_err(|error| error.to_string())?,
            Locator::TextPoint { source_id, offset } => conn
                .query_row(
                    "select id from highlights
                     where paper_id = ?1 and source_id = ?2 and locator_kind = 'text_point'
                       and start_offset = ?3
                     limit 1",
                    params![paper_id, source_id, offset],
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
    /// Read one highlight by id. `None` when it is already gone — a double
    /// delete is a race, not an error.
    pub fn get_highlight(
        &self,
        id: &str,
    ) -> StoreResult<Option<crate::domain::highlight::Highlight>> {
        let conn = self.open_connection()?;
        conn.query_row(
            "
            select id, paper_id, source_id, locator_kind, start_offset, end_offset,
                   page_index, rects_json, excerpt, color, label, author_kind,
                   author_model, created_at, updated_at, note
            from highlights
            where id = ?1
            ",
            params![id],
            highlight_from_row,
        )
        .optional()
        .map_err(|error| error.to_string())
    }

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
                       author_model, created_at, updated_at, note
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

    /// List a paper's highlights authored by the given author kind
    /// (`"user"` or `"agent"`), oldest first.
    pub fn list_highlights_by_author(
        &self,
        paper_id: &str,
        author_kind: &str,
    ) -> StoreResult<Vec<crate::domain::highlight::Highlight>> {
        let conn = self.open_connection()?;
        let mut stmt = conn
            .prepare(
                "
                select id, paper_id, source_id, locator_kind, start_offset, end_offset,
                       page_index, rects_json, excerpt, color, label, author_kind,
                       author_model, created_at, updated_at, note
                from highlights
                where paper_id = ?1 and author_kind = ?2
                order by created_at asc
                ",
            )
            .map_err(|error| error.to_string())?;
        let rows = stmt
            .query_map(params![paper_id, author_kind], highlight_from_row)
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

    /// Set or clear a passage's note (RFC 0061) — the passage's own text,
    /// distinct from its conversation thread. An empty/blank note clears it.
    pub fn set_highlight_note(&self, id: &str, note: Option<&str>) -> StoreResult<()> {
        let note = note.map(str::trim).filter(|value| !value.is_empty());
        let conn = self.open_connection()?;
        conn.execute(
            "update highlights set note = ?2, updated_at = datetime('now') where id = ?1",
            params![id, note],
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

    /// Delete an annotation and everything anchored to it (RFC 0079 R1.3).
    ///
    /// `remove_highlight` deletes the mark and leaves the passage's thread
    /// alive — and since the annotations panel derives every row from
    /// `highlights`, that thread renders nowhere. It was not deleted, just made
    /// unreachable. This removes both, in one transaction.
    ///
    /// Two ways a thread is found. `chat_threads.highlight_id` is the explicit
    /// link, set by RFC 0058's backfill; threads created since are matched on
    /// their anchor columns instead, which is the same comparison
    /// `find_highlight_by_locator` makes in the other direction. Point-anchored
    /// stickies never have a thread, so they only ever delete a highlight.
    ///
    /// Returns how many threads went with it, so the caller can tell the reader
    /// what it cost them.
    pub fn delete_annotation(&self, id: &str) -> StoreResult<usize> {
        use crate::domain::highlight::Locator;

        let Some(highlight) = self.get_highlight(id)? else {
            return Err(format!("Highlight not found: {id}"));
        };

        let mut conn = self.open_connection()?;
        let tx = conn.transaction().map_err(|error| error.to_string())?;

        let mut threads = tx
            .execute(
                "delete from chat_threads where highlight_id = ?1",
                params![id],
            )
            .map_err(|error| error.to_string())?;

        // Legacy and current threads with a null `highlight_id`: match the
        // anchor the thread was created with. Ranges only — a sticky note's
        // point locator has no thread shape to match.
        threads += match &highlight.locator {
            Locator::TextOffset {
                source_id,
                start_offset,
                end_offset,
            } => tx
                .execute(
                    "delete from chat_threads
                     where highlight_id is null and scope_kind = 'paper' and scope_id = ?1
                       and anchor_kind = 'text_offset' and source_id = ?2
                       and start_offset = ?3 and end_offset = ?4",
                    params![highlight.paper_id, source_id, start_offset, end_offset],
                )
                .map_err(|error| error.to_string())?,
            Locator::PdfRect {
                source_id,
                page_index,
                rects_json,
            } => tx
                .execute(
                    "delete from chat_threads
                     where highlight_id is null and scope_kind = 'paper' and scope_id = ?1
                       and anchor_kind = 'pdf_rect' and source_id = ?2
                       and page_index = ?3 and rects_json = ?4",
                    params![highlight.paper_id, source_id, page_index, rects_json],
                )
                .map_err(|error| error.to_string())?,
            Locator::TextPoint { .. } | Locator::PdfPoint { .. } => 0,
        };

        tx.execute("delete from highlights where id = ?1", params![id])
            .map_err(|error| error.to_string())?;
        tx.commit().map_err(|error| error.to_string())?;
        Ok(threads)
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
                        // Legacy anchored threads were visible marks — keep a color.
                        Some(HighlightColor::default()),
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

    /// RFC 0061: move existing thread `note` entries into the passage's `note`
    /// field, so notes and Q&A no longer share a thread. Idempotent — only fills
    /// highlights whose `note` is still null; migrated note entries are deleted
    /// from the thread (its questions/answers stay). Returns highlights updated.
    pub fn migrate_notes_into_highlight_field(&self) -> StoreResult<usize> {
        let conn = self.open_connection()?;
        struct NoteRow {
            entry_id: String,
            highlight_id: String,
            body: String,
        }
        let mut stmt = conn
            .prepare(
                // All note entries fold into the passage's `note` field —
                // including pinned ones. Notes pin *by default* in the legacy
                // flow, so `pinned` is not a deliberate keep-signal here; the
                // content is preserved in `highlights.note`, it just leaves the
                // Pins tab (which is the intended model shift — notes are now a
                // passage attachment, not a thread entry). Answers are untouched
                // and keep their pins.
                "select e.id, t.highlight_id, e.body
                 from chat_entries e
                 join chat_threads t on e.thread_id = t.id
                 join highlights h on t.highlight_id = h.id
                 where e.kind = 'note' and t.highlight_id is not null and h.note is null
                 order by e.created_at asc",
            )
            .map_err(|error| error.to_string())?;
        let rows: Vec<NoteRow> = stmt
            .query_map([], |r| {
                Ok(NoteRow {
                    entry_id: r.get(0)?,
                    highlight_id: r.get(1)?,
                    body: r.get(2)?,
                })
            })
            .map_err(|error| error.to_string())?
            .collect::<Result<_, _>>()
            .map_err(|error| error.to_string())?;
        drop(stmt);

        let mut notes: std::collections::BTreeMap<String, Vec<String>> =
            std::collections::BTreeMap::new();
        let mut entry_ids: Vec<String> = Vec::new();
        for row in rows {
            notes.entry(row.highlight_id).or_default().push(row.body);
            entry_ids.push(row.entry_id);
        }

        let count = notes.len();
        for (highlight_id, bodies) in notes {
            conn.execute(
                "update highlights set note = ?2, updated_at = datetime('now') where id = ?1",
                params![highlight_id, bodies.join("\n\n")],
            )
            .map_err(|error| error.to_string())?;
        }
        for entry_id in entry_ids {
            conn.execute("delete from chat_entries where id = ?1", params![entry_id])
                .map_err(|error| error.to_string())?;
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
        register_sqlite_vec();
        let conn = Connection::open(&self.db_path).map_err(|error| error.to_string())?;
        conn.execute_batch("pragma foreign_keys = on;")
            .map_err(|error| error.to_string())?;
        Ok(conn)
    }

    fn create_schema(&self, conn: &Connection) -> StoreResult<()> {
        conn.execute_batch(
            "
            create table if not exists projects (
              id text primary key,
              title text not null,
              goal text,
              created_at text not null,
              updated_at text not null
            );

            create table if not exists project_documents (
              id text primary key,
              project_id text not null,
              title text not null,
              format text not null check (format = 'markdown'),
              content text not null,
              harness_writable integer not null default 0,
              created_from_run_id text,
              created_from_state_revision integer,
              created_at text not null,
              updated_at text not null,
              foreign key (project_id) references projects(id) on delete cascade
            );

            create index if not exists idx_project_documents_project
              on project_documents(project_id, updated_at desc);

            create table if not exists vaults (
              id text primary key,
              project_id text not null unique,
              title text not null,
              path text not null unique,
              created_at text not null,
              updated_at text not null,
              foreign key (project_id) references projects(id) on delete cascade
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

            -- RFC 0079 R6.1: SQLite enforces `on delete cascade` by finding the
            -- child rows, which without an index on the FK column is a full scan
            -- of the child table per parent row deleted. Deleting one paper was
            -- therefore scanning every page/block/span/asset in the library, so
            -- the cost grew with the library rather than with the paper.
            create index if not exists idx_document_pages_paper_id
              on document_pages(paper_id);

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

            create index if not exists idx_document_blocks_paper_id
              on document_blocks(paper_id);

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

            -- The big one: spans run to thousands of rows per paper, so this is
            -- the scan that dominated paper deletion.
            create index if not exists idx_document_spans_paper_id
              on document_spans(paper_id);

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

            create index if not exists idx_document_assets_paper_id
              on document_assets(paper_id);

            create table if not exists document_chunks (
              id text primary key,
              paper_id text not null,
              source_id text not null,
              extraction_id text not null,
              chunk_index integer not null,
              chunker text not null,
              chunk_version integer not null,
              page_start integer not null,
              page_end integer not null,
              heading_path text,
              text text not null,
              token_estimate integer not null,
              source_start integer not null,
              source_end integer not null,
              created_at text not null,
              unique (extraction_id, chunk_index),
              foreign key (paper_id) references papers(id) on delete cascade,
              foreign key (source_id) references document_sources(id) on delete cascade,
              foreign key (extraction_id) references document_extractions(id) on delete cascade
            );

            create index if not exists idx_document_chunks_extraction_id
              on document_chunks(extraction_id);

            create index if not exists idx_document_chunks_paper_id
              on document_chunks(paper_id);

            create table if not exists document_chunk_blocks (
              chunk_id text not null,
              block_id text not null,
              ordinal integer not null,
              primary key (chunk_id, block_id),
              foreign key (chunk_id) references document_chunks(id) on delete cascade,
              foreign key (block_id) references document_blocks(id) on delete cascade
            );

            -- The reverse lookup: which chunk covers this block? Needed to
            -- highlight a retrieved chunk, and to find the chunk behind a
            -- passage the user selected.
            create index if not exists idx_document_chunk_blocks_block_id
              on document_chunk_blocks(block_id);

            -- Standalone, NOT content='document_chunks' (RFC 0075). External
            -- content keys on rowid, and this store rebuilds tables (see
            -- relax_highlight_color_not_null) — a rebuild would silently
            -- renumber rowids and leave every FTS row pointing at the wrong
            -- chunk, with no error. Duplicating disposable derived text is the
            -- cheaper failure mode.
            create virtual table if not exists document_chunks_fts using fts5(
              chunk_id unindexed,
              text,
              tokenize = 'unicode61 remove_diacritics 2'
            );

            -- Keeps the FTS index consistent no matter *how* a chunk dies.
            -- Explicit deletes are not enough: `document_chunks` is reachable by
            -- ON DELETE CASCADE from papers, sources, and extractions, and a
            -- cascade never runs the code at the call site. An orphaned FTS row
            -- has no error to report — it just keeps answering searches with a
            -- chunk id that no longer resolves.
            --
            -- RFC 0079 R6.3: keyed on `rowid`, which the insert mirrors from
            -- `document_chunks`. It used to match on `chunk_id`, an fts5
            -- `unindexed` column — a full scan of the index, once per deleted
            -- chunk. Deleting a paper from a 120-paper library spent ~890 ms in
            -- this trigger alone; keyed on rowid the same delete costs ~2 ms.
            drop trigger if exists trg_document_chunks_fts_delete;
            create trigger trg_document_chunks_fts_delete
            after delete on document_chunks
            begin
              delete from document_chunks_fts where rowid = old.rowid;
            end;

            create table if not exists document_chunk_embeddings (
              chunk_id text primary key,
              model text not null,
              model_version text not null,
              dimensions integer not null,
              chunk_version integer not null,
              embedding blob not null,
              created_at text not null,
              foreign key (chunk_id) references document_chunks(id) on delete cascade
            );

            create index if not exists idx_document_chunk_embeddings_model
              on document_chunk_embeddings(model, model_version);

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

            -- What the model is allowed to see, per thread (RFC 0077).
            -- Only *persistent* context lives here: the current selection and
            -- page are parameters to get_context, never rows, so a selection
            -- change needs no invalidation.
            create table if not exists chat_context_items (
              id text primary key,
              thread_id text not null,
              position integer not null,
              kind text not null,
              -- chunk items: a fast-path id plus a durable anchor. A
              -- CHUNK_VERSION bump re-mints chunk ids, so chunk_id alone
              -- would dangle through no fault of the user.
              chunk_id text,
              paper_id text,
              source_start integer,
              source_end integer,
              -- summary items
              body text,
              covers_through_entry_id text,
              -- RFC 0078: who put this here. The agent may drop only its own
              -- additions — a curated passage vanishing on its own is the kind
              -- of surprise that makes a feature untrustworthy.
              origin text not null default 'user',
              token_estimate integer not null,
              created_at text not null,
              foreign key (thread_id) references chat_threads(id) on delete cascade
            );

            -- No score column: ChunkHit.score is comparable only within one
            -- search response, so ordering context by it would be meaningless.
            create unique index if not exists idx_chat_context_items_position
              on chat_context_items(thread_id, position);

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
              color text,
              note text,
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

            create table if not exists vault_suggestion_runs (
              id text primary key,
              vault_id text not null,
              status text not null,
              message text not null default '',
              stop_reason text,
              error text,
              result_count integer not null default 0,
              started_at text,
              finished_at text,
              options_json text not null default '{}',
              query_paths_json text not null default '[]',
              created_at text not null,
              foreign key (vault_id) references vaults(id) on delete cascade
            );

            create index if not exists idx_vault_suggestion_runs_vault
              on vault_suggestion_runs(vault_id, created_at desc);

            create table if not exists vault_suggestions (
              id text primary key,
              vault_id text not null,
              run_id text not null,
              paper_ref text not null,
              candidate_json text not null,
              reason text not null,
              score real not null,
              state text not null,
              created_at text not null,
              updated_at text not null,
              unique (vault_id, paper_ref),
              foreign key (vault_id) references vaults(id) on delete cascade,
              foreign key (run_id) references vault_suggestion_runs(id) on delete cascade
            );

            create index if not exists idx_vault_suggestions_vault_state
              on vault_suggestions(vault_id, state);

            create table if not exists vault_suggestion_settings (
              vault_id text primary key,
              options_json text not null,
              updated_at text not null,
              foreign key (vault_id) references vaults(id) on delete cascade
            );
            ",
        )
        .map_err(|error| error.to_string())?;

        // The vector index (RFC 0076). Separate from create_schema's batch
        // because it needs the embedding dimensions interpolated, and because a
        // vec0 failure must not take the rest of the schema down with it —
        // lexical search and the reader work fine without it.
        create_vector_index(conn)?;

        add_column_if_missing(conn, "papers", "active_source_id", "text")?;
        add_column_if_missing(conn, "vaults", "project_id", "text")?;
        add_column_if_missing(conn, "papers", "active_extraction_id", "text")?;
        add_column_if_missing(conn, "document_sources", "landing_url", "text")?;
        add_column_if_missing(conn, "document_sources", "final_url", "text")?;
        add_column_if_missing(conn, "document_sources", "acquisition_method", "text")?;
        add_column_if_missing(conn, "search_runs", "mode", "text not null default 'deep'")?;
        add_column_if_missing(conn, "search_runs", "provider_set", "text")?;
        add_column_if_missing(conn, "search_runs", "query_expansions", "text")?;
        add_column_if_missing(conn, "search_candidates", "rank_signals_json", "text")?;
        add_column_if_missing(conn, "search_candidates", "provider_hits_json", "text")?;
        add_column_if_missing(
            conn,
            "vault_suggestion_runs",
            "options_json",
            "text not null default '{}'",
        )?;
        add_column_if_missing(
            conn,
            "vault_suggestion_runs",
            "query_paths_json",
            "text not null default '[]'",
        )
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
            let project_id = project_id_for_vault(vault.id);
            tx.execute(
                "insert into projects (id, title, goal, created_at, updated_at)
                 values (?1, ?2, null, datetime('now'), datetime('now'))",
                params![project_id, vault.title],
            )
            .map_err(|error| error.to_string())?;
            tx.execute(
                "
                insert into vaults (id, project_id, title, path, created_at, updated_at)
                values (?1, ?2, ?3, ?4, datetime('now'), datetime('now'))
                ",
                params![vault.id, project_id, vault.title, vault.path],
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
            projects: read_projects(conn)?,
            project_documents: read_project_document_summaries(conn)?,
            vaults: read_vaults(conn)?,
            papers: read_papers(conn)?,
            vault_papers: read_vault_papers(conn)?,
            document_sources: read_document_sources(conn)?,
            document_extractions: read_document_extractions(conn)?,
            document_pages: read_document_pages(conn)?,
            document_assets: read_document_assets(conn)?,
        })
    }
}

fn read_projects(conn: &Connection) -> StoreResult<Vec<Project>> {
    let mut stmt = conn
        .prepare("select id, title, goal from projects order by title, id")
        .map_err(|error| error.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            Ok(Project {
                id: row.get(0)?,
                title: row.get(1)?,
                goal: row.get(2)?,
            })
        })
        .map_err(|error| error.to_string())?;
    collect_rows(rows)
}

fn read_project_document_summaries(
    conn: &Connection,
) -> StoreResult<Vec<ProjectDocumentSummary>> {
    let mut statement = conn
        .prepare(
            "select id, project_id, title, format, harness_writable, updated_at
             from project_documents order by updated_at desc, title, id",
        )
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map([], |row| {
            Ok(ProjectDocumentSummary {
                id: row.get(0)?,
                project_id: row.get(1)?,
                title: row.get(2)?,
                format: row.get(3)?,
                harness_writable: row.get(4)?,
                updated_at: row.get(5)?,
            })
        })
        .map_err(|error| error.to_string())?;
    collect_rows(rows)
}

fn read_project_document(
    conn: &Connection,
    document_id: &str,
) -> StoreResult<Option<ProjectDocument>> {
    conn.query_row(
        "select id, project_id, title, format, content, harness_writable,
                created_from_run_id, created_from_state_revision, created_at, updated_at
         from project_documents where id = ?1",
        params![document_id],
        |row| {
            Ok(ProjectDocument {
                id: row.get(0)?,
                project_id: row.get(1)?,
                title: row.get(2)?,
                format: row.get(3)?,
                content: row.get(4)?,
                harness_writable: row.get(5)?,
                created_from_run_id: row.get(6)?,
                created_from_state_revision: row.get(7)?,
                created_at: row.get(8)?,
                updated_at: row.get(9)?,
            })
        },
    )
    .optional()
    .map_err(|error| error.to_string())
}

fn read_vaults(conn: &Connection) -> StoreResult<Vec<Vault>> {
    let mut stmt = conn
        .prepare("select id, project_id, title, path from vaults order by path")
        .map_err(|error| error.to_string())?;

    let rows = stmt
        .query_map([], |row| {
            Ok(Vault {
                id: row.get(0)?,
                project_id: row.get(1)?,
                title: row.get(2)?,
                path: row.get(3)?,
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

fn read_cite_records_for_vault(conn: &Connection, vault_id: &str) -> StoreResult<Vec<CiteRecord>> {
    let mut stmt = conn
        .prepare(
            "
            select p.title, p.authors_json, p.venue, p.year
            from papers p
            join vault_papers vp on vp.paper_id = p.id
            where vp.vault_id = ?1
            ",
        )
        .map_err(|error| error.to_string())?;

    let rows = stmt
        .query_map(params![vault_id], |row| {
            let authors_json: String = row.get(1)?;
            Ok(CiteRecord {
                title: row.get(0)?,
                authors: from_json(&authors_json),
                venue: row.get(2)?,
                year: row.get(3)?,
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

// Blocks and spans are read per-extraction, never library-wide (RFC 0075 R2) —
// see `read_document_blocks_for_extraction`. `LibrarySnapshot` used to carry
// every block and span in the library and let callers filter in memory, which
// was nearly free while a block was a whole page and spans did not exist. With
// structural extraction it is ~370 bytes of identifiers per span row, crossing
// IPC as JSON on every library read and on six different mutations.

fn document_block_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<DocumentBlock> {
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
}

fn document_span_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<DocumentSpan> {
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

/// A sticky note's anchor, encoded as the zero-size rect the `rects_json` column
/// already knows how to hold (RFC 0074). Keeping the point in the existing
/// column is what lets sticky notes ship without a schema migration.
fn point_rects_json(x: f64, y: f64) -> String {
    format!("[{{\"x\":{x},\"y\":{y},\"width\":0,\"height\":0}}]")
}

/// Inverse of [`point_rects_json`]. Anything unparseable reads as the page
/// origin — a misplaced sticky is recoverable, a dropped one is not.
fn point_from_rects_json(raw: Option<&str>) -> (f64, f64) {
    let Some(raw) = raw else {
        return (0.0, 0.0);
    };
    let parsed: Option<Vec<serde_json::Value>> = serde_json::from_str(raw).ok();
    let first = parsed.and_then(|rects| rects.into_iter().next());
    let Some(first) = first else {
        return (0.0, 0.0);
    };
    (
        first.get("x").and_then(|v| v.as_f64()).unwrap_or_default(),
        first.get("y").and_then(|v| v.as_f64()).unwrap_or_default(),
    )
}

fn read_highlight(conn: &Connection, id: &str) -> StoreResult<crate::domain::highlight::Highlight> {
    conn.query_row(
        "
        select id, paper_id, source_id, locator_kind, start_offset, end_offset,
               page_index, rects_json, excerpt, color, label, author_kind,
               author_model, created_at, updated_at, note
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
    let locator = match locator_kind.as_str() {
        "pdf_rect" => Locator::PdfRect {
            source_id: source_id.clone(),
            page_index: row.get::<_, Option<i32>>(6)?.unwrap_or_default(),
            rects_json: row.get::<_, Option<String>>(7)?.unwrap_or_default(),
        },
        // RFC 0074: the sticky's x/y live in the zero-size rect stored in
        // `rects_json`; an unreadable one degrades to the page's top-left rather
        // than losing the annotation.
        "pdf_point" => {
            let (x, y) = point_from_rects_json(row.get::<_, Option<String>>(7)?.as_deref());
            Locator::PdfPoint {
                source_id: source_id.clone(),
                page_index: row.get::<_, Option<i32>>(6)?.unwrap_or_default(),
                x,
                y,
            }
        }
        "text_point" => Locator::TextPoint {
            source_id: source_id.clone(),
            offset: row.get::<_, Option<i64>>(4)?.unwrap_or_default(),
        },
        _ => Locator::TextOffset {
            source_id: source_id.clone(),
            start_offset: row.get::<_, Option<i64>>(4)?.unwrap_or_default(),
            end_offset: row.get::<_, Option<i64>>(5)?.unwrap_or_default(),
        },
    };
    // Color is nullable (RFC 0061): a null/blank color = no color mark.
    let color: Option<HighlightColor> = row
        .get::<_, Option<String>>(9)?
        .filter(|value| !value.trim().is_empty())
        .map(|value| serde_json::from_str(&format!("\"{value}\"")).unwrap_or_default());
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
        note: row.get(15)?,
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
    find_or_create_thread_id_with(conn, scope_kind, scope_id, anchor, false)
}

/// `force_new` skips the whole-paper de-duplication, so "Ask about this paper"
/// starts a fresh conversation instead of appending to the one from last week.
///
/// RFC 0034 collapsed every `document` anchor onto a single thread per paper.
/// That is right for a *note* about the paper — there is one of those — and
/// wrong for a chat, where a new question is usually a new subject.
fn find_or_create_thread_id_with(
    conn: &Connection,
    scope_kind: &str,
    scope_id: &str,
    anchor: &ThreadAnchor,
    force_new: bool,
) -> StoreResult<ThreadResolution> {
    if matches!(anchor, ThreadAnchor::Document) && !force_new {
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

    /// Read the actionable suggestion inbox and its latest run for one vault.
    pub fn get_vault_suggestions(&self, vault_id: &str) -> StoreResult<VaultSuggestionSnapshot> {
        let conn = self.open_connection()?;
        Ok(VaultSuggestionSnapshot {
            suggestions: read_vault_suggestions(&conn, vault_id, Some("pending"))?,
            latest_run: read_latest_vault_suggestion_run(&conn, vault_id)?,
        })
    }

    /// Start a vault-scoped run ledger entry in `queued` state.
    pub fn create_vault_suggestion_run(&self, vault_id: &str) -> StoreResult<VaultSuggestionRun> {
        self.create_vault_suggestion_run_with_options(
            vault_id,
            &VaultSuggestionOptions::default(),
            &[],
        )
    }

    /// Start a run and snapshot the controls and approved query paths.
    pub fn create_vault_suggestion_run_with_options(
        &self,
        vault_id: &str,
        options: &VaultSuggestionOptions,
        query_paths: &[VaultSuggestionQueryPath],
    ) -> StoreResult<VaultSuggestionRun> {
        let conn = self.open_connection()?;
        let id = timestamped_id("vsr")?;
        let options_json = serde_json::to_string(options).map_err(|error| error.to_string())?;
        let query_paths_json =
            serde_json::to_string(query_paths).map_err(|error| error.to_string())?;
        conn.execute(
            "insert into vault_suggestion_runs
               (id, vault_id, status, message, started_at, options_json,
                query_paths_json, created_at)
             values (?1, ?2, 'queued', 'Queued', datetime('now'), ?3, ?4,
                     datetime('now'))",
            params![id, vault_id, options_json, query_paths_json],
        )
        .map_err(|error| error.to_string())?;
        read_vault_suggestion_run(&conn, &id)
    }

    /// Load the controls last saved for this vault, or their product defaults.
    pub fn get_vault_suggestion_options(
        &self,
        vault_id: &str,
    ) -> StoreResult<VaultSuggestionOptions> {
        let conn = self.open_connection()?;
        let options_json: Option<String> = conn
            .query_row(
                "select options_json from vault_suggestion_settings where vault_id = ?1",
                params![vault_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|error| error.to_string())?;
        options_json
            .map(|json| serde_json::from_str(&json).map_err(|error| error.to_string()))
            .transpose()
            .map(|options| options.unwrap_or_default())
    }

    /// Persist the controls used to prepare the vault's next query proposal.
    pub fn save_vault_suggestion_options(
        &self,
        vault_id: &str,
        options: &VaultSuggestionOptions,
    ) -> StoreResult<()> {
        let conn = self.open_connection()?;
        let options_json = serde_json::to_string(options).map_err(|error| error.to_string())?;
        conn.execute(
            "insert into vault_suggestion_settings (vault_id, options_json, updated_at)
             values (?1, ?2, datetime('now'))
             on conflict(vault_id) do update set
               options_json = excluded.options_json,
               updated_at = excluded.updated_at",
            params![vault_id, options_json],
        )
        .map_err(|error| error.to_string())?;
        Ok(())
    }

    /// Update the durable user-facing lifecycle for a suggestion run.
    pub fn set_vault_suggestion_run_status(
        &self,
        run_id: &str,
        status: &str,
        message: &str,
        stop_reason: Option<&str>,
        error: Option<&str>,
        result_count: i32,
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
                "update vault_suggestion_runs
                 set status = ?2, message = ?3, stop_reason = ?4, error = ?5,
                     result_count = ?6, finished_at = {finished_sql}
                 where id = ?1"
            ),
            params![run_id, status, message, stop_reason, error, result_count],
        )
        .map_err(|error| error.to_string())?;
        Ok(())
    }

    /// Atomically replace pending rows after a successful refresh.
    ///
    /// Added and dismissed rows survive and suppress matching candidates in
    /// later runs. This is also what keeps a failed refresh from erasing the
    /// previous inbox: callers replace only after all retrieval has succeeded.
    pub fn replace_vault_suggestions(
        &self,
        vault_id: &str,
        run_id: &str,
        suggestions: &[VaultSuggestion],
    ) -> StoreResult<()> {
        let mut conn = self.open_connection()?;
        let tx = conn.transaction().map_err(|error| error.to_string())?;
        tx.execute(
            "delete from vault_suggestions where vault_id = ?1 and state = 'pending'",
            params![vault_id],
        )
        .map_err(|error| error.to_string())?;

        for suggestion in suggestions {
            let candidate_json =
                serde_json::to_string(&suggestion.candidate).map_err(|error| error.to_string())?;
            tx.execute(
                "insert into vault_suggestions
                   (id, vault_id, run_id, paper_ref, candidate_json, reason,
                    score, state, created_at, updated_at)
                 values (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'pending', datetime('now'), datetime('now'))
                 on conflict(vault_id, paper_ref) do nothing",
                params![
                    suggestion.id,
                    vault_id,
                    run_id,
                    suggestion.paper_ref,
                    candidate_json,
                    suggestion.reason,
                    suggestion.score,
                ],
            )
            .map_err(|error| error.to_string())?;
        }
        tx.commit().map_err(|error| error.to_string())
    }

    /// Move one suggestion between pending, dismissed, and added.
    pub fn set_vault_suggestion_state(&self, suggestion_id: &str, state: &str) -> StoreResult<()> {
        let conn = self.open_connection()?;
        conn.execute(
            "update vault_suggestions
             set state = ?2, updated_at = datetime('now') where id = ?1",
            params![suggestion_id, state],
        )
        .map_err(|error| error.to_string())?;
        Ok(())
    }

    /// Persist a decision made on a provisional row before finalisation.
    pub fn upsert_vault_suggestion_decision(
        &self,
        suggestion: &VaultSuggestion,
        state: &str,
    ) -> StoreResult<()> {
        let conn = self.open_connection()?;
        let candidate_json =
            serde_json::to_string(&suggestion.candidate).map_err(|error| error.to_string())?;
        conn.execute(
            "insert into vault_suggestions
               (id, vault_id, run_id, paper_ref, candidate_json, reason, score,
                state, created_at, updated_at)
             values (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, datetime('now'), datetime('now'))
             on conflict(vault_id, paper_ref) do update set
               state = excluded.state, updated_at = datetime('now')",
            params![
                suggestion.id,
                suggestion.vault_id,
                suggestion.run_id,
                suggestion.paper_ref,
                candidate_json,
                suggestion.reason,
                suggestion.score,
                state,
            ],
        )
        .map_err(|error| error.to_string())?;
        Ok(())
    }

    /// Load one suggestion for Add and Undo commands.
    pub fn get_vault_suggestion(&self, suggestion_id: &str) -> StoreResult<VaultSuggestion> {
        let conn = self.open_connection()?;
        read_vault_suggestion(&conn, suggestion_id)
    }

    /// Candidate refs permanently suppressed by a prior decision.
    pub fn decided_vault_suggestion_refs(&self, vault_id: &str) -> StoreResult<Vec<String>> {
        let conn = self.open_connection()?;
        let mut statement = conn
            .prepare(
                "select paper_ref from vault_suggestions
                 where vault_id = ?1 and state in ('added', 'dismissed')",
            )
            .map_err(|error| error.to_string())?;
        let rows = statement
            .query_map(params![vault_id], |row| row.get(0))
            .map_err(|error| error.to_string())?;
        collect_rows(rows)
    }

    /// Mean current-model chunk embedding for papers in one vault.
    pub fn vault_embedding_centroid(
        &self,
        vault_id: &str,
        model: &str,
        model_version: &str,
    ) -> StoreResult<Option<Vec<f32>>> {
        let conn = self.open_connection()?;
        let mut statement = conn
            .prepare(
                "select e.embedding, e.dimensions
                 from document_chunk_embeddings e
                 join document_chunks c on c.id = e.chunk_id
                 join vault_papers vp on vp.paper_id = c.paper_id
                 where vp.vault_id = ?1 and e.model = ?2 and e.model_version = ?3",
            )
            .map_err(|error| error.to_string())?;
        let rows = statement
            .query_map(params![vault_id, model, model_version], |row| {
                Ok((row.get::<_, Vec<u8>>(0)?, row.get::<_, i64>(1)?))
            })
            .map_err(|error| error.to_string())?;
        let vectors = collect_rows(rows)?;
        mean_embedding(&vectors)
    }

    /// Mean current-model chunk embedding for each paper in one vault.
    ///
    /// Papers without ready chunk embeddings are omitted. Results are ordered
    /// by paper id so the same database state produces stable ranking inputs.
    pub fn vault_paper_embedding_centroids(
        &self,
        vault_id: &str,
        model: &str,
        model_version: &str,
    ) -> StoreResult<Vec<(String, Vec<f32>)>> {
        let conn = self.open_connection()?;
        let mut statement = conn
            .prepare(
                "select c.paper_id, e.embedding, e.dimensions
                 from document_chunk_embeddings e
                 join document_chunks c on c.id = e.chunk_id
                 join vault_papers vp on vp.paper_id = c.paper_id
                 where vp.vault_id = ?1 and e.model = ?2 and e.model_version = ?3
                 order by c.paper_id, c.chunk_index",
            )
            .map_err(|error| error.to_string())?;
        let rows = statement
            .query_map(params![vault_id, model, model_version], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Vec<u8>>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            })
            .map_err(|error| error.to_string())?;

        let mut by_paper: HashMap<String, Vec<(Vec<u8>, i64)>> = HashMap::new();
        for row in rows {
            let (paper_id, embedding, dimensions) = row.map_err(|error| error.to_string())?;
            by_paper
                .entry(paper_id)
                .or_default()
                .push((embedding, dimensions));
        }

        let mut centroids = Vec::new();
        for (paper_id, embeddings) in by_paper {
            if let Some(centroid) = mean_embedding(&embeddings)? {
                centroids.push((paper_id, centroid));
            }
        }
        centroids.sort_by(|left, right| left.0.cmp(&right.0));
        Ok(centroids)
    }

    /// Pick at most one vault due for the weekly suggestion schedule.
    pub fn due_vault_for_weekly_suggestions(&self) -> StoreResult<Option<String>> {
        let conn = self.open_connection()?;
        conn.query_row(
            "select v.id
             from vaults v
             where (select count(*) from vault_papers vp where vp.vault_id = v.id) >= 3
               and coalesce(
                 (select max(r.finished_at) from vault_suggestion_runs r where r.vault_id = v.id),
                 '1970-01-01'
               ) < datetime('now', '-7 days')
             order by v.updated_at desc, v.id
             limit 1",
            [],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| error.to_string())
    }
}

fn read_vault_suggestions(
    conn: &Connection,
    vault_id: &str,
    state: Option<&str>,
) -> StoreResult<Vec<VaultSuggestion>> {
    let sql = format!(
        "select {VAULT_SUGGESTION_COLUMNS} from vault_suggestions
         where vault_id = ?1 and (?2 is null or state = ?2)
         order by score desc, id"
    );
    let mut statement = conn.prepare(&sql).map_err(|error| error.to_string())?;
    let rows = statement
        .query_map(params![vault_id, state], vault_suggestion_from_row)
        .map_err(|error| error.to_string())?;
    collect_rows(rows)
}

const VAULT_SUGGESTION_COLUMNS: &str =
    "id, vault_id, run_id, paper_ref, candidate_json, reason, score, state, created_at, updated_at";

fn read_vault_suggestion(conn: &Connection, id: &str) -> StoreResult<VaultSuggestion> {
    conn.query_row(
        &format!("select {VAULT_SUGGESTION_COLUMNS} from vault_suggestions where id = ?1"),
        params![id],
        vault_suggestion_from_row,
    )
    .map_err(|error| error.to_string())
}

fn vault_suggestion_from_row(row: &rusqlite::Row) -> rusqlite::Result<VaultSuggestion> {
    let candidate_json: String = row.get(4)?;
    let candidate = serde_json::from_str(&candidate_json).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(4, rusqlite::types::Type::Text, Box::new(error))
    })?;
    Ok(VaultSuggestion {
        id: row.get(0)?,
        vault_id: row.get(1)?,
        run_id: row.get(2)?,
        paper_ref: row.get(3)?,
        candidate,
        reason: row.get(5)?,
        score: row.get(6)?,
        state: row.get(7)?,
        created_at: row.get(8)?,
        updated_at: row.get(9)?,
    })
}

const VAULT_SUGGESTION_RUN_COLUMNS: &str =
    "id, vault_id, status, message, stop_reason, error, result_count, started_at, finished_at, created_at, options_json, query_paths_json";

fn read_vault_suggestion_run(conn: &Connection, id: &str) -> StoreResult<VaultSuggestionRun> {
    conn.query_row(
        &format!("select {VAULT_SUGGESTION_RUN_COLUMNS} from vault_suggestion_runs where id = ?1"),
        params![id],
        vault_suggestion_run_from_row,
    )
    .map_err(|error| error.to_string())
}

fn read_latest_vault_suggestion_run(
    conn: &Connection,
    vault_id: &str,
) -> StoreResult<Option<VaultSuggestionRun>> {
    conn.query_row(
        &format!(
            "select {VAULT_SUGGESTION_RUN_COLUMNS} from vault_suggestion_runs
             where vault_id = ?1 order by created_at desc, id desc limit 1"
        ),
        params![vault_id],
        vault_suggestion_run_from_row,
    )
    .optional()
    .map_err(|error| error.to_string())
}

fn vault_suggestion_run_from_row(row: &rusqlite::Row) -> rusqlite::Result<VaultSuggestionRun> {
    let options_json: String = row.get(10)?;
    let query_paths_json: String = row.get(11)?;
    let options = serde_json::from_str(&options_json).unwrap_or_default();
    let query_paths = serde_json::from_str(&query_paths_json).unwrap_or_default();
    Ok(VaultSuggestionRun {
        id: row.get(0)?,
        vault_id: row.get(1)?,
        status: row.get(2)?,
        message: row.get(3)?,
        stop_reason: row.get(4)?,
        error: row.get(5)?,
        result_count: row.get(6)?,
        started_at: row.get(7)?,
        finished_at: row.get(8)?,
        created_at: row.get(9)?,
        options,
        query_paths,
    })
}

fn mean_embedding(rows: &[(Vec<u8>, i64)]) -> StoreResult<Option<Vec<f32>>> {
    let Some((_, dimensions)) = rows.first() else {
        return Ok(None);
    };
    let dimensions = *dimensions as usize;
    let mut mean = vec![0.0_f32; dimensions];
    let mut count = 0_f32;
    for (blob, row_dimensions) in rows {
        if *row_dimensions as usize != dimensions || blob.len() != dimensions * 4 {
            continue;
        }
        for (index, bytes) in blob.chunks_exact(4).enumerate() {
            mean[index] += f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        }
        count += 1.0;
    }
    if count == 0.0 {
        return Ok(None);
    }
    for value in &mut mean {
        *value /= count;
    }
    Ok(Some(mean))
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

/// Insert one chunk's vector into the `vec0` index.
///
/// Silently skips when the index is unavailable, and *refuses* a
/// dimension mismatch rather than letting `vec0` reject it mid-batch: a vector
/// of the wrong width means the row came from a different model, and indexing
/// it would put two vector spaces in one index where nothing downstream could
/// tell them apart.
fn index_chunk_vector(
    conn: &Connection,
    chunk_id: &str,
    blob: &[u8],
    dimensions: usize,
) -> StoreResult<()> {
    if !sqlite_vec_available(conn) {
        return Ok(());
    }
    if dimensions != VECTOR_DIMENSIONS {
        eprintln!(
            "[search] skipping {chunk_id}: {dimensions} dimensions, index expects {VECTOR_DIMENSIONS}"
        );
        return Ok(());
    }

    let paper_id: Option<String> = conn
        .query_row(
            "select paper_id from document_chunks where id = ?1",
            params![chunk_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| error.to_string())?;
    let Some(paper_id) = paper_id else {
        return Ok(());
    };

    // vec0 has no upsert; a re-embed of the same chunk must replace, not stack.
    conn.execute(
        "delete from document_chunk_vectors where chunk_id = ?1",
        params![chunk_id],
    )
    .map_err(|error| error.to_string())?;
    conn.execute(
        "
        insert into document_chunk_vectors (paper_id, chunk_id, embedding)
        values (?1, ?2, ?3)
        ",
        params![paper_id, chunk_id, blob],
    )
    .map_err(|error| error.to_string())?;
    Ok(())
}

/// Create the `vec0` index and the trigger that keeps it in step.
///
/// Returns `Ok(())` even when `vec0` is unavailable: semantic search is the
/// only casualty, and `SemanticStatus` reports it on every response rather than
/// the app failing to start.
fn create_vector_index(conn: &Connection) -> StoreResult<()> {
    if !sqlite_vec_available(conn) {
        eprintln!("[search] sqlite-vec unavailable; semantic search disabled");
        return Ok(());
    }

    // `paper_id` is a partition key, not a metadata column. A plain `k = n` KNN
    // searches globally and filters afterwards, so scoping to one paper could
    // return nothing when that paper's chunks all rank below the global top-n —
    // and the failure would read as "this PDF has nothing relevant" rather than
    // a bug. A partition key makes `k` mean "k within this scope" (RFC 0076).
    //
    // Dimensions are fixed in the table definition, so this column encodes the
    // current model. Changing models is a rebuild plus a re-embed, which is
    // already the expected path — every embedding row records its own model,
    // version, and dimensions so the mismatch is detectable.
    let create = format!(
        "
        create virtual table if not exists document_chunk_vectors using vec0(
          paper_id text partition key,
          chunk_id text primary key,
          embedding float[{VECTOR_DIMENSIONS}]
        );
        "
    );
    if let Err(error) = conn.execute_batch(&create) {
        eprintln!("[search] could not create vector index: {error}");
        return Ok(());
    }

    // Same hazard as the FTS index, same fix. `document_chunk_embeddings` is
    // reachable by cascade from chunks, papers, sources, and extractions, and a
    // cascade never runs the code at the call site. An orphaned vector is worse
    // than an orphaned FTS row: it returns a chunk id that no longer resolves,
    // so hydration drops it and the search quietly returns fewer results than
    // it found.
    conn.execute_batch(
        "
        create trigger if not exists trg_document_chunk_vectors_delete
        after delete on document_chunk_embeddings
        begin
          delete from document_chunk_vectors where chunk_id = old.chunk_id;
        end;
        ",
    )
    .map_err(|error| error.to_string())?;

    Ok(())
}

/// Make `vec0` available to every connection opened from here on (RFC 0076).
///
/// Static registration, not a loadable extension: `sqlite-vec` compiles into
/// the binary, so there is no `.dylib` to bundle, sign, or find at runtime.
/// `sqlite3_auto_extension` installs an initializer that SQLite runs for each
/// new connection, so this must happen before the first `Connection::open` —
/// hence the call at the top of `open_connection` rather than in `init`, which
/// not every code path reaches first.
///
/// Idempotent via `Once`. SQLite also de-duplicates auto-extensions, but doing
/// the FFI call once keeps the unsafe block off the hot path.
fn register_sqlite_vec() {
    static REGISTER: std::sync::Once = std::sync::Once::new();
    REGISTER.call_once(|| {
        // SAFETY: `sqlite3_vec_init` has the signature SQLite expects of an
        // extension entry point; the transmute only adds the `sqlite3_api_routines`
        // parameter that the C ABI passes and the Rust binding omits. This is the
        // registration form the sqlite-vec crate documents.
        unsafe {
            rusqlite::ffi::sqlite3_auto_extension(Some(std::mem::transmute::<
                *const (),
                unsafe extern "C" fn(
                    *mut rusqlite::ffi::sqlite3,
                    *mut *mut i8,
                    *const rusqlite::ffi::sqlite3_api_routines,
                ) -> i32,
            >(
                sqlite_vec::sqlite3_vec_init as *const (),
            )));
        }
    });
}

/// Whether `vec0` is actually usable on this connection.
///
/// Static linking makes a missing extension a compile error rather than a
/// runtime one, but this also catches a vector table that failed to create.
/// Visible, not fatal: refusing to start would take away a reader and a lexical
/// search that both work, over a feature that degrades cleanly (RFC 0076).
fn sqlite_vec_available(conn: &Connection) -> bool {
    conn.query_row("select vec_version()", [], |row| row.get::<_, String>(0))
        .is_ok()
}

/// `?, ?, ?` for an `in (...)` clause of `count` bindings.
fn placeholders(count: usize) -> String {
    std::iter::repeat_n("?", count)
        .collect::<Vec<_>>()
        .join(", ")
}

/// Turn user input into an FTS5 MATCH expression.
///
/// FTS5 query syntax is a language: bare `AND`/`OR`/`NEAR`, quotes, and `*` all
/// mean something, so a user typing `C++ (revised)` is a syntax error rather
/// than a search. Quoting each token makes every term a literal phrase and the
/// whole query an implicit AND.
fn fts_match_query(query: &str) -> String {
    query
        .split_whitespace()
        .map(|token| format!("\"{}\"", token.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Read chunks with an arbitrary tail (`where`/`join`/`order`/`limit`).
///
/// The chunk row and its `block_ids` come back together, because a chunk
/// without its provenance is not usable for anything this RFC builds.
fn read_chunks<P: rusqlite::Params>(
    conn: &Connection,
    tail: &str,
    params: P,
) -> StoreResult<Vec<DocumentChunk>> {
    let sql = format!(
        "
        select c.id, c.paper_id, c.source_id, c.extraction_id, c.chunk_index,
               c.chunker, c.chunk_version, c.page_start, c.page_end,
               c.heading_path, c.text, c.token_estimate, c.source_start,
               c.source_end
        from document_chunks c
        {tail}
        "
    );

    let mut stmt = conn.prepare(&sql).map_err(|error| error.to_string())?;
    let rows = stmt
        .query_map(params, |row| {
            Ok(DocumentChunk {
                id: row.get(0)?,
                paper_id: row.get(1)?,
                source_id: row.get(2)?,
                extraction_id: row.get(3)?,
                chunk_index: row.get(4)?,
                chunker: row.get(5)?,
                chunk_version: row.get(6)?,
                page_start: row.get(7)?,
                page_end: row.get(8)?,
                heading_path: row.get(9)?,
                text: row.get(10)?,
                token_estimate: row.get(11)?,
                source_start: row.get(12)?,
                source_end: row.get(13)?,
                block_ids: Vec::new(),
            })
        })
        .map_err(|error| error.to_string())?;
    let mut chunks: Vec<DocumentChunk> = collect_rows(rows)?;

    for chunk in &mut chunks {
        let mut stmt = conn
            .prepare(
                "
                select block_id from document_chunk_blocks
                where chunk_id = ?1
                order by ordinal
                ",
            )
            .map_err(|error| error.to_string())?;
        let rows = stmt
            .query_map(params![chunk.id], |row| row.get::<_, String>(0))
            .map_err(|error| error.to_string())?;
        chunk.block_ids = collect_rows(rows)?;
    }

    Ok(chunks)
}

fn read_document_blocks_for_extraction(
    conn: &Connection,
    extraction_id: &str,
) -> StoreResult<Vec<DocumentBlock>> {
    let mut stmt = conn
        .prepare(
            "
            select id, paper_id, source_id, extraction_id, page_index,
                   block_index, reading_order, kind, text, asset_id,
                   source_start, source_end, bbox_json
            from document_blocks
            where extraction_id = ?1
            order by reading_order, block_index
            ",
        )
        .map_err(|error| error.to_string())?;
    let rows = stmt
        .query_map(params![extraction_id], document_block_from_row)
        .map_err(|error| error.to_string())?;
    collect_rows(rows)
}

fn read_document_spans_for_extraction(
    conn: &Connection,
    extraction_id: &str,
) -> StoreResult<Vec<DocumentSpan>> {
    let mut stmt = conn
        .prepare(
            "
            select id, paper_id, source_id, extraction_id, block_id, page_index,
                   text, source_start, source_end, bbox_json
            from document_spans
            where extraction_id = ?1
            order by source_start, id
            ",
        )
        .map_err(|error| error.to_string())?;
    let rows = stmt
        .query_map(params![extraction_id], document_span_from_row)
        .map_err(|error| error.to_string())?;
    collect_rows(rows)
}

/// Drop an extraction's chunks without touching its pages, blocks, or spans.
/// FTS rows follow via `trg_document_chunks_fts_delete`.
fn clear_extraction_chunks(conn: &Connection, extraction_id: &str) -> StoreResult<()> {
    conn.execute(
        "delete from document_chunks where extraction_id = ?1",
        params![extraction_id],
    )
    .map_err(|error| error.to_string())?;
    Ok(())
}

/// Point every FTS row at its chunk's rowid (RFC 0079 R6.3).
///
/// The delete trigger now keys on `rowid`, but rows written before this change
/// got whatever rowid fts5 handed out. A mismatched row would survive its
/// chunk's deletion and keep answering searches with an id that resolves to
/// nothing — so the index is rebuilt once, from the table that owns the truth.
///
/// Idempotent and cheap after the first run: the count comparison is two
/// indexed aggregates, and a rebuild only happens when they disagree.
fn realign_chunk_fts_rowids(conn: &Connection) -> StoreResult<()> {
    let misaligned: i64 = conn
        .query_row(
            "select count(*) from document_chunks c
             where not exists (
               select 1 from document_chunks_fts f
               where f.rowid = c.rowid and f.chunk_id = c.id
             )",
            [],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    if misaligned == 0 {
        return Ok(());
    }

    eprintln!("[store] realigning {misaligned} chunk FTS row(s) onto chunk rowids");
    conn.execute("delete from document_chunks_fts", [])
        .map_err(|error| error.to_string())?;
    conn.execute(
        "insert into document_chunks_fts (rowid, chunk_id, text)
         select rowid, id, text from document_chunks",
        [],
    )
    .map_err(|error| error.to_string())?;
    Ok(())
}

/// Write chunks, their block provenance, and their FTS rows.
///
/// All three in one place because they are one fact recorded three ways —
/// splitting them across call sites is how an FTS index drifts from its table.
fn insert_chunks(conn: &Connection, chunks: &[DocumentChunk]) -> StoreResult<()> {
    for chunk in chunks {
        conn.execute(
            "
            insert into document_chunks (
              id, paper_id, source_id, extraction_id, chunk_index, chunker,
              chunk_version, page_start, page_end, heading_path, text,
              token_estimate, source_start, source_end, created_at
            )
            values (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, datetime('now'))
            ",
            params![
                chunk.id,
                chunk.paper_id,
                chunk.source_id,
                chunk.extraction_id,
                chunk.chunk_index,
                chunk.chunker,
                chunk.chunk_version,
                chunk.page_start,
                chunk.page_end,
                chunk.heading_path,
                chunk.text,
                chunk.token_estimate,
                chunk.source_start,
                chunk.source_end,
            ],
        )
        .map_err(|error| error.to_string())?;
        // Captured here, not after the loop below: the block inserts would
        // otherwise be the "last" insert by the time the FTS row is written.
        let chunk_rowid = conn.last_insert_rowid();

        for (ordinal, block_id) in chunk.block_ids.iter().enumerate() {
            conn.execute(
                "
                insert into document_chunk_blocks (chunk_id, block_id, ordinal)
                values (?1, ?2, ?3)
                ",
                params![chunk.id, block_id, ordinal as i64],
            )
            .map_err(|error| error.to_string())?;
        }

        // RFC 0079 R6.3: the FTS row carries the chunk's own rowid, so the
        // delete trigger can key on it. `chunk_id` is an `unindexed` fts5
        // column — deleting by it scans the whole index, and the trigger runs
        // once per chunk, which is what made deleting a paper take a second.
        conn.execute(
            "insert into document_chunks_fts (rowid, chunk_id, text) values (?1, ?2, ?3)",
            params![chunk_rowid, chunk.id, chunk.text],
        )
        .map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn clear_extraction_children(conn: &Connection, extraction_id: &str) -> StoreResult<()> {
    // Cascades to document_chunk_blocks and document_chunk_embeddings; the
    // FTS row goes with it via trg_document_chunks_fts_delete.
    conn.execute(
        "delete from document_chunks where extraction_id = ?1",
        params![extraction_id],
    )
    .map_err(|error| error.to_string())?;
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

/// RFC 0061 made `highlights.color` nullable in `create_schema` only — which is
/// inert on a database that already has the table — so every vault created
/// before it still rejects the color-less passages Note/Ask create (RFC 0072).
/// SQLite cannot drop a NOT NULL constraint, so rebuild the table.
///
/// Idempotent and cheap: one `pragma table_info` per launch, and a no-op once
/// the column is nullable.
fn relax_highlight_color_not_null(conn: &Connection) -> StoreResult<()> {
    if !column_is_not_null(conn, "highlights", "color")? {
        return Ok(());
    }

    // Foreign keys can only be toggled outside a transaction. Nothing declares a
    // foreign key referencing `highlights`, but the rebuild drops a table, so
    // follow SQLite's documented procedure rather than relying on that.
    conn.execute_batch("pragma foreign_keys = off;")
        .map_err(|error| error.to_string())?;

    // Explicit column lists on BOTH halves of the insert: a database migrated
    // from the pre-RFC-0061 shape has `note` appended last (it arrived via
    // `alter table`), while the canonical shape carries it between `color` and
    // `label`. `select *` would silently shift every column after `color`.
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
        // transaction open; close it explicitly so the pragma restore below sees
        // a clean connection.
        let _ = conn.execute_batch("rollback;");
    }
    conn.execute_batch("pragma foreign_keys = on;")
        .map_err(|error| error.to_string())?;

    rebuild.map_err(|error| error.to_string())
}

/// Whether `table.column` is declared NOT NULL. Companion to [`column_exists`]
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
fn insert_context_item_tx(
    tx: &rusqlite::Transaction<'_>,
    id: &str,
    thread_id: &str,
    draft: &ContextItemDraft,
) -> StoreResult<()> {
    let next_position: i32 = tx
        .query_row(
            "select coalesce(max(position) + 1, 0) from chat_context_items where thread_id = ?1",
            params![thread_id],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;

    tx.execute(
        "
        insert into chat_context_items (
          id, thread_id, position, kind, chunk_id, paper_id, source_start,
          source_end, body, covers_through_entry_id, origin, token_estimate, created_at
        )
        values (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, datetime('now'))
        ",
        params![
            id,
            thread_id,
            next_position,
            draft.kind,
            draft.chunk_id,
            draft.paper_id,
            draft.source_start,
            draft.source_end,
            draft.body,
            draft.covers_through_entry_id,
            draft.origin,
            draft.token_estimate,
        ],
    )
    .map_err(|error| error.to_string())?;
    Ok(())
}

const CONTEXT_ITEM_COLUMNS: &str = "id, thread_id, position, kind, chunk_id, paper_id, \
     source_start, source_end, body, covers_through_entry_id, origin, token_estimate, \
     created_at";

fn context_item_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ContextItem> {
    Ok(ContextItem {
        id: row.get(0)?,
        thread_id: row.get(1)?,
        position: row.get(2)?,
        kind: row.get(3)?,
        chunk_id: row.get(4)?,
        paper_id: row.get(5)?,
        source_start: row.get(6)?,
        source_end: row.get(7)?,
        body: row.get(8)?,
        covers_through_entry_id: row.get(9)?,
        origin: row.get(10)?,
        token_estimate: row.get(11)?,
        created_at: row.get(12)?,
    })
}

fn read_context_item(conn: &Connection, id: &str) -> StoreResult<ContextItem> {
    conn.query_row(
        &format!("select {CONTEXT_ITEM_COLUMNS} from chat_context_items where id = ?1"),
        params![id],
        context_item_from_row,
    )
    .map_err(|error| error.to_string())
}

fn read_context_items(conn: &Connection, thread_id: &str) -> StoreResult<Vec<ContextItem>> {
    let mut statement = conn
        .prepare(&format!(
            "select {CONTEXT_ITEM_COLUMNS} from chat_context_items
             where thread_id = ?1 order by position"
        ))
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map(params![thread_id], context_item_from_row)
        .map_err(|error| error.to_string())?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|error| error.to_string())
}

fn timestamped_id(prefix: &str) -> StoreResult<String> {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| error.to_string())?
        .as_nanos();

    Ok(format!("{prefix}_{nanos}"))
}

/// Backfills the Project boundary for libraries created before RFC 0109.
fn migrate_vaults_to_projects(conn: &mut Connection) -> StoreResult<()> {
    let tx = conn.transaction().map_err(|error| error.to_string())?;
    let legacy_vaults = {
        let mut statement = tx
            .prepare(
                "select id, title from vaults
                 where project_id is null or trim(project_id) = '' order by id",
            )
            .map_err(|error| error.to_string())?;
        let rows = statement
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|error| error.to_string())?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|error| error.to_string())?
    };

    for (vault_id, title) in legacy_vaults {
        let project_id = project_id_for_vault(&vault_id);
        tx.execute(
            "insert into projects (id, title, goal, created_at, updated_at)
             values (?1, ?2, null, datetime('now'), datetime('now'))
             on conflict(id) do nothing",
            params![project_id, title],
        )
        .map_err(|error| error.to_string())?;
        tx.execute(
            "update vaults set project_id = ?1 where id = ?2",
            params![project_id, vault_id],
        )
        .map_err(|error| error.to_string())?;
    }
    tx.commit().map_err(|error| error.to_string())?;

    conn.execute_batch(
        "
        create unique index if not exists idx_vaults_project_id on vaults(project_id);

        create trigger if not exists vaults_require_project_insert
        before insert on vaults
        when new.project_id is null
          or trim(new.project_id) = ''
          or not exists (select 1 from projects where id = new.project_id)
        begin
          select raise(abort, 'Vault must belong to an existing Project');
        end;

        create trigger if not exists vaults_require_project_update
        before update of project_id on vaults
        when new.project_id is null
          or trim(new.project_id) = ''
          or not exists (select 1 from projects where id = new.project_id)
        begin
          select raise(abort, 'Vault must belong to an existing Project');
        end;

        create trigger if not exists projects_delete_owned_vault
        after delete on projects
        begin
          delete from vaults where project_id = old.id;
        end;
        ",
    )
    .map_err(|error| error.to_string())?;
    Ok(())
}

fn normalize_project_title(input: &str) -> StoreResult<String> {
    let title = input.trim();
    if title.is_empty() {
        return Err("Project title cannot be empty".to_string());
    }
    Ok(title.to_string())
}

fn normalize_document_title(input: &str) -> StoreResult<String> {
    let title = input.trim();
    if title.is_empty() {
        return Err("Project document title cannot be empty".to_string());
    }
    Ok(title.to_string())
}

fn project_id_for_vault(vault_id: &str) -> String {
    format!("project:{vault_id}")
}

fn project_create_error(error: rusqlite::Error, title: &str, path: &str) -> String {
    let message = error.to_string();
    if message.contains("UNIQUE constraint failed: vaults.path") {
        format!("Project bibliography path already exists: {path}")
    } else if message.contains("UNIQUE constraint failed: vaults.id")
        || message.contains("UNIQUE constraint failed: projects.id")
    {
        format!("Project already exists: {title}")
    } else {
        message
    }
}

fn delete_unowned_papers(conn: &Connection) -> StoreResult<()> {
    conn.execute(
        "delete from papers
         where not exists (
           select 1 from vault_papers where vault_papers.paper_id = papers.id
         )",
        [],
    )
    .map_err(|error| error.to_string())?;
    Ok(())
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

    fn has_project(snapshot: &LibrarySnapshot, project_id: &str) -> bool {
        snapshot
            .projects
            .iter()
            .any(|project| project.id == project_id)
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

        assert_eq!(snapshot.projects.len(), default_vaults().len());
        assert!(snapshot.project_documents.is_empty());
        assert_eq!(snapshot.vaults.len(), default_vaults().len());
        assert_eq!(snapshot.papers.len(), default_papers().len());
        assert_eq!(snapshot.vault_papers.len(), default_memberships().len());
        assert!(snapshot.document_sources.is_empty());
        assert!(snapshot.document_extractions.is_empty());
        assert!(snapshot.document_pages.is_empty());
        assert!(snapshot.document_assets.is_empty());
        assert!(has_vault(&snapshot, "attention"));
        assert!(has_project(&snapshot, "project:attention"));
        assert_eq!(
            snapshot
                .vaults
                .iter()
                .find(|vault| vault.id == "attention")
                .map(|vault| vault.project_id.as_str()),
            Some("project:attention")
        );
        for project in &snapshot.projects {
            assert_eq!(
                snapshot
                    .vaults
                    .iter()
                    .filter(|vault| vault.project_id == project.id)
                    .count(),
                1,
                "every Project should own exactly one Vault"
            );
        }
        for vault in &snapshot.vaults {
            assert!(
                snapshot
                    .projects
                    .iter()
                    .any(|project| project.id == vault.project_id),
                "every Vault should belong to a returned Project"
            );
        }
        assert!(has_paper(&snapshot, "vaswani2017"));
        assert!(has_membership(&snapshot, "attention", "vaswani2017"));
        assert!(paper(&snapshot, "vaswani2017").active_source_id.is_none());
        assert!(paper(&snapshot, "vaswani2017")
            .active_extraction_id
            .is_none());

        Ok(())
    }

    #[test]
    fn project_creation_is_atomic_and_owns_one_vault() -> StoreResult<()> {
        let db = test_db()?;
        let before = db.store.get_library()?;
        let snapshot = db.store.create_project(&ProjectDraft {
            title: "  LoRA Interpretability  ".to_string(),
            goal: Some("Explain low-rank updates".to_string()),
        })?;

        assert_eq!(snapshot.projects.len(), before.projects.len() + 1);
        assert_eq!(snapshot.vaults.len(), before.vaults.len() + 1);
        let project = snapshot
            .projects
            .iter()
            .find(|project| project.id == "project:lora-interpretability")
            .expect("created Project should exist");
        let owned: Vec<&Vault> = snapshot
            .vaults
            .iter()
            .filter(|vault| vault.project_id == project.id)
            .collect();
        assert_eq!(project.title, "LoRA Interpretability");
        assert_eq!(project.goal.as_deref(), Some("Explain low-rank updates"));
        assert_eq!(owned.len(), 1);
        assert_eq!(owned[0].path, "/LoRA Interpretability");

        let error = db
            .store
            .create_project(&ProjectDraft {
                title: "LoRA Interpretability".to_string(),
                goal: None,
            })
            .expect_err("duplicate Project should fail atomically");
        let after_duplicate = db.store.get_library()?;
        assert!(error.contains("Project already exists"));
        assert_eq!(after_duplicate.projects.len(), snapshot.projects.len());
        assert_eq!(after_duplicate.vaults.len(), snapshot.vaults.len());
        Ok(())
    }

    #[test]
    fn project_document_crud_persists_markdown_and_write_permission() -> StoreResult<()> {
        let db = test_db()?;
        let created = db.store.create_project_document(&ProjectDocumentDraft {
            project_id: "project:attention".to_string(),
            title: "  Related work.md  ".to_string(),
            content: "# Related work\n\nInitial synthesis.".to_string(),
        })?;
        assert_eq!(created.title, "Related work.md");
        assert_eq!(created.format, "markdown");
        assert!(!created.harness_writable);

        let updated = db
            .store
            .update_project_document(&ProjectDocumentUpdate {
                id: created.id.clone(),
                title: "LoRA related work.md".to_string(),
                content: "# LoRA\n\nRevised synthesis.".to_string(),
                harness_writable: true,
            })?;
        assert_eq!(updated.title, "LoRA related work.md");
        assert_eq!(updated.content, "# LoRA\n\nRevised synthesis.");
        assert!(updated.harness_writable);

        let reopened_store = LibraryStore::for_test(db.dir.join("library.sqlite"));
        let reopened = reopened_store.get_project_document(&created.id)?;
        assert_eq!(reopened.content, updated.content);
        assert!(reopened.harness_writable);
        let snapshot = reopened_store.get_library()?;
        let summary = snapshot
            .project_documents
            .iter()
            .find(|summary| summary.id == created.id)
            .expect("document summary should be in the library snapshot");
        assert_eq!(summary.title, updated.title);
        assert!(summary.harness_writable);

        reopened_store.delete_project_document(&created.id)?;
        assert!(reopened_store.get_project_document(&created.id).is_err());
        Ok(())
    }

    #[test]
    fn project_document_requires_an_existing_project_and_cascades_on_delete() -> StoreResult<()> {
        let db = test_db()?;
        let error = db
            .store
            .create_project_document(&ProjectDocumentDraft {
                project_id: "project:missing".to_string(),
                title: "Orphan.md".to_string(),
                content: String::new(),
            })
            .expect_err("document must not be created outside a Project");
        assert_eq!(error, "Project not found: project:missing");

        let document = db.store.create_project_document(&ProjectDocumentDraft {
            project_id: "project:attention".to_string(),
            title: "Meeting notes.md".to_string(),
            content: "Working context, not evidence.".to_string(),
        })?;
        db.store.delete_project("project:attention")?;
        assert!(db.store.get_project_document(&document.id).is_err());
        Ok(())
    }

    #[test]
    fn renaming_project_keeps_vault_identity_and_path() -> StoreResult<()> {
        let db = test_db()?;
        let before = db.store.get_library()?;
        let vault = before
            .vaults
            .iter()
            .find(|vault| vault.project_id == "project:attention")
            .expect("seed Project should own a Vault")
            .clone();
        let snapshot = db.store.rename_project(&ProjectRenameDraft {
            id: "project:attention".to_string(),
            title: "Mechanistic Attention".to_string(),
        })?;
        let renamed = snapshot
            .projects
            .iter()
            .find(|project| project.id == "project:attention")
            .expect("renamed Project should keep its id");
        let after_vault = snapshot
            .vaults
            .iter()
            .find(|candidate| candidate.id == vault.id)
            .expect("owned Vault should remain");

        assert_eq!(renamed.title, "Mechanistic Attention");
        assert_eq!(after_vault.path, vault.path);
        assert_eq!(after_vault.title, vault.title);
        Ok(())
    }

    #[test]
    fn deleting_project_removes_vault_and_preserves_shared_paper() -> StoreResult<()> {
        let db = test_db()?;
        let snapshot = db.store.delete_project("project:self-supervised")?;

        assert!(!has_project(&snapshot, "project:self-supervised"));
        assert!(!has_vault(&snapshot, "self-supervised"));
        assert!(has_paper(&snapshot, "caron2021"));
        assert!(has_membership(&snapshot, "attention", "caron2021"));
        Ok(())
    }

    #[test]
    fn legacy_vault_migration_is_idempotent_and_preserves_identity() -> StoreResult<()> {
        let unique_id = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| error.to_string())?
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "i0i-project-migration-{}-{unique_id}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
        let path = dir.join("library.sqlite");
        {
            let conn = Connection::open(&path).map_err(|error| error.to_string())?;
            conn.execute_batch(
                "create table vaults (
                   id text primary key, title text not null, path text not null unique,
                   created_at text not null, updated_at text not null
                 );
                 insert into vaults values (
                   'legacy-vault', 'Legacy Research', '/legacy/research',
                   datetime('now'), datetime('now')
                 );
                 create table papers (
                   id text primary key, title text not null, authors_json text not null,
                   venue text not null, year integer not null, citations integer not null default 0,
                   tags_json text not null, note_count integer not null default 0,
                   annotation_count integer not null default 0, status text not null,
                   abstract text, created_at text not null, updated_at text not null
                 );
                 insert into papers values (
                   'legacy-paper', 'Legacy Paper', '[]', 'TEST', 2020, 0, '[]', 0, 0,
                   'UNREAD', null, datetime('now'), datetime('now')
                 );
                 create table vault_papers (
                   vault_id text not null, paper_id text not null, added_at text not null,
                   primary key (vault_id, paper_id),
                   foreign key (vault_id) references vaults(id) on delete cascade,
                   foreign key (paper_id) references papers(id) on delete cascade
                 );
                 insert into vault_papers values ('legacy-vault', 'legacy-paper', datetime('now'));",
            )
            .map_err(|error| error.to_string())?;
        }

        let store = LibraryStore::for_test(path);
        store.init()?;
        store.init()?;
        let snapshot = store.get_library()?;
        assert_eq!(snapshot.projects.len(), 1);
        assert_eq!(snapshot.vaults.len(), 1);
        assert_eq!(snapshot.projects[0].id, "project:legacy-vault");
        assert_eq!(snapshot.projects[0].title, "Legacy Research");
        assert_eq!(snapshot.vaults[0].id, "legacy-vault");
        assert_eq!(snapshot.vaults[0].path, "/legacy/research");
        assert_eq!(snapshot.vaults[0].project_id, snapshot.projects[0].id);
        assert!(has_paper(&snapshot, "legacy-paper"));
        assert!(has_membership(&snapshot, "legacy-vault", "legacy-paper"));

        let _ = fs::remove_dir_all(&dir);
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
    fn add_local_html_to_vault_persists_html_source_and_serves_snapshot() -> StoreResult<()> {
        let db = test_db()?;
        let draft = paper_draft("web:deadbeef1234");
        let source_id = "html:deadbeef1234";
        // The snapshot lives on disk; `local_path` points the reader at it.
        let snapshot_path = db.dir.join("source.html");
        fs::write(&snapshot_path, "<article><p>Saved page.</p></article>")
            .map_err(|error| error.to_string())?;
        let local_path = snapshot_path.to_string_lossy().to_string();

        db.store.add_local_html_to_vault(
            &draft,
            "attention",
            source_id,
            "https://example.com/post",
            &local_path,
        )?;

        let snapshot = db.store.get_library()?;
        let source = snapshot
            .document_sources
            .iter()
            .find(|source| source.id == source_id)
            .expect("saved web-page source should exist");
        assert!(has_membership(&snapshot, "attention", "web:deadbeef1234"));
        assert_eq!(source.source_kind, "html");
        assert_eq!(source.status, "cached");
        assert_eq!(source.acquisition_method.as_deref(), Some("html_import"));
        assert_eq!(
            source.source_url.as_deref(),
            Some("https://example.com/post")
        );
        assert_eq!(
            paper(&snapshot, "web:deadbeef1234")
                .active_source_id
                .as_deref(),
            Some(source_id)
        );

        // The reader read-path resolves the source id to its on-disk snapshot.
        assert_eq!(
            db.store.read_html_snapshot(source_id)?,
            "<article><p>Saved page.</p></article>"
        );

        // Idempotent re-add: still one membership, snapshot unchanged.
        db.store.add_local_html_to_vault(
            &draft,
            "attention",
            source_id,
            "https://example.com/post",
            &local_path,
        )?;
        let after = db.store.get_library()?;
        assert_eq!(
            after
                .document_sources
                .iter()
                .filter(|source| source.id == source_id)
                .count(),
            1
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
            &[],
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
        let structure = db.store.extraction_structure(&extraction.id)?;
        assert!(structure
            .blocks
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
            &[],
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
            db.store.extraction_structure(&existing.id)?.blocks[0]
                .text
                .as_deref(),
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
        assert!(db.store.extraction_structure(&forced.id)?.blocks.is_empty());

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
            ..ChatContextSummary::default()
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
            Some(crate::domain::highlight::HighlightColor::Yellow),
            Some("important"),
            &crate::domain::highlight::HighlightAuthor::User,
        )?;
        assert_eq!(
            hl.color,
            Some(crate::domain::highlight::HighlightColor::Yellow)
        );
        assert_eq!(hl.excerpt, "quoted text");

        db.store
            .recolor_highlight(&hl.id, crate::domain::highlight::HighlightColor::Red)?;
        let listed = db.store.list_highlights(paper_id)?;
        assert_eq!(listed.len(), 1);
        assert_eq!(
            listed[0].color,
            Some(crate::domain::highlight::HighlightColor::Red)
        );

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
    fn notes_migrate_from_thread_entries_into_the_highlight_note_field() -> StoreResult<()> {
        let db = test_db()?;
        let paper_id = "vaswani2017";
        let anchor = ThreadAnchor::TextOffset {
            source_id: "src-1".into(),
            start_offset: 3,
            end_offset: 12,
            selected_text: "abc".into(),
        };
        // Legacy flow: a note entry on an anchored thread, linked to a highlight.
        db.store
            .add_note_at_anchor_with_creation("paper", paper_id, &anchor, "my note")?;
        db.store.migrate_threads_to_highlights()?;

        // RFC 0061: move the note entry into the highlight's `note` field.
        let migrated = db.store.migrate_notes_into_highlight_field()?;
        assert_eq!(migrated, 1);

        let highlights = db.store.list_highlights(paper_id)?;
        assert_eq!(highlights.len(), 1);
        assert_eq!(highlights[0].note.as_deref(), Some("my note"));

        // Idempotent: the note field is now set, so a second run does nothing.
        assert_eq!(db.store.migrate_notes_into_highlight_field()?, 0);
        Ok(())
    }

    #[test]
    fn note_migration_moves_notes_out_of_pins_into_the_passage_note() -> StoreResult<()> {
        // Notes pin by default in the legacy flow, so they show in the Pins tab
        // today. The RFC 0061 model change moves a note onto the passage (its
        // `note` field); the migration must preserve the content and drop it
        // from Pins — not silently lose it.
        let db = test_db()?;
        let paper_id = "vaswani2017";
        let anchor = ThreadAnchor::TextOffset {
            source_id: "src-1".into(),
            start_offset: 3,
            end_offset: 12,
            selected_text: "abc".into(),
        };
        db.store
            .add_note_at_anchor_with_creation("paper", paper_id, &anchor, "kept note")?;
        db.store.migrate_threads_to_highlights()?;

        // Before: the auto-pinned note is in Pins.
        assert_eq!(
            db.store.list_pinned_chat_entries("paper", paper_id)?.len(),
            1
        );

        assert_eq!(db.store.migrate_notes_into_highlight_field()?, 1);

        // After: content lives on the passage; Pins no longer carries the note.
        let highlights = db.store.list_highlights(paper_id)?;
        assert_eq!(highlights[0].note.as_deref(), Some("kept note"));
        assert_eq!(
            db.store.list_pinned_chat_entries("paper", paper_id)?.len(),
            0
        );
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
            Some(HighlightColor::Green),
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
        assert_eq!(highlights[0].color, Some(HighlightColor::Green));

        assert_eq!(
            db.store.thread_highlight_id(&write.view.thread.id)?,
            Some(created.id)
        );

        // Idempotent: second run migrates nothing further.
        assert_eq!(db.store.migrate_threads_to_highlights()?, 0);

        Ok(())
    }

    /// RFC 0072: RFC 0061 made `highlights.color` nullable in `create_schema`
    /// only — inert on a database that already has the table — so every vault
    /// created before it still rejected the color-less passages Note/Ask
    /// create. `test_db()` builds the already-correct schema, so this test has
    /// to materialize the legacy shape by hand; that is the whole point of it.
    #[test]
    fn init_relaxes_legacy_not_null_color() -> StoreResult<()> {
        use crate::domain::highlight::{HighlightAuthor, HighlightColor, Locator};

        let unique_id = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| error.to_string())?
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "i0i-legacy-color-{}-{unique_id}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
        let path = dir.join("library.sqlite");

        // A pre-RFC-0061 vault, byte for byte: `color text not null`, and no
        // `note` column (that one arrives via `alter table` in `init`).
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
                  'hl-legacy', 'vaswani2017', 's', 'pdf_rect', null, null, 0, '[]', 'kept',
                  'yellow', null, 'user', null, datetime('now'), datetime('now')
                );
                ",
            )
            .map_err(|error| error.to_string())?;
        }

        let store = LibraryStore::for_test(path);
        store.init()?;
        store.init()?; // idempotent — the second launch must be a no-op

        // The color-less passage RFC 0061 promised and the legacy schema refused.
        let created = store.insert_highlight(
            "vaswani2017",
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

        // And the rebuild kept the existing annotation, color and all.
        let all = store.list_highlights("vaswani2017")?;
        let legacy = all
            .iter()
            .find(|highlight| highlight.id == "hl-legacy")
            .expect("legacy highlight survived the rebuild");
        assert_eq!(legacy.color, Some(HighlightColor::Yellow));
        assert_eq!(legacy.excerpt, "kept");

        let _ = fs::remove_dir_all(&dir);
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

    use crate::domain::highlight::{HighlightAuthor, HighlightColor, Locator};

    /// RFC 0079 R6.3: the FTS delete trigger now keys on `rowid` rather than
    /// the `unindexed` `chunk_id` column — ~890 ms of full-index scans per
    /// paper deletion became ~2 ms. The risk that buys is a stale FTS row
    /// surviving its chunk and answering searches with a dead id, so the
    /// cascade is checked end to end rather than the trigger in isolation.
    #[test]
    fn deleting_a_paper_leaves_no_orphan_fts_rows() -> StoreResult<()> {
        let db = test_db()?;
        let paper_id = "fts-cascade-paper";
        let extraction = extracted_paper(
            &db,
            paper_id,
            &[
                (
                    "paragraph",
                    "Scaled dot-product attention divides by sqrt(d_k).",
                ),
                (
                    "paragraph",
                    "Multi-head attention runs several in parallel.",
                ),
            ],
        )?;
        let chunk_ids: Vec<String> = db
            .store
            .chunks_for_extraction(&extraction.id)?
            .into_iter()
            .map(|chunk| chunk.id)
            .collect();
        assert!(!chunk_ids.is_empty(), "the fixture produced chunks");

        let fts_rows = |ids: &[String]| -> StoreResult<i64> {
            let conn = Connection::open(&db.store.db_path).map_err(|e| e.to_string())?;
            let mut total = 0;
            for id in ids {
                total += conn
                    .query_row(
                        "select count(*) from document_chunks_fts where chunk_id = ?1",
                        params![id],
                        |row| row.get::<_, i64>(0),
                    )
                    .map_err(|e| e.to_string())?;
            }
            Ok(total)
        };

        assert_eq!(
            fts_rows(&chunk_ids)?,
            chunk_ids.len() as i64,
            "every chunk is indexed before the delete"
        );

        db.store.delete_paper_globally(paper_id)?;

        assert_eq!(fts_rows(&chunk_ids)?, 0, "and none of them outlive it");

        Ok(())
    }

    /// RFC 0079 R6.2: `highlights` declares no FK to `papers`, so nothing
    /// cleaned these up — a deleted paper left its marks behind forever.
    #[test]
    fn delete_paper_globally_removes_its_highlights() -> StoreResult<()> {
        let db = test_db()?;
        let locator = Locator::TextOffset {
            source_id: "src".into(),
            start_offset: 0,
            end_offset: 9,
        };
        db.store.insert_highlight(
            "caron2021",
            &locator,
            "excerpt",
            Some(HighlightColor::Yellow),
            None,
            &HighlightAuthor::User,
        )?;

        db.store.delete_paper_globally("caron2021")?;

        assert!(db.store.list_highlights("caron2021")?.is_empty());

        Ok(())
    }

    /// RFC 0079 R1.3: deleting the mark used to leave the thread alive with no
    /// row rendering it — the conversation became unreachable, not deleted.
    #[test]
    fn delete_annotation_takes_its_thread_with_it() -> StoreResult<()> {
        let db = test_db()?;
        let anchor = ThreadAnchor::TextOffset {
            source_id: "src".into(),
            start_offset: 10,
            end_offset: 24,
            selected_text: "a passage".into(),
        };
        db.store
            .add_note_at_anchor("paper", "vaswani2017", &anchor, "my note")?;
        let highlight = db.store.insert_highlight(
            "vaswani2017",
            &Locator::TextOffset {
                source_id: "src".into(),
                start_offset: 10,
                end_offset: 24,
            },
            "a passage",
            Some(HighlightColor::Yellow),
            None,
            &HighlightAuthor::User,
        )?;

        let threads = db.store.delete_annotation(&highlight.id)?;

        assert_eq!(threads, 1, "the anchored thread goes with the mark");
        assert!(db.store.get_highlight(&highlight.id)?.is_none());
        assert!(db
            .store
            .list_chat_threads("paper", "vaswani2017")?
            .is_empty());

        Ok(())
    }

    /// A sticky note has a *point* locator and never has a thread, so deleting
    /// one must not reach for anchor columns that cannot match.
    #[test]
    fn delete_annotation_on_a_sticky_removes_only_the_sticky() -> StoreResult<()> {
        let db = test_db()?;
        db.store.add_note_at_anchor(
            "paper",
            "vaswani2017",
            &ThreadAnchor::Document,
            "unrelated",
        )?;
        let sticky = db.store.insert_highlight(
            "vaswani2017",
            &Locator::PdfPoint {
                source_id: "src".into(),
                page_index: 2,
                x: 0.5,
                y: 0.25,
            },
            "",
            None,
            None,
            &HighlightAuthor::User,
        )?;

        let threads = db.store.delete_annotation(&sticky.id)?;

        assert_eq!(threads, 0);
        assert!(db.store.get_highlight(&sticky.id)?.is_none());
        assert_eq!(
            db.store.list_chat_threads("paper", "vaswani2017")?.len(),
            1,
            "the whole-paper thread is untouched"
        );

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

    /// An `EXTRACTOR_VERSION` bump must actually migrate an existing library.
    ///
    /// This is the test that was missing: RFC 0075 claimed the bump alone was
    /// the whole migration, but `stale_document_extractions` only looks at
    /// status and `cached_pdf_sources_without_ready_extraction` accepts a ready
    /// extraction of any version. A real library sat at the old extractor —
    /// page-sized blocks, no spans — while every unit test passed.
    #[test]
    fn an_extractor_version_bump_marks_existing_extractions_outdated() -> StoreResult<()> {
        let db = test_db()?;
        // extracted_paper() writes at "0.2.0".
        let extraction = extracted_paper(&db, "aging-paper", &[("paragraph", "body")])?;
        assert_eq!(extraction.status, "ready");

        // Same version: nothing to do.
        assert!(db
            .store
            .outdated_document_extractions("pdfium_basic", "0.2.0")?
            .is_empty());

        // A newer extractor must see it as outdated...
        let outdated = db
            .store
            .outdated_document_extractions("pdfium_basic", "0.3.0")?;
        assert_eq!(outdated.len(), 1);
        assert_eq!(outdated[0].id, extraction.id);

        // ...while the sweeps that already existed do not, which is exactly why
        // this method has to exist.
        assert!(db
            .store
            .stale_document_extractions("pdfium_basic")?
            .is_empty());
        assert!(db
            .store
            .cached_pdf_sources_without_ready_extraction("pdfium_basic")?
            .is_empty());

        // A different extractor's rows are not ours to re-run.
        assert!(db
            .store
            .outdated_document_extractions("mineru", "0.3.0")?
            .is_empty());
        Ok(())
    }

    // ---- RFC 0076: scope resolution, vector index ----

    /// The conjunction table from RFC 0076, exhaustively.
    ///
    /// Scope is the part callers get wrong, and every wrong answer is silent:
    /// too wide searches the library when you meant one PDF, too narrow returns
    /// nothing and looks like "no matches".
    #[test]
    fn search_scope_intersects_papers_and_vaults() -> StoreResult<()> {
        let db = test_db()?;
        // Seeded library: vault "attention" holds "vaswani2017".
        let snapshot = db.store.create_vault(&VaultDraft {
            path: format!("{}/other-vault", db.dir.display()),
        })?;
        let other_vault = snapshot
            .vaults
            .iter()
            .find(|vault| vault.path.ends_with("other-vault"))
            .expect("created vault should be in the snapshot")
            .id
            .clone();
        db.store
            .add_paper_to_vaults(&paper_draft("lonely"), &[other_vault.clone()])?;

        let none: Vec<String> = Vec::new();

        // Both empty: the whole library.
        let all = db.store.resolve_search_scope(&none, &none)?;
        assert!(all.contains(&"vaswani2017".to_string()));
        assert!(all.contains(&"lonely".to_string()));

        // Paper only.
        assert_eq!(
            db.store
                .resolve_search_scope(&["vaswani2017".to_string()], &none)?,
            vec!["vaswani2017".to_string()]
        );

        // Vault only.
        assert_eq!(
            db.store
                .resolve_search_scope(&none, &[other_vault.clone()])?,
            vec!["lonely".to_string()]
        );

        // Both, intersecting.
        assert_eq!(
            db.store
                .resolve_search_scope(&["vaswani2017".to_string()], &["attention".to_string()])?,
            vec!["vaswani2017".to_string()]
        );

        // Both, disjoint: the paper is not in that vault. Empty, not an error.
        assert!(db
            .store
            .resolve_search_scope(&["lonely".to_string()], &["attention".to_string()])?
            .is_empty());

        // An id that does not exist drops out rather than being searched for.
        assert!(db
            .store
            .resolve_search_scope(&["ghost".to_string()], &none)?
            .is_empty());
        Ok(())
    }

    #[test]
    fn semantic_ranking_returns_nearest_chunks_within_scope() -> StoreResult<()> {
        let db = test_db()?;
        let near = extracted_paper(&db, "near-paper", &[("paragraph", "attention text")])?;
        let far = extracted_paper(&db, "far-paper", &[("paragraph", "unrelated text")])?;

        // Hand-built vectors: near-paper sits on the query axis, far-paper does not.
        let near_chunk = db.store.chunks_for_extraction(&near.id)?.remove(0);
        let far_chunk = db.store.chunks_for_extraction(&far.id)?.remove(0);
        let mut on_axis = vec![0.0f32; VECTOR_DIMENSIONS];
        on_axis[0] = 1.0;
        let mut off_axis = vec![0.0f32; VECTOR_DIMENSIONS];
        off_axis[1] = 1.0;

        db.store
            .save_chunk_embedding(&near_chunk.id, "m", "1", CHUNK_VERSION, &on_axis)?;
        db.store
            .save_chunk_embedding(&far_chunk.id, "m", "1", CHUNK_VERSION, &off_axis)?;

        let both = vec!["near-paper".to_string(), "far-paper".to_string()];
        let ranked = db.store.semantic_chunk_ranking(&both, &on_axis, 10)?;
        assert_eq!(ranked.len(), 2);
        assert_eq!(ranked[0].0, near_chunk.id, "nearest must rank first");

        // Scoping to the far paper returns *its* chunk, not the globally nearer
        // one — the whole reason paper_id is a partition key.
        let scoped = db
            .store
            .semantic_chunk_ranking(&["far-paper".to_string()], &on_axis, 10)?;
        assert_eq!(scoped.len(), 1);
        assert_eq!(scoped[0].0, far_chunk.id);
        Ok(())
    }

    #[test]
    fn a_re_embed_replaces_the_vector_rather_than_stacking() -> StoreResult<()> {
        let db = test_db()?;
        let extraction = extracted_paper(&db, "reembed-paper", &[("paragraph", "text")])?;
        let chunk = db.store.chunks_for_extraction(&extraction.id)?.remove(0);

        let mut vector = vec![0.0f32; VECTOR_DIMENSIONS];
        vector[0] = 1.0;
        db.store
            .save_chunk_embedding(&chunk.id, "m", "1", CHUNK_VERSION, &vector)?;
        db.store
            .save_chunk_embedding(&chunk.id, "m", "1", CHUNK_VERSION, &vector)?;

        let ranked =
            db.store
                .semantic_chunk_ranking(&["reembed-paper".to_string()], &vector, 10)?;
        assert_eq!(
            ranked.len(),
            1,
            "vec0 has no upsert; the insert must replace"
        );
        Ok(())
    }

    /// The vector index is a virtual table: no foreign keys, no cascade. Only
    /// the trigger keeps it consistent, and an orphaned vector is worse than an
    /// orphaned FTS row — it returns a chunk id that no longer resolves, so
    /// hydration drops it and a search quietly returns fewer results.
    #[test]
    fn deleting_a_paper_leaves_no_orphaned_vectors() -> StoreResult<()> {
        let db = test_db()?;
        let extraction = extracted_paper(&db, "vector-doomed", &[("paragraph", "text")])?;
        let chunk = db.store.chunks_for_extraction(&extraction.id)?.remove(0);
        let mut vector = vec![0.0f32; VECTOR_DIMENSIONS];
        vector[0] = 1.0;
        db.store
            .save_chunk_embedding(&chunk.id, "m", "1", CHUNK_VERSION, &vector)?;

        db.store.delete_paper_globally("vector-doomed")?;

        let conn = db.store.open_connection()?;
        let remaining: i64 = conn
            .query_row("select count(*) from document_chunk_vectors", [], |row| {
                row.get(0)
            })
            .map_err(|error| error.to_string())?;
        assert_eq!(remaining, 0);
        Ok(())
    }

    #[test]
    fn vector_backfill_indexes_embeddings_and_is_idempotent() -> StoreResult<()> {
        let db = test_db()?;
        let extraction = extracted_paper(&db, "backfill-paper", &[("paragraph", "text")])?;
        let chunk = db.store.chunks_for_extraction(&extraction.id)?.remove(0);
        let mut vector = vec![0.0f32; VECTOR_DIMENSIONS];
        vector[0] = 1.0;
        db.store
            .save_chunk_embedding(&chunk.id, "m", "1", CHUNK_VERSION, &vector)?;

        // Simulate an embedding written before the index existed.
        let conn = db.store.open_connection()?;
        conn.execute("delete from document_chunk_vectors", [])
            .map_err(|error| error.to_string())?;

        assert_eq!(db.store.index_missing_chunk_vectors()?, 1);
        assert_eq!(
            db.store.index_missing_chunk_vectors()?,
            0,
            "a second backfill must be a no-op"
        );
        Ok(())
    }

    /// A vector of the wrong width means the row came from a different model.
    /// Indexing it would put two vector spaces in one index, and nothing
    /// downstream could tell them apart.
    #[test]
    fn a_dimension_mismatch_is_skipped_not_indexed() -> StoreResult<()> {
        let db = test_db()?;
        let extraction = extracted_paper(&db, "mismatch-paper", &[("paragraph", "text")])?;
        let chunk = db.store.chunks_for_extraction(&extraction.id)?.remove(0);

        db.store
            .save_chunk_embedding(&chunk.id, "other-model", "1", CHUNK_VERSION, &[0.5; 8])?;

        let conn = db.store.open_connection()?;
        let indexed: i64 = conn
            .query_row("select count(*) from document_chunk_vectors", [], |row| {
                row.get(0)
            })
            .map_err(|error| error.to_string())?;
        assert_eq!(indexed, 0);

        // The embedding itself is still stored — it is canonical, and a later
        // index rebuilt for that model can use it.
        let stored: i64 = conn
            .query_row(
                "select count(*) from document_chunk_embeddings",
                [],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        assert_eq!(stored, 1);
        Ok(())
    }

    /// Spike (RFC 0076): does sqlite-vec register and answer a KNN query
    /// against our bundled SQLite? Statically linked via `sqlite3_auto_extension`,
    /// so there is no `.dylib` to find at runtime — but the crate is alpha and
    /// links its own SQLite symbols, so this is worth proving rather than
    /// assuming.
    #[test]
    fn sqlite_vec_registers_and_answers_knn() -> StoreResult<()> {
        unsafe {
            rusqlite::ffi::sqlite3_auto_extension(Some(std::mem::transmute::<
                *const (),
                unsafe extern "C" fn(
                    *mut rusqlite::ffi::sqlite3,
                    *mut *mut i8,
                    *const rusqlite::ffi::sqlite3_api_routines,
                ) -> i32,
            >(
                sqlite_vec::sqlite3_vec_init as *const (),
            )));
        }

        let conn = Connection::open_in_memory().map_err(|error| error.to_string())?;

        let version: String = conn
            .query_row("select vec_version()", [], |row| row.get(0))
            .map_err(|error| format!("vec_version failed: {error}"))?;
        assert!(!version.is_empty(), "sqlite-vec did not register");

        conn.execute_batch(
            "create virtual table v using vec0(chunk_id text primary key, embedding float[4]);",
        )
        .map_err(|error| format!("vec0 create failed: {error}"))?;

        for (id, vector) in [
            ("a", [1.0f32, 0.0, 0.0, 0.0]),
            ("b", [0.0f32, 1.0, 0.0, 0.0]),
        ] {
            let blob: Vec<u8> = vector.iter().flat_map(|v| v.to_le_bytes()).collect();
            conn.execute(
                "insert into v (chunk_id, embedding) values (?1, ?2)",
                params![id, blob],
            )
            .map_err(|error| format!("vec0 insert failed: {error}"))?;
        }

        let query: Vec<u8> = [0.9f32, 0.1, 0.0, 0.0]
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect();
        let nearest: String = conn
            .query_row(
                "select chunk_id from v where embedding match ?1 and k = 1 order by distance",
                params![query],
                |row| row.get(0),
            )
            .map_err(|error| format!("knn failed: {error}"))?;

        assert_eq!(nearest, "a");
        Ok(())
    }

    /// Spike (RFC 0076): can a KNN query be *scoped* to a set of papers?
    ///
    /// This decides whether semantic search can honour the scope filter at all.
    /// A plain `k = n` KNN searches globally, so post-filtering to one paper
    /// could return zero hits when that paper's chunks all rank below the
    /// global top-n. A partition key makes `k` mean "k within this paper".
    #[test]
    fn vec0_partition_key_scopes_knn_per_paper() -> StoreResult<()> {
        register_sqlite_vec_for_test();
        let conn = Connection::open_in_memory().map_err(|error| error.to_string())?;

        conn.execute_batch(
            "
            create virtual table v using vec0(
              paper_id text partition key,
              chunk_id text primary key,
              embedding float[4]
            );
            ",
        )
        .map_err(|error| format!("partitioned vec0 create failed: {error}"))?;

        // Paper A holds everything close to the query; paper B holds only
        // distant vectors. A global top-2 would be entirely paper A.
        let rows: [(&str, &str, [f32; 4]); 4] = [
            ("paper-a", "a1", [1.0, 0.0, 0.0, 0.0]),
            ("paper-a", "a2", [0.99, 0.01, 0.0, 0.0]),
            ("paper-b", "b1", [0.0, 0.0, 1.0, 0.0]),
            ("paper-b", "b2", [0.0, 0.0, 0.0, 1.0]),
        ];
        for (paper_id, chunk_id, vector) in rows {
            let blob: Vec<u8> = vector.iter().flat_map(|v| v.to_le_bytes()).collect();
            conn.execute(
                "insert into v (paper_id, chunk_id, embedding) values (?1, ?2, ?3)",
                params![paper_id, chunk_id, blob],
            )
            .map_err(|error| format!("partitioned insert failed: {error}"))?;
        }

        let query: Vec<u8> = [1.0f32, 0.0, 0.0, 0.0]
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect();

        let mut stmt = conn
            .prepare(
                "
                select chunk_id from v
                where embedding match ?1 and k = 2 and paper_id = ?2
                order by distance
                ",
            )
            .map_err(|error| error.to_string())?;
        let scoped: Vec<String> = stmt
            .query_map(params![query, "paper-b"], |row| row.get::<_, String>(0))
            .map_err(|error| format!("partitioned knn failed: {error}"))?
            .collect::<rusqlite::Result<Vec<String>>>()
            .map_err(|error| error.to_string())?;

        // The whole point: asking for 2 within paper-b returns paper-b's two
        // chunks, not paper-a's globally-nearer ones.
        assert_eq!(scoped.len(), 2);
        assert!(scoped.iter().all(|id| id.starts_with('b')));
        Ok(())
    }

    /// A partition filter must accept a *set* of papers (RFC 0076).
    ///
    /// The whole multi-paper scope design rests on this: one KNN with
    /// `paper_id in (...)` serves any scope. Without it we would need one query
    /// per paper merged afterwards. Asserted rather than assumed, because the
    /// dangerous failure is an `in` clause that compiles and is then ignored —
    /// that returns out-of-scope chunks with no error.
    #[test]
    fn vec0_partition_filter_accepts_a_set_of_papers() -> StoreResult<()> {
        register_sqlite_vec_for_test();
        let conn = Connection::open_in_memory().map_err(|error| error.to_string())?;
        conn.execute_batch(
            "
            create virtual table v using vec0(
              paper_id text partition key,
              chunk_id text primary key,
              embedding float[4]
            );
            ",
        )
        .map_err(|error| error.to_string())?;

        // Three chunks per paper, and `k = 2` below. The counts discriminate:
        // 2 rows means `k` is a total across the filtered partitions, 4 means
        // it is per-partition. That distinction sets how `candidates` must be
        // sized — a per-partition `k` would make a whole-library search fetch
        // `limit * 4` rows *per paper* and quietly blow the latency budget.
        for (paper_id, chunk_id, vector) in [
            ("paper-a", "a1", [1.0f32, 0.0, 0.0, 0.0]),
            ("paper-a", "a2", [0.98f32, 0.02, 0.0, 0.0]),
            ("paper-a", "a3", [0.96f32, 0.04, 0.0, 0.0]),
            ("paper-b", "b1", [0.9f32, 0.1, 0.0, 0.0]),
            ("paper-b", "b2", [0.88f32, 0.12, 0.0, 0.0]),
            ("paper-b", "b3", [0.86f32, 0.14, 0.0, 0.0]),
            ("paper-c", "c1", [0.0f32, 0.0, 1.0, 0.0]),
            ("paper-c", "c2", [0.0f32, 0.0, 0.9, 0.1]),
            ("paper-c", "c3", [0.0f32, 0.0, 0.8, 0.2]),
        ] {
            let blob: Vec<u8> = vector.iter().flat_map(|v| v.to_le_bytes()).collect();
            conn.execute(
                "insert into v (paper_id, chunk_id, embedding) values (?1, ?2, ?3)",
                params![paper_id, chunk_id, blob],
            )
            .map_err(|error| error.to_string())?;
        }

        let query: Vec<u8> = [1.0f32, 0.0, 0.0, 0.0]
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect();

        let attempted = conn
            .prepare(
                "
                select chunk_id from v
                where embedding match ?1 and k = 2
                  and paper_id in ('paper-a', 'paper-b')
                order by distance
                ",
            )
            .and_then(|mut stmt| {
                stmt.query_map(params![query], |row| row.get::<_, String>(0))?
                    .collect::<rusqlite::Result<Vec<String>>>()
            });

        let hits = attempted.map_err(|error| format!("`in` partition filter rejected: {error}"))?;

        assert!(
            !hits.iter().any(|id| id.starts_with('c')),
            "out-of-scope paper leaked through the filter: {hits:?}"
        );

        // `k` is a total across the filtered partitions, not per-partition.
        // `semantic_chunk_ranking` therefore passes `limit * 4` once, whatever
        // the scope size; if this ever flips, that constant has to be divided
        // by the number of papers or the whole-library path drowns in rows.
        // `k` is **per-partition**, not a total: `k = 2` over two partitions
        // returns four rows. `semantic_chunk_ranking` divides its candidate
        // budget by the scope size because of this — passing the budget through
        // unchanged would fetch `limit * 4` rows *per paper*, which is ~24,000
        // rows for a 500-paper library and silently blows the latency budget
        // while still returning correct results.
        assert_eq!(
            hits.len(),
            4,
            "k is per-partition; if this changes, revisit per_partition_k"
        );
        Ok(())
    }

    /// Registering twice in one test binary is harmless — SQLite keeps a list of
    /// auto-extensions and skips duplicates — but the tests share a process, so
    /// this keeps the intent obvious at each call site.
    fn register_sqlite_vec_for_test() {
        unsafe {
            rusqlite::ffi::sqlite3_auto_extension(Some(std::mem::transmute::<
                *const (),
                unsafe extern "C" fn(
                    *mut rusqlite::ffi::sqlite3,
                    *mut *mut i8,
                    *const rusqlite::ffi::sqlite3_api_routines,
                ) -> i32,
            >(
                sqlite_vec::sqlite3_vec_init as *const (),
            )));
        }
    }

    // ---- RFC 0075: chunks, FTS5, embeddings ----

    /// A ready extraction carrying `blocks`, so the chunk tests can start from
    /// a realistic document rather than poking rows in by hand.
    fn extracted_paper(
        db: &TestDb,
        paper_id: &str,
        blocks: &[(&str, &str)],
    ) -> StoreResult<DocumentExtraction> {
        let draft = paper_draft_with_pdf(paper_id, &format!("https://example.test/{paper_id}.pdf"));
        db.store
            .add_paper_to_vaults(&draft, &["attention".to_string()])?;
        let source = db.store.get_document_sources(paper_id)?.remove(0);
        db.store
            .set_document_source_cached(&source.id, "/tmp/chunked.pdf")?;

        let extraction = db.store.start_document_extraction(
            &source.id,
            "pdfium_basic",
            "0.2.0",
            &format!("pdfium_basic:{}", source.id),
            false,
        )?;

        let mut offset = 0_i64;
        let rows: Vec<DocumentBlock> = blocks
            .iter()
            .enumerate()
            .map(|(index, (kind, text))| {
                if index > 0 {
                    offset += 2;
                }
                let start = offset;
                offset += text.chars().count() as i64;
                DocumentBlock {
                    id: format!("{}:block:0:{index}", extraction.id),
                    paper_id: extraction.paper_id.clone(),
                    source_id: extraction.source_id.clone(),
                    extraction_id: extraction.id.clone(),
                    page_index: 0,
                    block_index: index as i32,
                    reading_order: index as i32,
                    kind: (*kind).to_string(),
                    text: Some((*text).to_string()),
                    asset_id: None,
                    source_start: Some(start),
                    source_end: Some(offset),
                    bbox_json: None,
                }
            })
            .collect();

        db.store.finish_document_extraction(
            &extraction.id,
            &[extraction_page(&extraction)],
            &rows,
            &[],
        )
    }

    #[test]
    fn finishing_an_extraction_writes_chunks_in_the_same_transaction() -> StoreResult<()> {
        let db = test_db()?;
        let extraction = extracted_paper(
            &db,
            "chunked-paper",
            &[
                ("heading", "Introduction"),
                ("paragraph", "Transformers rely entirely on attention."),
            ],
        )?;

        // The invariant: a ready extraction always has chunks.
        assert_eq!(extraction.status, "ready");
        let chunks = db.store.chunks_for_extraction(&extraction.id)?;
        assert!(!chunks.is_empty());
        assert_eq!(chunks[0].heading_path.as_deref(), Some("Introduction"));
        assert_eq!(chunks[0].chunk_version, CHUNK_VERSION);
        assert!(!chunks[0].block_ids.is_empty());
        Ok(())
    }

    /// Not a formality: FTS5 is a compile-time option, and every chunk search
    /// silently returns nothing if the bundled SQLite lacks it.
    #[test]
    fn fts5_round_trips_chunk_text() -> StoreResult<()> {
        let db = test_db()?;
        extracted_paper(
            &db,
            "searchable-paper",
            &[(
                "paragraph",
                "The encoder contains a stack of six identical layers.",
            )],
        )?;

        let hits = db
            .store
            .search_chunks_lexical("searchable-paper", "identical layers", 10)?;
        assert_eq!(hits.len(), 1);
        assert!(hits[0].text.contains("identical layers"));

        let misses = db
            .store
            .search_chunks_lexical("searchable-paper", "convolutional", 10)?;
        assert!(misses.is_empty());
        Ok(())
    }

    /// FTS5 query syntax is a language — an unescaped `(` or a bare `AND` is a
    /// syntax error, not a search. Users type these.
    #[test]
    fn lexical_search_survives_punctuation_in_the_query() -> StoreResult<()> {
        let db = test_db()?;
        extracted_paper(
            &db,
            "punctuation-paper",
            &[("paragraph", "We evaluate BLEU (revised) on the task.")],
        )?;

        for query in ["BLEU (revised)", "AND", "\"quoted", "*", "NEAR the"] {
            db.store
                .search_chunks_lexical("punctuation-paper", query, 10)
                .unwrap_or_else(|error| panic!("query {query:?} should not error: {error}"));
        }

        let hits = db
            .store
            .search_chunks_lexical("punctuation-paper", "BLEU (revised)", 10)?;
        assert_eq!(hits.len(), 1);
        Ok(())
    }

    #[test]
    fn re_extraction_leaves_no_orphaned_chunk_rows() -> StoreResult<()> {
        let db = test_db()?;
        let extraction = extracted_paper(
            &db,
            "reextracted-paper",
            &[("paragraph", "Original body text about attention.")],
        )?;

        let chunks = db.store.chunks_for_extraction(&extraction.id)?;
        assert!(!chunks.is_empty());
        // Full width, so it actually reaches the vector index — a short vector
        // would be skipped by the dimension guard and this test would pass
        // without ever proving the vector teardown works.
        let mut vector = vec![0.0f32; VECTOR_DIMENSIONS];
        vector[0] = 1.0;
        db.store
            .save_chunk_embedding(&chunks[0].id, "test-model", "1", CHUNK_VERSION, &vector)?;
        let conn = db.store.open_connection()?;
        let indexed: i64 = conn
            .query_row("select count(*) from document_chunk_vectors", [], |row| {
                row.get(0)
            })
            .map_err(|error| error.to_string())?;
        assert_eq!(indexed, 1, "vector must be indexed before we test teardown");
        drop(conn);

        // Forcing a restart tears the extraction's children down.
        db.store.start_document_extraction(
            &extraction.source_id,
            "pdfium_basic",
            "0.2.0",
            &format!("pdfium_basic:{}", extraction.source_id),
            true,
        )?;

        let conn = db.store.open_connection()?;
        let counts: (i64, i64, i64, i64, i64) = conn
            .query_row(
                "
                select
                  (select count(*) from document_chunks),
                  (select count(*) from document_chunk_blocks),
                  (select count(*) from document_chunk_embeddings),
                  (select count(*) from document_chunks_fts),
                  (select count(*) from document_chunk_vectors)
                ",
                [],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                },
            )
            .map_err(|error| error.to_string())?;

        // The two virtual tables are what matter: neither participates in
        // foreign key cascades, so only their triggers keep them consistent.
        // This is also the path that runs on the user's next launch, when the
        // EXTRACTOR_VERSION bump re-extracts everything.
        assert_eq!(
            counts,
            (0, 0, 0, 0, 0),
            "chunks/blocks/embeddings/fts/vectors"
        );
        Ok(())
    }

    /// The other cascade path: papers → extractions → chunks. It never runs
    /// `clear_extraction_children`, so only the trigger keeps FTS consistent.
    #[test]
    fn deleting_a_paper_leaves_no_orphaned_fts_rows() -> StoreResult<()> {
        let db = test_db()?;
        extracted_paper(
            &db,
            "doomed-paper",
            &[("paragraph", "Text that should not outlive its paper.")],
        )?;

        db.store.delete_paper_globally("doomed-paper")?;

        let conn = db.store.open_connection()?;
        let fts_rows: i64 = conn
            .query_row("select count(*) from document_chunks_fts", [], |row| {
                row.get(0)
            })
            .map_err(|error| error.to_string())?;
        assert_eq!(fts_rows, 0);
        Ok(())
    }

    #[test]
    fn embedding_coverage_counts_the_gap() -> StoreResult<()> {
        let db = test_db()?;
        let extraction = extracted_paper(
            &db,
            "coverage-paper",
            &[
                ("heading", "First"),
                ("paragraph", &"word ".repeat(600)),
                ("heading", "Second"),
                ("paragraph", &"other ".repeat(600)),
            ],
        )?;

        let chunks = db.store.chunks_for_extraction(&extraction.id)?;
        assert!(chunks.len() >= 2, "expected several chunks to embed");

        let before = db
            .store
            .embedding_coverage("coverage-paper", "test-model", "1")?;
        assert_eq!(before.embedded, 0);
        assert_eq!(before.chunks, chunks.len() as i64);

        db.store.save_chunk_embedding(
            &chunks[0].id,
            "test-model",
            "1",
            CHUNK_VERSION,
            &[0.5; 8],
        )?;

        let after = db
            .store
            .embedding_coverage("coverage-paper", "test-model", "1")?;
        assert_eq!(after.embedded, 1);

        // A different model shares no coverage — that is what makes a model
        // swap a re-embed rather than a silent mix of vector spaces.
        let other = db
            .store
            .embedding_coverage("coverage-paper", "other-model", "1")?;
        assert_eq!(other.embedded, 0);
        Ok(())
    }

    #[test]
    fn vault_paper_centroids_remain_separate() -> StoreResult<()> {
        let db = test_db()?;
        let first = extracted_paper(&db, "centroid-a", &[("paragraph", "first paper")])?;
        let second = extracted_paper(&db, "centroid-b", &[("paragraph", "second paper")])?;
        let first_chunk = db.store.chunks_for_extraction(&first.id)?.remove(0);
        let second_chunk = db.store.chunks_for_extraction(&second.id)?.remove(0);
        db.store.save_chunk_embedding(
            &first_chunk.id,
            "test-model",
            "1",
            CHUNK_VERSION,
            &[1.0, 0.0],
        )?;
        db.store.save_chunk_embedding(
            &second_chunk.id,
            "test-model",
            "1",
            CHUNK_VERSION,
            &[0.0, 1.0],
        )?;

        let centroids = db
            .store
            .vault_paper_embedding_centroids("attention", "test-model", "1")?;

        assert!(centroids.contains(&("centroid-a".to_string(), vec![1.0, 0.0])));
        assert!(centroids.contains(&("centroid-b".to_string(), vec![0.0, 1.0])));
        Ok(())
    }

    #[test]
    fn chunks_missing_embedding_is_the_whole_work_queue() -> StoreResult<()> {
        let db = test_db()?;
        let extraction =
            extracted_paper(&db, "queue-paper", &[("paragraph", &"token ".repeat(900))])?;
        let chunks = db.store.chunks_for_extraction(&extraction.id)?;
        assert!(chunks.len() >= 2);

        let pending = db.store.chunks_missing_embedding("m", "1", 100)?;
        assert_eq!(pending.len(), chunks.len());

        db.store
            .save_chunk_embedding(&chunks[0].id, "m", "1", CHUNK_VERSION, &[0.1; 4])?;

        let pending = db.store.chunks_missing_embedding("m", "1", 100)?;
        assert_eq!(pending.len(), chunks.len() - 1);
        assert!(pending.iter().all(|chunk| chunk.id != chunks[0].id));
        Ok(())
    }

    #[test]
    fn embeddings_round_trip_as_little_endian_f32() -> StoreResult<()> {
        let db = test_db()?;
        let extraction = extracted_paper(&db, "blob-paper", &[("paragraph", "vector text")])?;
        let chunk = db.store.chunks_for_extraction(&extraction.id)?.remove(0);

        let vector: Vec<f32> = vec![-1.5, 0.0, 0.25, 12345.678];
        db.store
            .save_chunk_embedding(&chunk.id, "m", "1", CHUNK_VERSION, &vector)?;

        let conn = db.store.open_connection()?;
        let (blob, dimensions): (Vec<u8>, i64) = conn
            .query_row(
                "select embedding, dimensions from document_chunk_embeddings where chunk_id = ?1",
                params![chunk.id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(|error| error.to_string())?;

        // sqlite-vec reads exactly this layout, which is why phase 2 can build
        // an index without re-embedding anything.
        assert_eq!(dimensions, 4);
        assert_eq!(blob.len(), 16);
        let decoded: Vec<f32> = blob
            .chunks_exact(4)
            .map(|bytes| f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
            .collect();
        assert_eq!(decoded, vector);
        Ok(())
    }

    // ---- Chat context items (RFC 0077) ----

    fn chunk_context_draft(chunk: &DocumentChunk) -> ContextItemDraft {
        ContextItemDraft {
            kind: crate::domain::context::CONTEXT_KIND_CHUNK.to_string(),
            chunk_id: Some(chunk.id.clone()),
            paper_id: Some(chunk.paper_id.clone()),
            source_start: Some(chunk.source_start),
            source_end: Some(chunk.source_end),
            body: None,
            covers_through_entry_id: None,
            origin: crate::domain::context::ORIGIN_USER.to_string(),
            token_estimate: chunk.token_estimate,
        }
    }

    #[test]
    fn a_new_whole_paper_chat_does_not_append_to_the_last_one() -> StoreResult<()> {
        let db = test_db()?;
        let first = db.store.persist_anchored_turn_with_creation(
            "paper",
            "vaswani2017",
            &ThreadAnchor::Document,
            &ChatEntryDraft::question("what is attention?".to_string()),
            &ChatEntryDraft::answer("a weighting".to_string(), "m".to_string(), summary()),
            false,
        )?;

        // A note about the paper still folds into the one whole-paper thread —
        // there is one of those. A *chat* does not: a new question is usually a
        // new subject, and appending buries it and drags the old history into
        // every later prompt.
        let same = db.store.persist_anchored_turn_with_creation(
            "paper",
            "vaswani2017",
            &ThreadAnchor::Document,
            &ChatEntryDraft::question("and scaling?".to_string()),
            &ChatEntryDraft::answer("1/sqrt(d)".to_string(), "m".to_string(), summary()),
            false,
        )?;
        assert_eq!(same.view.thread.id, first.view.thread.id);
        assert!(!same.created);

        let fresh = db.store.persist_anchored_turn_with_creation(
            "paper",
            "vaswani2017",
            &ThreadAnchor::Document,
            &ChatEntryDraft::question("unrelated question".to_string()),
            &ChatEntryDraft::answer("unrelated answer".to_string(), "m".to_string(), summary()),
            true,
        )?;
        assert_ne!(fresh.view.thread.id, first.view.thread.id);
        assert!(fresh.created);
        assert_eq!(fresh.view.entries.len(), 2, "the new chat starts empty");

        let threads = db.store.list_chat_threads("paper", "vaswani2017")?;
        let whole_paper = threads
            .iter()
            .filter(|thread| matches!(thread.anchor, ThreadAnchor::Document))
            .count();
        assert_eq!(whole_paper, 2, "both conversations stay reachable");
        Ok(())
    }

    #[test]
    fn context_items_get_dense_positions_in_insertion_order() -> StoreResult<()> {
        let db = test_db()?;
        let extraction = extracted_paper(
            &db,
            "vaswani2017",
            &[
                ("paragraph", "First passage."),
                ("paragraph", "Second passage."),
            ],
        )?;
        let chunks = db.store.chunks_for_extraction(&extraction.id)?;
        let thread = db
            .store
            .add_note_at_anchor("paper", "vaswani2017", &ThreadAnchor::Document, "n")?
            .thread;

        for chunk in &chunks {
            db.store
                .insert_context_item(&thread.id, &chunk_context_draft(chunk))?;
        }

        let items = db.store.context_items(&thread.id)?;
        assert_eq!(items.len(), chunks.len());
        let positions: Vec<i32> = items.iter().map(|item| item.position).collect();
        assert_eq!(positions, (0..chunks.len() as i32).collect::<Vec<_>>());
        Ok(())
    }

    #[test]
    fn context_items_delete_by_item_id_or_chunk_id() -> StoreResult<()> {
        let db = test_db()?;
        let extraction = extracted_paper(&db, "vaswani2017", &[("paragraph", "A passage.")])?;
        let chunk = db.store.chunks_for_extraction(&extraction.id)?.remove(0);
        let thread = db
            .store
            .add_note_at_anchor("paper", "vaswani2017", &ThreadAnchor::Document, "n")?
            .thread;

        let item = db
            .store
            .insert_context_item(&thread.id, &chunk_context_draft(&chunk))?;
        assert!(db
            .store
            .delete_context_item(&thread.id, &ContextKey::Item(item.id.clone()))?);
        assert!(db.store.context_items(&thread.id)?.is_empty());

        db.store
            .insert_context_item(&thread.id, &chunk_context_draft(&chunk))?;
        assert!(db
            .store
            .delete_context_item(&thread.id, &ContextKey::Chunk(chunk.id.clone()))?);
        assert!(db.store.context_items(&thread.id)?.is_empty());

        // Deleting what is already gone is not an error.
        assert!(!db
            .store
            .delete_context_item(&thread.id, &ContextKey::Chunk(chunk.id))?);
        Ok(())
    }

    #[test]
    fn deleting_a_thread_takes_its_context_with_it() -> StoreResult<()> {
        let db = test_db()?;
        let extraction = extracted_paper(&db, "vaswani2017", &[("paragraph", "A passage.")])?;
        let chunk = db.store.chunks_for_extraction(&extraction.id)?.remove(0);
        let thread = db
            .store
            .add_note_at_anchor("paper", "vaswani2017", &ThreadAnchor::Document, "n")?
            .thread;
        db.store
            .insert_context_item(&thread.id, &chunk_context_draft(&chunk))?;

        db.store.delete_chat_thread(&thread.id)?;

        // The cascade, not an explicit delete in delete_chat_thread — the same
        // structural fix the FTS trigger made in RFC 0075.
        let conn = db.store.open_connection()?;
        let remaining: i64 = conn
            .query_row(
                "select count(*) from chat_context_items where thread_id = ?1",
                params![thread.id],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        assert_eq!(remaining, 0);
        Ok(())
    }

    #[test]
    fn compaction_swaps_chunk_items_for_one_summary_atomically() -> StoreResult<()> {
        let db = test_db()?;
        let extraction = extracted_paper(
            &db,
            "vaswani2017",
            &[
                ("paragraph", "First passage."),
                ("paragraph", "Second passage."),
            ],
        )?;
        let chunks = db.store.chunks_for_extraction(&extraction.id)?;
        let thread = db
            .store
            .add_note_at_anchor("paper", "vaswani2017", &ThreadAnchor::Document, "n")?
            .thread;
        let mut superseded = Vec::new();
        for chunk in &chunks {
            superseded.push(
                db.store
                    .insert_context_item(&thread.id, &chunk_context_draft(chunk))?
                    .id,
            );
        }

        db.store.compact_context_items(
            &thread.id,
            &ContextItemDraft {
                kind: crate::domain::context::CONTEXT_KIND_SUMMARY.to_string(),
                chunk_id: None,
                paper_id: None,
                source_start: None,
                source_end: None,
                body: Some("They discussed passages.".to_string()),
                covers_through_entry_id: Some("entry-9".to_string()),
                origin: crate::domain::context::ORIGIN_USER.to_string(),
                token_estimate: 6,
            },
            &superseded,
        )?;

        let items = db.store.context_items(&thread.id)?;
        assert_eq!(
            items.len(),
            1,
            "the chunk items were replaced, not added to"
        );
        assert_eq!(items[0].kind, "summary");
        assert_eq!(items[0].covers_through_entry_id.as_deref(), Some("entry-9"));

        // The chat itself is untouched: compaction shrinks context, not history.
        assert_eq!(db.store.get_chat_thread(&thread.id)?.entries.len(), 1);
        Ok(())
    }

    #[test]
    fn chunks_overlapping_finds_the_passage_after_its_chunk_id_is_gone() -> StoreResult<()> {
        let db = test_db()?;
        let extraction =
            extracted_paper(&db, "vaswani2017", &[("paragraph", "A durable passage.")])?;
        let chunk = db.store.chunks_for_extraction(&extraction.id)?.remove(0);
        let (start, end) = (chunk.source_start, chunk.source_end);

        // A rechunk re-mints ids. The character range still names the passage.
        db.store.rechunk_extraction(&extraction.id)?;

        let found = db.store.chunks_overlapping("vaswani2017", start, end)?;
        assert!(!found.is_empty(), "the durable anchor must still resolve");
        assert!(found[0].text.contains("durable passage"));
        Ok(())
    }

    #[test]
    fn chunk_rects_groups_block_geometry_by_page_and_skips_blocks_without_it() -> StoreResult<()> {
        let db = test_db()?;
        let draft = paper_draft_with_pdf("vaswani2017", "https://example.test/geometry.pdf");
        db.store
            .add_paper_to_vaults(&draft, &["attention".to_string()])?;
        let source = db.store.get_document_sources("vaswani2017")?.remove(0);
        db.store
            .set_document_source_cached(&source.id, "/tmp/geometry.pdf")?;
        let extraction = db.store.start_document_extraction(
            &source.id,
            "pdfium_basic",
            "0.2.0",
            &format!("pdfium_basic:{}", source.id),
            false,
        )?;

        let geometry = serde_json::json!({"x": 0.1, "y": 0.2, "width": 0.5, "height": 0.05});
        let blocks: Vec<DocumentBlock> = ["First line here.", "Second line here.", "Third line."]
            .iter()
            .enumerate()
            .map(|(index, text)| DocumentBlock {
                id: format!("{}:block:0:{index}", extraction.id),
                paper_id: extraction.paper_id.clone(),
                source_id: extraction.source_id.clone(),
                extraction_id: extraction.id.clone(),
                page_index: 0,
                block_index: index as i32,
                reading_order: index as i32,
                kind: "paragraph".to_string(),
                text: Some((*text).to_string()),
                asset_id: None,
                source_start: Some(index as i64 * 20),
                source_end: Some(index as i64 * 20 + 18),
                // The last block predates RFC 0075 geometry.
                bbox_json: (index < 2).then(|| geometry.to_string()),
            })
            .collect();
        db.store.finish_document_extraction(
            &extraction.id,
            &[extraction_page(&extraction)],
            &blocks,
            &[],
        )?;

        let chunk = db.store.chunks_for_extraction(&extraction.id)?.remove(0);
        let pages = db.store.chunk_rects(&chunk.id)?;

        assert_eq!(pages.len(), 1, "all three blocks are on page 0");
        assert_eq!(pages[0].page_index, 0);
        assert_eq!(
            pages[0].rects.len(),
            2,
            "the block without bbox_json contributes nothing rather than failing"
        );
        Ok(())
    }

    #[test]
    fn chunk_rects_is_empty_rather_than_an_error_for_an_unknown_chunk() -> StoreResult<()> {
        let db = test_db()?;
        assert!(db.store.chunk_rects("no-such-chunk")?.is_empty());
        Ok(())
    }

    #[test]
    fn rechunking_replaces_chunks_without_touching_blocks() -> StoreResult<()> {
        let db = test_db()?;
        let extraction = extracted_paper(
            &db,
            "rechunk-paper",
            &[("paragraph", "Body text that will be re-chunked.")],
        )?;

        let before = db.store.chunks_for_extraction(&extraction.id)?;
        let blocks_before = db.store.extraction_structure(&extraction.id)?.blocks.len();

        let count = db.store.rechunk_extraction(&extraction.id)?;
        assert_eq!(count, before.len());

        let after = db.store.chunks_for_extraction(&extraction.id)?;
        assert_eq!(after.len(), before.len());
        assert_eq!(
            db.store.extraction_structure(&extraction.id)?.blocks.len(),
            blocks_before,
            "blocks are canonical; re-chunking must not disturb them"
        );

        // FTS rows were replaced, not duplicated.
        let hits = db
            .store
            .search_chunks_lexical("rechunk-paper", "re-chunked", 10)?;
        assert_eq!(hits.len(), 1);

        // Nothing is left needing a re-chunk at the current version.
        assert!(!db
            .store
            .extractions_needing_rechunk()?
            .contains(&extraction.id));
        Ok(())
    }

    #[test]
    fn dismissed_vault_suggestion_survives_refresh_replacement() -> StoreResult<()> {
        let db = test_db()?;
        let first_run = db.store.create_vault_suggestion_run("attention")?;
        let candidate = sample_candidate("Durably dismissed", Some("10.1/dismissed"));
        let suggestion = VaultSuggestion {
            id: "vs-dismissed".to_string(),
            vault_id: "attention".to_string(),
            run_id: first_run.id.clone(),
            paper_ref: candidate_dedup_key(&candidate),
            candidate: candidate.clone(),
            reason: "Related".to_string(),
            score: 1.0,
            state: "pending".to_string(),
            created_at: String::new(),
            updated_at: String::new(),
        };
        db.store.replace_vault_suggestions(
            "attention",
            &first_run.id,
            std::slice::from_ref(&suggestion),
        )?;
        db.store
            .set_vault_suggestion_state(&suggestion.id, "dismissed")?;

        let second_run = db.store.create_vault_suggestion_run("attention")?;
        db.store.replace_vault_suggestions(
            "attention",
            &second_run.id,
            std::slice::from_ref(&suggestion),
        )?;

        assert!(db
            .store
            .get_vault_suggestions("attention")?
            .suggestions
            .is_empty());
        assert_eq!(
            db.store.decided_vault_suggestion_refs("attention")?,
            vec![candidate_dedup_key(&candidate)]
        );
        Ok(())
    }

    #[test]
    fn vault_suggestion_settings_and_run_snapshot_are_durable() -> StoreResult<()> {
        let db = test_db()?;
        let options = VaultSuggestionOptions {
            focus: Some("empirical methods".to_string()),
            year_from: Some(2020),
            year_to: Some(2026),
            query_path_count: 3,
            result_count: 10,
            include_reviews: false,
        };
        let paths = vec![VaultSuggestionQueryPath {
            id: "path-1".to_string(),
            intent: "Methods".to_string(),
            query: "empirical sparse attention".to_string(),
        }];

        db.store
            .save_vault_suggestion_options("attention", &options)?;
        let run =
            db.store
                .create_vault_suggestion_run_with_options("attention", &options, &paths)?;

        assert_eq!(db.store.get_vault_suggestion_options("attention")?, options);
        assert_eq!(run.options, options);
        assert_eq!(run.query_paths, paths);
        Ok(())
    }

    #[test]
    fn pending_vault_suggestions_are_replaced_atomically() -> StoreResult<()> {
        let db = test_db()?;
        let run = db.store.create_vault_suggestion_run("attention")?;
        let make = |id: &str| {
            let candidate = sample_candidate(id, None);
            VaultSuggestion {
                id: format!("vs-{id}"),
                vault_id: "attention".to_string(),
                run_id: run.id.clone(),
                paper_ref: candidate_dedup_key(&candidate),
                candidate,
                reason: "Related".to_string(),
                score: 1.0,
                state: "pending".to_string(),
                created_at: String::new(),
                updated_at: String::new(),
            }
        };
        db.store
            .replace_vault_suggestions("attention", &run.id, &[make("old")])?;
        db.store
            .replace_vault_suggestions("attention", &run.id, &[make("new")])?;

        let suggestions = db.store.get_vault_suggestions("attention")?.suggestions;
        assert_eq!(suggestions.len(), 1);
        assert_eq!(suggestions[0].candidate.title, "new");
        Ok(())
    }

    #[test]
    fn mean_embedding_averages_compatible_rows() -> StoreResult<()> {
        let rows = vec![
            (
                [1.0_f32, 3.0]
                    .into_iter()
                    .flat_map(f32::to_le_bytes)
                    .collect(),
                2,
            ),
            (
                [3.0_f32, 5.0]
                    .into_iter()
                    .flat_map(f32::to_le_bytes)
                    .collect(),
                2,
            ),
        ];
        assert_eq!(mean_embedding(&rows)?, Some(vec![2.0, 4.0]));
        Ok(())
    }
}
