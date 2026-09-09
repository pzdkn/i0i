use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Manager};

use crate::domain::chat::{
    ChatContextSummary, ChatEntry, ChatEntryDraft, ChatThread, ChatThreadSummary, ChatThreadView,
    PinnedHighlight, ThreadAnchor, ENTRY_ANSWER, ENTRY_NOTE, ENTRY_QUESTION,
};
use crate::domain::chunking::{chunk_blocks, CHUNK_VERSION};
use crate::domain::context::{ContextItem, ContextItemDraft, ContextKey, PageRects};
use crate::domain::discovery::PaperCandidate;
use crate::domain::harness::{
    next_schedule_occurrence, render_harness_search_goal, AgentRunLimits, DueHarnessClaim,
    EffectiveInstructionStack, EffectiveRunContext, HarnessAutonomy, HarnessConfiguration,
    HarnessConfigurationVersion, HarnessEvent, HarnessRun, HarnessRunTrigger, HarnessSnapshot,
    HarnessUsage, PaperDispositionKind, PriorResearchRunOutcome, ResearchCheckpoint,
    ResearchHarness, ResearchPaperDisposition, ResearchRunOutcome, ResearchStateSynthesis,
    ResearchSynthesisChange, RunContextEntry, RunContextObservation, HARNESS_POLICY_SUMMARY,
    HARNESS_POLICY_VERSION,
};
use crate::domain::harness_improvement::{
    HarnessImprovement, HarnessImprovementStatus, HarnessImprovementTarget,
    HarnessImprovementValue, HarnessObservation, HarnessObservationDraft, HarnessObservationKind,
    HarnessReflection, HarnessReflectionDraft, PROPOSAL_POLICY_VERSION, REFLECTION_POLICY_VERSION,
};
use crate::domain::library::{
    CiteRecord, DocumentAsset, DocumentBlock, DocumentChunk, DocumentExtraction, DocumentPage,
    DocumentSource, DocumentSpan, EmbeddingCoverage, ExtractionStructure, LibrarySnapshot, Paper,
    PaperDraft, PaperMetadataEnrichment, PaperMetadataUpdate, PaperSourceDraft, Project,
    ProjectDocument, ProjectDocumentDraft, ProjectDocumentSummary, ProjectDocumentUpdate,
    ProjectDraft, ProjectRenameDraft, Vault, VaultDraft, VaultPaper, VaultRenameDraft,
};
use crate::domain::reconciliation::{
    CandidateDecisionKind, HarnessChangeSet, HarnessChangeSetStatus, ReconciliationTelemetry,
    RunReconciliationPlan,
};
use crate::domain::research::{
    candidate_dedup_key, RankedCandidate, Search, SearchCandidate, SearchDraft, SearchRun,
    SearchRunStatus,
};
use crate::domain::research_document::{
    CreateFromResearchRequest, ProjectDocumentCitation, ResearchDocumentGeneration,
    ResearchDocumentShape, DOCUMENT_GENERATION_POLICY_VERSION,
};
use crate::domain::research_state::{
    EntryLifecycle, EntryRelation, EntryRelationDraft, EntryRelationKind, EpistemicStatus,
    EvidenceLinkDraft, ResearchContextKind, ResearchContextLink, ResearchContextLinkDraft,
    ResearchEntryDetail, ResearchEntryDraft, ResearchEntryKind, ResearchEntrySummary,
    ResearchEntryUpdate, ResearchEntryVersion, ResearchEvidenceCandidate, ResearchEvidenceLink,
    ResearchStateMutation, ResearchStateRevision, ResearchStateSnapshot,
};
use crate::domain::vault_suggestion::{
    VaultSuggestion, VaultSuggestionOptions, VaultSuggestionQueryPath, VaultSuggestionRun,
    VaultSuggestionSnapshot,
};
use crate::pdf_layout::NormRect;
use crate::services::research::reconciliation::{ordered_entries, validate_reconciliation_plan};

type StoreResult<T> = Result<T, String>;

#[path = "research_finalization.rs"]
mod research_finalization;

/// Width of the `vec0` embedding column, tied to the active model rather than
/// restated.
///
/// If these drifted apart, `index_chunk_vector` would skip every chunk on a
/// dimension mismatch and semantic search would return nothing forever, with
/// only a log line to explain it — so the two are the same constant, not two
/// constants that agree today.
const VECTOR_DIMENSIONS: usize = crate::services::embedding::MODEL_DIMENSIONS;
/// Minimum text budget kept available for each not-yet-assessed addition.
const FIRST_PAPER_ASSESSMENT_RESERVE_CHARS: i64 = 1_000;

#[derive(Clone)]
pub struct LibraryStore {
    db_path: PathBuf,
}

pub struct AnchoredThreadWrite {
    pub view: ChatThreadView,
    pub created: bool,
}

/// Stable identity returned by an idempotent agent note write.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentNoteReceipt {
    pub thread_id: String,
    pub entry_id: String,
}

/// One validated operation in an agent-authored State commit.
pub enum AgentStateChange {
    Create {
        operation_key: String,
        draft: ResearchEntryDraft,
        evidence_relationships: Vec<String>,
    },
    Revise {
        update: ResearchEntryUpdate,
        evidence_relationships: Vec<String>,
    },
    SetLifecycle {
        entry_id: String,
        lifecycle: EntryLifecycle,
        reason: String,
    },
}

/// Stable result of one idempotent State batch.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentStateUpdateReceipt {
    pub revision: i64,
    pub affected_entry_ids: Vec<String>,
    pub created_entry_ids: HashMap<String, String>,
}

/// Audited result of applying one Run's paper-retention decisions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentPaperRetentionSummary {
    pub attempted: i64,
    pub read: i64,
    pub unavailable: i64,
    pub removed: i64,
    pub retained: i64,
}

/// Stable identity returned by an idempotent agent search start.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSearchReceipt {
    pub search_id: String,
    pub run_id: String,
}

/// Stable result of one idempotent agent candidate collection.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentVaultAddReceipt {
    pub paper_id: String,
    pub membership_added: bool,
    pub source_ids: Vec<String>,
}

/// Source text delegated to one evidence-question model call, grouped by paper.
#[derive(Debug, Clone)]
pub struct AgentQuestionDelivery {
    pub paper_id: String,
    pub returned_text_chars: u64,
}

/// One persisted candidate snapshot emitted while an agent search runs.
#[derive(Debug, Clone)]
pub struct AgentSearchCandidateEvent {
    pub sequence: i64,
    pub phase: String,
    pub candidate: PaperCandidate,
}

/// One persisted activity item emitted while an agent search runs.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSearchActivity {
    pub sequence: i64,
    pub kind: String,
    pub message: String,
    pub error: Option<String>,
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

    /// Open a library at an explicit path for isolated application harnesses.
    pub(crate) fn at_path(db_path: PathBuf) -> Self {
        Self { db_path }
    }

    #[cfg(test)]
    pub(crate) fn for_test(db_path: PathBuf) -> Self {
        Self::at_path(db_path)
    }

    pub fn init(&self) -> StoreResult<()> {
        let mut conn = self.open_connection()?;
        self.create_schema(&conn)?;
        migrate_vaults_to_projects(&mut conn)?;
        migrate_projects_to_harnesses(&mut conn)?;
        migrate_harness_authority_and_versions(&mut conn)?;
        migrate_harnesses_to_simple_research(&mut conn)?;
        migrate_projects_to_research_state(&mut conn)?;
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

    /// Materialize a saved paper's provider abstract as typed, citable evidence.
    pub fn materialize_paper_abstract(&self, paper_id: &str) -> StoreResult<DocumentChunk> {
        let conn = self.open_connection()?;
        let abstract_text: String = conn
            .query_row(
                "select abstract from papers where id = ?1 and abstract is not null",
                params![paper_id],
                |row| row.get(0),
            )
            .map_err(|error| {
                if matches!(error, rusqlite::Error::QueryReturnedNoRows) {
                    format!("Paper has no abstract: {paper_id}")
                } else {
                    error.to_string()
                }
            })?;
        let chunk_id = materialize_metadata_abstract_text(&conn, paper_id, &abstract_text, None)?;
        read_chunks(&conn, "where c.id = ?1", params![chunk_id])?
            .into_iter()
            .next()
            .ok_or_else(|| format!("Materialized abstract chunk was not found: {paper_id}"))
    }

    /// Index the exact cached HTML text without assigning synthetic PDF pages.
    pub(crate) fn materialize_html_text(
        &self,
        source: &DocumentSource,
        text: &str,
    ) -> StoreResult<Vec<DocumentChunk>> {
        let mut conn = self.open_connection()?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|e| e.to_string())?;
        let extraction_id = format!("html_text:{}:{}", source.id, short_sha256(text));
        let existing = read_chunks(&tx, "where c.extraction_id = ?1", params![extraction_id])?;
        if !existing.is_empty() {
            return Ok(existing);
        }
        tx.execute("insert into document_extractions (id,paper_id,source_id,extractor,extractor_version,annotation_source_id,status,created_at,updated_at)
            values (?1,?2,?3,'html_text','1',?1,'ready',datetime('now'),datetime('now')) on conflict(id) do nothing",
            params![extraction_id,source.paper_id,source.id]).map_err(|e| e.to_string())?;
        let mut chunks: Vec<DocumentChunk> = Vec::new();
        let chars: Vec<char> = text.chars().collect();
        for (index, part) in chars.chunks(4000).enumerate() {
            let block_id = format!("{extraction_id}:block:{index}");
            let start = (index * 4000) as i64;
            let end = start + part.len() as i64;
            let content: String = part.iter().collect();
            tx.execute("insert into document_blocks (id,paper_id,source_id,extraction_id,page_index,block_index,reading_order,kind,text,source_start,source_end)
                values (?1,?2,?3,?4,0,?5,?5,'paragraph',?6,?7,?8)",
                params![block_id,source.paper_id,source.id,extraction_id,index as i64,content,start,end]).map_err(|e| e.to_string())?;
            chunks.push(DocumentChunk {
                id: format!("{extraction_id}:chunk:{index}"),
                paper_id: source.paper_id.clone(),
                source_id: source.id.clone(),
                extraction_id: extraction_id.clone(),
                chunk_index: index as i32,
                chunker: "html_text".into(),
                chunk_version: CHUNK_VERSION,
                page_start: 0,
                page_end: 0,
                heading_path: None,
                text: content,
                token_estimate: ((part.len() + 3) / 4) as i32,
                source_start: start,
                source_end: end,
                block_ids: vec![block_id],
            });
        }
        insert_chunks(&tx, &chunks)?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(chunks)
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
        insert_default_harness(&tx, &project_id)?;
        insert_initial_research_state(&tx, &project_id)?;
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

    /// Queues a generation against one exact Research State revision.
    pub fn create_research_document_generation(
        &self,
        request: &CreateFromResearchRequest,
        retry_of_id: Option<&str>,
    ) -> StoreResult<ResearchDocumentGeneration> {
        validate_generation_request(self, request)?;
        let mut selected = request.selected_entry_ids.clone();
        selected.sort();
        selected.dedup();
        let fingerprint_source = serde_json::to_string(&(
            request.project_id.as_str(),
            request.state_revision,
            &selected,
            request.shape.as_str(),
            request.title.trim(),
            request.custom_instruction.as_deref().map(str::trim),
            request.include_non_active,
        ))
        .map_err(|error| error.to_string())?;
        let fingerprint = short_sha256(&fingerprint_source);
        let id = timestamped_id("research_document_generation")?;
        let conn = self.open_connection()?;
        conn.execute(
            "insert into research_document_generations
             (id, project_id, status, shape, title, custom_instruction,
              state_revision, selected_entry_ids_json, include_non_active,
              originating_run_id, policy_version, model_identifier, retry_of_id,
              request_fingerprint, created_at)
             values (?1, ?2, 'queued', ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10,
                     'deterministic-template-v1', ?11, ?12, datetime('now'))",
            params![
                id,
                request.project_id,
                request.shape.as_str(),
                request.title.trim(),
                request.custom_instruction.as_deref().map(str::trim),
                request.state_revision,
                serde_json::to_string(&selected).map_err(|error| error.to_string())?,
                request.include_non_active,
                request.originating_run_id,
                DOCUMENT_GENERATION_POLICY_VERSION,
                retry_of_id,
                fingerprint
            ],
        )
        .map_err(|error| {
            if error.to_string().contains("request_fingerprint") {
                "An identical research document generation is already active".to_string()
            } else {
                error.to_string()
            }
        })?;
        read_research_document_generation(&conn, &id)
    }

    /// Loads one generation attempt by its durable id.
    pub fn get_research_document_generation(
        &self,
        generation_id: &str,
    ) -> StoreResult<ResearchDocumentGeneration> {
        let conn = self.open_connection()?;
        read_research_document_generation(&conn, generation_id)
    }

    /// Cancels a queued job immediately or flags a generating job to stop.
    pub fn cancel_research_document_generation(
        &self,
        generation_id: &str,
    ) -> StoreResult<ResearchDocumentGeneration> {
        let conn = self.open_connection()?;
        conn.execute(
            "update research_document_generations
             set cancellation_requested = 1,
                 status = case when status = 'queued' then 'cancelled' else status end,
                 finished_at = case when status = 'queued' then datetime('now') else finished_at end
             where id = ?1 and status in ('queued', 'generating')",
            params![generation_id],
        )
        .map_err(|error| error.to_string())?;
        read_research_document_generation(&conn, generation_id)
    }

    /// Creates a new immutable attempt from a failed or cancelled request.
    pub fn retry_research_document_generation(
        &self,
        generation_id: &str,
    ) -> StoreResult<ResearchDocumentGeneration> {
        let prior = self.get_research_document_generation(generation_id)?;
        if !matches!(prior.status.as_str(), "failed" | "cancelled") {
            return Err("Only failed or cancelled generation may be retried".to_string());
        }
        self.create_research_document_generation(
            &CreateFromResearchRequest {
                project_id: prior.project_id,
                state_revision: prior.state_revision,
                selected_entry_ids: prior.selected_entry_ids,
                shape: prior.shape,
                title: prior.title,
                custom_instruction: prior.custom_instruction,
                originating_run_id: prior.originating_run_id,
                include_non_active: prior.include_non_active,
            },
            Some(generation_id),
        )
    }

    /// Renders, validates, and atomically creates the ordinary Markdown document.
    pub fn execute_research_document_generation(
        &self,
        generation_id: &str,
    ) -> StoreResult<ResearchDocumentGeneration> {
        let generation = self.get_research_document_generation(generation_id)?;
        if generation.status == "cancelled" || generation.cancellation_requested {
            return Ok(generation);
        }
        if generation.status != "queued" {
            return Err("Research document generation is not queued".to_string());
        }
        let mut conn = self.open_connection()?;
        let tx = conn.transaction().map_err(|error| error.to_string())?;
        tx.execute(
            "update research_document_generations set status = 'generating',
             started_at = datetime('now') where id = ?1 and status = 'queued'",
            params![generation_id],
        )
        .map_err(|error| error.to_string())?;
        let cancellation_requested: bool = tx
            .query_row(
                "select cancellation_requested from research_document_generations where id = ?1",
                params![generation_id],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        if cancellation_requested {
            tx.execute(
                "update research_document_generations set status = 'cancelled',
                 finished_at = datetime('now') where id = ?1",
                params![generation_id],
            )
            .map_err(|error| error.to_string())?;
            tx.commit().map_err(|error| error.to_string())?;
            return self.get_research_document_generation(generation_id);
        }

        let mut entries = Vec::new();
        for entry_id in &generation.selected_entry_ids {
            let detail = read_research_entry_detail_from_conn(
                &tx,
                entry_id,
                Some(generation.state_revision),
            )?;
            if detail.entry.project_id != generation.project_id {
                return Err("Selected Research Entry crossed the Project boundary".to_string());
            }
            if detail.entry.lifecycle != EntryLifecycle::Active && !generation.include_non_active {
                continue;
            }
            entries.push(detail);
        }
        if entries.is_empty() {
            return Err("No eligible Research Entries remain for generation".to_string());
        }
        let (content, citations) = render_research_document(&tx, &generation, &entries)?;
        validate_generated_document(&content, &entries, &citations)?;
        let document_id = timestamped_id("project_document")?;
        tx.execute(
            "insert into project_documents
             (id, project_id, title, format, content, harness_writable,
              created_from_run_id, created_from_state_revision, generation_id,
              output_shape, created_at, updated_at)
             values (?1, ?2, ?3, 'markdown', ?4, 0, ?5, ?6, ?7, ?8,
                     datetime('now'), datetime('now'))",
            params![
                document_id,
                generation.project_id,
                generation.title,
                content,
                generation.originating_run_id,
                generation.state_revision,
                generation.id,
                generation.shape.as_str()
            ],
        )
        .map_err(|error| error.to_string())?;
        for citation in &citations {
            tx.execute(
                "insert into project_document_citations
                 (document_id, citation_key, paper_id, evidence_link_ids_json,
                  title_snapshot, authors_snapshot_json, year_snapshot, created_at)
                 values (?1, ?2, ?3, ?4, ?5, ?6, ?7, datetime('now'))",
                params![
                    document_id,
                    citation.citation_key,
                    citation.paper_id,
                    serde_json::to_string(&citation.evidence_link_ids)
                        .map_err(|error| error.to_string())?,
                    citation.title_snapshot,
                    serde_json::to_string(&citation.authors_snapshot)
                        .map_err(|error| error.to_string())?,
                    citation.year_snapshot
                ],
            )
            .map_err(|error| error.to_string())?;
        }
        tx.execute(
            "update research_document_generations set status = 'ready',
             resulting_document_id = ?2, input_entry_count = ?3, citation_count = ?4,
             finished_at = datetime('now') where id = ?1",
            params![
                generation_id,
                document_id,
                entries.len() as i64,
                citations.len() as i64
            ],
        )
        .map_err(|error| error.to_string())?;
        append_harness_control_event(
            &tx,
            &generation.project_id,
            "research_document_created",
            &format!(
                "Created {} from Research State revision {} using {} entries",
                generation.shape.as_str(),
                generation.state_revision,
                entries.len()
            ),
        )?;
        tx.commit().map_err(|error| error.to_string())?;
        self.get_research_document_generation(generation_id)
    }

    /// Records a bounded caller-visible failure on an unfinished attempt.
    pub fn fail_research_document_generation(
        &self,
        generation_id: &str,
        error: &str,
    ) -> StoreResult<()> {
        let conn = self.open_connection()?;
        conn.execute(
            "update research_document_generations set status = 'failed', error = ?2,
             finished_at = datetime('now') where id = ?1 and status in ('queued','generating')",
            params![generation_id, error.chars().take(1_000).collect::<String>()],
        )
        .map_err(|cause| cause.to_string())?;
        Ok(())
    }

    /// Loads current Harness configuration together with Run and Activity history.
    pub fn get_harness_snapshot(&self, project_id: &str) -> StoreResult<HarnessSnapshot> {
        let conn = self.open_connection()?;
        let harness = read_research_harness(&conn, project_id)?
            .ok_or_else(|| format!("Research Harness not found for Project: {project_id}"))?;
        Ok(HarnessSnapshot {
            harness,
            runs: read_harness_runs(&conn, project_id)?,
            events: read_harness_events_for_project(&conn, project_id)?,
        })
    }

    /// Lists immutable Harness configuration versions newest first.
    pub fn list_harness_configuration_versions(
        &self,
        project_id: &str,
    ) -> StoreResult<Vec<HarnessConfigurationVersion>> {
        let conn = self.open_connection()?;
        read_harness_configuration_versions(&conn, project_id)
    }

    /// Removes superseded standalone configuration rows while preserving Runs.
    pub fn clear_harness_configuration_history(&self, project_id: &str) -> StoreResult<usize> {
        let conn = self.open_connection()?;
        let current_version: i64 = conn
            .query_row(
                "select configuration_version from research_harnesses where project_id = ?1",
                params![project_id],
                |row| row.get(0),
            )
            .map_err(|_| format!("Research Harness not found for Project: {project_id}"))?;
        conn.execute(
            "delete from harness_configuration_versions
             where project_id = ?1 and version <> ?2",
            params![project_id, current_version],
        )
        .map_err(|error| error.to_string())
    }

    /// Load one persisted Research Run by its stable identifier.
    pub fn get_harness_run(&self, run_id: &str) -> StoreResult<HarnessRun> {
        let conn = self.open_connection()?;
        read_harness_run(&conn, run_id)
    }

    /// Loads the immutable effective instruction stack captured for one Run.
    pub fn get_harness_run_instructions(
        &self,
        run_id: &str,
    ) -> StoreResult<EffectiveInstructionStack> {
        let conn = self.open_connection()?;
        read_harness_run(&conn, run_id).map(|run| run.effective_instructions)
    }

    /// Loads the immutable audit checkpoint assembled from persisted Run records.
    pub fn get_research_checkpoint(&self, run_id: &str) -> StoreResult<ResearchCheckpoint> {
        let conn = self.open_connection()?;
        read_research_checkpoint(&conn, run_id)
    }

    /// Lists persisted Run checkpoints newest first for one Project.
    pub fn list_research_checkpoints(
        &self,
        project_id: &str,
    ) -> StoreResult<Vec<ResearchCheckpoint>> {
        let conn = self.open_connection()?;
        let runs = read_harness_runs(&conn, project_id)?;
        runs.iter()
            .map(|run| read_research_checkpoint(&conn, &run.id))
            .collect()
    }

    /// Restores Research State by appending a new revision; no history is rewound.
    pub fn restore_research_checkpoint(
        &self,
        project_id: &str,
        run_id: &str,
        expected_current_revision: i64,
    ) -> StoreResult<ResearchStateSnapshot> {
        let mut conn = self.open_connection()?;
        let tx = conn.transaction().map_err(|error| error.to_string())?;
        let run = read_harness_run(&tx, run_id)?;
        if run.project_id != project_id {
            return Err("Research checkpoint does not belong to the active Project".to_string());
        }
        if !matches!(run.status.as_str(), "ready" | "failed" | "cancelled") {
            return Err("Only a terminal Research Run checkpoint may be restored".to_string());
        }
        let target_revision = run.resulting_state_revision.ok_or_else(|| {
            "This Research Run has no resulting Research State revision to restore".to_string()
        })?;
        require_current_state_revision(&tx, &run.project_id, expected_current_revision)?;
        let target = read_research_state(&tx, &run.project_id, Some(target_revision))?;
        let current = read_research_state(&tx, &run.project_id, Some(expected_current_revision))?;
        let target_active: std::collections::HashSet<String> = target
            .entries
            .iter()
            .filter(|entry| entry.lifecycle == EntryLifecycle::Active)
            .map(|entry| entry.id.clone())
            .collect();
        let next_revision = expected_current_revision + 1;
        insert_state_revision(
            &tx,
            &run.project_id,
            next_revision,
            None,
            &format!("Restored Research State from checkpoint {run_id}"),
        )?;

        for entry in target
            .entries
            .iter()
            .filter(|entry| entry.lifecycle == EntryLifecycle::Active)
        {
            let detail =
                read_research_entry_detail_from_conn(&tx, &entry.id, Some(target_revision))?;
            let draft = research_entry_draft_from_detail(
                &detail,
                &format!("Restored from checkpoint {run_id}"),
            );
            validate_research_entry_draft(&tx, &run.project_id, &draft)?;
            insert_research_entry_version(
                &tx,
                &entry.id,
                &run.project_id,
                next_revision,
                EntryLifecycle::Active,
                None,
                &draft,
            )?;
            tx.execute(
                "update research_entries set kind = ?2, epistemic_status = ?3, text = ?4,
                 lifecycle = 'active', last_revision = ?5, updated_at = datetime('now')
                 where id = ?1 and project_id = ?6",
                params![
                    entry.id,
                    entry.kind.as_str(),
                    entry.epistemic_status.as_str(),
                    entry.text,
                    next_revision,
                    run.project_id
                ],
            )
            .map_err(|error| error.to_string())?;
        }

        let superseded_ids: Vec<String> = current
            .entries
            .iter()
            .filter(|entry| {
                entry.lifecycle == EntryLifecycle::Active && !target_active.contains(&entry.id)
            })
            .map(|entry| entry.id.clone())
            .collect();
        for entry_id in &superseded_ids {
            let detail = read_research_entry_detail_from_conn(
                &tx,
                entry_id,
                Some(expected_current_revision),
            )?;
            let draft = research_entry_draft_from_detail(
                &detail,
                &format!("Superseded by checkpoint restoration {run_id}"),
            );
            validate_research_entry_draft(&tx, &run.project_id, &draft)?;
            insert_research_entry_version(
                &tx,
                entry_id,
                &run.project_id,
                next_revision,
                EntryLifecycle::Superseded,
                None,
                &draft,
            )?;
            tx.execute(
                "update research_entries set lifecycle = 'superseded', last_revision = ?2,
                 updated_at = datetime('now') where id = ?1 and project_id = ?3",
                params![entry_id, next_revision, run.project_id],
            )
            .map_err(|error| error.to_string())?;
        }
        set_current_state_revision(&tx, &run.project_id, next_revision)?;
        append_structured_harness_event(
            &tx,
            run_id,
            "checkpoint_restored",
            &format!("Restored Research State as revision {next_revision}"),
            Some(serde_json::json!({
                "checkpointStateRevision": target_revision,
                "previousCurrentRevision": expected_current_revision,
                "resultingRevision": next_revision,
                "supersededEntryIds": superseded_ids,
            })),
            Some("restoration"),
            None,
            None,
            "researcher",
        )?;
        tx.commit().map_err(|error| error.to_string())?;
        self.get_research_state(&run.project_id, None)
    }

    /// Finds the Project Harness Run linked to a concrete Deep Research Run.
    pub fn get_harness_run_for_search_run(
        &self,
        search_run_id: &str,
    ) -> StoreResult<Option<HarnessRun>> {
        let conn = self.open_connection()?;
        let harness_run_id: Option<String> = conn
            .query_row(
                "select id from harness_runs where search_run_id = ?1",
                params![search_run_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|error| error.to_string())?;
        harness_run_id
            .map(|id| read_harness_run(&conn, &id))
            .transpose()
    }

    /// Records bounded structured progress when a Search Run belongs to a Harness Run.
    pub fn record_harness_search_progress(
        &self,
        search_run_id: &str,
        kind: &str,
        summary: &str,
        detail: Option<serde_json::Value>,
        phase: Option<&str>,
        progress_current: Option<i64>,
        progress_total: Option<i64>,
    ) -> StoreResult<()> {
        let conn = self.open_connection()?;
        let harness_run_id = conn
            .query_row(
                "select id from harness_runs where search_run_id = ?1",
                params![search_run_id],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|error| error.to_string())?;
        if let Some(harness_run_id) = harness_run_id {
            append_structured_harness_event(
                &conn,
                &harness_run_id,
                kind,
                summary,
                detail,
                phase,
                progress_current,
                progress_total,
                "harness",
            )?;
        }
        Ok(())
    }

    /// Lists only candidates first produced by this Harness Run's linked search Run.
    pub fn harness_reconciliation_candidates(
        &self,
        run_id: &str,
    ) -> StoreResult<Vec<SearchCandidate>> {
        let conn = self.open_connection()?;
        let run = read_harness_run(&conn, run_id)?;
        let search_run_id = run
            .search_run_id
            .as_deref()
            .ok_or_else(|| "Research Run has no linked search Run".to_string())?;
        let candidates = self.list_search_candidates(&run.search_id)?;
        Ok(candidates
            .into_iter()
            .filter(|candidate| candidate.first_seen_run_id == search_run_id)
            .collect())
    }

    /// Returns bounded persisted telemetry for operational reflection.
    pub fn harness_reconciliation_telemetry(
        &self,
        run_id: &str,
    ) -> StoreResult<ReconciliationTelemetry> {
        let conn = self.open_connection()?;
        let run = read_harness_run(&conn, run_id)?;
        let search_run_id = run
            .search_run_id
            .as_deref()
            .ok_or_else(|| "Research Run has no linked Search Run".to_string())?;
        let query_expansions: Option<String> = conn
            .query_row(
                "select query_expansions from search_runs where id = ?1",
                params![search_run_id],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        let executed_queries = query_expansions
            .as_deref()
            .map(serde_json::from_str::<Vec<String>>)
            .transpose()
            .map_err(|error| error.to_string())?
            .unwrap_or_default()
            .into_iter()
            .take(50)
            .map(|query| query.chars().take(500).collect())
            .collect();
        let (provider_completions, provider_failures): (u32, u32) = conn
            .query_row(
                "select
                   sum(case when kind = 'provider_query_completed' then 1 else 0 end),
                   sum(case when kind = 'provider_query_failed' then 1 else 0 end)
                 from harness_events where run_id = ?1",
                params![run_id],
                |row| {
                    Ok((
                        row.get::<_, Option<u32>>(0)?.unwrap_or(0),
                        row.get::<_, Option<u32>>(1)?.unwrap_or(0),
                    ))
                },
            )
            .map_err(|error| error.to_string())?;
        Ok(ReconciliationTelemetry {
            executed_queries,
            provider_completions,
            provider_failures,
            provider_queries: run.provider_query_count,
            llm_calls: run.llm_call_count,
            iterations: run.iteration_count,
            inspected_candidates: run.inspected_candidate_count,
            stop_reason: run.stop_reason,
        })
    }

    /// Persists one validated immutable reconciliation plan for a ready Run.
    pub fn create_harness_change_set(
        &self,
        run_id: &str,
        plan: &RunReconciliationPlan,
    ) -> StoreResult<HarnessChangeSet> {
        let candidates = self.harness_reconciliation_candidates(run_id)?;
        let mut conn = self.open_connection()?;
        let tx = conn.transaction().map_err(|error| error.to_string())?;
        let run = read_harness_run(&tx, run_id)?;
        if !matches!(run.status.as_str(), "reconciling" | "ready") {
            return Err(
                "Only a reconciling or ready Research Run may create a Change Set".to_string(),
            );
        }
        validate_reconciliation_plan(plan, &run.effective_instructions, &candidates)?;
        let id = timestamped_id("harness_change_set")?;
        tx.execute(
            "insert into harness_change_sets (
               id, run_id, project_id, starting_state_revision, status, plan_json,
               considered_candidates_json, created_at
             ) values (?1, ?2, ?3, ?4, 'proposed', ?5, ?6, datetime('now'))",
            params![
                id,
                run.id,
                run.project_id,
                run.starting_state_revision,
                serde_json::to_string(plan).map_err(|error| error.to_string())?,
                serde_json::to_string(&candidates).map_err(|error| error.to_string())?
            ],
        )
        .map_err(|error| error.to_string())?;
        append_harness_event(
            &tx,
            run_id,
            "change_set_proposed",
            &format!(
                "Prepared {} candidate decisions and {} Research State entries",
                plan.candidate_decisions.len(),
                plan.entries.len()
            ),
        )?;
        for decision in &plan.candidate_decisions {
            append_structured_harness_event(
                &tx,
                run_id,
                "candidate_decided",
                &format!("Candidate {}", decision.decision.as_str()),
                Some(serde_json::json!({
                    "candidateId": decision.candidate_id,
                    "decision": decision.decision.as_str(),
                    "reason": decision.reason,
                    "relevanceConfidence": decision.relevance_confidence,
                    "withinScope": decision.within_scope,
                })),
                Some("reconciliation"),
                None,
                None,
                "harness",
            )?;
        }
        tx.commit().map_err(|error| error.to_string())?;
        self.get_harness_change_set(run_id)
    }

    /// Records an explicit terminal reconciliation failure without Project mutation.
    pub fn fail_harness_reconciliation(
        &self,
        run_id: &str,
        error: &str,
    ) -> StoreResult<HarnessChangeSet> {
        let candidates = self.harness_reconciliation_candidates(run_id)?;
        let conn = self.open_connection()?;
        let run = read_harness_run(&conn, run_id)?;
        let id = timestamped_id("harness_change_set")?;
        conn.execute(
            "insert into harness_change_sets (
               id, run_id, project_id, starting_state_revision, status, plan_json,
               considered_candidates_json, error, created_at, decided_at
             ) values (?1, ?2, ?3, ?4, 'failed', null, ?5, ?6, datetime('now'), datetime('now'))",
            params![
                id,
                run.id,
                run.project_id,
                run.starting_state_revision,
                serde_json::to_string(&candidates).map_err(|value| value.to_string())?,
                error.chars().take(2_000).collect::<String>()
            ],
        )
        .map_err(|value| value.to_string())?;
        append_harness_event(
            &conn,
            run_id,
            "reconciliation_failed",
            "Research reconciliation failed validation; no Project changes were made",
        )?;
        self.get_harness_change_set(run_id)
    }

    /// Loads the Change Set associated one-to-one with a Harness Run.
    pub fn get_harness_change_set(&self, run_id: &str) -> StoreResult<HarnessChangeSet> {
        let conn = self.open_connection()?;
        read_harness_change_set_by_run(&conn, run_id)
    }

    /// Rejects a proposed Change Set while retaining its full plan and candidates.
    pub fn reject_harness_change_set(
        &self,
        id: &str,
        reason: &str,
    ) -> StoreResult<HarnessChangeSet> {
        if reason.trim().is_empty() || reason.trim().chars().count() > 1_000 {
            return Err(
                "Change Set rejection requires a reason of at most 1000 characters".to_string(),
            );
        }
        let conn = self.open_connection()?;
        let current = read_harness_change_set(&conn, id)?;
        if current.status != HarnessChangeSetStatus::Proposed {
            return Err("Only a proposed Change Set may be rejected".to_string());
        }
        conn.execute(
            "update harness_change_sets set status = 'rejected', decision_reason = ?2,
             decided_at = datetime('now') where id = ?1 and status = 'proposed'",
            params![id, reason.trim()],
        )
        .map_err(|error| error.to_string())?;
        append_harness_event(
            &conn,
            &current.run_id,
            "change_set_rejected",
            "Researcher rejected the proposed Project changes",
        )?;
        read_harness_change_set(&conn, id)
    }

    /// Revalidates and replaces the bounded plan while it still awaits review.
    pub fn edit_harness_change_set(
        &self,
        id: &str,
        plan: &RunReconciliationPlan,
    ) -> StoreResult<HarnessChangeSet> {
        let conn = self.open_connection()?;
        let current = read_harness_change_set(&conn, id)?;
        if current.status != HarnessChangeSetStatus::Proposed {
            return Err("Only a proposed Change Set may be edited".to_string());
        }
        let run = read_harness_run(&conn, &current.run_id)?;
        validate_reconciliation_plan(
            plan,
            &run.effective_instructions,
            &current.considered_candidates,
        )?;
        conn.execute(
            "update harness_change_sets set plan_json = ?2 where id = ?1",
            params![
                id,
                serde_json::to_string(plan).map_err(|error| error.to_string())?
            ],
        )
        .map_err(|error| error.to_string())?;
        append_harness_event(
            &conn,
            &current.run_id,
            "change_set_edited",
            "Researcher edited and revalidated the proposed Project changes",
        )?;
        read_harness_change_set(&conn, id)
    }

    /// Atomically applies one validated Change Set within its snapshotted authority.
    pub fn apply_harness_change_set(&self, id: &str) -> StoreResult<HarnessChangeSet> {
        match self.apply_harness_change_set_transaction(id) {
            Ok(change_set) => Ok(change_set),
            Err(error) => {
                let conn = self.open_connection()?;
                if let Ok(current) = read_harness_change_set(&conn, id) {
                    if current.status == HarnessChangeSetStatus::Proposed {
                        conn.execute(
                            "update harness_change_sets set status = 'failed', error = ?2,
                             decided_at = datetime('now') where id = ?1 and status = 'proposed'",
                            params![id, error.chars().take(2_000).collect::<String>()],
                        )
                        .map_err(|value| value.to_string())?;
                        append_harness_event(
                            &conn,
                            &current.run_id,
                            "change_set_failed",
                            "Project changes failed validation or persistence and were rolled back",
                        )?;
                    }
                }
                Err(error)
            }
        }
    }

    fn apply_harness_change_set_transaction(&self, id: &str) -> StoreResult<HarnessChangeSet> {
        let mut conn = self.open_connection()?;
        let tx = conn.transaction().map_err(|error| error.to_string())?;
        let current = read_harness_change_set(&tx, id)?;
        if current.status != HarnessChangeSetStatus::Proposed {
            return Err("Only a proposed Change Set may be applied".to_string());
        }
        let plan = current
            .plan
            .as_ref()
            .ok_or_else(|| "A failed Change Set has no applicable plan".to_string())?;
        let run = read_harness_run(&tx, &current.run_id)?;
        if !matches!(run.status.as_str(), "reconciling" | "ready") {
            return Err(
                "Only a reconciling or ready Research Run may apply Project changes".to_string(),
            );
        }
        validate_reconciliation_plan(
            plan,
            &run.effective_instructions,
            &current.considered_candidates,
        )?;
        let state_revision: i64 = tx
            .query_row(
                "select current_revision from research_state_heads where project_id = ?1",
                params![current.project_id],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        if state_revision != current.starting_state_revision {
            tx.execute(
                "update harness_change_sets set status = 'superseded',
                 decision_reason = 'Research State changed after planning', decided_at = datetime('now')
                 where id = ?1",
                params![id],
            )
            .map_err(|error| error.to_string())?;
            append_harness_event(
                &tx,
                &current.run_id,
                "change_set_superseded",
                "Research State changed after planning; no Project changes were made",
            )?;
            tx.commit().map_err(|error| error.to_string())?;
            return self.get_harness_change_set(&current.run_id);
        }

        let accepted_ids: std::collections::HashSet<&str> = plan
            .candidate_decisions
            .iter()
            .filter(|decision| decision.decision == CandidateDecisionKind::Accept)
            .map(|decision| decision.candidate_id.as_str())
            .collect();
        let accepted_paper_ids: Vec<String> = current
            .considered_candidates
            .iter()
            .filter(|candidate| accepted_ids.contains(candidate.id.as_str()))
            .map(|candidate| candidate.candidate.id.clone())
            .collect();
        if !run.configuration_snapshot.may_add_papers {
            let adds_new_paper = current
                .considered_candidates
                .iter()
                .filter(|candidate| accepted_ids.contains(candidate.id.as_str()))
                .any(|candidate| {
                    !run.effective_instructions
                        .run_context
                        .vault_paper_ids
                        .contains(&candidate.candidate.id)
                });
            if adds_new_paper {
                return Err(
                    "This Run was not authorized to add Papers to the Project Vault".to_string(),
                );
            }
        }
        let vault_id: String = tx
            .query_row(
                "select id from vaults where project_id = ?1",
                params![current.project_id],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        let mut abstract_chunks = HashMap::new();
        for candidate in current
            .considered_candidates
            .iter()
            .filter(|candidate| accepted_ids.contains(candidate.id.as_str()))
        {
            upsert_reconciliation_paper(&tx, &vault_id, candidate)?;
            if candidate.candidate.abstract_text.is_some() {
                let chunk_id = materialize_metadata_abstract(&tx, candidate)?;
                abstract_chunks.insert(candidate.id.as_str(), chunk_id);
            }
            tx.execute(
                "update search_candidates set saved = 1 where id = ?1",
                params![candidate.id],
            )
            .map_err(|error| error.to_string())?;
        }

        let next_revision = state_revision + 1;
        insert_state_revision(
            &tx,
            &current.project_id,
            next_revision,
            Some(&current.run_id),
            "Research Run reconciliation applied",
        )?;
        let mut entry_ids = HashMap::new();
        for entry in &plan.entries {
            entry_ids.insert(entry.handle.as_str(), timestamped_id("research_entry")?);
        }
        let existing_ids: std::collections::HashSet<&str> = run
            .effective_instructions
            .run_context
            .active_entries
            .iter()
            .map(|entry| entry.id.as_str())
            .collect();
        for entry in ordered_entries(plan, &existing_ids)? {
            let evidence = entry
                .evidence
                .iter()
                .map(|evidence| {
                    let candidate = current
                        .considered_candidates
                        .iter()
                        .find(|candidate| candidate.id == evidence.candidate_id)
                        .expect("validated evidence candidate");
                    let chunk_id = find_inspected_evidence_chunk(
                        &tx,
                        &candidate.candidate.id,
                        &evidence.excerpt,
                    )?
                    .or_else(|| abstract_chunks.get(evidence.candidate_id.as_str()).cloned())
                    .ok_or_else(|| {
                        format!(
                            "No inspected evidence contains the planned excerpt: {}",
                            evidence.candidate_id
                        )
                    })?;
                    Ok(EvidenceLinkDraft {
                        chunk_id,
                        excerpt: Some(evidence.excerpt.clone()),
                        support_note: evidence.support_note.clone(),
                    })
                })
                .collect::<StoreResult<Vec<_>>>()?;
            let relations = entry
                .relations
                .iter()
                .map(|relation| EntryRelationDraft {
                    target_entry_id: entry_ids
                        .get(relation.target.as_str())
                        .cloned()
                        .unwrap_or_else(|| relation.target.clone()),
                    kind: relation.kind,
                })
                .collect();
            let draft = ResearchEntryDraft {
                kind: entry.kind,
                epistemic_status: entry.epistemic_status,
                text: entry.text.clone(),
                evidence,
                relations,
                context: Vec::new(),
                reason: Some("Research Run reconciliation".to_string()),
            };
            validate_research_entry_draft(&tx, &current.project_id, &draft)?;
            let entry_id = &entry_ids[entry.handle.as_str()];
            insert_new_research_entry(
                &tx,
                entry_id,
                &current.project_id,
                next_revision,
                Some(&current.run_id),
                &draft,
            )?;
            append_harness_event(
                &tx,
                &current.run_id,
                "research_entry_added",
                &format!("Added {} to Research State", entry.kind.as_str()),
            )?;
        }
        set_current_state_revision(&tx, &current.project_id, next_revision)?;
        let resulting_vault_revision: i64 = tx
            .query_row(
                "select membership_revision from vaults where project_id = ?1",
                params![current.project_id],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        if !accepted_paper_ids.is_empty() {
            append_structured_harness_event(
                &tx,
                &current.run_id,
                "vault_membership_changed",
                &format!(
                    "Accepted {} Papers into the Project Vault",
                    accepted_paper_ids.len()
                ),
                Some(serde_json::json!({
                    "paperIds": accepted_paper_ids,
                    "vaultRevision": resulting_vault_revision,
                })),
                Some("reconciliation"),
                None,
                None,
                "harness",
            )?;
        }
        tx.execute(
            "update harness_runs set resulting_state_revision = ?2,
             resulting_vault_revision = ?3 where id = ?1",
            params![current.run_id, next_revision, resulting_vault_revision],
        )
        .map_err(|error| error.to_string())?;
        tx.execute(
            "update harness_reflections set next_direction = ?2 where run_id = ?1",
            params![current.run_id, plan.next_direction.trim()],
        )
        .map_err(|error| error.to_string())?;
        tx.execute(
            "update harness_change_sets set status = 'applied', resulting_state_revision = ?2,
             decided_at = datetime('now') where id = ?1",
            params![id, next_revision],
        )
        .map_err(|error| error.to_string())?;
        append_structured_harness_event(
            &tx,
            &current.run_id,
            "change_set_applied",
            &format!(
                "Applied {} Papers and {} Research State entries as revision {next_revision}",
                accepted_ids.len(),
                plan.entries.len()
            ),
            Some(serde_json::json!({
                "changeSetId": id,
                "resultingStateRevision": next_revision,
                "resultingVaultRevision": resulting_vault_revision,
                "acceptedCandidateCount": accepted_ids.len(),
                "researchEntryCount": plan.entries.len(),
            })),
            Some("reconciliation"),
            None,
            None,
            "harness",
        )?;
        tx.commit().map_err(|error| error.to_string())?;
        self.get_harness_change_set(&current.run_id)
    }

    /// Saves researcher-owned settings as the next configuration version.
    pub fn save_harness_configuration(
        &self,
        project_id: &str,
        configuration: &HarnessConfiguration,
    ) -> StoreResult<HarnessSnapshot> {
        self.save_harness_configuration_at(project_id, configuration, Utc::now())
    }

    /// Saves settings against an explicit clock for schedule tests.
    pub fn save_harness_configuration_at(
        &self,
        project_id: &str,
        configuration: &HarnessConfiguration,
        now: DateTime<Utc>,
    ) -> StoreResult<HarnessSnapshot> {
        validate_harness_configuration(configuration)?;
        let json = serde_json::to_string(configuration).map_err(|error| error.to_string())?;
        let next_run_at = if configuration.schedule.enabled {
            Some(next_schedule_occurrence(&configuration.schedule, now)?.to_rfc3339())
        } else {
            None
        };
        let mut conn = self.open_connection()?;
        let tx = conn.transaction().map_err(|error| error.to_string())?;
        validate_harness_authority(&tx, project_id, configuration)?;
        let current_version: i64 = tx
            .query_row(
                "select configuration_version from research_harnesses where project_id = ?1",
                params![project_id],
                |row| row.get(0),
            )
            .map_err(|_| format!("Research Harness not found for Project: {project_id}"))?;
        let next_version = current_version + 1;
        let updated = tx
            .execute(
                "update research_harnesses
                 set configuration_json = ?2,
                     configuration_version = ?5,
                     status = case
                       when ?3 then case when status in ('inactive', 'stopped') then 'idle' else status end
                       when status = 'inactive' then 'idle'
                       else status end,
                     schedule_enabled = ?3,
                     next_run_at = ?4,
                     terminal_stop_reason = case when ?3 then null else terminal_stop_reason end,
                     updated_at = datetime('now')
                 where project_id = ?1",
                params![
                    project_id,
                    json,
                    configuration.schedule.enabled,
                    next_run_at,
                    next_version
                ],
            )
            .map_err(|error| error.to_string())?;
        if updated == 0 {
            return Err(format!(
                "Research Harness not found for Project: {project_id}"
            ));
        }
        insert_harness_configuration_version(
            &tx,
            project_id,
            next_version,
            configuration,
            "researcher",
            None,
            "Researcher saved Harness Settings",
        )?;
        append_harness_control_event(
            &tx,
            project_id,
            "harness_configuration_saved",
            &format!("Saved Harness configuration v{next_version}"),
        )?;
        tx.commit().map_err(|error| error.to_string())?;
        self.get_harness_snapshot(project_id)
    }

    /// Prevents future scheduled claims while allowing an active Run to finish.
    pub fn pause_research_harness(&self, project_id: &str) -> StoreResult<HarnessSnapshot> {
        self.set_harness_requested_status(
            project_id,
            "paused",
            "harness_paused",
            "Research Harness paused",
        )
    }

    /// Reactivates a Harness and computes its next future scheduled occurrence.
    pub fn resume_research_harness(&self, project_id: &str) -> StoreResult<HarnessSnapshot> {
        let snapshot = self.get_harness_snapshot(project_id)?;
        let next_run_at = if snapshot.harness.configuration.schedule.enabled {
            Some(
                next_schedule_occurrence(&snapshot.harness.configuration.schedule, Utc::now())?
                    .to_rfc3339(),
            )
        } else {
            None
        };
        let conn = self.open_connection()?;
        conn.execute(
            "update research_harnesses
             set status = case when status = 'running' then status else 'idle' end,
                 requested_post_run_status = 'idle', terminal_stop_reason = null,
                 schedule_enabled = ?2, next_run_at = ?3, updated_at = datetime('now')
             where project_id = ?1",
            params![
                project_id,
                snapshot.harness.configuration.schedule.enabled,
                next_run_at
            ],
        )
        .map_err(|error| error.to_string())?;
        append_harness_control_event(
            &conn,
            project_id,
            "harness_resumed",
            "Research Harness resumed",
        )?;
        self.get_harness_snapshot(project_id)
    }

    /// Stops future claims and requests cancellation of any active Run.
    pub fn stop_research_harness(&self, project_id: &str) -> StoreResult<HarnessSnapshot> {
        let conn = self.open_connection()?;
        let (status, requested_status): (String, String) = conn
            .query_row(
                "select status, requested_post_run_status from research_harnesses
                 where project_id = ?1",
                params![project_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(|_| format!("Research Harness not found for Project: {project_id}"))?;
        if status == "stopped" && requested_status == "stopped" {
            return self.get_harness_snapshot(project_id);
        }
        let updated = conn
            .execute(
                "update research_harnesses
                 set status = case when status = 'running' then status else 'stopped' end,
                     requested_post_run_status = 'stopped', schedule_enabled = 0,
                     next_run_at = null, terminal_stop_reason = 'stopped_by_user',
                     updated_at = datetime('now') where project_id = ?1",
                params![project_id],
            )
            .map_err(|error| error.to_string())?;
        if updated == 0 {
            return Err(format!(
                "Research Harness not found for Project: {project_id}"
            ));
        }
        append_harness_control_event(
            &conn,
            project_id,
            "harness_stopped",
            "Research Harness stopped by user",
        )?;
        self.get_harness_snapshot(project_id)
    }

    /// Enforces terminal Project limits before either manual or scheduled starts.
    pub fn ensure_harness_can_start(&self, project_id: &str) -> StoreResult<()> {
        let snapshot = self.get_harness_snapshot(project_id)?;
        if snapshot.harness.status == "stopped" {
            return Err(format!(
                "Research Harness is stopped: {}",
                snapshot
                    .harness
                    .terminal_stop_reason
                    .as_deref()
                    .unwrap_or("resume required")
            ));
        }
        let conn = self.open_connection()?;
        if let Some(reason) = terminal_stop_reason(
            &conn,
            project_id,
            &snapshot.harness.configuration,
            Utc::now(),
        )? {
            conn.execute(
                "update research_harnesses set status = 'stopped', schedule_enabled = 0,
                 next_run_at = null, requested_post_run_status = 'stopped',
                 terminal_stop_reason = ?2, updated_at = datetime('now') where project_id = ?1",
                params![project_id, reason],
            )
            .map_err(|error| error.to_string())?;
            append_harness_control_event(&conn, project_id, "harness_stopped", &reason)?;
            return Err(format!("Research Harness stopped: {reason}"));
        }
        Ok(())
    }

    fn set_harness_requested_status(
        &self,
        project_id: &str,
        status: &str,
        event_kind: &str,
        summary: &str,
    ) -> StoreResult<HarnessSnapshot> {
        let conn = self.open_connection()?;
        let updated = conn
            .execute(
                "update research_harnesses
                 set status = case when status = 'running' then status else ?2 end,
                     requested_post_run_status = ?2, updated_at = datetime('now')
                 where project_id = ?1",
                params![project_id, status],
            )
            .map_err(|error| error.to_string())?;
        if updated == 0 {
            return Err(format!(
                "Research Harness not found for Project: {project_id}"
            ));
        }
        append_harness_control_event(&conn, project_id, event_kind, summary)?;
        self.get_harness_snapshot(project_id)
    }

    /// Claims at most one due schedule and advances it beyond `now`.
    pub fn claim_due_harness(
        &self,
        now: DateTime<Utc>,
        startup: bool,
    ) -> StoreResult<Option<DueHarnessClaim>> {
        let mut conn = self.open_connection()?;
        let tx = conn.transaction().map_err(|error| error.to_string())?;
        let candidate: Option<(String, String, String)> = tx
            .query_row(
                "select project_id, next_run_at, configuration_json
                 from research_harnesses
                 where schedule_enabled = 1 and next_run_at <= ?1 and status = 'idle'
                   and not exists (
                     select 1 from harness_runs r where r.project_id = research_harnesses.project_id
                       and r.status in ('queued','planning','searching','assessing','ranking','reconciling','canceling')
                   )
                 order by next_run_at, project_id limit 1",
                params![now.to_rfc3339()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()
            .map_err(|error| error.to_string())?;
        let Some((project_id, scheduled_for, configuration_json)) = candidate else {
            return Ok(None);
        };
        let configuration: HarnessConfiguration =
            serde_json::from_str(&configuration_json).map_err(|error| error.to_string())?;
        if let Some(reason) = terminal_stop_reason(&tx, &project_id, &configuration, now)? {
            tx.execute(
                "update research_harnesses set status = 'stopped', schedule_enabled = 0,
                 next_run_at = null, requested_post_run_status = 'stopped',
                 terminal_stop_reason = ?2, updated_at = datetime('now') where project_id = ?1",
                params![project_id, reason],
            )
            .map_err(|error| error.to_string())?;
            append_harness_control_event(&tx, &project_id, "harness_stopped", &reason)?;
            tx.commit().map_err(|error| error.to_string())?;
            return Ok(None);
        }
        let next = next_schedule_occurrence(&configuration.schedule, now)?.to_rfc3339();
        tx.execute(
            "update research_harnesses set next_run_at = ?2, last_scheduled_for = ?3,
             updated_at = datetime('now') where project_id = ?1 and next_run_at = ?3",
            params![project_id, next, scheduled_for],
        )
        .map_err(|error| error.to_string())?;
        append_harness_control_event(
            &tx,
            &project_id,
            if startup {
                "startup_catch_up_claimed"
            } else {
                "scheduled_run_claimed"
            },
            &format!("Claimed occurrence {scheduled_for}; next occurrence {next}"),
        )?;
        tx.commit().map_err(|error| error.to_string())?;
        Ok(Some(DueHarnessClaim {
            project_id,
            scheduled_for,
            trigger: if startup {
                HarnessRunTrigger::StartupCatchUp
            } else {
                HarnessRunTrigger::Scheduled
            },
        }))
    }

    /// Fails interrupted Harness Runs and restores their requested lifecycle state.
    pub fn recover_interrupted_harness_runs(&self) -> StoreResult<usize> {
        self.recover_prepared_synthesis()?;
        let mut conn = self.open_connection()?;
        let tx = conn.transaction().map_err(|error| error.to_string())?;
        let run_ids = {
            let mut statement = tx
                .prepare(
                    "select id from harness_runs
                     where status in ('queued','planning','searching','assessing','ranking','canceling')
                        or (status = 'reconciling' and execution_kind = 'codex_agent')
                     order by id",
                )
                .map_err(|error| error.to_string())?;
            let rows = statement
                .query_map([], |row| row.get::<_, String>(0))
                .map_err(|error| error.to_string())?;
            collect_rows(rows)?
        };
        let reconciling_runs = {
            let mut statement = tx
                .prepare(
                    "select id, search_run_id from harness_runs
                     where status = 'reconciling' and execution_kind != 'codex_agent'
                       and search_run_id is not null order by id",
                )
                .map_err(|error| error.to_string())?;
            let rows = statement
                .query_map([], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })
                .map_err(|error| error.to_string())?;
            collect_rows(rows)?
        };
        for run_id in &run_ids {
            tx.execute(
                "update agent_search_runs set cancel_requested = 1
                 where parent_run_id = ?1",
                params![run_id],
            )
            .map_err(|error| error.to_string())?;
            tx.execute(
                "update search_runs set status = 'cancelled', stop_reason = 'application_restarted',
                     finished_at = datetime('now')
                 where id in (select run_id from agent_search_runs where parent_run_id = ?1)
                   and status not in ('ready','failed','cancelled')",
                params![run_id],
            )
            .map_err(|error| error.to_string())?;
            tx.execute(
                "update harness_runs set status = 'failed',
                 stop_reason = case
                   when status = 'canceling' and stop_reason is not null then stop_reason
                   else 'application_restarted'
                 end,
                 summary = 'Research Run interrupted; committed work was preserved',
                 finished_at = datetime('now') where id = ?1",
                params![run_id],
            )
            .map_err(|error| error.to_string())?;
            append_harness_event(
                &tx,
                run_id,
                "interrupted",
                "Application restarted before Research Run completion",
            )?;
        }
        tx.execute(
            "update research_harnesses set status = requested_post_run_status,
             updated_at = datetime('now') where status = 'running'
             and not exists (
               select 1 from harness_runs run where run.project_id = research_harnesses.project_id
               and run.status = 'reconciling'
             )",
            [],
        )
        .map_err(|error| error.to_string())?;
        tx.commit().map_err(|error| error.to_string())?;
        for (run_id, search_run_id) in &reconciling_runs {
            let conn = self.open_connection()?;
            append_harness_event(
                &conn,
                run_id,
                "reconciliation_recovered",
                "Application restarted after search; finalizing the completed safe boundary",
            )?;
            if let Err(error) = self.record_automatic_harness_reflection(run_id) {
                self.record_harness_reflection_failure(run_id, &error)?;
            }
            self.finalize_harness_run(search_run_id)?;
        }
        Ok(run_ids.len() + reconciling_runs.len())
    }

    /// Persists one immutable operational reflection and detects recurrence.
    pub fn persist_harness_reflection(
        &self,
        run_id: &str,
        draft: &HarnessReflectionDraft,
    ) -> StoreResult<HarnessReflection> {
        let mut conn = self.open_connection()?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| error.to_string())?;
        let result = Self::persist_harness_reflection_on(&tx, run_id, draft)?;
        tx.commit().map_err(|error| error.to_string())?;
        Ok(result)
    }

    /// Execute the operation inside the caller's transaction.
    fn persist_harness_reflection_on(
        tx: &Connection,
        run_id: &str,
        draft: &HarnessReflectionDraft,
    ) -> StoreResult<HarnessReflection> {
        validate_reflection_draft(draft)?;
        let (project_id, status, configuration_version): (String, String, i64) = tx
            .query_row(
                "select project_id, status, configuration_version from harness_runs where id = ?1",
                params![run_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .map_err(|error| error.to_string())?;
        if !matches!(status.as_str(), "reconciling" | "ready") {
            return Err(
                "Only a reconciling or completed Research Run may record a reflection".to_string(),
            );
        }
        let reflection_id = format!("{run_id}:reflection");
        tx.execute(
            "insert into harness_reflections
             (id, run_id, project_id, policy_version, configuration_version,
              summary, next_direction, metrics_json, created_at)
             values (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, datetime('now'))",
            params![
                reflection_id,
                run_id,
                project_id,
                REFLECTION_POLICY_VERSION,
                configuration_version,
                draft.summary.trim(),
                draft.next_direction.as_deref().map(str::trim),
                draft.metrics_json
            ],
        )
        .map_err(|error| error.to_string())?;
        for (index, observation) in draft.observations.iter().enumerate() {
            let observation_id = format!("{reflection_id}:observation:{index}");
            tx.execute(
                "insert into harness_observations
                 (id, reflection_id, run_id, project_id, kind, signature, severity,
                  confidence, description, metrics_json, target, proposed_value_json,
                  proposal_eligible, created_at)
                 values (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, datetime('now'))",
                params![
                    observation_id,
                    reflection_id,
                    run_id,
                    project_id,
                    observation.kind.as_str(),
                    normalize_signature(&observation.signature),
                    observation.severity,
                    observation.confidence,
                    observation.description.trim(),
                    observation.metrics_json,
                    observation.target.map(HarnessImprovementTarget::as_str),
                    observation
                        .proposed_value
                        .as_ref()
                        .map(serde_json::to_string)
                        .transpose()
                        .map_err(|error| error.to_string())?,
                    observation.proposal_eligible
                ],
            )
            .map_err(|error| error.to_string())?;
            if observation.proposal_eligible {
                detect_harness_improvement(
                    &tx,
                    &project_id,
                    &normalize_signature(&observation.signature),
                    observation.target.expect("validated eligible target"),
                )?;
            }
            append_structured_harness_event(
                &tx,
                run_id,
                "operational_observation",
                &observation
                    .description
                    .trim()
                    .chars()
                    .take(500)
                    .collect::<String>(),
                Some(serde_json::json!({
                    "observationId": observation_id,
                    "kind": observation.kind.as_str(),
                    "signature": normalize_signature(&observation.signature),
                    "severity": observation.severity,
                    "confidence": observation.confidence,
                    "target": observation.target.map(HarnessImprovementTarget::as_str),
                    "proposalEligible": observation.proposal_eligible,
                })),
                Some("reflection"),
                None,
                None,
                "harness",
            )?;
        }
        append_structured_harness_event(
            &tx,
            run_id,
            "harness_reflection_recorded",
            &draft.summary.trim().chars().take(500).collect::<String>(),
            Some(serde_json::json!({
                "reflectionId": reflection_id.clone(),
                "observationCount": draft.observations.len(),
                "nextDirection": draft.next_direction.as_deref(),
            })),
            Some("reflection"),
            None,
            None,
            "harness",
        )?;
        read_harness_reflection(tx, &reflection_id)
    }

    /// Builds a bounded reflection solely from persisted Run telemetry.
    pub fn record_automatic_harness_reflection(&self, harness_run_id: &str) -> StoreResult<()> {
        let conn = self.open_connection()?;
        let (status, stop_reason, search_run_id): (String, Option<String>, Option<String>) = conn
            .query_row(
                "select status, stop_reason, search_run_id from harness_runs where id = ?1",
                params![harness_run_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .map_err(|error| error.to_string())?;
        if !matches!(status.as_str(), "reconciling" | "ready") {
            return Ok(());
        }
        let reflection_exists: bool = conn
            .query_row(
                "select exists(select 1 from harness_reflections where run_id = ?1)",
                params![harness_run_id],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        if reflection_exists {
            return Ok(());
        }
        let search_run_id =
            search_run_id.ok_or_else(|| "Completed Harness Run has no Search Run".to_string())?;
        let (added_count, iteration, provider_set, query_expansions): (
            i64,
            i64,
            Option<String>,
            Option<String>,
        ) = conn
            .query_row(
                "select added_count, iteration, provider_set, query_expansions
                 from search_runs where id = ?1",
                params![search_run_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .map_err(|error| error.to_string())?;
        let plan = conn
            .query_row(
                "select plan_json from harness_change_sets where run_id = ?1",
                params![harness_run_id],
                |row| row.get::<_, Option<String>>(0),
            )
            .optional()
            .map_err(|error| error.to_string())?
            .flatten()
            .map(|json| serde_json::from_str::<RunReconciliationPlan>(&json))
            .transpose()
            .map_err(|error| error.to_string())?;
        let accepted_count = plan
            .as_ref()
            .map(|plan| {
                plan.candidate_decisions
                    .iter()
                    .filter(|decision| decision.decision == CandidateDecisionKind::Accept)
                    .count()
            })
            .unwrap_or(0);
        let rejected_count = plan
            .as_ref()
            .map(|plan| {
                plan.candidate_decisions
                    .iter()
                    .filter(|decision| decision.decision == CandidateDecisionKind::Reject)
                    .count()
            })
            .unwrap_or(0);
        let metrics = serde_json::json!({
            "addedCount": added_count,
            "acceptedCandidateCount": accepted_count,
            "rejectedCandidateCount": rejected_count,
            "iterations": iteration,
            "providerSet": provider_set,
            "queryExpansions": query_expansions,
            "stopReason": stop_reason,
        });
        let planned_reflection = plan
            .as_ref()
            .and_then(|plan| plan.operational_reflection.as_ref());
        let observations = match planned_reflection {
            Some(reflection) => reflection
                .observations
                .iter()
                .map(|observation| HarnessObservationDraft {
                    kind: observation.kind,
                    signature: observation.signature.clone(),
                    severity: observation.severity,
                    confidence: observation.confidence,
                    description: observation.description.clone(),
                    metrics_json: metrics.to_string(),
                    target: observation.target,
                    proposed_value: observation.proposed_value.clone(),
                    proposal_eligible: observation.proposal_eligible,
                })
                .collect(),
            None if accepted_count == 0 => vec![HarnessObservationDraft {
                kind: HarnessObservationKind::WastedWork,
                signature: "completed-run-no-meaningful-additions".to_string(),
                severity: 0.5,
                confidence: 1.0,
                description:
                    "The completed Run added no accepted Papers or Research State changes."
                        .to_string(),
                metrics_json: metrics.to_string(),
                target: None,
                proposed_value: None,
                proposal_eligible: false,
            }],
            None => Vec::new(),
        };
        let summary = planned_reflection
            .map(|reflection| reflection.summary.clone())
            .unwrap_or_else(|| {
                format!(
                    "Run completed after {iteration} iteration(s) with {accepted_count} accepted and {rejected_count} rejected candidate(s)."
                )
            });
        let next_direction = planned_reflection
            .and_then(|reflection| reflection.next_direction.clone())
            .or_else(|| plan.as_ref().map(|plan| plan.next_direction.clone()));
        drop(conn);
        self.persist_harness_reflection(
            harness_run_id,
            &HarnessReflectionDraft {
                summary,
                next_direction,
                metrics_json: metrics.to_string(),
                observations,
            },
        )?;
        Ok(())
    }

    pub fn record_harness_reflection_failure(
        &self,
        harness_run_id: &str,
        error: &str,
    ) -> StoreResult<()> {
        let conn = self.open_connection()?;
        append_harness_event(
            &conn,
            harness_run_id,
            "harness_reflection_failed",
            &format!(
                "Operational reflection failed: {}",
                error.chars().take(300).collect::<String>()
            ),
        )
    }

    pub fn list_harness_improvements(
        &self,
        project_id: &str,
        status: Option<HarnessImprovementStatus>,
    ) -> StoreResult<Vec<HarnessImprovement>> {
        let conn = self.open_connection()?;
        read_harness_improvements(&conn, project_id, status)
    }

    pub fn get_harness_improvement(&self, id: &str) -> StoreResult<HarnessImprovement> {
        let conn = self.open_connection()?;
        read_harness_improvement(&conn, id)
    }

    pub fn edit_harness_improvement(
        &self,
        id: &str,
        proposed_value: &HarnessImprovementValue,
    ) -> StoreResult<HarnessImprovement> {
        let current = self.get_harness_improvement(id)?;
        if current.status != HarnessImprovementStatus::Proposed {
            return Err("Only a proposed Harness improvement may be edited".to_string());
        }
        let normalized = normalize_improvement_value(current.target, proposed_value)?;
        let conn = self.open_connection()?;
        conn.execute(
            "update harness_improvements set proposed_value_json = ?2,
             expected_effect = ?3 where id = ?1 and status = 'proposed'",
            params![
                id,
                serde_json::to_string(&normalized).map_err(|error| error.to_string())?,
                improvement_preview(current.target, &normalized)
            ],
        )
        .map_err(|error| error.to_string())?;
        append_harness_control_event(
            &conn,
            &current.project_id,
            "harness_improvement_edited",
            &format!("Edited Harness improvement {id}"),
        )?;
        self.get_harness_improvement(id)
    }

    pub fn accept_harness_improvement(&self, id: &str) -> StoreResult<HarnessImprovement> {
        let mut conn = self.open_connection()?;
        let tx = conn.transaction().map_err(|error| error.to_string())?;
        let improvement = read_harness_improvement(&tx, id)?;
        if improvement.status != HarnessImprovementStatus::Proposed {
            return Err("Only a proposed Harness improvement may be accepted".to_string());
        }
        let mut harness = read_research_harness(&tx, &improvement.project_id)?
            .ok_or_else(|| "Research Harness not found".to_string())?;
        if harness.configuration_version != improvement.base_configuration_version
            || configuration_value(&harness.configuration, improvement.target)
                != improvement.before_value
        {
            tx.execute(
                "update harness_improvements set status = 'superseded', decision_actor = 'system',
                 decision_reason = 'configuration_changed', decided_at = datetime('now') where id = ?1",
                params![id],
            )
            .map_err(|error| error.to_string())?;
            append_harness_control_event(
                &tx,
                &improvement.project_id,
                "harness_improvement_superseded",
                &format!("Improvement {id} was superseded by a configuration change"),
            )?;
            tx.commit().map_err(|error| error.to_string())?;
            return self.get_harness_improvement(id);
        }
        apply_improvement_value(
            &mut harness.configuration,
            improvement.target,
            &improvement.proposed_value,
        )?;
        let json =
            serde_json::to_string(&harness.configuration).map_err(|error| error.to_string())?;
        let next_version = harness.configuration_version + 1;
        tx.execute(
            "update research_harnesses set configuration_json = ?2,
             configuration_version = ?3, updated_at = datetime('now') where project_id = ?1",
            params![improvement.project_id, json, next_version],
        )
        .map_err(|error| error.to_string())?;
        insert_harness_configuration_version(
            &tx,
            &improvement.project_id,
            next_version,
            &harness.configuration,
            "improvement",
            Some(id),
            "Accepted Harness improvement",
        )?;
        tx.execute(
            "update harness_improvements set status = 'accepted', decision_actor = 'researcher',
             resulting_configuration_version = ?2, decided_at = datetime('now') where id = ?1",
            params![id, next_version],
        )
        .map_err(|error| error.to_string())?;
        append_harness_control_event(
            &tx,
            &improvement.project_id,
            "harness_improvement_accepted",
            &format!("Accepted improvement {id} as configuration v{next_version}"),
        )?;
        tx.commit().map_err(|error| error.to_string())?;
        self.get_harness_improvement(id)
    }

    pub fn reject_harness_improvement(
        &self,
        id: &str,
        reason: Option<&str>,
    ) -> StoreResult<HarnessImprovement> {
        let current = self.get_harness_improvement(id)?;
        if current.status != HarnessImprovementStatus::Proposed {
            return Err("Only a proposed Harness improvement may be rejected".to_string());
        }
        let conn = self.open_connection()?;
        conn.execute(
            "update harness_improvements set status = 'rejected', decision_actor = 'researcher',
             decision_reason = ?2, decided_at = datetime('now') where id = ?1",
            params![id, reason.map(str::trim).filter(|value| !value.is_empty())],
        )
        .map_err(|error| error.to_string())?;
        append_harness_control_event(
            &conn,
            &current.project_id,
            "harness_improvement_rejected",
            &format!("Rejected Harness improvement {id}"),
        )?;
        self.get_harness_improvement(id)
    }

    /// Creates an immutable Harness Run before its Deep Research execution.
    pub fn create_harness_run(&self, project_id: &str, search_id: &str) -> StoreResult<HarnessRun> {
        self.create_harness_run_with_trigger(project_id, search_id, HarnessRunTrigger::Manual, None)
    }

    /// Creates an immutable Run for a manual or claimed scheduled trigger.
    pub fn create_harness_run_with_trigger(
        &self,
        project_id: &str,
        search_id: &str,
        trigger: HarnessRunTrigger,
        scheduled_for: Option<&str>,
    ) -> StoreResult<HarnessRun> {
        let mut conn = self.open_connection()?;
        let tx = conn.transaction().map_err(|error| error.to_string())?;
        let harness = read_research_harness(&tx, project_id)?
            .ok_or_else(|| format!("Research Harness not found for Project: {project_id}"))?;
        validate_harness_configuration(&harness.configuration)?;
        validate_harness_authority(&tx, project_id, &harness.configuration)?;
        let id = timestamped_id("harness_run")?;
        let snapshot =
            serde_json::to_string(&harness.configuration).map_err(|error| error.to_string())?;
        let starting_state_revision: i64 = tx
            .query_row(
                "select current_revision from research_state_heads where project_id = ?1",
                params![project_id],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        let starting_vault_revision: i64 = tx
            .query_row(
                "select membership_revision from vaults where project_id = ?1",
                params![project_id],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        let effective_instructions = build_effective_instruction_stack(
            &tx,
            project_id,
            starting_state_revision,
            &harness.configuration,
        )?;
        let oriented_search_goal = render_harness_search_goal(&effective_instructions)?;
        let effective_instructions_json =
            serde_json::to_string(&effective_instructions).map_err(|error| error.to_string())?;
        tx.execute(
            "insert into harness_runs (
               id, project_id, status, configuration_snapshot_json,
               configuration_version, policy_version, effective_instruction_stack_json,
               search_id, starting_state_revision, starting_vault_revision,
               trigger, scheduled_for, started_at
             ) values (?1, ?2, 'queued', ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, datetime('now'))",
            params![
                id,
                project_id,
                snapshot,
                harness.configuration_version,
                HARNESS_POLICY_VERSION,
                effective_instructions_json,
                search_id,
                starting_state_revision,
                starting_vault_revision,
                trigger.as_str(),
                scheduled_for
            ],
        )
        .map_err(|error| {
            if error.to_string().contains("harness_runs.project_id") {
                "A Research Run is already active for this Project".to_string()
            } else {
                error.to_string()
            }
        })?;
        let oriented = tx
            .execute(
                "update searches set goal = ?2, updated_at = datetime('now') where id = ?1",
                params![search_id, oriented_search_goal],
            )
            .map_err(|error| error.to_string())?;
        if oriented != 1 {
            return Err(format!("Research search not found: {search_id}"));
        }
        tx.execute(
            "update research_harnesses set status = 'running', updated_at = datetime('now')
             where project_id = ?1",
            params![project_id],
        )
        .map_err(|error| error.to_string())?;
        append_harness_event(
            &tx,
            &id,
            "run_started",
            &format!("{} Research Run queued", trigger.as_str().replace('_', " ")),
        )?;
        tx.commit().map_err(|error| error.to_string())?;
        let conn = self.open_connection()?;
        read_harness_run(&conn, &id)
    }

    /// Convert a newly queued Run into an immutable managed-Codex snapshot.
    pub fn configure_codex_harness_run(
        &self,
        harness_run_id: &str,
        model: &str,
        limits: &AgentRunLimits,
    ) -> StoreResult<HarnessRun> {
        let mut conn = self.open_connection()?;
        let tx = conn.transaction().map_err(|error| error.to_string())?;
        let mut run = read_harness_run(&tx, harness_run_id)?;
        if run.status != "queued" {
            return Err("Only a queued Research Run can start Codex".to_string());
        }
        run.effective_instructions
            .run_context
            .maximum_provider_queries = limits.maximum_provider_queries;
        run.effective_instructions.run_context.maximum_llm_calls = limits.maximum_llm_calls;
        let instructions_json = serde_json::to_string(&run.effective_instructions)
            .map_err(|error| error.to_string())?;
        let limits_json = serde_json::to_string(limits).map_err(|error| error.to_string())?;
        tx.execute(
            "update harness_runs
             set status = 'planning', execution_kind = 'codex_agent', runtime_model = ?2,
                 effective_instruction_stack_json = ?3, agent_limits_json = ?4
             where id = ?1 and status = 'queued'",
            params![harness_run_id, model, instructions_json, limits_json],
        )
        .map_err(|error| error.to_string())?;
        append_harness_event(
            &tx,
            harness_run_id,
            "agent_starting",
            "Starting the managed research agent",
        )?;
        tx.commit().map_err(|error| error.to_string())?;
        let conn = self.open_connection()?;
        read_harness_run(&conn, harness_run_id)
    }

    /// Persist the app-server identities before the Codex turn does any work.
    pub fn attach_codex_turn(
        &self,
        harness_run_id: &str,
        thread_id: &str,
        turn_id: &str,
    ) -> StoreResult<HarnessRun> {
        let mut conn = self.open_connection()?;
        let tx = conn.transaction().map_err(|error| error.to_string())?;
        let updated = tx
            .execute(
                "update harness_runs
                 set status = 'searching', runtime_thread_id = ?2, runtime_turn_id = ?3
                 where id = ?1 and execution_kind = 'codex_agent'
                   and status in ('planning', 'searching')",
                params![harness_run_id, thread_id, turn_id],
            )
            .map_err(|error| error.to_string())?;
        if updated != 1 {
            return Err("Managed Research Run is no longer active".to_string());
        }
        append_harness_event(
            &tx,
            harness_run_id,
            "agent_started",
            "Managed research agent is working",
        )?;
        tx.commit().map_err(|error| error.to_string())?;
        let conn = self.open_connection()?;
        read_harness_run(&conn, harness_run_id)
    }

    /// Close a managed Run to new work and request cancellation of its child Searches.
    pub fn request_codex_harness_cancellation(
        &self,
        project_id: &str,
        reason: &str,
    ) -> StoreResult<(HarnessRun, bool)> {
        let mut conn = self.open_connection()?;
        conn.busy_timeout(Duration::from_secs(5))
            .map_err(|error| error.to_string())?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| error.to_string())?;
        let run_id: String = tx
            .query_row(
                "select id from harness_runs
                 where project_id = ?1 and execution_kind = 'codex_agent'
                 order by started_at desc, id desc limit 1",
                params![project_id],
                |row| row.get(0),
            )
            .map_err(|_| format!("Managed Research Run not found for Project: {project_id}"))?;
        let changed = tx
            .execute(
                "update harness_runs set status = 'canceling', stop_reason = ?2
                 where id = ?1 and status in
                   ('queued','planning','searching','assessing','ranking','reconciling')",
                params![run_id, reason],
            )
            .map_err(|error| error.to_string())?;
        if changed == 1 {
            tx.execute(
                "update agent_search_runs set cancel_requested = 1
                 where parent_run_id = ?1 and cancel_requested = 0",
                params![run_id],
            )
            .map_err(|error| error.to_string())?;
            append_harness_event(
                &tx,
                &run_id,
                "cancellation_requested",
                "Research Run cancellation requested",
            )?;
        }
        tx.commit().map_err(|error| error.to_string())?;
        let conn = self.open_connection()?;
        Ok((read_harness_run(&conn, &run_id)?, changed == 1))
    }

    /// Append one public lifecycle observation without storing model reasoning.
    pub fn record_harness_activity(
        &self,
        harness_run_id: &str,
        kind: &str,
        summary: &str,
        phase: Option<&str>,
    ) -> StoreResult<()> {
        let conn = self.open_connection()?;
        append_structured_harness_event(
            &conn,
            harness_run_id,
            kind,
            summary,
            None,
            phase,
            None,
            None,
            "harness",
        )
    }

    /// Charge attempted Reader work even if persisting its response later fails.
    fn record_agent_reader_attempt(
        &self,
        run_id: &str,
        project_id: &str,
        paper_id: &str,
        returned_text_chars: u64,
    ) -> StoreResult<()> {
        let mut conn = self.open_connection()?;
        conn.busy_timeout(Duration::from_secs(5))
            .map_err(|error| error.to_string())?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| error.to_string())?;
        let limits_json: Option<String> = tx
            .query_row(
                "select agent_limits_json from harness_runs
                 where id = ?1 and project_id = ?2 and execution_kind = 'codex_agent'
                   and status in ('planning', 'searching', 'assessing', 'ranking')",
                params![run_id, project_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|error| error.to_string())?
            .flatten();
        let limits_json = limits_json.ok_or("Managed Research Run is no longer active")?;
        let limits: AgentRunLimits =
            serde_json::from_str(&limits_json).map_err(|error| error.to_string())?;
        let already_read: bool = tx
            .query_row(
                "select exists(select 1 from agent_reader_usage
                 where run_id = ?1 and paper_id = ?2)",
                params![run_id, paper_id],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        let (distinct_papers, returned_chars): (u32, i64) = tx
            .query_row(
                "select count(*), coalesce(sum(returned_text_chars), 0)
                 from agent_reader_usage where run_id = ?1",
                params![run_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(|error| error.to_string())?;
        if !already_read && distinct_papers >= limits.maximum_distinct_papers_read {
            return Err("Research Run paper-read limit is exhausted".to_string());
        }
        let returned_text_chars: i64 = returned_text_chars
            .try_into()
            .map_err(|_| "Reader response is too large to account for".to_string())?;
        let unattempted_other_additions: i64 = tx
            .query_row(
                "select count(*) from agent_vault_additions addition
                 left join agent_reader_usage usage
                   on usage.run_id = addition.run_id and usage.paper_id = addition.paper_id
                 where addition.run_id = ?1 and addition.membership_added = 1
                   and addition.paper_id <> ?2 and usage.paper_id is null",
                params![run_id, paper_id],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        let reserved_chars =
            unattempted_other_additions.saturating_mul(FIRST_PAPER_ASSESSMENT_RESERVE_CHARS);
        if returned_chars
            .saturating_add(returned_text_chars)
            .saturating_add(reserved_chars)
            > limits.maximum_returned_text_chars as i64
        {
            return Err(
                "Research Run returned-text limit must preserve capacity for unassessed additions"
                    .to_string(),
            );
        }
        tx.execute(
            "insert into agent_reader_usage
               (run_id, paper_id, returned_text_chars, read_count)
             values (?1, ?2, ?3, 1)
             on conflict(run_id, paper_id) do update set
               returned_text_chars = returned_text_chars + excluded.returned_text_chars,
               read_count = read_count + 1",
            params![run_id, paper_id, returned_text_chars],
        )
        .map_err(|error| error.to_string())?;
        tx.commit().map_err(|error| error.to_string())
    }

    /// Persist successful Reader delivery and exact anchors in one transaction.
    pub fn record_agent_reader_delivery(
        &self,
        run_id: &str,
        project_id: &str,
        paper_id: &str,
        returned_text_chars: u64,
        anchors: &HashMap<String, crate::services::mcp::PassageAnchor>,
    ) -> StoreResult<()> {
        self.record_agent_reader_attempt(run_id, project_id, paper_id, returned_text_chars)?;
        let mut conn = self.open_connection()?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| error.to_string())?;
        let passage_refs: Vec<String> = anchors.keys().cloned().collect();
        for passage_ref in &passage_refs {
            if anchors[passage_ref].paper_id != paper_id {
                return Err("Reader anchor belongs to a different paper".into());
            }
            tx.execute(
                "insert into agent_reader_passages
                   (run_id, passage_ref, paper_id, delivered_at)
                 values (?1, ?2, ?3, datetime('now'))
                 on conflict(run_id, passage_ref) do nothing",
                params![run_id, passage_ref, paper_id],
            )
            .map_err(|error| error.to_string())?;
        }
        Self::persist_delivered_anchors_on(&tx, run_id, anchors)?;
        append_structured_harness_event(
            &tx,
            run_id,
            "agent_passages_delivered",
            &format!("Reader returned {} referenced passages", passage_refs.len()),
            Some(serde_json::json!({
                "paperId": paper_id,
                "passageRefs": passage_refs,
            })),
            Some("reading"),
            None,
            None,
            "harness",
        )?;
        tx.commit().map_err(|error| error.to_string())
    }

    /// Atomically admit one delegated evidence question against Run limits.
    ///
    /// Text and the model call are charged before the external request starts.
    /// Passage references become citable separately, only after a validated
    /// answer returns.
    pub fn admit_agent_evidence_question(
        &self,
        run_id: &str,
        project_id: &str,
        deliveries: &[AgentQuestionDelivery],
    ) -> StoreResult<()> {
        if deliveries.is_empty() {
            return Err("Evidence question requires source text".to_string());
        }
        let mut conn = self.open_connection()?;
        conn.busy_timeout(Duration::from_secs(5))
            .map_err(|error| error.to_string())?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| error.to_string())?;
        let (limits_json, direct_llm_calls): (String, u32) = tx
            .query_row(
                "select agent_limits_json, llm_call_count from harness_runs
                 where id = ?1 and project_id = ?2 and execution_kind = 'codex_agent'
                   and status in ('planning', 'searching', 'assessing', 'ranking')",
                params![run_id, project_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(|_| "Managed Research Run is no longer active".to_string())?;
        let limits: AgentRunLimits =
            serde_json::from_str(&limits_json).map_err(|error| error.to_string())?;
        let reserved_child_calls: u32 = tx
            .query_row(
                "select coalesce(sum(reserved_llm_calls), 0)
                 from agent_search_runs where parent_run_id = ?1",
                params![run_id],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        if direct_llm_calls
            .saturating_add(reserved_child_calls)
            .saturating_add(1)
            > limits.maximum_llm_calls
        {
            return Err("Research Run model-call limit is exhausted".to_string());
        }

        let existing_papers: HashSet<String> = {
            let mut statement = tx
                .prepare("select paper_id from agent_reader_usage where run_id = ?1")
                .map_err(|error| error.to_string())?;
            let rows = statement
                .query_map(params![run_id], |row| row.get(0))
                .map_err(|error| error.to_string())?;
            collect_rows(rows)?.into_iter().collect()
        };
        let new_papers: HashSet<&str> = deliveries
            .iter()
            .map(|delivery| delivery.paper_id.as_str())
            .filter(|paper_id| !existing_papers.contains(*paper_id))
            .collect();
        if existing_papers.len().saturating_add(new_papers.len())
            > limits.maximum_distinct_papers_read as usize
        {
            return Err("Research Run paper-read limit is exhausted".to_string());
        }
        let current_chars: i64 = tx
            .query_row(
                "select coalesce(sum(returned_text_chars), 0)
                 from agent_reader_usage where run_id = ?1",
                params![run_id],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        let added_chars: u64 = deliveries
            .iter()
            .map(|delivery| delivery.returned_text_chars)
            .sum();
        if current_chars.saturating_add(added_chars as i64)
            > limits.maximum_returned_text_chars as i64
        {
            return Err("Research Run returned-text limit is exhausted".to_string());
        }
        for delivery in deliveries {
            let belongs_to_project: bool = tx
                .query_row(
                    "select exists(
                       select 1 from vault_papers vp join vaults v on v.id = vp.vault_id
                       where vp.paper_id = ?1 and v.project_id = ?2
                     )",
                    params![delivery.paper_id, project_id],
                    |row| row.get(0),
                )
                .map_err(|error| error.to_string())?;
            if !belongs_to_project {
                return Err("Evidence question paper is outside this Project".to_string());
            }
            let chars: i64 = delivery
                .returned_text_chars
                .try_into()
                .map_err(|_| "Evidence question context is too large to account for".to_string())?;
            tx.execute(
                "insert into agent_reader_usage
                   (run_id, paper_id, returned_text_chars, read_count)
                 values (?1, ?2, ?3, 1)
                 on conflict(run_id, paper_id) do update set
                   returned_text_chars = returned_text_chars + excluded.returned_text_chars,
                   read_count = read_count + 1",
                params![run_id, delivery.paper_id, chars],
            )
            .map_err(|error| error.to_string())?;
        }
        tx.execute(
            "update harness_runs set llm_call_count = llm_call_count + 1 where id = ?1",
            params![run_id],
        )
        .map_err(|error| error.to_string())?;
        append_structured_harness_event(
            &tx,
            run_id,
            "agent_evidence_question_started",
            &format!(
                "Asked one evidence question over {} papers and {added_chars} characters",
                deliveries.len()
            ),
            Some(serde_json::json!({
                "paperCount": deliveries.len(),
                "returnedTextChars": added_chars,
            })),
            Some("reading"),
            None,
            None,
            "harness",
        )?;
        tx.commit().map_err(|error| error.to_string())
    }

    /// Make validated question-answer citations eligible for later State writes.
    pub fn record_agent_question_citations(
        &self,
        run_id: &str,
        project_id: &str,
        anchors: &HashMap<String, crate::services::mcp::PassageAnchor>,
    ) -> StoreResult<()> {
        let citations: Vec<(String, String)> = anchors
            .iter()
            .map(|(reference, anchor)| (anchor.paper_id.clone(), reference.clone()))
            .collect();
        if citations.is_empty() {
            return Ok(());
        }
        let mut conn = self.open_connection()?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| error.to_string())?;
        let active: bool = tx
            .query_row(
                "select exists(select 1 from harness_runs
                 where id = ?1 and project_id = ?2 and execution_kind = 'codex_agent'
                   and status in ('planning', 'searching', 'assessing', 'ranking'))",
                params![run_id, project_id],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        if !active {
            return Err("Managed Research Run is no longer active".to_string());
        }
        for (paper_id, passage_ref) in &citations {
            let was_admitted: bool = tx
                .query_row(
                    "select exists(select 1 from agent_reader_usage
                     where run_id = ?1 and paper_id = ?2)",
                    params![run_id, paper_id],
                    |row| row.get(0),
                )
                .map_err(|error| error.to_string())?;
            if !was_admitted {
                return Err("Question citation refers to unread evidence".to_string());
            }
            tx.execute(
                "insert into agent_reader_passages
                   (run_id, passage_ref, paper_id, delivered_at)
                 values (?1, ?2, ?3, datetime('now'))
                 on conflict(run_id, passage_ref) do nothing",
                params![run_id, passage_ref, paper_id],
            )
            .map_err(|error| error.to_string())?;
        }
        Self::persist_delivered_anchors_on(&tx, run_id, anchors)?;
        append_structured_harness_event(
            &tx,
            run_id,
            "agent_evidence_question_answered",
            &format!(
                "Evidence answer returned {} cited passages",
                citations.len()
            ),
            Some(serde_json::json!({
                "passageRefs": citations.iter().map(|(_, reference)| reference).collect::<Vec<_>>(),
            })),
            Some("reading"),
            None,
            None,
            "harness",
        )?;
        tx.commit().map_err(|error| error.to_string())
    }

    /// List child searches that must settle before their parent can finish.
    pub fn list_agent_search_runs_for_parent(
        &self,
        project_id: &str,
        parent_run_id: &str,
    ) -> StoreResult<Vec<SearchRun>> {
        let conn = self.open_connection()?;
        let mut statement = conn
            .prepare(
                "select r.id from agent_search_runs a
                 join search_runs r on r.id = a.run_id
                 where a.project_id = ?1 and a.parent_run_id = ?2 order by r.created_at, r.id",
            )
            .map_err(|error| error.to_string())?;
        let ids = statement
            .query_map(params![project_id, parent_run_id], |row| {
                row.get::<_, String>(0)
            })
            .map_err(|error| error.to_string())?;
        collect_rows(ids)?
            .into_iter()
            .map(|id| read_search_run(&conn, &id))
            .collect()
    }

    /// Record a required synthesis phase that honestly produced no mutation.
    pub fn record_agent_state_unchanged(&self, run_id: &str, reason: &str) -> StoreResult<()> {
        let mut conn = self.open_connection()?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|e| e.to_string())?;
        let result = Self::record_agent_state_unchanged_on(&tx, run_id, reason)?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(result)
    }

    /// Execute within the finalization transaction.
    fn record_agent_state_unchanged_on(
        conn: &Connection,
        run_id: &str,
        reason: &str,
    ) -> StoreResult<()> {
        validate_outcome_text("no-change reason", reason, 1_000)?;
        let valid: bool = conn
            .query_row(
                "select exists(select 1 from harness_runs
                 where id = ?1 and execution_kind = 'codex_agent'
                   and status = 'reconciling')",
                params![run_id],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        if !valid {
            return Err("Only a synthesizing managed Run may record no State change".to_string());
        }
        append_structured_harness_event(
            &conn,
            run_id,
            "agent_state_unchanged",
            reason.trim(),
            Some(serde_json::json!({"reason": reason.trim()})),
            Some("synthesizing"),
            None,
            None,
            "harness",
        )
    }

    /// Validate and apply exhaustive retention decisions for newly added papers.
    pub fn finalize_agent_paper_retention(
        &self,
        run_id: &str,
        dispositions: &[ResearchPaperDisposition],
    ) -> StoreResult<AgentPaperRetentionSummary> {
        let mut conn = self.open_connection()?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| error.to_string())?;
        let result = Self::finalize_agent_paper_retention_on(&tx, run_id, dispositions)?;
        tx.commit().map_err(|error| error.to_string())?;
        Ok(result)
    }

    /// Execute the operation inside the caller's transaction.
    fn finalize_agent_paper_retention_on(
        tx: &Connection,
        run_id: &str,
        dispositions: &[ResearchPaperDisposition],
    ) -> StoreResult<AgentPaperRetentionSummary> {
        let run = read_harness_run(&tx, run_id)?;
        if run.execution_kind != "codex_agent" || run.status != "reconciling" {
            return Err("Only a synthesizing managed Run may finalize papers".to_string());
        }
        let vault_id: String = tx
            .query_row(
                "select id from vaults where project_id = ?1",
                params![run.project_id],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        let additions: Vec<String> = {
            let mut statement = tx
                .prepare(
                    "select paper_id from agent_vault_additions
                     where run_id = ?1 and membership_added = 1 order by paper_id",
                )
                .map_err(|error| error.to_string())?;
            let rows = statement
                .query_map(params![run_id], |row| row.get(0))
                .map_err(|error| error.to_string())?;
            collect_rows(rows)?
        };
        let decisions: HashMap<&str, &ResearchPaperDisposition> = dispositions
            .iter()
            .map(|decision| (decision.paper_id.as_str(), decision))
            .collect();
        if decisions.len() != dispositions.len()
            || additions.len() != dispositions.len()
            || additions
                .iter()
                .any(|paper_id| !decisions.contains_key(paper_id.as_str()))
        {
            return Err(
                "Paper dispositions must cover every newly added paper exactly once".to_string(),
            );
        }

        let mut removed: i64 = 0;
        let mut unavailable: i64 = 0;
        for paper_id in &additions {
            let decision = decisions[paper_id.as_str()];
            validate_outcome_text("paper disposition reason", &decision.reason, 500)?;
            let attempted: bool = tx
                .query_row(
                    "select exists(select 1 from agent_reader_usage
                     where run_id = ?1 and paper_id = ?2 and read_count > 0)",
                    params![run_id, paper_id],
                    |row| row.get(0),
                )
                .map_err(|error| error.to_string())?;
            if !attempted {
                return Err(format!(
                    "Newly added paper was not assessed through Reader: {paper_id}"
                ));
            }
            let has_source: bool = tx
                .query_row(
                    "select exists(select 1 from document_sources
                     where paper_id = ?1 and (
                       coalesce(trim(source_url), '') <> '' or
                       coalesce(trim(landing_url), '') <> '' or
                       coalesce(trim(local_path), '') <> ''
                     ))",
                    params![paper_id],
                    |row| row.get(0),
                )
                .map_err(|error| error.to_string())?;
            if decision.disposition.retained() && !has_source {
                return Err(format!(
                    "Newly retained paper has no actionable source: {paper_id}"
                ));
            }
            let retained = decision.disposition.retained();
            if decision.disposition == PaperDispositionKind::Unavailable {
                unavailable += 1;
            }
            if !retained {
                removed += tx
                    .execute(
                        "delete from vault_papers where vault_id = ?1 and paper_id = ?2",
                        params![vault_id, paper_id],
                    )
                    .map_err(|error| error.to_string())? as i64;
            }
            tx.execute(
                "insert into agent_paper_dispositions
                   (run_id, paper_id, disposition, reason, retained, created_at)
                 values (?1, ?2, ?3, ?4, ?5, datetime('now'))
                 on conflict(run_id, paper_id) do update set
                   disposition = excluded.disposition, reason = excluded.reason,
                   retained = excluded.retained",
                params![
                    run_id,
                    paper_id,
                    decision.disposition.as_str(),
                    decision.reason.trim(),
                    retained,
                ],
            )
            .map_err(|error| error.to_string())?;
        }
        let (attempted, read) = read_agent_delivery_counts(tx, run_id)?;
        let retained = additions.len() as i64 - removed;
        append_structured_harness_event(
            &tx,
            run_id,
            "agent_papers_assessed",
            &format!("Assessed {attempted} papers; retained {retained} and removed {removed}"),
            Some(serde_json::json!({
                "attempted": attempted,
                "read": read,
                "unavailable": unavailable,
                "removed": removed,
                "retained": retained,
            })),
            Some("synthesizing"),
            None,
            None,
            "harness",
        )?;
        Ok(AgentPaperRetentionSummary {
            attempted,
            read,
            unavailable,
            removed,
            retained,
        })
    }

    /// Finish a managed Run from committed State, Vault, search, and Reader facts.
    pub fn finish_codex_harness_run(
        &self,
        harness_run_id: &str,
        status: &str,
        stop_reason: &str,
    ) -> StoreResult<HarnessRun> {
        let mut conn = self.open_connection()?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| error.to_string())?;
        let result = Self::finish_codex_harness_run_on(&tx, harness_run_id, status, stop_reason)?;
        tx.commit().map_err(|error| error.to_string())?;
        Ok(result)
    }

    /// Execute the operation inside the caller's transaction.
    fn finish_codex_harness_run_on(
        tx: &Connection,
        harness_run_id: &str,
        status: &str,
        stop_reason: &str,
    ) -> StoreResult<HarnessRun> {
        if !matches!(status, "ready" | "failed" | "cancelled") {
            return Err(format!("Invalid managed Research Run status: {status}"));
        }
        let run = read_harness_run(&tx, harness_run_id)?;
        if matches!(run.status.as_str(), "ready" | "failed" | "cancelled") {
            return Ok(run);
        }
        let active_children: i64 = tx
            .query_row(
                "select count(*) from agent_search_runs a
                 join search_runs r on r.id = a.run_id
                 where a.parent_run_id = ?1
                   and r.status not in ('ready', 'failed', 'cancelled')",
                params![harness_run_id],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        if active_children > 0 {
            return Err("Managed Research Run still has active child searches".to_string());
        }
        if status == "ready" && run.execution_kind == "codex_agent" {
            let incomplete_additions: i64 = tx
                .query_row(
                    "select count(*) from agent_vault_additions addition
                     left join agent_paper_dispositions disposition
                       on disposition.run_id = addition.run_id
                      and disposition.paper_id = addition.paper_id
                     left join agent_reader_usage usage
                       on usage.run_id = addition.run_id and usage.paper_id = addition.paper_id
                     where addition.run_id = ?1 and addition.membership_added = 1
                       and (disposition.paper_id is null or usage.paper_id is null)",
                    params![harness_run_id],
                    |row| row.get(0),
                )
                .map_err(|error| error.to_string())?;
            if incomplete_additions > 0 {
                return Err(
                    "Managed Research Run has unassessed or undispositioned paper additions"
                        .to_string(),
                );
            }
            let synthesis_completed: bool = tx
                .query_row(
                    "select exists(select 1 from harness_events
                     where run_id = ?1 and kind in ('agent_state_committed','agent_state_unchanged')
                       and phase = 'synthesizing')",
                    params![harness_run_id],
                    |row| row.get(0),
                )
                .map_err(|error| error.to_string())?;
            if !synthesis_completed {
                return Err("Managed Research Run has no completed State synthesis".to_string());
            }
            let has_outcome: bool = tx.query_row(
                "select exists(select 1 from harness_reflections where run_id=?1 and json_type(metrics_json,'$.agentOutcome')='object')",
                [harness_run_id], |row|row.get(0),
            ).map_err(|error|error.to_string())?;
            if !has_outcome {
                return Err("Managed Research Run has no persisted outcome".to_string());
            }
        }
        let (provider_queries, child_llm_calls, inspected_candidates): (u32, u32, u32) = tx
            .query_row(
                "select coalesce(sum(r.provider_query_count), 0),
                        coalesce(sum(r.llm_call_count), 0),
                        coalesce(sum(r.inspected_candidate_count), 0)
                 from agent_search_runs a join search_runs r on r.id = a.run_id
                 where a.parent_run_id = ?1",
                params![harness_run_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .map_err(|error| error.to_string())?;
        let llm_calls = run.llm_call_count.saturating_add(child_llm_calls);
        let (papers_read, returned_chars): (u32, i64) = tx
            .query_row(
                "select count(*), coalesce(sum(returned_text_chars), 0)
                 from agent_reader_usage where run_id = ?1",
                params![harness_run_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(|error| error.to_string())?;
        let state_iterations: u32 = tx
            .query_row(
                "select count(*) from research_state_revisions where run_id = ?1",
                params![harness_run_id],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        let resulting_state_revision: i64 = tx
            .query_row(
                "select current_revision from research_state_heads where project_id = ?1",
                params![run.project_id],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        let resulting_vault_revision: i64 = tx
            .query_row(
                "select membership_revision from vaults where project_id = ?1",
                params![run.project_id],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        let retained_papers: i64 = tx
            .query_row(
                "select count(*) from agent_vault_additions addition
                 join agent_paper_dispositions disposition
                   on disposition.run_id = addition.run_id
                  and disposition.paper_id = addition.paper_id
                 where addition.run_id = ?1 and addition.membership_added = 1
                   and disposition.retained = 1",
                params![harness_run_id],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        let summary = format!(
            "Attempted evidence access for {papers_read} papers ({returned_chars} characters charged), retained {retained_papers} papers, and committed {state_iterations} State revisions"
        );
        tx.execute(
            "update harness_runs
             set status = ?2, stop_reason = ?3, summary = ?4,
                 resulting_state_revision = ?5, resulting_vault_revision = ?6,
                 provider_query_count = ?7, llm_call_count = ?8,
                 iteration_count = ?9, inspected_candidate_count = ?10,
                 finished_at = datetime('now') where id = ?1",
            params![
                harness_run_id,
                status,
                stop_reason,
                summary,
                resulting_state_revision,
                resulting_vault_revision,
                provider_queries,
                llm_calls,
                state_iterations,
                inspected_candidates,
            ],
        )
        .map_err(|error| error.to_string())?;
        tx.execute(
            "update searches set status = ?2, stop_reason = ?3, summary = ?4,
             updated_at = datetime('now') where id = ?1",
            params![run.search_id, status, stop_reason, summary],
        )
        .map_err(|error| error.to_string())?;
        append_harness_event(&tx, harness_run_id, status, &summary)?;
        if status == "ready" {
            settle_harness_after_run(
                &tx,
                &run.project_id,
                harness_run_id,
                resulting_state_revision > run.starting_state_revision || retained_papers > 0,
            )?;
        } else {
            tx.execute(
                "update research_harnesses set status = requested_post_run_status,
                 updated_at = datetime('now') where project_id = ?1",
                params![run.project_id],
            )
            .map_err(|error| error.to_string())?;
        }
        read_harness_run(tx, harness_run_id)
    }

    /// Move a completed Codex turn into its short outcome-persistence phase.
    pub fn begin_codex_harness_finalization(&self, run_id: &str) -> StoreResult<()> {
        let conn = self.open_connection()?;
        let changed = conn
            .execute(
                "update harness_runs set status = 'reconciling'
                 where id = ?1 and execution_kind = 'codex_agent'
                   and status in ('planning', 'searching', 'assessing', 'ranking')",
                params![run_id],
            )
            .map_err(|error| error.to_string())?;
        if changed == 0 {
            return Err("Managed Research Run cannot begin finalization".to_string());
        }
        append_structured_harness_event(
            &conn,
            run_id,
            "synthesizing",
            "Synthesizing Research State from the completed investigation",
            None,
            Some("synthesizing"),
            None,
            None,
            "harness",
        )
    }

    /// Validate and retain one managed Run outcome in the existing reflection record.
    pub fn persist_agent_run_outcome(
        &self,
        run_id: &str,
        outcome: &ResearchRunOutcome,
    ) -> StoreResult<HarnessReflection> {
        let mut conn = self.open_connection()?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|e| e.to_string())?;
        let result = Self::persist_agent_run_outcome_on(&tx, run_id, outcome)?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(result)
    }

    /// Execute within the finalization transaction.
    fn persist_agent_run_outcome_on(
        conn: &Connection,
        run_id: &str,
        outcome: &ResearchRunOutcome,
    ) -> StoreResult<HarnessReflection> {
        validate_research_run_outcome_shape(outcome)?;
        validate_research_state_synthesis_shape(
            outcome
                .state_synthesis
                .as_ref()
                .ok_or("Managed Research outcome requires a State synthesis")?,
            outcome.next_direction.as_deref(),
        )?;
        let (project_id, execution_kind, status): (String, String, String) = conn
            .query_row(
                "select project_id, execution_kind, status from harness_runs where id = ?1",
                params![run_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .map_err(|error| error.to_string())?;
        if execution_kind != "codex_agent" || status != "reconciling" {
            return Err("Only a finalizing managed Run may record an outcome".to_string());
        }

        let allowed_search_runs: HashSet<String> = {
            let mut statement = conn
                .prepare("select run_id from agent_search_runs where parent_run_id = ?1")
                .map_err(|error| error.to_string())?;
            let rows = statement
                .query_map(params![run_id], |row| row.get(0))
                .map_err(|error| error.to_string())?;
            collect_rows(rows)?.into_iter().collect()
        };
        let allowed_entry_ids: HashSet<String> = {
            let mut statement = conn
                .prepare("select id from research_entries where project_id = ?1")
                .map_err(|error| error.to_string())?;
            let rows = statement
                .query_map(params![project_id], |row| row.get(0))
                .map_err(|error| error.to_string())?;
            collect_rows(rows)?.into_iter().collect()
        };
        let allowed_passage_refs: HashSet<String> = {
            let mut statement = conn
                .prepare("select passage_ref from agent_reader_passages where run_id = ?1")
                .map_err(|error| error.to_string())?;
            let rows = statement
                .query_map(params![run_id], |row| row.get(0))
                .map_err(|error| error.to_string())?;
            collect_rows(rows)?.into_iter().collect()
        };
        for task in &outcome.task_outcomes {
            if task
                .search_run_ids
                .iter()
                .any(|id| !allowed_search_runs.contains(id))
            {
                return Err("Research outcome references an unrelated Search Run".to_string());
            }
            if task
                .motivating_entry_ids
                .iter()
                .any(|id| !allowed_entry_ids.contains(id))
            {
                return Err("Research outcome references an unknown State entry".to_string());
            }
            if task
                .cited_passage_refs
                .iter()
                .any(|id| !allowed_passage_refs.contains(id))
            {
                return Err(
                    "Research outcome references a passage the Run did not read".to_string()
                );
            }
        }

        let metrics_json = serde_json::json!({
            "schemaVersion": 1,
            "agentOutcome": outcome,
        })
        .to_string();
        Self::persist_harness_reflection_on(
            conn,
            run_id,
            &HarnessReflectionDraft {
                summary: outcome.summary.clone(),
                next_direction: outcome.next_direction.clone(),
                metrics_json,
                observations: Vec::new(),
            },
        )
    }

    /// Return recent validated managed outcomes within count and character bounds.
    pub fn list_recent_agent_run_outcomes(
        &self,
        project_id: &str,
        maximum_outcomes: usize,
        maximum_chars: usize,
    ) -> StoreResult<Vec<PriorResearchRunOutcome>> {
        if maximum_outcomes == 0 || maximum_chars == 0 {
            return Ok(Vec::new());
        }
        let conn = self.open_connection()?;
        let mut statement = conn
            .prepare(
                "select run.id, reflection.metrics_json
                 from harness_reflections reflection
                 join harness_runs run on run.id = reflection.run_id
                 where run.project_id = ?1 and run.execution_kind = 'codex_agent'
                   and run.status = 'ready'
                 order by reflection.created_at desc, reflection.id desc limit 20",
            )
            .map_err(|error| error.to_string())?;
        let rows = statement
            .query_map(params![project_id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|error| error.to_string())?;
        let mut outcomes: Vec<PriorResearchRunOutcome> = Vec::new();
        let mut used_chars: usize = 0;
        for row in rows {
            let (run_id, metrics_json) = row.map_err(|error| error.to_string())?;
            let Ok(metrics) = serde_json::from_str::<serde_json::Value>(&metrics_json) else {
                continue;
            };
            let Some(value) = metrics.get("agentOutcome") else {
                continue;
            };
            let Ok(outcome) = serde_json::from_value::<ResearchRunOutcome>(value.clone()) else {
                continue;
            };
            let outcome = crate::services::research::synthesis::continuation(outcome);
            let size = serde_json::to_string(&outcome)
                .map_err(|error| error.to_string())?
                .chars()
                .count();
            if used_chars.saturating_add(size) > maximum_chars {
                continue;
            }
            used_chars += size;
            outcomes.push(PriorResearchRunOutcome { run_id, outcome });
            if outcomes.len() == maximum_outcomes {
                break;
            }
        }
        Ok(outcomes)
    }

    /// Attaches the concrete Deep Research Run and catches up its current status.
    pub fn attach_harness_search_run(
        &self,
        harness_run_id: &str,
        search_run_id: &str,
    ) -> StoreResult<HarnessRun> {
        let mut conn = self.open_connection()?;
        let tx = conn.transaction().map_err(|error| error.to_string())?;
        let search_run = read_search_run(&tx, search_run_id)?;
        let linked_status = if search_run.status == "ready" {
            "reconciling"
        } else {
            &search_run.status
        };
        let linked_finished_at = if search_run.status == "ready" {
            None
        } else {
            search_run.finished_at.as_deref()
        };
        let updated = tx
            .execute(
                "update harness_runs
                 set search_run_id = ?2, status = ?3, stop_reason = ?4, summary = ?5,
                     finished_at = ?6
                 where id = ?1",
                params![
                    harness_run_id,
                    search_run_id,
                    linked_status,
                    search_run.stop_reason,
                    search_run.error,
                    linked_finished_at
                ],
            )
            .map_err(|error| error.to_string())?;
        if updated == 0 {
            return Err(format!("Harness Run not found: {harness_run_id}"));
        }
        append_harness_event(
            &tx,
            harness_run_id,
            "search_linked",
            "Bounded scholarly search started",
        )?;
        if search_run.status != "queued" {
            append_harness_event(
                &tx,
                harness_run_id,
                linked_status,
                if search_run.status == "ready" {
                    "Reconciling candidate decisions, Research State, and reflection"
                } else {
                    search_run
                        .error
                        .as_deref()
                        .or(search_run.stop_reason.as_deref())
                        .unwrap_or(&search_run.status)
                },
            )?;
        }
        if matches!(search_run.status.as_str(), "failed" | "cancelled") {
            let project_id: String = tx
                .query_row(
                    "select project_id from harness_runs where id = ?1",
                    params![harness_run_id],
                    |row| row.get(0),
                )
                .map_err(|error| error.to_string())?;
            tx.execute(
                "update research_harnesses set status = requested_post_run_status, updated_at = datetime('now')
                 where project_id = ?1",
                params![project_id],
            )
            .map_err(|error| error.to_string())?;
        }
        tx.commit().map_err(|error| error.to_string())?;
        let conn = self.open_connection()?;
        read_harness_run(&conn, harness_run_id)
    }

    /// Marks a Harness Run failed when its delegated search cannot be started.
    pub fn fail_harness_run(&self, harness_run_id: &str, error: &str) -> StoreResult<()> {
        let mut conn = self.open_connection()?;
        let tx = conn.transaction().map_err(|cause| cause.to_string())?;
        let project_id: String = tx
            .query_row(
                "select project_id from harness_runs where id = ?1",
                params![harness_run_id],
                |row| row.get(0),
            )
            .map_err(|cause| cause.to_string())?;
        tx.execute(
            "update harness_runs
             set status = 'failed', summary = ?2, finished_at = datetime('now')
             where id = ?1",
            params![harness_run_id, error],
        )
        .map_err(|cause| cause.to_string())?;
        tx.execute(
            "update research_harnesses set status = requested_post_run_status,
             updated_at = datetime('now') where project_id = ?1",
            params![project_id],
        )
        .map_err(|cause| cause.to_string())?;
        append_harness_event(&tx, harness_run_id, "failed", error)?;
        tx.commit().map_err(|cause| cause.to_string())
    }

    /// Loads one current or historical Research State revision.
    pub fn get_research_state(
        &self,
        project_id: &str,
        revision: Option<i64>,
    ) -> StoreResult<ResearchStateSnapshot> {
        let conn = self.open_connection()?;
        read_research_state(&conn, project_id, revision)
    }

    /// Loads one Research Entry with provenance and immutable history.
    pub fn get_research_entry(
        &self,
        entry_id: &str,
        revision: Option<i64>,
    ) -> StoreResult<ResearchEntryDetail> {
        let conn = self.open_connection()?;
        read_research_entry_detail(&conn, entry_id, revision)
    }

    /// Lists bounded canonical chunks available as evidence in one Project.
    pub fn list_research_evidence_candidates(
        &self,
        project_id: &str,
        query: Option<&str>,
    ) -> StoreResult<Vec<ResearchEvidenceCandidate>> {
        let conn = self.open_connection()?;
        let query = query.map(str::trim).filter(|value| !value.is_empty());
        let pattern = query.map(|value| format!("%{value}%"));
        let mut statement = conn
            .prepare(
                "select c.id, c.paper_id, p.title, c.page_start, c.page_end,
                        c.heading_path, c.text
                 from document_chunks c
                 join papers p on p.id = c.paper_id
                 join vault_papers vp on vp.paper_id = c.paper_id
                 join vaults v on v.id = vp.vault_id
                 where v.project_id = ?1 and (?2 is null or c.text like ?2 or p.title like ?2)
                 order by p.title, c.chunk_index limit 100",
            )
            .map_err(|error| error.to_string())?;
        let rows = statement
            .query_map(params![project_id, pattern], |row| {
                Ok(ResearchEvidenceCandidate {
                    chunk_id: row.get(0)?,
                    paper_id: row.get(1)?,
                    paper_title: row.get(2)?,
                    page_start: row.get(3)?,
                    page_end: row.get(4)?,
                    heading_path: row.get(5)?,
                    text: row.get(6)?,
                })
            })
            .map_err(|error| error.to_string())?;
        collect_rows(rows)
    }

    /// Creates one manual Research Entry as the next State revision.
    pub fn create_research_entry(
        &self,
        project_id: &str,
        expected_revision: i64,
        draft: &ResearchEntryDraft,
    ) -> StoreResult<ResearchStateMutation> {
        let mut conn = self.open_connection()?;
        let tx = conn.transaction().map_err(|error| error.to_string())?;
        require_current_state_revision(&tx, project_id, expected_revision)?;
        let next_revision = expected_revision + 1;
        let entry_id = timestamped_id("research_entry")?;
        validate_research_entry_draft(&tx, project_id, draft)?;
        insert_state_revision(
            &tx,
            project_id,
            next_revision,
            None,
            manual_reason(draft.reason.as_deref()),
        )?;
        insert_new_research_entry(&tx, &entry_id, project_id, next_revision, None, draft)?;
        set_current_state_revision(&tx, project_id, next_revision)?;
        tx.commit().map_err(|error| error.to_string())?;

        let state = self.get_research_state(project_id, None)?;
        let entry = self.get_research_entry(&entry_id, None)?;
        Ok(ResearchStateMutation { state, entry })
    }

    /// Revises one entry without changing its semantic kind or stable id.
    pub fn revise_research_entry(
        &self,
        expected_revision: i64,
        update: &ResearchEntryUpdate,
    ) -> StoreResult<ResearchStateMutation> {
        let mut conn = self.open_connection()?;
        let tx = conn.transaction().map_err(|error| error.to_string())?;
        let current = read_current_research_entry(&tx, &update.id)?;
        require_current_state_revision(&tx, &current.project_id, expected_revision)?;
        let draft = ResearchEntryDraft {
            kind: current.kind,
            epistemic_status: update.epistemic_status,
            text: update.text.clone(),
            evidence: update.evidence.clone(),
            relations: update.relations.clone(),
            context: update.context.clone(),
            reason: update.reason.clone(),
        };
        validate_research_entry_draft(&tx, &current.project_id, &draft)?;
        let next_revision = expected_revision + 1;
        insert_state_revision(
            &tx,
            &current.project_id,
            next_revision,
            None,
            manual_reason(update.reason.as_deref()),
        )?;
        insert_research_entry_version(
            &tx,
            &update.id,
            &current.project_id,
            next_revision,
            current.lifecycle,
            None,
            &draft,
        )?;
        tx.execute(
            "update research_entries
             set epistemic_status = ?2, text = ?3, last_revision = ?4,
                 updated_at = datetime('now') where id = ?1",
            params![
                update.id,
                update.epistemic_status.as_str(),
                update.text.trim(),
                next_revision
            ],
        )
        .map_err(|error| error.to_string())?;
        set_current_state_revision(&tx, &current.project_id, next_revision)?;
        tx.commit().map_err(|error| error.to_string())?;

        let state = self.get_research_state(&current.project_id, None)?;
        let entry = self.get_research_entry(&update.id, None)?;
        Ok(ResearchStateMutation { state, entry })
    }

    /// Appends a contested or superseded lifecycle revision.
    pub fn set_research_entry_lifecycle(
        &self,
        entry_id: &str,
        expected_revision: i64,
        lifecycle: EntryLifecycle,
        reason: &str,
    ) -> StoreResult<ResearchStateMutation> {
        if lifecycle == EntryLifecycle::Active {
            return Err("Lifecycle review may only contest or supersede an entry".to_string());
        }
        let reason = reason.trim();
        if reason.is_empty() {
            return Err("A lifecycle change requires a reason".to_string());
        }
        let mut conn = self.open_connection()?;
        let tx = conn.transaction().map_err(|error| error.to_string())?;
        let current = read_current_research_entry(&tx, entry_id)?;
        require_current_state_revision(&tx, &current.project_id, expected_revision)?;
        let previous =
            read_research_entry_detail_from_conn(&tx, entry_id, Some(expected_revision))?;
        let draft = ResearchEntryDraft {
            kind: current.kind,
            epistemic_status: current.epistemic_status,
            text: current.text.clone(),
            evidence: previous
                .evidence
                .iter()
                .map(|link| EvidenceLinkDraft {
                    chunk_id: link.chunk_id.clone(),
                    excerpt: Some(link.excerpt.clone()),
                    support_note: link.support_note.clone(),
                })
                .collect(),
            relations: previous
                .relations
                .iter()
                .map(|link| EntryRelationDraft {
                    target_entry_id: link.target_entry_id.clone(),
                    kind: link.kind,
                })
                .collect(),
            context: previous
                .context
                .iter()
                .map(|link| ResearchContextLinkDraft {
                    kind: link.kind,
                    context_id: link.context_id.clone(),
                    label: link.label.clone(),
                })
                .collect(),
            reason: Some(reason.to_string()),
        };
        let next_revision = expected_revision + 1;
        insert_state_revision(&tx, &current.project_id, next_revision, None, reason)?;
        insert_research_entry_version(
            &tx,
            entry_id,
            &current.project_id,
            next_revision,
            lifecycle,
            None,
            &draft,
        )?;
        tx.execute(
            "update research_entries
             set lifecycle = ?2, last_revision = ?3, updated_at = datetime('now')
             where id = ?1",
            params![entry_id, lifecycle.as_str(), next_revision],
        )
        .map_err(|error| error.to_string())?;
        set_current_state_revision(&tx, &current.project_id, next_revision)?;
        tx.commit().map_err(|error| error.to_string())?;

        let state = self.get_research_state(&current.project_id, None)?;
        let entry = self.get_research_entry(entry_id, None)?;
        Ok(ResearchStateMutation { state, entry })
    }

    /// Publishes one completed Run's prepared entries as one atomic State revision.
    pub fn publish_harness_run_research_state(
        &self,
        run_id: &str,
        expected_revision: i64,
        drafts: &[ResearchEntryDraft],
    ) -> StoreResult<ResearchStateSnapshot> {
        if drafts.is_empty() {
            return Err("A Research State publication must contain an entry".to_string());
        }
        let mut conn = self.open_connection()?;
        let tx = conn.transaction().map_err(|error| error.to_string())?;
        let (project_id, status): (String, String) = tx
            .query_row(
                "select project_id, status from harness_runs where id = ?1",
                params![run_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(|error| error.to_string())?;
        if !matches!(status.as_str(), "reconciling" | "ready") {
            return Err(
                "Only a reconciling or successfully completed Research Run may publish State"
                    .to_string(),
            );
        }
        require_current_state_revision(&tx, &project_id, expected_revision)?;
        for draft in drafts {
            validate_research_entry_draft(&tx, &project_id, draft)?;
        }
        let next_revision = expected_revision + 1;
        insert_state_revision(
            &tx,
            &project_id,
            next_revision,
            Some(run_id),
            "Research Run published State",
        )?;
        for draft in drafts {
            let entry_id = timestamped_id("research_entry")?;
            insert_new_research_entry(
                &tx,
                &entry_id,
                &project_id,
                next_revision,
                Some(run_id),
                draft,
            )?;
            append_harness_event(
                &tx,
                run_id,
                "research_entry_added",
                &format!("Added {} to Research State", draft.kind.as_str()),
            )?;
        }
        set_current_state_revision(&tx, &project_id, next_revision)?;
        tx.execute(
            "update harness_runs set resulting_state_revision = ?2 where id = ?1",
            params![run_id, next_revision],
        )
        .map_err(|error| error.to_string())?;
        append_harness_event(
            &tx,
            run_id,
            "research_state_published",
            &format!("Research State advanced to revision {next_revision}"),
        )?;
        tx.commit().map_err(|error| error.to_string())?;
        self.get_research_state(&project_id, None)
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

    /// Add one agent-authored note and its retry receipt atomically.
    ///
    /// Replaying the same caller/tool/request tuple with the same payload
    /// returns the original identities. Reusing it for different content is a
    /// conflict and does not write another note.
    #[allow(clippy::too_many_arguments)]
    pub fn add_agent_note_idempotent(
        &self,
        scope_kind: &str,
        scope_id: &str,
        anchor: &ThreadAnchor,
        body: &str,
        caller: &str,
        run_id: Option<&str>,
        request_id: &str,
        payload_hash: &str,
    ) -> StoreResult<AgentNoteReceipt> {
        const TOOL: &str = "reader_add_note";
        let mut conn = self.open_connection()?;
        let tx = conn.transaction().map_err(|error| error.to_string())?;

        let existing: Option<(String, String)> = tx
            .query_row(
                "select payload_hash, result_json from mcp_mutation_receipts
                 where caller = ?1 and tool = ?2 and request_id = ?3",
                params![caller, TOOL, request_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(|error| error.to_string())?;
        if let Some((existing_hash, result_json)) = existing {
            if existing_hash != payload_hash {
                return Err("Request ID was already used with a different payload".to_string());
            }
            return serde_json::from_str(&result_json).map_err(|error| error.to_string());
        }
        require_agent_run_write_admission(&tx, run_id, None)?;

        let thread = find_or_create_thread_id(&tx, scope_kind, scope_id, anchor)?;
        let entry_id = timestamped_id("entry")?;
        let draft = ChatEntryDraft::agent_note(
            body.to_string(),
            caller.to_string(),
            run_id.map(str::to_string),
        );
        insert_chat_entry(&tx, &entry_id, &thread.id, &draft)?;
        let receipt = AgentNoteReceipt {
            thread_id: thread.id,
            entry_id,
        };
        let result_json = serde_json::to_string(&receipt).map_err(|error| error.to_string())?;
        tx.execute(
            "insert into mcp_mutation_receipts
               (caller, tool, request_id, payload_hash, result_json, created_at)
             values (?1, ?2, ?3, ?4, ?5, datetime('now'))",
            params![caller, TOOL, request_id, payload_hash, result_json],
        )
        .map_err(|error| error.to_string())?;
        tx.commit().map_err(|error| error.to_string())?;
        Ok(receipt)
    }

    /// Return a prior State update result, rejecting a changed retry payload.
    pub fn find_agent_state_update_receipt(
        &self,
        caller: &str,
        request_id: &str,
        payload_hash: &str,
    ) -> StoreResult<Option<AgentStateUpdateReceipt>> {
        let conn = self.open_connection()?;
        let existing: Option<(String, String)> = conn
            .query_row(
                "select payload_hash, result_json from mcp_mutation_receipts
                 where caller = ?1 and tool = 'state_update' and request_id = ?2",
                params![caller, request_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(|error| error.to_string())?;
        let Some((existing_hash, result_json)) = existing else {
            return Ok(None);
        };
        if existing_hash != payload_hash {
            return Err("Request ID was already used with a different payload".to_string());
        }
        serde_json::from_str(&result_json)
            .map(Some)
            .map_err(|error| error.to_string())
    }

    /// Apply a bounded agent State batch as one revision and one retry receipt.
    #[allow(clippy::too_many_arguments)]
    pub fn apply_agent_state_update(
        &self,
        project_id: &str,
        base_revision: i64,
        caller: &str,
        run_id: Option<&str>,
        request_id: &str,
        payload_hash: &str,
        changes: Vec<AgentStateChange>,
    ) -> StoreResult<AgentStateUpdateReceipt> {
        let mut conn = self.open_connection()?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| error.to_string())?;
        let result = Self::apply_agent_state_update_on(
            &tx,
            project_id,
            base_revision,
            caller,
            run_id,
            request_id,
            payload_hash,
            changes,
        )?;
        tx.commit().map_err(|error| error.to_string())?;
        Ok(result)
    }

    /// Execute the operation inside the caller's transaction.
    fn apply_agent_state_update_on(
        tx: &Connection,
        project_id: &str,
        base_revision: i64,
        caller: &str,
        run_id: Option<&str>,
        request_id: &str,
        payload_hash: &str,
        changes: Vec<AgentStateChange>,
    ) -> StoreResult<AgentStateUpdateReceipt> {
        const TOOL: &str = "state_update";
        if changes.is_empty() || changes.len() > 20 {
            return Err("State update must contain between 1 and 20 changes".to_string());
        }
        let existing: Option<(String, String)> = tx
            .query_row(
                "select payload_hash, result_json from mcp_mutation_receipts
                 where caller = ?1 and tool = ?2 and request_id = ?3",
                params![caller, TOOL, request_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(|error| error.to_string())?;
        if let Some((existing_hash, result_json)) = existing {
            if existing_hash != payload_hash {
                return Err("Request ID was already used with a different payload".to_string());
            }
            return serde_json::from_str(&result_json).map_err(|error| error.to_string());
        }
        require_agent_run_write_admission(&tx, run_id, Some(project_id))?;
        require_current_state_revision(&tx, project_id, base_revision)?;

        // Allocate create ids before validation so relations may target another
        // create operation in the same atomic batch.
        let mut created_ids: HashMap<String, String> = HashMap::new();
        for change in &changes {
            if let AgentStateChange::Create { operation_key, .. } = change {
                if operation_key.trim().is_empty() || created_ids.contains_key(operation_key) {
                    return Err("Create operation keys must be non-empty and unique".to_string());
                }
                // Batch allocation can outpace the platform clock resolution.
                created_ids.insert(
                    operation_key.clone(),
                    format!("research_entry_{}", uuid::Uuid::new_v4().simple()),
                );
            }
        }
        let pending_entry_ids: HashSet<String> = created_ids.values().cloned().collect();

        enum PreparedChange {
            Create(String, String, ResearchEntryDraft, Vec<String>),
            Revise(ResearchEntryUpdate, ResearchEntryDraft, Vec<String>),
            Lifecycle(String, EntryLifecycle, ResearchEntryDraft),
        }
        let mut prepared = Vec::new();
        let mut new_statements: HashSet<String> = HashSet::new();
        for change in changes {
            match change {
                AgentStateChange::Create {
                    operation_key,
                    mut draft,
                    evidence_relationships,
                } => {
                    resolve_local_relation_targets(&mut draft.relations, &created_ids);
                    validate_evidence_relationships(&draft, &evidence_relationships)?;
                    validate_research_entry_draft_with_pending(
                        &tx,
                        project_id,
                        &draft,
                        &pending_entry_ids,
                    )?;
                    let normalized_statement = normalize_state_statement(&draft.text);
                    let duplicate = !new_statements.insert(normalized_statement.clone())
                        || read_research_entry_summaries(&tx, project_id, base_revision)?
                            .into_iter()
                            .any(|entry| {
                                entry.lifecycle == EntryLifecycle::Active
                                    && normalize_state_statement(&entry.text)
                                        == normalized_statement
                            });
                    if duplicate {
                        return Err("Equivalent active Research Entry already exists".to_string());
                    }
                    prepared.push(PreparedChange::Create(
                        operation_key.clone(),
                        created_ids[&operation_key].clone(),
                        draft,
                        evidence_relationships,
                    ));
                }
                AgentStateChange::Revise {
                    mut update,
                    evidence_relationships,
                } => {
                    resolve_local_relation_targets(&mut update.relations, &created_ids);
                    let current = read_current_research_entry(&tx, &update.id)?;
                    if current.project_id != project_id {
                        return Err(format!(
                            "Research Entry is outside this Project: {}",
                            update.id
                        ));
                    }
                    let draft = ResearchEntryDraft {
                        kind: current.kind,
                        epistemic_status: update.epistemic_status,
                        text: update.text.clone(),
                        evidence: update.evidence.clone(),
                        relations: update.relations.clone(),
                        context: update.context.clone(),
                        reason: update.reason.clone(),
                    };
                    validate_evidence_relationships(&draft, &evidence_relationships)?;
                    validate_research_entry_draft_with_pending(
                        &tx,
                        project_id,
                        &draft,
                        &pending_entry_ids,
                    )?;
                    prepared.push(PreparedChange::Revise(
                        update,
                        draft,
                        evidence_relationships,
                    ));
                }
                AgentStateChange::SetLifecycle {
                    entry_id,
                    lifecycle,
                    reason,
                } => {
                    let current = read_current_research_entry(&tx, &entry_id)?;
                    if current.project_id != project_id || reason.trim().is_empty() {
                        return Err(
                            "Lifecycle change requires a scoped entry and reason".to_string()
                        );
                    }
                    let detail =
                        read_research_entry_detail_from_conn(&tx, &entry_id, Some(base_revision))?;
                    prepared.push(PreparedChange::Lifecycle(
                        entry_id,
                        lifecycle,
                        research_entry_draft_from_detail(&detail, &reason),
                    ));
                }
            }
        }

        let revision = base_revision + 1;
        insert_state_revision(
            &tx,
            project_id,
            revision,
            run_id,
            "Agent Research State update",
        )?;
        for change in &prepared {
            if let PreparedChange::Create(_, entry_id, draft, _) = change {
                insert_research_entry_header(&tx, entry_id, project_id, revision, run_id, draft)?;
            }
        }
        let mut affected_entry_ids = Vec::new();
        let mut created_entry_ids = HashMap::new();
        for change in prepared {
            match change {
                PreparedChange::Create(key, entry_id, draft, relationships) => {
                    insert_research_entry_version(
                        &tx,
                        &entry_id,
                        project_id,
                        revision,
                        EntryLifecycle::Active,
                        run_id,
                        &draft,
                    )?;
                    set_evidence_relationships(&tx, &entry_id, revision, &relationships)?;
                    created_entry_ids.insert(key, entry_id.clone());
                    affected_entry_ids.push(entry_id);
                }
                PreparedChange::Revise(update, draft, relationships) => {
                    let current = read_current_research_entry(&tx, &update.id)?;
                    insert_research_entry_version(
                        &tx,
                        &update.id,
                        project_id,
                        revision,
                        current.lifecycle,
                        run_id,
                        &draft,
                    )?;
                    set_evidence_relationships(&tx, &update.id, revision, &relationships)?;
                    tx.execute(
                        "update research_entries set epistemic_status = ?2, text = ?3,
                         last_revision = ?4, updated_at = datetime('now') where id = ?1",
                        params![
                            update.id,
                            update.epistemic_status.as_str(),
                            update.text.trim(),
                            revision
                        ],
                    )
                    .map_err(|error| error.to_string())?;
                    affected_entry_ids.push(update.id);
                }
                PreparedChange::Lifecycle(entry_id, lifecycle, draft) => {
                    let current = read_current_research_entry(&tx, &entry_id)?;
                    insert_research_entry_version(
                        &tx,
                        &entry_id,
                        project_id,
                        revision,
                        lifecycle,
                        run_id.or(current.origin_run_id.as_deref()),
                        &draft,
                    )?;
                    tx.execute(
                        "update research_entries set lifecycle = ?2, last_revision = ?3,
                         updated_at = datetime('now') where id = ?1",
                        params![entry_id, lifecycle.as_str(), revision],
                    )
                    .map_err(|error| error.to_string())?;
                    affected_entry_ids.push(entry_id);
                }
            }
        }
        set_current_state_revision(&tx, project_id, revision)?;
        let receipt = AgentStateUpdateReceipt {
            revision,
            affected_entry_ids,
            created_entry_ids,
        };
        let result_json = serde_json::to_string(&receipt).map_err(|error| error.to_string())?;
        tx.execute(
            "insert into mcp_mutation_receipts
               (caller, tool, request_id, payload_hash, result_json, created_at)
             values (?1, ?2, ?3, ?4, ?5, datetime('now'))",
            params![caller, TOOL, request_id, payload_hash, result_json],
        )
        .map_err(|error| error.to_string())?;
        if let Some(run_id) = run_id {
            let managed_status: Option<String> = tx
                .query_row(
                    "select status from harness_runs
                     where id = ?1 and execution_kind = 'codex_agent'",
                    params![run_id],
                    |row| row.get(0),
                )
                .optional()
                .map_err(|error| error.to_string())?;
            if let Some(status) = managed_status {
                append_structured_harness_event(
                    &tx,
                    run_id,
                    "agent_state_committed",
                    &format!("Committed Research State revision {revision}"),
                    Some(serde_json::json!({
                        "revision": revision,
                        "affectedEntryIds": receipt.affected_entry_ids,
                    })),
                    Some(if status == "reconciling" {
                        "synthesizing"
                    } else {
                        "assessing"
                    }),
                    None,
                    None,
                    "harness",
                )?;
            }
        }
        Ok(receipt)
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
                       e.created_at, e.author_kind, e.author_id, e.run_id,
                       t.title, t.anchor_kind, t.source_id, t.start_offset,
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
                    thread_title: row.get(11)?,
                    anchor: thread_anchor_from_row(row, 12)?,
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
              content_revision integer not null default 1,
              created_from_run_id text,
              created_from_state_revision integer,
              generation_id text,
              output_shape text,
              created_at text not null,
              updated_at text not null,
              foreign key (project_id) references projects(id) on delete cascade
            );

            create index if not exists idx_project_documents_project
              on project_documents(project_id, updated_at desc);

            create table if not exists research_document_generations (
              id text primary key,
              project_id text not null,
              status text not null,
              shape text not null,
              title text not null,
              custom_instruction text,
              state_revision integer not null,
              selected_entry_ids_json text not null,
              include_non_active integer not null,
              originating_run_id text,
              policy_version text not null,
              model_identifier text not null,
              resulting_document_id text,
              retry_of_id text,
              request_fingerprint text not null,
              error text,
              cancellation_requested integer not null default 0,
              input_entry_count integer not null default 0,
              citation_count integer not null default 0,
              created_at text not null,
              started_at text,
              finished_at text,
              foreign key (project_id) references projects(id) on delete cascade,
              foreign key (originating_run_id) references harness_runs(id) on delete set null,
              foreign key (resulting_document_id) references project_documents(id) on delete set null,
              foreign key (retry_of_id) references research_document_generations(id) on delete set null
            );

            create unique index if not exists idx_research_document_generation_active
              on research_document_generations(project_id, request_fingerprint)
              where status in ('queued', 'generating');

            create table if not exists project_document_citations (
              document_id text not null,
              citation_key text not null,
              paper_id text not null,
              evidence_link_ids_json text not null,
              title_snapshot text not null,
              authors_snapshot_json text not null,
              year_snapshot integer not null,
              created_at text not null,
              primary key (document_id, citation_key),
              foreign key (document_id) references project_documents(id) on delete cascade,
              foreign key (paper_id) references papers(id) on delete cascade
            );

            create table if not exists research_harnesses (
              project_id text primary key,
              status text not null,
              configuration_json text not null,
              configuration_version integer not null,
              schedule_enabled integer not null default 0,
              next_run_at text,
              last_scheduled_for text,
              requested_post_run_status text not null default 'idle',
              completed_cycle_count integer not null default 0,
              consecutive_unproductive_runs integer not null default 0,
              terminal_stop_reason text,
              updated_at text not null,
              foreign key (project_id) references projects(id) on delete cascade
            );

            create table if not exists harness_configuration_versions (
              project_id text not null,
              version integer not null,
              configuration_json text not null,
              actor text not null,
              source_improvement_id text,
              reason text not null,
              created_at text not null,
              primary key (project_id, version),
              foreign key (project_id) references projects(id) on delete cascade,
              foreign key (source_improvement_id) references harness_improvements(id) on delete set null
            );

            create table if not exists harness_runs (
              id text primary key,
              project_id text not null,
              status text not null,
              configuration_snapshot_json text not null,
              configuration_version integer not null,
              policy_version text not null,
              effective_instruction_stack_json text,
              search_id text not null,
              search_run_id text unique,
              stop_reason text,
              summary text,
              starting_state_revision integer not null default 0,
              resulting_state_revision integer,
              starting_vault_revision integer not null default 0,
              resulting_vault_revision integer,
              provider_query_count integer not null default 0,
              llm_call_count integer not null default 0,
              iteration_count integer not null default 0,
              inspected_candidate_count integer not null default 0,
              execution_kind text not null default 'legacy_search',
              runtime_model text,
              runtime_thread_id text,
              runtime_turn_id text,
              agent_limits_json text,
              trigger text not null default 'manual',
              scheduled_for text,
              started_at text not null,
              finished_at text,
              foreign key (project_id) references projects(id) on delete cascade,
              foreign key (search_id) references searches(id) on delete cascade,
              foreign key (search_run_id) references search_runs(id) on delete cascade
            );

            create unique index if not exists idx_harness_runs_one_active
              on harness_runs(project_id)
              where status in ('queued', 'planning', 'searching', 'assessing', 'ranking', 'reconciling', 'canceling');

            create table if not exists agent_reader_usage (
              run_id text not null,
              paper_id text not null,
              returned_text_chars integer not null default 0,
              read_count integer not null default 0,
              primary key (run_id, paper_id),
              foreign key (run_id) references harness_runs(id) on delete cascade,
              foreign key (paper_id) references papers(id) on delete cascade
            );

            create table if not exists research_synthesis_attempts (
              run_id text not null references harness_runs(id) on delete cascade,
              attempt integer not null,
              schema_version integer not null default 147,
              model_id text,
              thread_id text,
              turn_id text,
              proposal text not null,
              evidence_json text not null,
              vault_revision integer not null,
              issues text,
              committed integer not null default 0,
              created_at text not null default (datetime('now')),
              primary key (run_id, attempt)
            );
            create table if not exists agent_passage_anchors (
              run_id text not null,
              passage_ref text not null,
              anchor_json text not null,
              source_version text not null,
              primary key (run_id, passage_ref),
              foreign key (run_id, passage_ref) references agent_reader_passages(run_id, passage_ref) on delete cascade
            );

            create table if not exists agent_reader_passages (
              run_id text not null,
              passage_ref text not null,
              paper_id text not null,
              delivered_at text not null,
              primary key (run_id, passage_ref),
              foreign key (run_id) references harness_runs(id) on delete cascade,
              foreign key (paper_id) references papers(id) on delete cascade
            );

            create table if not exists agent_vault_additions (
              run_id text not null,
              paper_id text not null,
              membership_added integer not null,
              created_at text not null,
              primary key (run_id, paper_id),
              foreign key (run_id) references harness_runs(id) on delete cascade,
              foreign key (paper_id) references papers(id) on delete cascade
            );

            create table if not exists agent_paper_dispositions (
              run_id text not null,
              paper_id text not null,
              disposition text not null,
              reason text not null,
              retained integer not null,
              created_at text not null,
              primary key (run_id, paper_id),
              foreign key (run_id) references harness_runs(id) on delete cascade,
              foreign key (paper_id) references papers(id) on delete cascade
            );

            create table if not exists harness_change_sets (
              id text primary key,
              run_id text not null unique,
              project_id text not null,
              starting_state_revision integer not null,
              status text not null,
              plan_json text,
              considered_candidates_json text not null,
              error text,
              decision_reason text,
              resulting_state_revision integer,
              created_at text not null,
              decided_at text,
              foreign key (run_id) references harness_runs(id) on delete cascade,
              foreign key (project_id) references projects(id) on delete cascade
            );

            create index if not exists idx_harness_change_sets_project
              on harness_change_sets(project_id, created_at desc);

            create table if not exists harness_events (
              id text primary key,
              run_id text not null,
              sequence integer not null,
              kind text not null,
              summary text not null,
              detail_json text,
              phase text,
              progress_current integer,
              progress_total integer,
              actor text not null default 'system',
              occurred_at text not null,
              unique (run_id, sequence),
              foreign key (run_id) references harness_runs(id) on delete cascade
            );

            create index if not exists idx_harness_events_run
              on harness_events(run_id, sequence);

            create table if not exists harness_control_events (
              id text primary key,
              project_id text not null,
              sequence integer not null,
              kind text not null,
              summary text not null,
              occurred_at text not null,
              unique (project_id, sequence),
              foreign key (project_id) references projects(id) on delete cascade
            );

            create table if not exists harness_reflections (
              id text primary key,
              run_id text not null unique,
              project_id text not null,
              policy_version text not null,
              configuration_version integer not null,
              summary text not null,
              next_direction text,
              metrics_json text not null,
              created_at text not null,
              foreign key (run_id) references harness_runs(id) on delete cascade,
              foreign key (project_id) references projects(id) on delete cascade
            );

            create table if not exists harness_observations (
              id text primary key,
              reflection_id text not null,
              run_id text not null,
              project_id text not null,
              kind text not null,
              signature text not null,
              severity real not null,
              confidence real not null,
              description text not null,
              metrics_json text not null,
              target text,
              proposed_value_json text,
              proposal_eligible integer not null,
              created_at text not null,
              foreign key (reflection_id) references harness_reflections(id) on delete cascade,
              foreign key (run_id) references harness_runs(id) on delete cascade,
              foreign key (project_id) references projects(id) on delete cascade
            );

            create index if not exists idx_harness_observations_recurrence
              on harness_observations(project_id, signature, target, created_at desc);

            create table if not exists harness_improvements (
              id text primary key,
              project_id text not null,
              status text not null,
              target text not null,
              base_configuration_version integer not null,
              before_value_json text not null,
              proposed_value_json text not null,
              rationale text not null,
              expected_effect text not null,
              policy_version text not null,
              fingerprint text not null unique,
              decision_actor text,
              decision_reason text,
              resulting_configuration_version integer,
              created_at text not null,
              decided_at text,
              foreign key (project_id) references projects(id) on delete cascade
            );

            create table if not exists harness_improvement_observations (
              improvement_id text not null,
              observation_id text not null,
              primary key (improvement_id, observation_id),
              foreign key (improvement_id) references harness_improvements(id) on delete cascade,
              foreign key (observation_id) references harness_observations(id) on delete cascade
            );

            create table if not exists harness_improvement_runs (
              improvement_id text not null,
              run_id text not null,
              primary key (improvement_id, run_id),
              foreign key (improvement_id) references harness_improvements(id) on delete cascade,
              foreign key (run_id) references harness_runs(id) on delete cascade
            );

            create table if not exists research_state_heads (
              project_id text primary key,
              current_revision integer not null default 0,
              foreign key (project_id) references projects(id) on delete cascade
            );

            create table if not exists research_state_revisions (
              project_id text not null,
              revision integer not null,
              run_id text,
              reason text not null,
              created_at text not null,
              primary key (project_id, revision),
              foreign key (project_id) references projects(id) on delete cascade,
              foreign key (run_id) references harness_runs(id) on delete set null
            );

            create table if not exists research_entries (
              id text primary key,
              project_id text not null,
              kind text not null,
              epistemic_status text not null,
              text text not null,
              lifecycle text not null,
              first_revision integer not null,
              last_revision integer not null,
              origin_run_id text,
              created_at text not null,
              updated_at text not null,
              foreign key (project_id) references projects(id) on delete cascade,
              foreign key (origin_run_id) references harness_runs(id) on delete set null
            );

            create index if not exists idx_research_entries_project
              on research_entries(project_id, last_revision desc, id);

            create table if not exists research_entry_revisions (
              entry_id text not null,
              project_id text not null,
              state_revision integer not null,
              kind text not null,
              epistemic_status text not null,
              text text not null,
              lifecycle text not null,
              origin_run_id text,
              reason text not null,
              created_at text not null,
              primary key (entry_id, state_revision),
              foreign key (entry_id) references research_entries(id) on delete cascade,
              foreign key (project_id, state_revision)
                references research_state_revisions(project_id, revision) on delete cascade,
              foreign key (origin_run_id) references harness_runs(id) on delete set null
            );

            create index if not exists idx_research_entry_revisions_project
              on research_entry_revisions(project_id, state_revision, entry_id);

            create table if not exists research_evidence_links (
              id text primary key,
              entry_id text not null,
              state_revision integer not null,
              paper_id text not null,
              source_id text not null,
              extraction_id text not null,
              chunk_id text not null,
              excerpt text not null,
              source_start integer not null,
              source_end integer not null,
              page_start integer not null,
              page_end integer not null,
              support_note text,
              relationship text not null default 'unspecified',
              foreign key (entry_id, state_revision)
                references research_entry_revisions(entry_id, state_revision) on delete cascade,
              foreign key (paper_id) references papers(id) on delete cascade
            );

            create index if not exists idx_research_evidence_entry
              on research_evidence_links(entry_id, state_revision);

            create table if not exists research_entry_relations (
              id text primary key,
              entry_id text not null,
              state_revision integer not null,
              target_entry_id text not null,
              kind text not null,
              foreign key (entry_id, state_revision)
                references research_entry_revisions(entry_id, state_revision) on delete cascade,
              foreign key (target_entry_id) references research_entries(id) on delete cascade
            );

            create index if not exists idx_research_relations_entry
              on research_entry_relations(entry_id, state_revision);

            create table if not exists research_context_links (
              id text primary key,
              entry_id text not null,
              state_revision integer not null,
              kind text not null,
              context_id text not null,
              label text not null,
              foreign key (entry_id, state_revision)
                references research_entry_revisions(entry_id, state_revision) on delete cascade
            );

            create index if not exists idx_research_context_entry
              on research_context_links(entry_id, state_revision);

            create table if not exists vaults (
              id text primary key,
              project_id text not null unique,
              title text not null,
              path text not null unique,
              membership_revision integer not null default 0,
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
              author_kind text not null default 'user',
              author_id text,
              run_id text,
              created_at text not null,
              foreign key (thread_id) references chat_threads(id) on delete cascade
            );

            create index if not exists idx_chat_entries_thread
              on chat_entries(thread_id, created_at);

            create index if not exists idx_chat_entries_pinned
              on chat_entries(thread_id, pinned);

            create table if not exists mcp_mutation_receipts (
              caller text not null,
              tool text not null,
              request_id text not null,
              payload_hash text not null,
              result_json text not null,
              created_at text not null,
              primary key (caller, tool, request_id)
            );

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
              provider_query_count integer not null default 0,
              llm_call_count integer not null default 0,
              inspected_candidate_count integer not null default 0,
              started_at text,
              finished_at text,
              error text,
              created_at text not null,
              foreign key (search_id) references searches(id) on delete cascade
            );

            create index if not exists idx_search_runs_search_id
              on search_runs(search_id);

            create table if not exists agent_search_runs (
              run_id text primary key,
              search_id text not null,
              project_id text not null,
              vault_id text not null,
              caller text not null,
              parent_run_id text,
              model_id text,
              reserved_provider_queries integer not null default 0,
              reserved_llm_calls integer not null default 0,
              cancel_requested integer not null default 0,
              created_at text not null,
              foreign key (run_id) references search_runs(id) on delete cascade,
              foreign key (search_id) references searches(id) on delete cascade,
              foreign key (project_id) references projects(id) on delete cascade,
              foreign key (vault_id) references vaults(id) on delete cascade,
              foreign key (parent_run_id) references harness_runs(id) on delete cascade
            );

            create index if not exists idx_agent_search_runs_scope
              on agent_search_runs(project_id, caller, parent_run_id);

            create table if not exists agent_search_candidate_events (
              sequence integer primary key autoincrement,
              run_id text not null,
              phase text not null,
              candidate_json text not null,
              created_at text not null,
              foreign key (run_id) references agent_search_runs(run_id) on delete cascade
            );

            create index if not exists idx_agent_search_candidate_events_run
              on agent_search_candidate_events(run_id, sequence);

            create table if not exists agent_search_activity (
              sequence integer primary key autoincrement,
              run_id text not null,
              kind text not null,
              message text not null,
              error text,
              created_at text not null,
              foreign key (run_id) references agent_search_runs(run_id) on delete cascade
            );

            create index if not exists idx_agent_search_activity_run
              on agent_search_activity(run_id, sequence);

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
        add_column_if_missing(
            conn,
            "chat_entries",
            "author_kind",
            "text not null default 'user'",
        )?;
        add_column_if_missing(conn, "chat_entries", "author_id", "text")?;
        add_column_if_missing(conn, "chat_entries", "run_id", "text")?;
        add_column_if_missing(
            conn,
            "research_evidence_links",
            "relationship",
            "text not null default 'unspecified'",
        )?;
        add_column_if_missing(conn, "vaults", "project_id", "text")?;
        add_column_if_missing(
            conn,
            "vaults",
            "membership_revision",
            "integer not null default 0",
        )?;
        add_column_if_missing(
            conn,
            "project_documents",
            "content_revision",
            "integer not null default 1",
        )?;
        add_column_if_missing(conn, "papers", "active_extraction_id", "text")?;
        add_column_if_missing(conn, "document_sources", "landing_url", "text")?;
        add_column_if_missing(conn, "document_sources", "final_url", "text")?;
        add_column_if_missing(conn, "document_sources", "acquisition_method", "text")?;
        add_column_if_missing(conn, "search_runs", "mode", "text not null default 'deep'")?;
        add_column_if_missing(conn, "search_runs", "provider_set", "text")?;
        add_column_if_missing(conn, "search_runs", "query_expansions", "text")?;
        add_column_if_missing(
            conn,
            "harness_runs",
            "starting_state_revision",
            "integer not null default 0",
        )?;
        conn.execute(
            "update vaults set membership_revision = (
               select count(*) from vault_papers where vault_id = vaults.id
             ) where membership_revision = 0",
            [],
        )
        .map_err(|error| error.to_string())?;
        add_column_if_missing(
            conn,
            "harness_runs",
            "effective_instruction_stack_json",
            "text",
        )?;
        add_column_if_missing(conn, "harness_runs", "resulting_state_revision", "integer")?;
        add_column_if_missing(
            conn,
            "research_harnesses",
            "schedule_enabled",
            "integer not null default 0",
        )?;
        add_column_if_missing(conn, "research_harnesses", "next_run_at", "text")?;
        add_column_if_missing(conn, "research_harnesses", "last_scheduled_for", "text")?;
        add_column_if_missing(
            conn,
            "research_harnesses",
            "requested_post_run_status",
            "text not null default 'idle'",
        )?;
        add_column_if_missing(
            conn,
            "research_harnesses",
            "completed_cycle_count",
            "integer not null default 0",
        )?;
        add_column_if_missing(
            conn,
            "research_harnesses",
            "consecutive_unproductive_runs",
            "integer not null default 0",
        )?;
        add_column_if_missing(conn, "research_harnesses", "terminal_stop_reason", "text")?;
        add_column_if_missing(
            conn,
            "harness_runs",
            "trigger",
            "text not null default 'manual'",
        )?;
        add_column_if_missing(conn, "harness_runs", "scheduled_for", "text")?;
        add_column_if_missing(
            conn,
            "harness_runs",
            "starting_vault_revision",
            "integer not null default 0",
        )?;
        add_column_if_missing(conn, "harness_runs", "resulting_vault_revision", "integer")?;
        for column in [
            "provider_query_count",
            "llm_call_count",
            "iteration_count",
            "inspected_candidate_count",
        ] {
            add_column_if_missing(conn, "harness_runs", column, "integer not null default 0")?;
        }
        add_column_if_missing(
            conn,
            "harness_runs",
            "execution_kind",
            "text not null default 'legacy_search'",
        )?;
        add_column_if_missing(conn, "harness_runs", "runtime_model", "text")?;
        // Earlier installations created anchors before source versions existed.
        // Empty versions stay explicitly unknown; only a new read supplies one.
        add_column_if_missing(
            conn,
            "agent_passage_anchors",
            "source_version",
            "text not null default ''",
        )?;
        for column in ["model_id", "thread_id", "turn_id"] {
            add_column_if_missing(conn, "research_synthesis_attempts", column, "text")?;
        }
        add_column_if_missing(conn, "harness_runs", "runtime_thread_id", "text")?;
        add_column_if_missing(conn, "harness_runs", "runtime_turn_id", "text")?;
        add_column_if_missing(conn, "harness_runs", "agent_limits_json", "text")?;
        for column in [
            "provider_query_count",
            "llm_call_count",
            "inspected_candidate_count",
        ] {
            add_column_if_missing(conn, "search_runs", column, "integer not null default 0")?;
        }
        add_column_if_missing(
            conn,
            "agent_search_runs",
            "reserved_provider_queries",
            "integer not null default 0",
        )?;
        add_column_if_missing(
            conn,
            "agent_search_runs",
            "reserved_llm_calls",
            "integer not null default 0",
        )?;
        add_column_if_missing(conn, "harness_events", "detail_json", "text")?;
        add_column_if_missing(conn, "harness_events", "phase", "text")?;
        add_column_if_missing(conn, "harness_events", "progress_current", "integer")?;
        add_column_if_missing(conn, "harness_events", "progress_total", "integer")?;
        add_column_if_missing(
            conn,
            "harness_events",
            "actor",
            "text not null default 'system'",
        )?;
        add_column_if_missing(conn, "project_documents", "generation_id", "text")?;
        add_column_if_missing(conn, "project_documents", "output_shape", "text")?;
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
        )?;
        conn.execute_batch(
            "drop index if exists idx_harness_runs_one_active;
             create unique index idx_harness_runs_one_active on harness_runs(project_id)
               where status in ('queued', 'planning', 'searching', 'assessing', 'ranking', 'reconciling', 'canceling');
             create unique index if not exists idx_harness_runs_scheduled_occurrence
               on harness_runs(project_id, scheduled_for) where scheduled_for is not null;
             create index if not exists idx_research_harnesses_due
               on research_harnesses(schedule_enabled, next_run_at, project_id);
             create trigger if not exists trg_vault_papers_revision_insert
             after insert on vault_papers begin
               update vaults set membership_revision = membership_revision + 1,
                 updated_at = datetime('now') where id = new.vault_id;
             end;
             create trigger if not exists trg_vault_papers_revision_delete
             after delete on vault_papers begin
               update vaults set membership_revision = membership_revision + 1,
                 updated_at = datetime('now') where id = old.vault_id;
             end;
             create trigger if not exists trg_project_document_content_revision
             after update of content on project_documents
             when old.content <> new.content begin
               update project_documents set content_revision = old.content_revision + 1
                 where id = new.id;
             end;",
        )
        .map_err(|error| error.to_string())
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
            insert_default_harness(&tx, &project_id)?;
            insert_initial_research_state(&tx, &project_id)?;
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

fn read_project_document_summaries(conn: &Connection) -> StoreResult<Vec<ProjectDocumentSummary>> {
    let mut statement = conn
        .prepare(
            "select id, project_id, title, format, harness_writable, content_revision, updated_at
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
                content_revision: row.get(5)?,
                updated_at: row.get(6)?,
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
                created_from_run_id, created_from_state_revision, generation_id,
                output_shape, content_revision, created_at, updated_at
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
                generation_id: row.get(8)?,
                output_shape: row.get(9)?,
                content_revision: row.get(10)?,
                citations: Vec::new(),
                created_at: row.get(11)?,
                updated_at: row.get(12)?,
            })
        },
    )
    .optional()
    .map_err(|error| error.to_string())
    .and_then(|document| {
        document
            .map(|mut document| {
                document.citations = read_project_document_citations(conn, &document.id)?;
                Ok(document)
            })
            .transpose()
    })
}

fn read_project_document_citations(
    conn: &Connection,
    document_id: &str,
) -> StoreResult<Vec<ProjectDocumentCitation>> {
    let mut statement = conn
        .prepare(
            "select document_id, citation_key, paper_id, evidence_link_ids_json,
             title_snapshot, authors_snapshot_json, year_snapshot, created_at
             from project_document_citations where document_id = ?1 order by citation_key",
        )
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map(params![document_id], |row| {
            let evidence: String = row.get(3)?;
            let authors: String = row.get(5)?;
            Ok(ProjectDocumentCitation {
                document_id: row.get(0)?,
                citation_key: row.get(1)?,
                paper_id: row.get(2)?,
                evidence_link_ids: parse_json_column(3, &evidence)?,
                title_snapshot: row.get(4)?,
                authors_snapshot: parse_json_column(5, &authors)?,
                year_snapshot: row.get(6)?,
                created_at: row.get(7)?,
            })
        })
        .map_err(|error| error.to_string())?;
    collect_rows(rows)
}

fn read_research_document_generation(
    conn: &Connection,
    id: &str,
) -> StoreResult<ResearchDocumentGeneration> {
    conn.query_row(
        "select id, project_id, status, shape, title, custom_instruction,
         state_revision, selected_entry_ids_json, include_non_active,
         originating_run_id, policy_version, model_identifier,
         resulting_document_id, retry_of_id, error, cancellation_requested,
         input_entry_count, citation_count, created_at, started_at, finished_at
         from research_document_generations where id = ?1",
        params![id],
        |row| {
            let shape: String = row.get(3)?;
            let selected: String = row.get(7)?;
            Ok(ResearchDocumentGeneration {
                id: row.get(0)?,
                project_id: row.get(1)?,
                status: row.get(2)?,
                shape: parse_sql_enum(3, &shape, ResearchDocumentShape::parse)?,
                title: row.get(4)?,
                custom_instruction: row.get(5)?,
                state_revision: row.get(6)?,
                selected_entry_ids: parse_json_column(7, &selected)?,
                include_non_active: row.get(8)?,
                originating_run_id: row.get(9)?,
                policy_version: row.get(10)?,
                model_identifier: row.get(11)?,
                resulting_document_id: row.get(12)?,
                retry_of_id: row.get(13)?,
                error: row.get(14)?,
                cancellation_requested: row.get(15)?,
                input_entry_count: row.get(16)?,
                citation_count: row.get(17)?,
                created_at: row.get(18)?,
                started_at: row.get(19)?,
                finished_at: row.get(20)?,
            })
        },
    )
    .map_err(|error| error.to_string())
}

fn validate_generation_request(
    store: &LibraryStore,
    request: &CreateFromResearchRequest,
) -> StoreResult<()> {
    if request.title.trim().is_empty() {
        return Err("Generated document title cannot be empty".to_string());
    }
    if request.selected_entry_ids.is_empty() {
        return Err("Select at least one Research Entry".to_string());
    }
    if request.selected_entry_ids.len() > 100 {
        return Err("A generation may include at most 100 Research Entries".to_string());
    }
    if request
        .custom_instruction
        .as_deref()
        .is_some_and(|value| value.chars().count() > 1_000)
    {
        return Err("Custom document direction exceeds 1000 characters".to_string());
    }
    let state = store.get_research_state(&request.project_id, Some(request.state_revision))?;
    for entry_id in &request.selected_entry_ids {
        if !state.entries.iter().any(|entry| entry.id == *entry_id) {
            return Err(format!(
                "Research Entry is not visible in pinned revision {}: {entry_id}",
                request.state_revision
            ));
        }
    }
    if let Some(run_id) = request.originating_run_id.as_deref() {
        let conn = store.open_connection()?;
        let valid: bool = conn
            .query_row(
                "select exists(select 1 from harness_runs where id = ?1 and project_id = ?2
                 and status = 'ready')",
                params![run_id, request.project_id],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        if !valid {
            return Err("Originating Run must be completed inside this Project".to_string());
        }
    }
    Ok(())
}

fn shape_outline(shape: ResearchDocumentShape) -> &'static [&'static str] {
    match shape {
        ResearchDocumentShape::Survey => &[
            "Scope and bounded corpus",
            "Established findings",
            "Themes and disagreements",
            "Open questions and qualified gaps",
            "Hypotheses and future work",
        ],
        ResearchDocumentShape::RelatedWork => &[
            "Thematic synthesis",
            "Method and evidence contrasts",
            "Limitations of the bounded set",
        ],
        ResearchDocumentShape::ResearchGapAnalysis => &[
            "Coverage statement",
            "Supported background",
            "Qualified gaps",
            "Competing explanations and unanswered questions",
            "Candidate next searches",
        ],
        ResearchDocumentShape::HypothesisReport => &[
            "Motivating findings",
            "Speculative hypotheses",
            "Supporting and opposing premises",
            "Falsifiable predictions",
        ],
        ResearchDocumentShape::ExperimentPlan => &[
            "Research question and hypothesis",
            "Proposed intervention or measurement",
            "Variables, controls, and expected observations",
            "Risks and alternative explanations",
            "Evidence motivating the design",
        ],
        ResearchDocumentShape::Custom => &["Purpose", "Research synthesis"],
    }
}

fn render_research_document(
    conn: &Connection,
    generation: &ResearchDocumentGeneration,
    entries: &[ResearchEntryDetail],
) -> StoreResult<(String, Vec<ProjectDocumentCitation>)> {
    let mut paper_evidence: HashMap<String, Vec<String>> = HashMap::new();
    for entry in entries {
        for evidence in &entry.evidence {
            paper_evidence
                .entry(evidence.paper_id.clone())
                .or_default()
                .push(evidence.id.clone());
        }
    }
    let mut citations = Vec::new();
    let mut keys: HashMap<String, String> = HashMap::new();
    let mut used_keys = std::collections::HashSet::new();
    let mut paper_ids = paper_evidence.keys().cloned().collect::<Vec<_>>();
    paper_ids.sort();
    for paper_id in paper_ids {
        let paper = read_paper(conn, &paper_id)?
            .ok_or_else(|| format!("Citation Paper no longer resolves: {paper_id}"))?;
        let base = citation_key_base(&paper);
        let mut key = base.clone();
        let mut suffix = 2;
        while !used_keys.insert(key.clone()) {
            key = format!("{base}{suffix}");
            suffix += 1;
        }
        keys.insert(paper_id.clone(), key.clone());
        let mut evidence_link_ids = paper_evidence.remove(&paper_id).unwrap_or_default();
        evidence_link_ids.sort();
        evidence_link_ids.dedup();
        citations.push(ProjectDocumentCitation {
            document_id: String::new(),
            citation_key: key,
            paper_id,
            evidence_link_ids,
            title_snapshot: paper.title,
            authors_snapshot: paper.authors,
            year_snapshot: paper.year,
            created_at: String::new(),
        });
    }

    let mut content = format!(
        "# {}\n\n> Generated from Research State revision {} using {} explicitly selected entries. Generated prose is an authored Project document, not Research State or source evidence.\n\n",
        generation.title,
        generation.state_revision,
        entries.len()
    );
    if let Some(direction) = generation.custom_instruction.as_deref() {
        content.push_str(&format!(
            "**Researcher direction:** {}\n\n",
            direction.trim()
        ));
    }
    let mut rendered_entry_ids = std::collections::HashSet::new();
    for heading in shape_outline(generation.shape) {
        content.push_str(&format!("## {heading}\n\n"));
        let matching = entries
            .iter()
            .filter(|detail| entry_matches_heading(detail, heading));
        let mut count = 0;
        for detail in matching {
            count += 1;
            rendered_entry_ids.insert(detail.entry.id.as_str());
            content.push_str(&render_research_entry_bullet(detail, entries, &keys));
        }
        if count == 0 {
            content.push_str("- No selected entries belong to this section.\n");
        }
        content.push('\n');
    }
    let additional = entries
        .iter()
        .filter(|entry| !rendered_entry_ids.contains(entry.entry.id.as_str()))
        .collect::<Vec<_>>();
    if !additional.is_empty() {
        content.push_str("## Additional selected material\n\n");
        for detail in additional {
            content.push_str(&render_research_entry_bullet(detail, entries, &keys));
        }
        content.push('\n');
    }
    if entries.iter().any(|entry| !entry.context.is_empty()) {
        content.push_str("## Researcher working context (not evidence)\n\n");
        for entry in entries.iter().filter(|entry| !entry.context.is_empty()) {
            content.push_str(&format!("- {}\n", entry.entry.text));
        }
        content.push('\n');
    }
    content.push_str("## References\n\n");
    for citation in &citations {
        content.push_str(&format!(
            "- [@{}] {} ({}). *{}*.\n",
            citation.citation_key,
            citation.authors_snapshot.join(", "),
            citation.year_snapshot,
            citation.title_snapshot
        ));
    }
    Ok((content, citations))
}

fn render_research_entry_bullet(
    detail: &ResearchEntryDetail,
    entries: &[ResearchEntryDetail],
    citation_keys: &HashMap<String, String>,
) -> String {
    let citation_evidence = if detail.entry.epistemic_status == EpistemicStatus::SourceSupported {
        detail.evidence.iter().collect::<Vec<_>>()
    } else if detail.entry.epistemic_status == EpistemicStatus::AgentSynthesis {
        detail
            .relations
            .iter()
            .filter_map(|relation| {
                entries
                    .iter()
                    .find(|candidate| candidate.entry.id == relation.target_entry_id)
            })
            .flat_map(|premise| premise.evidence.iter())
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    let handles = citation_evidence
        .into_iter()
        .filter_map(|evidence| citation_keys.get(&evidence.paper_id))
        .map(|key| format!("[@{key}]"))
        .collect::<Vec<_>>();
    let citation_text = if handles.is_empty() {
        String::new()
    } else {
        format!(" {}", handles.join(" "))
    };
    format!(
        "- **{}:** {}{}\n",
        entry_qualifier(detail),
        detail.entry.text,
        citation_text
    )
}

fn entry_matches_heading(entry: &ResearchEntryDetail, heading: &str) -> bool {
    let heading = heading.to_lowercase();
    match entry.entry.kind {
        ResearchEntryKind::Finding => {
            heading.contains("finding")
                || heading.contains("background")
                || heading.contains("synthesis")
                || heading.contains("evidence")
                || heading.contains("theme")
        }
        ResearchEntryKind::Question => {
            heading.contains("question")
                || heading.contains("research question")
                || heading.contains("search")
        }
        ResearchEntryKind::Gap => {
            heading.contains("gap") || heading.contains("limitation") || heading.contains("search")
        }
        ResearchEntryKind::Hypothesis => {
            heading.contains("hypothes")
                || heading.contains("prediction")
                || heading.contains("explanation")
        }
        ResearchEntryKind::ExperimentIdea => {
            heading.contains("experiment")
                || heading.contains("intervention")
                || heading.contains("variable")
                || heading.contains("risk")
                || heading.contains("future work")
        }
    }
}

fn entry_qualifier(entry: &ResearchEntryDetail) -> String {
    let lifecycle = if entry.entry.lifecycle == EntryLifecycle::Active {
        String::new()
    } else {
        format!("{} ", entry.entry.lifecycle.as_str())
    };
    let epistemic = match entry.entry.epistemic_status {
        EpistemicStatus::SourceSupported => "source-supported finding",
        EpistemicStatus::AgentSynthesis => "analysis",
        EpistemicStatus::ResearcherContext => "researcher context",
        EpistemicStatus::Speculative => match entry.entry.kind {
            ResearchEntryKind::Gap => "qualified gap in this bounded review",
            ResearchEntryKind::Hypothesis => "speculative hypothesis",
            ResearchEntryKind::ExperimentIdea => "proposed experiment",
            _ => "speculative entry",
        },
    };
    format!("{lifecycle}{epistemic}")
}

fn citation_key_base(paper: &Paper) -> String {
    let author = paper
        .authors
        .first()
        .and_then(|value| value.split_whitespace().last())
        .unwrap_or("paper");
    let raw = format!("{author}{}", paper.year).to_lowercase();
    let key: String = raw
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .collect();
    if key.is_empty() {
        "paper".to_string()
    } else {
        key
    }
}

fn validate_generated_document(
    content: &str,
    entries: &[ResearchEntryDetail],
    citations: &[ProjectDocumentCitation],
) -> StoreResult<()> {
    if content.chars().count() > 100_000 {
        return Err("Generated document exceeds the 100000-character budget".to_string());
    }
    let allowed = citations
        .iter()
        .map(|citation| citation.citation_key.as_str())
        .collect::<std::collections::HashSet<_>>();
    let citation_pattern =
        regex::Regex::new(r"\[@([A-Za-z0-9_-]+)\]").map_err(|error| error.to_string())?;
    for captures in citation_pattern.captures_iter(content) {
        if !allowed.contains(&captures[1]) {
            return Err(format!(
                "Generated document used unknown citation handle: {}",
                &captures[1]
            ));
        }
    }
    for entry in entries.iter().filter(|entry| {
        entry.entry.epistemic_status == EpistemicStatus::SourceSupported
            && entry.entry.kind == ResearchEntryKind::Finding
    }) {
        if entry.evidence.is_empty() {
            return Err(format!(
                "Source-supported Finding lacks evidence: {}",
                entry.entry.id
            ));
        }
        let represented = entry.evidence.iter().any(|evidence| {
            citations.iter().any(|citation| {
                citation.paper_id == evidence.paper_id
                    && citation.evidence_link_ids.contains(&evidence.id)
            })
        });
        if !represented {
            return Err(format!(
                "Source-supported Finding lacks a resolvable citation: {}",
                entry.entry.id
            ));
        }
    }
    for entry in entries {
        let qualifier = entry_qualifier(entry);
        if !content.contains(&format!("**{qualifier}:**")) {
            return Err(format!(
                "Generated document omitted epistemic qualification for entry: {}",
                entry.entry.id
            ));
        }
    }
    Ok(())
}

fn read_research_harness(
    conn: &Connection,
    project_id: &str,
) -> StoreResult<Option<ResearchHarness>> {
    conn.query_row(
        "select project_id, status, configuration_json, configuration_version,
                next_run_at, last_scheduled_for, requested_post_run_status,
                completed_cycle_count, consecutive_unproductive_runs,
                terminal_stop_reason, updated_at
         from research_harnesses where project_id = ?1",
        params![project_id],
        |row| {
            let configuration_json: String = row.get(2)?;
            let configuration = serde_json::from_str(&configuration_json).map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(
                    configuration_json.len(),
                    rusqlite::types::Type::Text,
                    Box::new(error),
                )
            })?;
            Ok(ResearchHarness {
                project_id: row.get(0)?,
                status: row.get(1)?,
                configuration,
                configuration_version: row.get(3)?,
                next_run_at: row.get(4)?,
                last_scheduled_for: row.get(5)?,
                requested_post_run_status: row.get(6)?,
                completed_cycle_count: row.get(7)?,
                consecutive_unproductive_runs: row.get(8)?,
                terminal_stop_reason: row.get(9)?,
                updated_at: row.get(10)?,
            })
        },
    )
    .optional()
    .map_err(|error| error.to_string())
}

fn read_harness_run(conn: &Connection, run_id: &str) -> StoreResult<HarnessRun> {
    conn.query_row(
        "select id, project_id, status, configuration_snapshot_json,
                configuration_version, policy_version, effective_instruction_stack_json,
                search_id, search_run_id,
                stop_reason, summary, starting_state_revision,
                resulting_state_revision, starting_vault_revision, resulting_vault_revision,
                provider_query_count, llm_call_count, iteration_count, inspected_candidate_count,
                execution_kind, runtime_model, runtime_thread_id, runtime_turn_id,
                agent_limits_json, trigger, scheduled_for, started_at, finished_at
         from harness_runs where id = ?1",
        params![run_id],
        harness_run_from_row,
    )
    .map_err(|error| error.to_string())
}

/// Count attempted papers separately from papers with durably delivered text.
fn read_agent_delivery_counts(conn: &Connection, run_id: &str) -> StoreResult<(i64, i64)> {
    conn.query_row(
        "select count(*), coalesce(sum(exists(
            select 1 from agent_reader_passages p join agent_passage_anchors a
              on a.run_id=p.run_id and a.passage_ref=p.passage_ref
            where p.run_id=u.run_id and p.paper_id=u.paper_id
              and a.source_version <> '' and length(json_extract(a.anchor_json, '$.quote')) > 0
         )), 0) from agent_reader_usage u where run_id = ?1",
        [run_id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )
    .map_err(|error| error.to_string())
}

fn read_research_checkpoint(conn: &Connection, run_id: &str) -> StoreResult<ResearchCheckpoint> {
    let run = read_harness_run(conn, run_id)?;
    let change_set = conn
        .query_row(
            &format!(
                "select {HARNESS_CHANGE_SET_COLUMNS} from harness_change_sets where run_id = ?1"
            ),
            params![run_id],
            harness_change_set_from_row,
        )
        .optional()
        .map_err(|error| error.to_string())?;
    let plan = change_set
        .as_ref()
        .and_then(|change_set| change_set.plan.as_ref());
    let legacy_accepted_candidate_count = plan
        .map(|plan| {
            plan.candidate_decisions
                .iter()
                .filter(|decision| decision.decision == CandidateDecisionKind::Accept)
                .count() as i64
        })
        .unwrap_or(0);
    let rejected_candidate_count = plan
        .map(|plan| {
            plan.candidate_decisions
                .iter()
                .filter(|decision| decision.decision == CandidateDecisionKind::Reject)
                .count() as i64
        })
        .unwrap_or(0);
    let mut added_paper_ids = match (&change_set, plan) {
        (Some(change_set), Some(plan)) if change_set.status == HarnessChangeSetStatus::Applied => {
            plan.candidate_decisions
                .iter()
                .filter(|decision| decision.decision == CandidateDecisionKind::Accept)
                .filter_map(|decision| {
                    change_set
                        .considered_candidates
                        .iter()
                        .find(|candidate| candidate.id == decision.candidate_id)
                })
                .map(|candidate| candidate.candidate.id.clone())
                .filter(|paper_id| {
                    !run.effective_instructions
                        .run_context
                        .vault_paper_ids
                        .contains(paper_id)
                })
                .collect()
        }
        _ => Vec::new(),
    };
    let mut managed_added_paper_ids = conn
        .prepare(
            "select addition.paper_id from agent_vault_additions addition
             left join agent_paper_dispositions disposition
               on disposition.run_id = addition.run_id and disposition.paper_id = addition.paper_id
             where addition.run_id = ?1 and addition.membership_added = 1
               and (disposition.run_id is null or disposition.retained = 1)
             order by addition.created_at, addition.paper_id",
        )
        .map_err(|error| error.to_string())?
        .query_map(params![run_id], |row| row.get::<_, String>(0))
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    added_paper_ids.append(&mut managed_added_paper_ids);
    added_paper_ids.sort();
    added_paper_ids.dedup();
    let accepted_candidate_count = if run.execution_kind == "codex_agent" {
        added_paper_ids.len() as i64
    } else {
        legacy_accepted_candidate_count
    };
    let (attempted_paper_count, read_paper_count) = read_agent_delivery_counts(conn, run_id)?;
    let (unavailable_paper_count, removed_paper_count, mut retained_paper_count): (i64, i64, i64) =
        conn.query_row(
            "select coalesce(sum(disposition = 'unavailable'), 0),
                    coalesce(sum(retained = 0), 0), coalesce(sum(retained = 1), 0)
             from agent_paper_dispositions where run_id = ?1",
            params![run_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .map_err(|error| error.to_string())?;
    // Runs created before paper dispositions existed retained every recorded
    // addition. Preserve that meaning when presenting their checkpoints.
    if run.execution_kind == "codex_agent"
        && unavailable_paper_count == 0
        && removed_paper_count == 0
        && retained_paper_count == 0
    {
        retained_paper_count = added_paper_ids.len() as i64;
    }
    let reflection = conn
        .query_row(
            "select id, next_direction, metrics_json from harness_reflections where run_id = ?1",
            params![run_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        )
        .optional()
        .map_err(|error| error.to_string())?;
    let terminal = matches!(run.status.as_str(), "ready" | "failed" | "cancelled");
    let complete = terminal
        && matches!(
            run.stop_reason.as_deref(),
            Some("target_reached" | "coverage_sufficient" | "converged")
        );
    let reflection_id = reflection.as_ref().map(|(id, _, _)| id.clone());
    let next_direction = reflection
        .as_ref()
        .and_then(|(_, next_direction, _)| next_direction.clone())
        .or_else(|| plan.map(|plan| plan.next_direction.clone()));
    let outcome = reflection
        .as_ref()
        .and_then(|(_, _, metrics_json)| {
            serde_json::from_str::<serde_json::Value>(metrics_json).ok()
        })
        .and_then(|metrics| metrics.get("agentOutcome").cloned())
        .and_then(|value| serde_json::from_value(value).ok());
    Ok(ResearchCheckpoint {
        run_id: run.id,
        project_id: run.project_id,
        status: run.status,
        starting_state_revision: run.starting_state_revision,
        resulting_state_revision: run.resulting_state_revision,
        starting_vault_revision: run.starting_vault_revision,
        resulting_vault_revision: run.resulting_vault_revision,
        applied_change_set_id: change_set.as_ref().and_then(|change_set| {
            (change_set.status == HarnessChangeSetStatus::Applied).then(|| change_set.id.clone())
        }),
        accepted_candidate_count,
        rejected_candidate_count,
        added_paper_ids,
        attempted_paper_count,
        read_paper_count,
        unavailable_paper_count,
        removed_paper_count,
        retained_paper_count,
        affected_documents: Vec::new(),
        usage: HarnessUsage {
            provider_queries: run.provider_query_count,
            llm_calls: run.llm_call_count,
            iterations: run.iteration_count,
            inspected_candidates: run.inspected_candidate_count,
        },
        stop_reason: run.stop_reason.clone(),
        complete,
        converged: run.stop_reason.as_deref() == Some("converged"),
        reflection_id,
        next_direction,
        outcome,
        started_at: run.started_at,
        finished_at: run.finished_at,
        restore_available: terminal && run.resulting_state_revision.is_some(),
    })
}

const HARNESS_CHANGE_SET_COLUMNS: &str =
    "id, run_id, project_id, starting_state_revision, status, plan_json,
     considered_candidates_json, error, decision_reason, resulting_state_revision,
     created_at, decided_at";

fn read_harness_change_set_by_run(
    conn: &Connection,
    run_id: &str,
) -> StoreResult<HarnessChangeSet> {
    conn.query_row(
        &format!("select {HARNESS_CHANGE_SET_COLUMNS} from harness_change_sets where run_id = ?1"),
        params![run_id],
        harness_change_set_from_row,
    )
    .map_err(|error| error.to_string())
}

fn read_harness_change_set(conn: &Connection, id: &str) -> StoreResult<HarnessChangeSet> {
    conn.query_row(
        &format!("select {HARNESS_CHANGE_SET_COLUMNS} from harness_change_sets where id = ?1"),
        params![id],
        harness_change_set_from_row,
    )
    .map_err(|error| error.to_string())
}

fn harness_change_set_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<HarnessChangeSet> {
    let status: String = row.get(4)?;
    let plan_json: Option<String> = row.get(5)?;
    let candidates_json: String = row.get(6)?;
    let plan = plan_json
        .map(|json| deserialize_sql_json(5, &json))
        .transpose()?;
    let considered_candidates = deserialize_sql_json(6, &candidates_json)?;
    Ok(HarnessChangeSet {
        id: row.get(0)?,
        run_id: row.get(1)?,
        project_id: row.get(2)?,
        starting_state_revision: row.get(3)?,
        status: parse_sql_enum(4, &status, HarnessChangeSetStatus::parse)?,
        plan,
        considered_candidates,
        error: row.get(7)?,
        decision_reason: row.get(8)?,
        resulting_state_revision: row.get(9)?,
        created_at: row.get(10)?,
        decided_at: row.get(11)?,
    })
}

fn deserialize_sql_json<T: serde::de::DeserializeOwned>(
    column: usize,
    json: &str,
) -> rusqlite::Result<T> {
    serde_json::from_str(json).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(
            column,
            rusqlite::types::Type::Text,
            Box::new(error),
        )
    })
}

fn insert_harness_configuration_version(
    conn: &Connection,
    project_id: &str,
    version: i64,
    configuration: &HarnessConfiguration,
    actor: &str,
    source_improvement_id: Option<&str>,
    reason: &str,
) -> StoreResult<()> {
    let json = serde_json::to_string(configuration).map_err(|error| error.to_string())?;
    conn.execute(
        "insert into harness_configuration_versions
         (project_id, version, configuration_json, actor, source_improvement_id, reason, created_at)
         values (?1, ?2, ?3, ?4, ?5, ?6, datetime('now'))",
        params![
            project_id,
            version,
            json,
            actor,
            source_improvement_id,
            reason
        ],
    )
    .map_err(|error| error.to_string())?;
    Ok(())
}

fn read_harness_configuration_versions(
    conn: &Connection,
    project_id: &str,
) -> StoreResult<Vec<HarnessConfigurationVersion>> {
    let mut statement = conn
        .prepare(
            "select project_id, version, configuration_json, actor,
                    source_improvement_id, reason, created_at
             from harness_configuration_versions where project_id = ?1
             order by version desc",
        )
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map(params![project_id], |row| {
            let json: String = row.get(2)?;
            let configuration = serde_json::from_str(&json).map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(
                    json.len(),
                    rusqlite::types::Type::Text,
                    Box::new(error),
                )
            })?;
            Ok(HarnessConfigurationVersion {
                project_id: row.get(0)?,
                version: row.get(1)?,
                configuration,
                actor: row.get(3)?,
                source_improvement_id: row.get(4)?,
                reason: row.get(5)?,
                created_at: row.get(6)?,
            })
        })
        .map_err(|error| error.to_string())?;
    collect_rows(rows)
}

fn build_effective_instruction_stack(
    conn: &Connection,
    project_id: &str,
    starting_state_revision: i64,
    configuration: &HarnessConfiguration,
) -> StoreResult<EffectiveInstructionStack> {
    let mut entries_statement = conn
        .prepare(
            "select id, kind, epistemic_status, text, lifecycle
             from research_entries where project_id = ?1 and lifecycle = 'active'
             order by last_revision desc, id limit 100",
        )
        .map_err(|error| error.to_string())?;
    let entry_rows = entries_statement
        .query_map(params![project_id], |row| {
            Ok(RunContextEntry {
                id: row.get(0)?,
                kind: row.get(1)?,
                epistemic_status: row.get(2)?,
                text: row.get(3)?,
                lifecycle: row.get(4)?,
            })
        })
        .map_err(|error| error.to_string())?;
    let active_entries = collect_rows(entry_rows)?;

    let (vault_id, vault_paper_count, vault_membership_rowid): (String, i64, i64) = conn
        .query_row(
            "select v.id, count(vp.paper_id), coalesce(max(vp.rowid), 0)
             from vaults v left join vault_papers vp on vp.vault_id = v.id
             where v.project_id = ?1 group by v.id",
            params![project_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .map_err(|error| error.to_string())?;
    let vault_revision = format!("{vault_paper_count}:{vault_membership_rowid}");
    let mut papers_statement = conn
        .prepare(
            "select vp.paper_id from vault_papers vp
             join vaults v on v.id = vp.vault_id
             where v.project_id = ?1 order by vp.paper_id limit 500",
        )
        .map_err(|error| error.to_string())?;
    let paper_rows = papers_statement
        .query_map(params![project_id], |row| row.get(0))
        .map_err(|error| error.to_string())?;
    let vault_paper_ids = collect_rows(paper_rows)?;
    let prior_next_direction = conn
        .query_row(
            "select reflection.next_direction
             from harness_reflections reflection
             join harness_runs run on run.id = reflection.run_id
             where run.project_id = ?1 and reflection.next_direction is not null
             order by reflection.created_at desc limit 1",
            params![project_id],
            |row| row.get::<_, Option<String>>(0),
        )
        .optional()
        .map_err(|error| error.to_string())?
        .flatten();
    let mut observations_statement = conn
        .prepare(
            "select observation.kind, observation.description
             from harness_observations observation
             join harness_runs run on run.id = observation.run_id
             where observation.project_id = ?1 and run.status = 'ready'
             order by observation.created_at desc, observation.id desc limit 20",
        )
        .map_err(|error| error.to_string())?;
    let observation_rows = observations_statement
        .query_map(params![project_id], |row| {
            Ok(RunContextObservation {
                kind: row.get(0)?,
                description: row.get(1)?,
            })
        })
        .map_err(|error| error.to_string())?;
    let prior_observations = collect_rows(observation_rows)?;
    let strategy = configuration.depth.budget();
    let maximum_provider_queries = configuration
        .stop_conditions
        .maximum_provider_queries
        .unwrap_or(strategy.max_provider_queries)
        .min(strategy.max_provider_queries);
    let maximum_llm_calls = configuration
        .stop_conditions
        .maximum_llm_calls
        .unwrap_or(strategy.max_llm_calls)
        .min(strategy.max_llm_calls);
    Ok(EffectiveInstructionStack {
        product_policy_version: HARNESS_POLICY_VERSION.to_string(),
        product_policy_summary: HARNESS_POLICY_SUMMARY.to_string(),
        project_research_instructions: configuration.research_instructions.clone(),
        structured_settings: configuration.clone(),
        run_context: EffectiveRunContext {
            starting_state_revision,
            active_entries,
            vault_id,
            vault_revision,
            vault_paper_ids,
            prior_next_direction,
            prior_observations,
            maximum_provider_queries,
            maximum_llm_calls,
            paper_budget: configuration.paper_budget,
        },
    })
}

fn legacy_effective_instruction_stack(
    configuration: &HarnessConfiguration,
    starting_state_revision: i64,
) -> EffectiveInstructionStack {
    let strategy = configuration.depth.budget();
    EffectiveInstructionStack {
        product_policy_version: HARNESS_POLICY_VERSION.to_string(),
        product_policy_summary: HARNESS_POLICY_SUMMARY.to_string(),
        project_research_instructions: configuration.research_instructions.clone(),
        structured_settings: configuration.clone(),
        run_context: EffectiveRunContext {
            starting_state_revision,
            active_entries: Vec::new(),
            vault_id: String::new(),
            vault_revision: "legacy".to_string(),
            vault_paper_ids: Vec::new(),
            prior_next_direction: None,
            prior_observations: Vec::new(),
            maximum_provider_queries: configuration
                .stop_conditions
                .maximum_provider_queries
                .unwrap_or(strategy.max_provider_queries)
                .min(strategy.max_provider_queries),
            maximum_llm_calls: configuration
                .stop_conditions
                .maximum_llm_calls
                .unwrap_or(strategy.max_llm_calls)
                .min(strategy.max_llm_calls),
            paper_budget: configuration.paper_budget,
        },
    }
}

fn read_harness_runs(conn: &Connection, project_id: &str) -> StoreResult<Vec<HarnessRun>> {
    let mut statement = conn
        .prepare(
            "select id, project_id, status, configuration_snapshot_json,
                    configuration_version, policy_version, effective_instruction_stack_json,
                    search_id, search_run_id,
                    stop_reason, summary, starting_state_revision,
                    resulting_state_revision, starting_vault_revision, resulting_vault_revision,
                    provider_query_count, llm_call_count, iteration_count, inspected_candidate_count,
                    execution_kind, runtime_model, runtime_thread_id, runtime_turn_id,
                    agent_limits_json, trigger, scheduled_for, started_at, finished_at
             from harness_runs where project_id = ?1 order by started_at desc, id desc",
        )
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map(params![project_id], harness_run_from_row)
        .map_err(|error| error.to_string())?;
    collect_rows(rows)
}

fn harness_run_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<HarnessRun> {
    let snapshot_json: String = row.get(3)?;
    let configuration_snapshot = serde_json::from_str(&snapshot_json).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(
            snapshot_json.len(),
            rusqlite::types::Type::Text,
            Box::new(error),
        )
    })?;
    let starting_state_revision: i64 = row.get(11)?;
    let effective_json: Option<String> = row.get(6)?;
    let effective_instructions = effective_json
        .map(|json| {
            serde_json::from_str(&json).map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(
                    json.len(),
                    rusqlite::types::Type::Text,
                    Box::new(error),
                )
            })
        })
        .transpose()?
        .unwrap_or_else(|| {
            legacy_effective_instruction_stack(&configuration_snapshot, starting_state_revision)
        });
    let limits_json: Option<String> = row.get(23)?;
    let agent_limits = limits_json
        .map(|json| deserialize_sql_json(23, &json))
        .transpose()?;
    let trigger: String = row.get(24)?;
    Ok(HarnessRun {
        id: row.get(0)?,
        project_id: row.get(1)?,
        status: row.get(2)?,
        configuration_snapshot,
        configuration_version: row.get(4)?,
        policy_version: row.get(5)?,
        effective_instructions,
        search_id: row.get(7)?,
        search_run_id: row.get(8)?,
        stop_reason: row.get(9)?,
        summary: row.get(10)?,
        starting_state_revision,
        resulting_state_revision: row.get(12)?,
        starting_vault_revision: row.get(13)?,
        resulting_vault_revision: row.get(14)?,
        provider_query_count: row.get(15)?,
        llm_call_count: row.get(16)?,
        iteration_count: row.get(17)?,
        inspected_candidate_count: row.get(18)?,
        execution_kind: row.get(19)?,
        runtime_model: row.get(20)?,
        runtime_thread_id: row.get(21)?,
        runtime_turn_id: row.get(22)?,
        agent_limits,
        trigger: parse_sql_enum(24, &trigger, HarnessRunTrigger::parse)?,
        scheduled_for: row.get(25)?,
        started_at: row.get(26)?,
        finished_at: row.get(27)?,
    })
}

fn read_harness_events_for_project(
    conn: &Connection,
    project_id: &str,
) -> StoreResult<Vec<HarnessEvent>> {
    let mut statement = conn
        .prepare(
            "select id, run_id, sequence, kind, summary, detail_json, phase,
                    progress_current, progress_total, actor, occurred_at from (
               select e.id, e.run_id, e.sequence, e.kind, e.summary, e.detail_json, e.phase,
                      e.progress_current, e.progress_total, e.actor, e.occurred_at
               from harness_events e join harness_runs r on r.id = e.run_id
               where r.project_id = ?1
               union all
               select e.id, '', e.sequence, e.kind, e.summary, null, null, null, null,
                      case when e.kind = 'scheduled_run_claimed' then 'scheduler'
                           when e.kind like '%failed' then 'system' else 'researcher' end,
                      e.occurred_at
               from harness_control_events e where e.project_id = ?1
             ) order by occurred_at desc, id desc",
        )
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map(params![project_id], |row| {
            Ok(HarnessEvent {
                id: row.get(0)?,
                run_id: row.get(1)?,
                sequence: row.get(2)?,
                kind: row.get(3)?,
                summary: row.get(4)?,
                detail: row
                    .get::<_, Option<String>>(5)?
                    .map(|json| deserialize_sql_json(5, &json))
                    .transpose()?,
                phase: row.get(6)?,
                progress_current: row.get(7)?,
                progress_total: row.get(8)?,
                actor: row.get(9)?,
                occurred_at: row.get(10)?,
            })
        })
        .map_err(|error| error.to_string())?;
    collect_rows(rows)
}

fn read_harness_reflection(conn: &Connection, id: &str) -> StoreResult<HarnessReflection> {
    conn.query_row(
        "select id, run_id, project_id, policy_version, configuration_version,
         summary, next_direction, metrics_json, created_at from harness_reflections where id = ?1",
        params![id],
        |row| {
            Ok(HarnessReflection {
                id: row.get(0)?,
                run_id: row.get(1)?,
                project_id: row.get(2)?,
                policy_version: row.get(3)?,
                configuration_version: row.get(4)?,
                summary: row.get(5)?,
                next_direction: row.get(6)?,
                metrics_json: row.get(7)?,
                created_at: row.get(8)?,
            })
        },
    )
    .map_err(|error| error.to_string())
}

fn read_harness_improvements(
    conn: &Connection,
    project_id: &str,
    status: Option<HarnessImprovementStatus>,
) -> StoreResult<Vec<HarnessImprovement>> {
    let mut statement = conn
        .prepare(
            "select id from harness_improvements where project_id = ?1
             and (?2 is null or status = ?2) order by created_at desc, id desc",
        )
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map(
            params![project_id, status.map(HarnessImprovementStatus::as_str)],
            |row| row.get::<_, String>(0),
        )
        .map_err(|error| error.to_string())?;
    let ids = collect_rows(rows)?;
    ids.iter()
        .map(|id| read_harness_improvement(conn, id))
        .collect()
}

fn read_harness_improvement(conn: &Connection, id: &str) -> StoreResult<HarnessImprovement> {
    let mut improvement = conn
        .query_row(
            "select id, project_id, status, target, base_configuration_version,
             before_value_json, proposed_value_json, rationale, expected_effect,
             policy_version, decision_actor, decision_reason,
             resulting_configuration_version, created_at, decided_at
             from harness_improvements where id = ?1",
            params![id],
            |row| {
                let status: String = row.get(2)?;
                let target: String = row.get(3)?;
                let before: String = row.get(5)?;
                let proposed: String = row.get(6)?;
                Ok(HarnessImprovement {
                    id: row.get(0)?,
                    project_id: row.get(1)?,
                    status: parse_sql_enum(2, &status, HarnessImprovementStatus::parse)?,
                    target: parse_sql_enum(3, &target, HarnessImprovementTarget::parse)?,
                    base_configuration_version: row.get(4)?,
                    before_value: parse_json_column(5, &before)?,
                    proposed_value: parse_json_column(6, &proposed)?,
                    rationale: row.get(7)?,
                    expected_effect: row.get(8)?,
                    policy_version: row.get(9)?,
                    decision_actor: row.get(10)?,
                    decision_reason: row.get(11)?,
                    resulting_configuration_version: row.get(12)?,
                    run_ids: Vec::new(),
                    observation_ids: Vec::new(),
                    observations: Vec::new(),
                    created_at: row.get(13)?,
                    decided_at: row.get(14)?,
                })
            },
        )
        .map_err(|error| error.to_string())?;
    improvement.run_ids = read_string_column(
        conn,
        "select run_id from harness_improvement_runs where improvement_id = ?1 order by run_id",
        id,
    )?;
    improvement.observation_ids = read_string_column(
        conn,
        "select observation_id from harness_improvement_observations where improvement_id = ?1 order by observation_id",
        id,
    )?;
    improvement.observations = read_harness_observations(conn, id)?;
    Ok(improvement)
}

fn read_harness_observations(
    conn: &Connection,
    improvement_id: &str,
) -> StoreResult<Vec<HarnessObservation>> {
    let mut statement = conn
        .prepare(
            "select o.id, o.reflection_id, o.run_id, o.project_id, o.kind,
             o.signature, o.severity, o.confidence, o.description, o.metrics_json,
             o.target, o.proposed_value_json, o.proposal_eligible, o.created_at
             from harness_observations o
             join harness_improvement_observations link on link.observation_id = o.id
             where link.improvement_id = ?1 order by o.created_at, o.id",
        )
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map(params![improvement_id], |row| {
            let kind: String = row.get(4)?;
            let target: Option<String> = row.get(10)?;
            let proposed_json: Option<String> = row.get(11)?;
            Ok(HarnessObservation {
                id: row.get(0)?,
                reflection_id: row.get(1)?,
                run_id: row.get(2)?,
                project_id: row.get(3)?,
                kind: parse_sql_enum(4, &kind, HarnessObservationKind::parse)?,
                signature: row.get(5)?,
                severity: row.get(6)?,
                confidence: row.get(7)?,
                description: row.get(8)?,
                metrics_json: row.get(9)?,
                target: target
                    .as_deref()
                    .map(HarnessImprovementTarget::parse)
                    .transpose()
                    .map_err(|error| {
                        rusqlite::Error::FromSqlConversionFailure(
                            10,
                            rusqlite::types::Type::Text,
                            Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, error)),
                        )
                    })?,
                proposed_value: proposed_json
                    .as_deref()
                    .map(|json| parse_json_column(11, json))
                    .transpose()?,
                proposal_eligible: row.get(12)?,
                created_at: row.get(13)?,
            })
        })
        .map_err(|error| error.to_string())?;
    collect_rows(rows)
}

fn read_string_column(conn: &Connection, sql: &str, id: &str) -> StoreResult<Vec<String>> {
    let mut statement = conn.prepare(sql).map_err(|error| error.to_string())?;
    let rows = statement
        .query_map(params![id], |row| row.get::<_, String>(0))
        .map_err(|error| error.to_string())?;
    collect_rows(rows)
}

fn parse_json_column<T: serde::de::DeserializeOwned>(
    column: usize,
    json: &str,
) -> rusqlite::Result<T> {
    serde_json::from_str(json).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(
            column,
            rusqlite::types::Type::Text,
            Box::new(error),
        )
    })
}

fn read_vaults(conn: &Connection) -> StoreResult<Vec<Vault>> {
    let mut stmt = conn
        .prepare(
            "select id, project_id, title, path, membership_revision from vaults order by path",
        )
        .map_err(|error| error.to_string())?;

    let rows = stmt
        .query_map([], |row| {
            Ok(Vault {
                id: row.get(0)?,
                project_id: row.get(1)?,
                title: row.get(2)?,
                path: row.get(3)?,
                membership_revision: row.get(4)?,
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
            select id, thread_id, kind, body, model, context_json, pinned, created_at,
                   author_kind, author_id, run_id
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
        select id, thread_id, kind, body, model, context_json, pinned, created_at,
               author_kind, author_id, run_id
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
        author_kind: row.get(8)?,
        author_id: row.get(9)?,
        run_id: row.get(10)?,
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
        "source_passage" => ThreadAnchor::SourcePassage {
            source_id: source_id.unwrap_or_default(),
            page_index,
            start_offset: start_offset.unwrap_or_default(),
            end_offset: end_offset.unwrap_or_default(),
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
        ThreadAnchor::SourcePassage {
            source_id,
            page_index,
            start_offset,
            end_offset,
            selected_text,
        } => (
            "source_passage",
            Some(source_id.clone()),
            Some(*start_offset),
            Some(*end_offset),
            Some(selected_text.clone()),
            *page_index,
            None,
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
          id, thread_id, kind, body, model, context_json, pinned,
          author_kind, author_id, run_id, created_at
        )
        values (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, datetime('now'))
        ",
        params![
            id,
            thread_id,
            draft.kind,
            draft.body,
            draft.model,
            context_json,
            draft.pinned as i64,
            draft.author_kind,
            draft.author_id,
            draft.run_id,
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
    /// Atomically create one scoped agent Search, Run, and retry receipt.
    #[allow(clippy::too_many_arguments)]
    pub fn create_agent_search_run(
        &self,
        project_id: &str,
        vault_id: &str,
        caller: &str,
        parent_run_id: Option<&str>,
        request_id: &str,
        payload_hash: &str,
        model_id: Option<&str>,
        draft: &SearchDraft,
        maximum_concurrent: i64,
    ) -> StoreResult<AgentSearchReceipt> {
        const TOOL: &str = "search_start";
        let receipt_caller = format!("{caller}@{project_id}");
        let mut conn = self.open_connection()?;
        conn.busy_timeout(Duration::from_secs(5))
            .map_err(|error| error.to_string())?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| error.to_string())?;
        let existing: Option<(String, String)> = tx
            .query_row(
                "select payload_hash, result_json from mcp_mutation_receipts
                 where caller = ?1 and tool = ?2 and request_id = ?3",
                params![receipt_caller, TOOL, request_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(|error| error.to_string())?;
        if let Some((existing_hash, result_json)) = existing {
            if existing_hash != payload_hash {
                return Err("Request ID was already used with a different payload".to_string());
            }
            return serde_json::from_str(&result_json).map_err(|error| error.to_string());
        }

        let scoped_vault: bool = tx
            .query_row(
                "select exists(select 1 from vaults where id = ?1 and project_id = ?2)",
                params![vault_id, project_id],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        if !scoped_vault {
            return Err("Search Vault is outside this Project".to_string());
        }
        if let Some(parent_run_id) = parent_run_id {
            let parent: Option<(String, Option<String>, String)> = tx
                .query_row(
                    "select effective_instruction_stack_json, agent_limits_json, status
                     from harness_runs
                     where id = ?1 and project_id = ?2",
                    params![parent_run_id, project_id],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )
                .optional()
                .map_err(|error| error.to_string())?;
            let Some((parent_stack_json, parent_limits_json, parent_status)) = parent else {
                return Err("Parent Research Run is outside this Project".to_string());
            };
            if !matches!(
                parent_status.as_str(),
                "queued" | "planning" | "searching" | "assessing" | "ranking"
            ) {
                return Err("Parent Research Run is no longer active".to_string());
            }
            let parent_stack: EffectiveInstructionStack = serde_json::from_str(&parent_stack_json)
                .map_err(|error| format!("Parent Research Run limits are unreadable: {error}"))?;
            let (child_searches, reserved_provider_queries, reserved_llm_calls): (u32, u32, u32) =
                tx.query_row(
                    "select count(*), coalesce(sum(reserved_provider_queries), 0),
                            coalesce(sum(reserved_llm_calls), 0)
                     from agent_search_runs where parent_run_id = ?1",
                    params![parent_run_id],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )
                .map_err(|error| error.to_string())?;
            let maximum_child_searches = parent_limits_json
                .map(|json| {
                    serde_json::from_str::<AgentRunLimits>(&json)
                        .map(|limits| limits.maximum_child_searches)
                        .map_err(|error| error.to_string())
                })
                .transpose()?
                .unwrap_or(u32::MAX);
            if child_searches >= maximum_child_searches {
                return Err("Parent Research Run child-search limit is exhausted".to_string());
            }
            if reserved_provider_queries.saturating_add(draft.strategy.max_provider_queries)
                > parent_stack.run_context.maximum_provider_queries
                || reserved_llm_calls.saturating_add(draft.strategy.max_llm_calls)
                    > parent_stack.run_context.maximum_llm_calls
            {
                return Err("Parent Research Run search budget is exhausted".to_string());
            }
        }
        let active: i64 = tx
            .query_row(
                "select count(*) from agent_search_runs a
                 join search_runs r on r.id = a.run_id
                 where a.project_id = ?1 and a.caller = ?2
                   and a.parent_run_id is ?3
                   and r.status not in ('ready', 'failed', 'cancelled')",
                params![project_id, caller, parent_run_id],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        if active >= maximum_concurrent {
            return Err(format!(
                "Search concurrency limit reached ({maximum_concurrent})"
            ));
        }

        let search_id = timestamped_id("search")?;
        let run_id = timestamped_id("run")?;
        let constraints = serde_json::to_string(&draft.constraints).map_err(|e| e.to_string())?;
        let strategy = serde_json::to_string(&draft.strategy).map_err(|e| e.to_string())?;
        let provider_set = serde_json::to_string(&draft.constraints.providers)
            .map_err(|error| error.to_string())?;
        tx.execute(
            "insert into searches
               (id, title, goal, constraints, strategy, schedule, status,
                created_at, updated_at)
             values (?1, ?2, ?3, ?4, ?5, null, 'queued', datetime('now'), datetime('now'))",
            params![search_id, draft.title, draft.goal, constraints, strategy],
        )
        .map_err(|error| error.to_string())?;
        tx.execute(
            "insert into search_runs
               (id, search_id, mode, provider_set, query_expansions, status, created_at, started_at)
             values (?1, ?2, 'agent_child', ?3, '[]', 'queued', datetime('now'), datetime('now'))",
            params![run_id, search_id, provider_set],
        )
        .map_err(|error| error.to_string())?;
        tx.execute(
            "insert into agent_search_runs
               (run_id, search_id, project_id, vault_id, caller, parent_run_id,
                model_id, reserved_provider_queries, reserved_llm_calls, created_at)
             values (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, datetime('now'))",
            params![
                run_id,
                search_id,
                project_id,
                vault_id,
                caller,
                parent_run_id,
                model_id,
                draft.strategy.max_provider_queries,
                draft.strategy.max_llm_calls,
            ],
        )
        .map_err(|error| error.to_string())?;
        let receipt = AgentSearchReceipt { search_id, run_id };
        let result_json = serde_json::to_string(&receipt).map_err(|error| error.to_string())?;
        tx.execute(
            "insert into mcp_mutation_receipts
               (caller, tool, request_id, payload_hash, result_json, created_at)
             values (?1, ?2, ?3, ?4, ?5, datetime('now'))",
            params![receipt_caller, TOOL, request_id, payload_hash, result_json],
        )
        .map_err(|error| error.to_string())?;
        tx.commit().map_err(|error| error.to_string())?;
        Ok(receipt)
    }

    /// Return a scoped agent Search Run and whether cancellation was requested.
    pub fn get_agent_search_run(
        &self,
        project_id: &str,
        run_id: &str,
    ) -> StoreResult<(SearchRun, bool)> {
        let conn = self.open_connection()?;
        let scoped: bool = conn
            .query_row(
                "select exists(select 1 from agent_search_runs where run_id = ?1 and project_id = ?2)",
                params![run_id, project_id],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        if !scoped {
            return Err("Agent Search Run is outside this Project".to_string());
        }
        let cancel_requested: bool = conn
            .query_row(
                "select cancel_requested from agent_search_runs where run_id = ?1",
                params![run_id],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        Ok((read_search_run(&conn, run_id)?, cancel_requested))
    }

    /// Resolve a candidate that appeared in an agent Search scoped to a Project.
    pub fn get_agent_search_candidate(
        &self,
        project_id: &str,
        candidate_id: &str,
    ) -> StoreResult<PaperCandidate> {
        let conn = self.open_connection()?;
        let mut statement = conn
            .prepare(
                "select event.candidate_json
                 from agent_search_candidate_events event
                 join agent_search_runs run on run.run_id = event.run_id
                 where run.project_id = ?1 order by event.sequence desc",
            )
            .map_err(|error| error.to_string())?;
        let rows = statement
            .query_map(params![project_id], |row| row.get::<_, String>(0))
            .map_err(|error| error.to_string())?;
        for row in rows {
            let json = row.map_err(|error| error.to_string())?;
            let candidate: PaperCandidate =
                serde_json::from_str(&json).map_err(|error| error.to_string())?;
            if candidate.id == candidate_id {
                return Ok(candidate);
            }
        }
        Err("Search candidate is outside this Project".to_string())
    }

    /// Atomically save one scoped candidate, its Vault membership, and retry receipt.
    pub fn add_agent_search_candidate_to_vault(
        &self,
        project_id: &str,
        vault_id: &str,
        caller: &str,
        parent_run_id: Option<&str>,
        request_id: &str,
        payload_hash: &str,
        paper: &PaperDraft,
    ) -> StoreResult<AgentVaultAddReceipt> {
        const TOOL: &str = "vault_add_paper";
        let receipt_caller = format!("{caller}@{project_id}");
        let mut conn = self.open_connection()?;
        conn.busy_timeout(Duration::from_secs(5))
            .map_err(|error| error.to_string())?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| error.to_string())?;
        let existing: Option<(String, String)> = tx
            .query_row(
                "select payload_hash, result_json from mcp_mutation_receipts
                 where caller = ?1 and tool = ?2 and request_id = ?3",
                params![receipt_caller, TOOL, request_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(|error| error.to_string())?;
        if let Some((existing_hash, result_json)) = existing {
            if existing_hash != payload_hash {
                return Err("Request ID was already used with a different payload".to_string());
            }
            return serde_json::from_str(&result_json).map_err(|error| error.to_string());
        }

        let scoped_vault: bool = tx
            .query_row(
                "select exists(select 1 from vaults where id = ?1 and project_id = ?2)",
                params![vault_id, project_id],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        if !scoped_vault {
            return Err("Target Vault is outside this Project".to_string());
        }
        if let Some(parent_run_id) = parent_run_id {
            require_agent_run_write_admission(&tx, Some(parent_run_id), Some(project_id))?;
        }
        let membership_exists: bool = tx
            .query_row(
                "select exists(select 1 from vault_papers where vault_id = ?1 and paper_id = ?2)",
                params![vault_id, paper.id],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        let authors_json = to_json(&paper.authors)?;
        let tags_json = to_json(&paper.tags)?;
        tx.execute(
            "insert into papers (
               id, title, authors_json, venue, year, citations, tags_json,
               note_count, annotation_count, status, abstract, created_at, updated_at
             ) values (?1, ?2, ?3, ?4, ?5, ?6, ?7, 0, 0, ?8, ?9,
                       datetime('now'), datetime('now'))
             on conflict(id) do update set
               title = excluded.title, authors_json = excluded.authors_json,
               venue = excluded.venue, year = excluded.year,
               citations = excluded.citations, abstract = excluded.abstract,
               updated_at = datetime('now')",
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
        tx.execute(
            "insert into vault_papers (vault_id, paper_id, added_at)
             values (?1, ?2, datetime('now'))
             on conflict(vault_id, paper_id) do nothing",
            params![vault_id, paper.id],
        )
        .map_err(|error| error.to_string())?;

        let receipt = AgentVaultAddReceipt {
            paper_id: paper.id.clone(),
            membership_added: !membership_exists,
            source_ids: paper
                .sources
                .iter()
                .map(|source| document_source_id(&paper.id, source))
                .collect(),
        };
        if let Some(parent_run_id) = parent_run_id {
            tx.execute(
                "insert into agent_vault_additions
                   (run_id, paper_id, membership_added, created_at)
                 values (?1, ?2, ?3, datetime('now'))
                 on conflict(run_id, paper_id) do nothing",
                params![parent_run_id, paper.id, receipt.membership_added],
            )
            .map_err(|error| error.to_string())?;
        }
        let result_json = serde_json::to_string(&receipt).map_err(|error| error.to_string())?;
        tx.execute(
            "insert into mcp_mutation_receipts
               (caller, tool, request_id, payload_hash, result_json, created_at)
             values (?1, ?2, ?3, ?4, ?5, datetime('now'))",
            params![receipt_caller, TOOL, request_id, payload_hash, result_json],
        )
        .map_err(|error| error.to_string())?;
        tx.commit().map_err(|error| error.to_string())?;
        Ok(receipt)
    }

    /// Return the optional model override recorded for an agent Search Run.
    pub fn agent_search_model_id(&self, run_id: &str) -> StoreResult<Option<String>> {
        let conn = self.open_connection()?;
        conn.query_row(
            "select model_id from agent_search_runs where run_id = ?1",
            params![run_id],
            |row| row.get(0),
        )
        .optional()
        .map(|value| value.flatten())
        .map_err(|error| error.to_string())
    }

    /// Record candidate snapshots for incremental, run-scoped polling.
    pub fn append_agent_search_candidates(
        &self,
        run_id: &str,
        phase: &str,
        candidates: &[PaperCandidate],
    ) -> StoreResult<()> {
        let mut conn = self.open_connection()?;
        let tx = conn.transaction().map_err(|error| error.to_string())?;
        let tracked: bool = tx
            .query_row(
                "select exists(
                   select 1 from agent_search_runs child
                   where child.run_id = ?1 and (
                     child.parent_run_id is null or exists(
                       select 1 from harness_runs parent
                       where parent.id = child.parent_run_id and parent.status in
                         ('queued','planning','searching','assessing','ranking')
                     )
                   )
                 )",
                params![run_id],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        if !tracked {
            return Ok(());
        }
        for candidate in candidates {
            let candidate_json =
                serde_json::to_string(candidate).map_err(|error| error.to_string())?;
            tx.execute(
                "insert into agent_search_candidate_events
                   (run_id, phase, candidate_json, created_at)
                 values (?1, ?2, ?3, datetime('now'))",
                params![run_id, phase, candidate_json],
            )
            .map_err(|error| error.to_string())?;
        }
        tx.commit().map_err(|error| error.to_string())
    }

    /// Highest candidate event currently visible for one run.
    pub fn agent_search_candidate_high_water(&self, run_id: &str) -> StoreResult<i64> {
        let conn = self.open_connection()?;
        conn.query_row(
            "select coalesce(max(sequence), 0) from agent_search_candidate_events where run_id = ?1",
            params![run_id],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())
    }

    /// Read candidate events within a captured high-water mark.
    pub fn list_agent_search_candidate_events(
        &self,
        run_id: &str,
        after_sequence: i64,
        high_water: i64,
        limit: usize,
    ) -> StoreResult<Vec<AgentSearchCandidateEvent>> {
        let conn = self.open_connection()?;
        let mut statement = conn
            .prepare(
                "select sequence, phase, candidate_json
                 from agent_search_candidate_events
                 where run_id = ?1 and sequence > ?2 and sequence <= ?3
                 order by sequence limit ?4",
            )
            .map_err(|error| error.to_string())?;
        let rows = statement
            .query_map(
                params![run_id, after_sequence, high_water, limit as i64],
                |row| {
                    let candidate_json: String = row.get(2)?;
                    let candidate = serde_json::from_str(&candidate_json).map_err(|error| {
                        rusqlite::Error::FromSqlConversionFailure(
                            2,
                            rusqlite::types::Type::Text,
                            Box::new(error),
                        )
                    })?;
                    Ok(AgentSearchCandidateEvent {
                        sequence: row.get(0)?,
                        phase: row.get(1)?,
                        candidate,
                    })
                },
            )
            .map_err(|error| error.to_string())?;
        collect_rows(rows)
    }

    /// Append one run-scoped Search activity item when the run is agent-owned.
    pub fn append_agent_search_activity(
        &self,
        run_id: &str,
        kind: &str,
        message: &str,
        error: Option<&str>,
    ) -> StoreResult<()> {
        let conn = self.open_connection()?;
        conn.execute(
            "insert into agent_search_activity (run_id, kind, message, error, created_at)
             select ?1, ?2, ?3, ?4, datetime('now')
             where exists(select 1 from agent_search_runs where run_id = ?1)",
            params![run_id, kind, message, error],
        )
        .map(|_| ())
        .map_err(|error| error.to_string())
    }

    /// Return the newest bounded activity slice for one Search Run.
    pub fn list_agent_search_activity(
        &self,
        run_id: &str,
        limit: usize,
    ) -> StoreResult<Vec<AgentSearchActivity>> {
        let conn = self.open_connection()?;
        let mut statement = conn
            .prepare(
                "select sequence, kind, message, error from agent_search_activity
                 where run_id = ?1 order by sequence desc limit ?2",
            )
            .map_err(|error| error.to_string())?;
        let rows = statement
            .query_map(params![run_id, limit as i64], |row| {
                Ok(AgentSearchActivity {
                    sequence: row.get(0)?,
                    kind: row.get(1)?,
                    message: row.get(2)?,
                    error: row.get(3)?,
                })
            })
            .map_err(|error| error.to_string())?;
        let mut activity = collect_rows(rows)?;
        activity.reverse();
        Ok(activity)
    }

    /// Mark cancellation once and return whether this call changed the intent.
    pub fn request_agent_search_cancel(
        &self,
        project_id: &str,
        run_id: &str,
    ) -> StoreResult<(SearchRun, bool)> {
        let mut conn = self.open_connection()?;
        let tx = conn.transaction().map_err(|error| error.to_string())?;
        let exists: bool = tx
            .query_row(
                "select exists(select 1 from agent_search_runs where run_id = ?1 and project_id = ?2)",
                params![run_id, project_id],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        if !exists {
            return Err("Agent Search Run is outside this Project".to_string());
        }
        let run = read_search_run(&tx, run_id)?;
        let terminal = matches!(run.status.as_str(), "ready" | "failed" | "cancelled");
        let changed = if terminal {
            0
        } else {
            tx.execute(
                "update agent_search_runs set cancel_requested = 1
                 where run_id = ?1 and project_id = ?2 and cancel_requested = 0",
                params![run_id, project_id],
            )
            .map_err(|error| error.to_string())?
        };
        if changed == 1 {
            tx.execute(
                "insert into agent_search_activity (run_id, kind, message, created_at)
                 values (?1, 'cancellation_requested', 'Cancellation requested', datetime('now'))",
                params![run_id],
            )
            .map_err(|error| error.to_string())?;
        }
        tx.commit().map_err(|error| error.to_string())?;
        Ok((run, changed == 1))
    }

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

    /// Persists actual bounded-loop usage on the Search Run and linked Harness Run.
    pub fn set_search_run_usage(
        &self,
        run_id: &str,
        provider_queries: u32,
        llm_calls: u32,
        iterations: u32,
        inspected_candidates: u32,
    ) -> StoreResult<()> {
        let mut conn = self.open_connection()?;
        let tx = conn.transaction().map_err(|error| error.to_string())?;
        tx.execute(
            "update search_runs set provider_query_count = ?2, llm_call_count = ?3,
             inspected_candidate_count = ?4 where id = ?1",
            params![run_id, provider_queries, llm_calls, inspected_candidates],
        )
        .map_err(|error| error.to_string())?;
        tx.execute(
            "update harness_runs set provider_query_count = ?2, llm_call_count = ?3,
             iteration_count = ?4, inspected_candidate_count = ?5
             where search_run_id = ?1",
            params![
                run_id,
                provider_queries,
                llm_calls,
                iterations,
                inspected_candidates
            ],
        )
        .map_err(|error| error.to_string())?;
        if let Some(harness_run_id) = tx
            .query_row(
                "select id from harness_runs where search_run_id = ?1",
                params![run_id],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|error| error.to_string())?
        {
            append_structured_harness_event(
                &tx,
                &harness_run_id,
                "usage_recorded",
                "Recorded actual bounded research usage",
                Some(serde_json::json!({
                    "providerQueries": provider_queries,
                    "llmCalls": llm_calls,
                    "iterations": iterations,
                    "inspectedCandidates": inspected_candidates,
                })),
                Some("complete"),
                None,
                None,
                "system",
            )?;
        }
        tx.commit().map_err(|error| error.to_string())
    }

    /// Adds post-search reconciliation calls to both ledgers.
    pub fn add_search_run_llm_calls(&self, run_id: &str, calls: u32) -> StoreResult<()> {
        let mut conn = self.open_connection()?;
        let tx = conn.transaction().map_err(|error| error.to_string())?;
        tx.execute(
            "update search_runs set llm_call_count = llm_call_count + ?2 where id = ?1",
            params![run_id, calls],
        )
        .map_err(|error| error.to_string())?;
        tx.execute(
            "update harness_runs set llm_call_count = llm_call_count + ?2
             where search_run_id = ?1",
            params![run_id, calls],
        )
        .map_err(|error| error.to_string())?;
        if let Some((harness_run_id, total_calls)) = tx
            .query_row(
                "select id, llm_call_count from harness_runs where search_run_id = ?1",
                params![run_id],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, u32>(1)?)),
            )
            .optional()
            .map_err(|error| error.to_string())?
        {
            append_structured_harness_event(
                &tx,
                &harness_run_id,
                "reconciliation_usage_recorded",
                "Recorded reconciliation model calls",
                Some(serde_json::json!({ "llmCalls": calls, "totalLlmCalls": total_calls })),
                Some("reconciliation"),
                None,
                None,
                "system",
            )?;
        }
        tx.commit().map_err(|error| error.to_string())
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
        let mut conn = self.open_connection()?;
        let tx = conn.transaction().map_err(|error| error.to_string())?;
        let finished_sql = if finished {
            "datetime('now')"
        } else {
            "finished_at"
        };
        tx.execute(
            &format!(
                "update search_runs
                   set status = ?2, iteration = ?3, stop_reason = ?4, error = ?5,
                       finished_at = {finished_sql}
                 where id = ?1"
            ),
            params![run_id, status.as_str(), iteration, stop_reason, error],
        )
        .map_err(|e| e.to_string())?;

        let linked_harness_run = tx
            .query_row(
                "select id, project_id from harness_runs where search_run_id = ?1",
                params![run_id],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .optional()
            .map_err(|error| error.to_string())?;
        if let Some((harness_run_id, project_id)) = linked_harness_run {
            let awaiting_reconciliation = finished && status == SearchRunStatus::Ready;
            let terminal = finished && !awaiting_reconciliation;
            let harness_status = if awaiting_reconciliation {
                "reconciling"
            } else {
                status.as_str()
            };
            let resulting_vault_revision = if terminal {
                Some(
                    tx.query_row(
                        "select membership_revision from vaults where project_id = ?1",
                        params![project_id],
                        |row| row.get::<_, i64>(0),
                    )
                    .map_err(|error| error.to_string())?,
                )
            } else {
                None
            };
            let harness_finished_sql = if terminal {
                "datetime('now')"
            } else {
                "finished_at"
            };
            tx.execute(
                &format!(
                    "update harness_runs
                     set status = ?2, stop_reason = ?3, summary = ?4,
                         resulting_state_revision = case when ?5 is not null
                           then coalesce(resulting_state_revision, starting_state_revision)
                           else resulting_state_revision end,
                         resulting_vault_revision = coalesce(?5, resulting_vault_revision),
                         finished_at = {harness_finished_sql}
                     where id = ?1"
                ),
                params![
                    harness_run_id,
                    harness_status,
                    stop_reason,
                    error,
                    resulting_vault_revision
                ],
            )
            .map_err(|error| error.to_string())?;
            append_harness_event(
                &tx,
                &harness_run_id,
                harness_status,
                if awaiting_reconciliation {
                    "Reconciling candidate decisions, Research State, and reflection"
                } else if error.is_some() {
                    "Research Run failed"
                } else {
                    stop_reason.unwrap_or(status.as_str())
                },
            )?;
            if terminal {
                append_structured_harness_event(
                    &tx,
                    &harness_run_id,
                    "stop_decided",
                    &format!(
                        "Research Run stopped: {}",
                        stop_reason.unwrap_or(status.as_str()).replace('_', " ")
                    ),
                    Some(serde_json::json!({
                        "status": status.as_str(),
                        "stopReason": stop_reason,
                        "complete": matches!(stop_reason, Some("target_reached" | "coverage_sufficient" | "converged")),
                        "converged": stop_reason == Some("converged"),
                    })),
                    Some("complete"),
                    None,
                    None,
                    "system",
                )?;
                let added_count: i64 = tx
                    .query_row(
                        "select added_count from search_runs where id = ?1",
                        params![run_id],
                        |row| row.get(0),
                    )
                    .map_err(|error| error.to_string())?;
                let state_changed: bool = tx
                    .query_row(
                        "select resulting_state_revision > starting_state_revision
                         from harness_runs where id = ?1",
                        params![harness_run_id],
                        |row| row.get(0),
                    )
                    .map_err(|error| error.to_string())?;
                settle_harness_after_run(
                    &tx,
                    &project_id,
                    &harness_run_id,
                    added_count > 0 || state_changed,
                )?;
            }
        }
        tx.commit().map_err(|error| error.to_string())?;
        Ok(())
    }

    /// Completes a linked Harness Run after reconciliation and reflection settle.
    pub fn finalize_harness_run(&self, search_run_id: &str) -> StoreResult<()> {
        let mut conn = self.open_connection()?;
        let tx = conn.transaction().map_err(|error| error.to_string())?;
        let (search_status, stop_reason, added_count): (String, Option<String>, i64) = tx
            .query_row(
                "select status, stop_reason, added_count from search_runs where id = ?1",
                params![search_run_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .map_err(|error| error.to_string())?;
        if search_status != "ready" {
            return Err("Only a ready Search Run may finalize its Research Run".to_string());
        }
        let (harness_run_id, project_id, status): (String, String, String) = tx
            .query_row(
                "select id, project_id, status from harness_runs where search_run_id = ?1",
                params![search_run_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .map_err(|error| error.to_string())?;
        if status != "reconciling" {
            return Err("Research Run is not awaiting finalization".to_string());
        }
        let resulting_vault_revision: i64 = tx
            .query_row(
                "select membership_revision from vaults where project_id = ?1",
                params![project_id],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        tx.execute(
            "update harness_runs set status = 'ready',
             resulting_state_revision = coalesce(resulting_state_revision, starting_state_revision),
             resulting_vault_revision = coalesce(resulting_vault_revision, ?2),
             finished_at = datetime('now') where id = ?1 and status = 'reconciling'",
            params![harness_run_id, resulting_vault_revision],
        )
        .map_err(|error| error.to_string())?;
        append_harness_event(
            &tx,
            &harness_run_id,
            "ready",
            stop_reason.as_deref().unwrap_or("ready"),
        )?;
        append_structured_harness_event(
            &tx,
            &harness_run_id,
            "stop_decided",
            &format!(
                "Research Run stopped: {}",
                stop_reason.as_deref().unwrap_or("ready").replace('_', " ")
            ),
            Some(serde_json::json!({
                "status": "ready",
                "stopReason": stop_reason,
                "complete": matches!(stop_reason.as_deref(), Some("target_reached" | "coverage_sufficient" | "converged")),
                "converged": stop_reason.as_deref() == Some("converged"),
            })),
            Some("complete"),
            None,
            None,
            "system",
        )?;
        let state_changed: bool = tx
            .query_row(
                "select resulting_state_revision > starting_state_revision
                 from harness_runs where id = ?1",
                params![harness_run_id],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        settle_harness_after_run(
            &tx,
            &project_id,
            &harness_run_id,
            added_count > 0 || state_changed,
        )?;
        tx.commit().map_err(|error| error.to_string())
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
                finished_at, error, created_at, provider_query_count, llm_call_count,
                inspected_candidate_count
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
        provider_query_count: row.get(14)?,
        llm_call_count: row.get(15)?,
        inspected_candidate_count: row.get(16)?,
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
        if !matches!(source.source_kind.as_str(), "pdf" | "html")
            || source.source_url.trim().is_empty()
        {
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

fn upsert_reconciliation_paper(
    conn: &Connection,
    vault_id: &str,
    candidate: &SearchCandidate,
) -> StoreResult<()> {
    let paper = &candidate.candidate;
    conn.execute(
        "insert into papers (
           id, title, authors_json, venue, year, citations, tags_json,
           note_count, annotation_count, status, abstract, created_at, updated_at
         ) values (?1, ?2, ?3, ?4, ?5, ?6, '[]', 0, 0, 'UNREAD', ?7,
                   datetime('now'), datetime('now'))
         on conflict(id) do update set
           title = excluded.title, authors_json = excluded.authors_json,
           venue = excluded.venue, year = excluded.year, citations = excluded.citations,
           abstract = coalesce(papers.abstract, excluded.abstract), updated_at = datetime('now')",
        params![
            paper.id,
            paper.title,
            serde_json::to_string(&paper.authors).map_err(|error| error.to_string())?,
            paper.venue.as_deref().unwrap_or("Unknown"),
            paper.year.unwrap_or(0),
            paper.citation_count.unwrap_or(0),
            paper.abstract_text
        ],
    )
    .map_err(|error| error.to_string())?;
    conn.execute(
        "insert into vault_papers (vault_id, paper_id, added_at)
         values (?1, ?2, datetime('now')) on conflict(vault_id, paper_id) do nothing",
        params![vault_id, paper.id],
    )
    .map_err(|error| error.to_string())?;
    Ok(())
}

/// Stores provider abstract text as an immutable, visibly typed extraction.
fn materialize_metadata_abstract(
    conn: &Connection,
    candidate: &SearchCandidate,
) -> StoreResult<String> {
    let paper = &candidate.candidate;
    let abstract_text = paper
        .abstract_text
        .as_deref()
        .ok_or_else(|| format!("Candidate has no abstract: {}", candidate.id))?;
    materialize_metadata_abstract_text(
        conn,
        &paper.id,
        abstract_text,
        paper.external_url.as_deref(),
    )
}

fn materialize_metadata_abstract_text(
    conn: &Connection,
    paper_id: &str,
    abstract_text: &str,
    external_url: Option<&str>,
) -> StoreResult<String> {
    let digest = format!("{:x}", Sha256::digest(abstract_text.as_bytes()));
    let suffix = &digest[..16];
    let source_id = format!("metadata_abstract:{paper_id}:{suffix}");
    let extraction_id = format!("metadata_abstract_extraction:{paper_id}:{suffix}");
    let page_id = format!("metadata_abstract_page:{paper_id}:{suffix}");
    let block_id = format!("metadata_abstract_block:{paper_id}:{suffix}");
    let chunk_id = format!("metadata_abstract_chunk:{paper_id}:{suffix}");
    conn.execute(
        "insert into document_sources (
           id, paper_id, source_kind, source_url, landing_url, final_url,
           acquisition_method, local_path, status, error, created_at, updated_at
         ) values (?1, ?2, 'metadata_abstract', ?3, ?4, ?3, 'provider_metadata',
                   null, 'cached', null, datetime('now'), datetime('now'))
         on conflict(id) do nothing",
        params![source_id, paper_id, external_url, external_url],
    )
    .map_err(|error| error.to_string())?;
    conn.execute(
        "insert into document_extractions (
           id, paper_id, source_id, extractor, extractor_version,
           annotation_source_id, status, error, created_at, updated_at
         ) values (?1, ?2, ?3, 'metadata_abstract', '1', ?3, 'ready', null,
                   datetime('now'), datetime('now'))
         on conflict(id) do nothing",
        params![extraction_id, paper_id, source_id],
    )
    .map_err(|error| error.to_string())?;
    let exists: bool = conn
        .query_row(
            "select exists(select 1 from document_chunks where id = ?1)",
            params![chunk_id],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    if !exists {
        let text_length = abstract_text.chars().count() as i64;
        conn.execute(
            "insert into document_pages
             (id, paper_id, source_id, extraction_id, page_index, width, height)
             values (?1, ?2, ?3, ?4, 0, 1, 1)",
            params![page_id, paper_id, source_id, extraction_id],
        )
        .map_err(|error| error.to_string())?;
        conn.execute(
            "insert into document_blocks (
               id, paper_id, source_id, extraction_id, page_index, block_index,
               reading_order, kind, text, asset_id, source_start, source_end, bbox_json
             ) values (?1, ?2, ?3, ?4, 0, 0, 0, 'abstract', ?5, null, 0, ?6, null)",
            params![
                block_id,
                paper_id,
                source_id,
                extraction_id,
                abstract_text,
                text_length
            ],
        )
        .map_err(|error| error.to_string())?;
        insert_chunks(
            conn,
            &[DocumentChunk {
                id: chunk_id.clone(),
                paper_id: paper_id.to_string(),
                source_id: source_id.clone(),
                extraction_id: extraction_id.clone(),
                chunk_index: 0,
                chunker: "metadata_abstract".to_string(),
                chunk_version: 1,
                page_start: 0,
                page_end: 0,
                heading_path: Some("Provider abstract".to_string()),
                text: abstract_text.to_string(),
                token_estimate: ((text_length + 3) / 4) as i32,
                source_start: 0,
                source_end: text_length,
                block_ids: vec![block_id],
            }],
        )?;
    }
    conn.execute(
        "update papers set active_source_id = coalesce(active_source_id, ?2),
         active_extraction_id = coalesce(active_extraction_id, ?3), updated_at = datetime('now')
         where id = ?1",
        params![paper_id, source_id, extraction_id],
    )
    .map_err(|error| error.to_string())?;
    Ok(chunk_id)
}

fn find_inspected_evidence_chunk(
    conn: &Connection,
    paper_id: &str,
    excerpt: &str,
) -> StoreResult<Option<String>> {
    conn.query_row(
        "select c.id from document_chunks c
         join document_extractions e on e.id = c.extraction_id and e.status = 'ready'
         join document_sources s on s.id = c.source_id
         where c.paper_id = ?1 and instr(c.text, ?2) > 0
         order by case when s.source_kind = 'metadata_abstract' then 1 else 0 end, c.chunk_index
         limit 1",
        params![paper_id, excerpt],
        |row| row.get(0),
    )
    .optional()
    .map_err(|error| error.to_string())
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

fn insert_default_harness(conn: &Connection, project_id: &str) -> StoreResult<()> {
    let configuration = serde_json::to_string(&HarnessConfiguration::default())
        .map_err(|error| error.to_string())?;
    conn.execute(
        "insert into research_harnesses (
           project_id, status, configuration_json, configuration_version, updated_at
         ) values (?1, 'inactive', ?2, 1, datetime('now'))
         on conflict(project_id) do nothing",
        params![project_id, configuration],
    )
    .map_err(|error| error.to_string())?;
    conn.execute(
        "insert into harness_configuration_versions
         (project_id, version, configuration_json, actor, reason, created_at)
         select project_id, configuration_version, configuration_json, 'migration',
                'Initial Harness configuration', datetime('now')
         from research_harnesses where project_id = ?1
         on conflict(project_id, version) do nothing",
        params![project_id],
    )
    .map_err(|error| error.to_string())?;
    Ok(())
}

fn migrate_projects_to_harnesses(conn: &mut Connection) -> StoreResult<()> {
    let project_ids = {
        let mut statement = conn
            .prepare("select id from projects order by id")
            .map_err(|error| error.to_string())?;
        let rows = statement
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|error| error.to_string())?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|error| error.to_string())?
    };
    let tx = conn.transaction().map_err(|error| error.to_string())?;
    for project_id in project_ids {
        insert_default_harness(&tx, &project_id)?;
    }
    tx.commit().map_err(|error| error.to_string())
}

fn migrate_harness_authority_and_versions(conn: &mut Connection) -> StoreResult<()> {
    let rows = {
        let mut statement = conn
            .prepare(
                "select project_id, configuration_version, configuration_json
                 from research_harnesses order by project_id",
            )
            .map_err(|error| error.to_string())?;
        let mapped = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })
            .map_err(|error| error.to_string())?;
        mapped
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|error| error.to_string())?
    };
    let tx = conn.transaction().map_err(|error| error.to_string())?;
    for (project_id, version, raw) in rows {
        let mut value: serde_json::Value =
            serde_json::from_str(&raw).map_err(|error| error.to_string())?;
        let object = value
            .as_object_mut()
            .ok_or_else(|| "Harness configuration must be a JSON object".to_string())?;
        let legacy = !object.contains_key("autonomy");
        if legacy {
            object.insert(
                "autonomy".to_string(),
                serde_json::Value::String("manual".to_string()),
            );
            if let Some(schedule) = object
                .get_mut("schedule")
                .and_then(serde_json::Value::as_object_mut)
            {
                schedule.insert("enabled".to_string(), serde_json::Value::Bool(false));
            }
        }
        object
            .entry("scope")
            .or_insert_with(|| serde_json::Value::String(String::new()));
        object
            .entry("exclusions")
            .or_insert_with(|| serde_json::Value::String(String::new()));
        object
            .entry("mayAddPapers")
            .or_insert(serde_json::Value::Bool(false));
        object
            .entry("writableDocumentIds")
            .or_insert_with(|| serde_json::Value::Array(Vec::new()));
        let normalized = serde_json::to_string(&value).map_err(|error| error.to_string())?;
        tx.execute(
            "update research_harnesses
             set configuration_json = ?2,
                 schedule_enabled = case when ?3 then 0 else schedule_enabled end,
                 next_run_at = case when ?3 then null else next_run_at end
             where project_id = ?1",
            params![project_id, normalized, legacy],
        )
        .map_err(|error| error.to_string())?;
        tx.execute(
            "insert into harness_configuration_versions
             (project_id, version, configuration_json, actor, reason, created_at)
             values (?1, ?2, ?3, 'migration', 'Migrated Harness configuration', datetime('now'))
             on conflict(project_id, version) do nothing",
            params![project_id, version, normalized],
        )
        .map_err(|error| error.to_string())?;
    }
    tx.commit().map_err(|error| error.to_string())
}

/// Collapses legacy researcher inputs into one instruction without adding history.
fn migrate_harnesses_to_simple_research(conn: &mut Connection) -> StoreResult<()> {
    let rows = {
        let mut statement = conn
            .prepare(
                "select project_id, configuration_version, configuration_json
                 from research_harnesses order by project_id",
            )
            .map_err(|error| error.to_string())?;
        let mapped = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })
            .map_err(|error| error.to_string())?;
        mapped
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|error| error.to_string())?
    };

    let tx = conn.transaction().map_err(|error| error.to_string())?;
    for (project_id, version, raw) in rows {
        let configuration: HarnessConfiguration =
            serde_json::from_str(&raw).map_err(|error| error.to_string())?;
        let simple = configuration.into_simple_research();
        let normalized = serde_json::to_string(&simple).map_err(|error| error.to_string())?;
        if normalized == raw {
            continue;
        }
        tx.execute(
            "update research_harnesses
             set configuration_json = ?2, updated_at = datetime('now')
             where project_id = ?1",
            params![project_id, normalized],
        )
        .map_err(|error| error.to_string())?;
        tx.execute(
            "update harness_configuration_versions set configuration_json = ?3
             where project_id = ?1 and version = ?2",
            params![project_id, version, normalized],
        )
        .map_err(|error| error.to_string())?;
    }
    tx.commit().map_err(|error| error.to_string())
}

fn insert_initial_research_state(conn: &Connection, project_id: &str) -> StoreResult<()> {
    conn.execute(
        "insert into research_state_heads (project_id, current_revision)
         values (?1, 0) on conflict(project_id) do nothing",
        params![project_id],
    )
    .map_err(|error| error.to_string())?;
    conn.execute(
        "insert into research_state_revisions (project_id, revision, run_id, reason, created_at)
         values (?1, 0, null, 'Initial empty Research State', datetime('now'))
         on conflict(project_id, revision) do nothing",
        params![project_id],
    )
    .map_err(|error| error.to_string())?;
    Ok(())
}

fn migrate_projects_to_research_state(conn: &mut Connection) -> StoreResult<()> {
    let project_ids = {
        let mut statement = conn
            .prepare("select id from projects order by id")
            .map_err(|error| error.to_string())?;
        let rows = statement
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|error| error.to_string())?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|error| error.to_string())?
    };
    let tx = conn.transaction().map_err(|error| error.to_string())?;
    for project_id in project_ids {
        insert_initial_research_state(&tx, &project_id)?;
    }
    tx.commit().map_err(|error| error.to_string())
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

fn validate_harness_configuration(configuration: &HarnessConfiguration) -> StoreResult<()> {
    if configuration.canonical_instructions().trim().is_empty() {
        return Err("Research instructions cannot be empty".to_string());
    }
    for (name, value, limit) in [
        ("research goal", configuration.goal.as_str(), 1_000),
        (
            "research instructions",
            configuration.research_instructions.as_str(),
            8_000,
        ),
        ("research scope", configuration.scope.as_str(), 2_000),
        (
            "research exclusions",
            configuration.exclusions.as_str(),
            2_000,
        ),
    ] {
        if value.chars().count() > limit {
            return Err(format!("Harness {name} exceeds {limit} characters"));
        }
    }
    if configuration.paper_budget <= 0 {
        return Err("Paper budget must be greater than zero".to_string());
    }
    if configuration.sources.is_empty() {
        return Err("Choose at least one research source".to_string());
    }
    if !configuration
        .sources
        .iter()
        .any(|source| source == "browser")
    {
        return Err("Browser discovery is required for Research Runs".to_string());
    }
    if configuration
        .sources
        .iter()
        .any(|source| !matches!(source.as_str(), "browser" | "open_alex" | "arxiv"))
    {
        return Err("Research source is not supported".to_string());
    }
    if configuration.schedule.enabled {
        if configuration.autonomy == HarnessAutonomy::Manual {
            return Err("Manual autonomy cannot enable scheduled Runs".to_string());
        }
        next_schedule_occurrence(&configuration.schedule, Utc::now())?;
    }
    if configuration.writable_document_ids.len() > 50 {
        return Err("Harness may select at most 50 writable documents".to_string());
    }
    let mut writable = configuration.writable_document_ids.clone();
    writable.sort();
    writable.dedup();
    if writable.len() != configuration.writable_document_ids.len() {
        return Err("Harness writable document ids must be distinct".to_string());
    }
    let limits = &configuration.stop_conditions;
    for (name, value) in [
        ("maximum cycles", limits.maximum_cycles),
        (
            "maximum unproductive Runs",
            limits.maximum_unproductive_runs,
        ),
        ("maximum Run seconds", limits.maximum_run_seconds),
    ] {
        if value.is_some_and(|limit| limit <= 0) {
            return Err(format!("Harness {name} must be greater than zero"));
        }
    }
    if limits.maximum_llm_calls.is_some_and(|limit| limit < 4) {
        return Err(
            "Harness model call limit must be at least 4 to reserve bounded reconciliation"
                .to_string(),
        );
    }
    for (name, value) in [
        ("provider query limit", limits.maximum_provider_queries),
        ("model call limit", limits.maximum_llm_calls),
    ] {
        if value.is_some_and(|limit| limit == 0) {
            return Err(format!("Harness {name} must be greater than zero"));
        }
    }
    if let Some(end_at) = limits.end_at.as_deref() {
        DateTime::parse_from_rfc3339(end_at)
            .map_err(|_| "Harness end time must be RFC 3339".to_string())?;
    }
    Ok(())
}

fn validate_harness_authority(
    conn: &Connection,
    project_id: &str,
    configuration: &HarnessConfiguration,
) -> StoreResult<()> {
    for document_id in &configuration.writable_document_ids {
        let valid: bool = conn
            .query_row(
                "select exists(select 1 from project_documents where id = ?1 and project_id = ?2)",
                params![document_id, project_id],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        if !valid {
            return Err(format!(
                "Writable document does not belong to this Project: {document_id}"
            ));
        }
    }
    Ok(())
}

fn manual_reason(reason: Option<&str>) -> &str {
    reason
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("Manual Research State edit")
}

fn research_entry_draft_from_detail(
    detail: &ResearchEntryDetail,
    reason: &str,
) -> ResearchEntryDraft {
    ResearchEntryDraft {
        kind: detail.entry.kind,
        epistemic_status: detail.entry.epistemic_status,
        text: detail.entry.text.clone(),
        evidence: detail
            .evidence
            .iter()
            .map(|link| EvidenceLinkDraft {
                chunk_id: link.chunk_id.clone(),
                excerpt: Some(link.excerpt.clone()),
                support_note: link.support_note.clone(),
            })
            .collect(),
        relations: detail
            .relations
            .iter()
            .map(|relation| EntryRelationDraft {
                target_entry_id: relation.target_entry_id.clone(),
                kind: relation.kind,
            })
            .collect(),
        context: detail
            .context
            .iter()
            .map(|context| ResearchContextLinkDraft {
                kind: context.kind,
                context_id: context.context_id.clone(),
                label: context.label.clone(),
            })
            .collect(),
        reason: Some(reason.to_string()),
    }
}

fn require_current_state_revision(
    conn: &Connection,
    project_id: &str,
    expected_revision: i64,
) -> StoreResult<()> {
    let current: i64 = conn
        .query_row(
            "select current_revision from research_state_heads where project_id = ?1",
            params![project_id],
            |row| row.get(0),
        )
        .map_err(|error| {
            if matches!(error, rusqlite::Error::QueryReturnedNoRows) {
                format!("Research State not found for Project: {project_id}")
            } else {
                error.to_string()
            }
        })?;
    if current != expected_revision {
        return Err(format!(
            "Research State changed: expected revision {expected_revision}, current revision is {current}"
        ));
    }
    Ok(())
}

fn insert_state_revision(
    conn: &Connection,
    project_id: &str,
    revision: i64,
    run_id: Option<&str>,
    reason: &str,
) -> StoreResult<()> {
    conn.execute(
        "insert into research_state_revisions
           (project_id, revision, run_id, reason, created_at)
         values (?1, ?2, ?3, ?4, datetime('now'))",
        params![project_id, revision, run_id, reason.trim()],
    )
    .map_err(|error| error.to_string())?;
    Ok(())
}

fn set_current_state_revision(
    conn: &Connection,
    project_id: &str,
    revision: i64,
) -> StoreResult<()> {
    conn.execute(
        "update research_state_heads set current_revision = ?2 where project_id = ?1",
        params![project_id, revision],
    )
    .map_err(|error| error.to_string())?;
    Ok(())
}

fn validate_research_entry_draft(
    conn: &Connection,
    project_id: &str,
    draft: &ResearchEntryDraft,
) -> StoreResult<()> {
    validate_research_entry_draft_with_pending(conn, project_id, draft, &HashSet::new())
}

/// Validate an entry while allowing ids allocated by the same atomic batch.
fn validate_research_entry_draft_with_pending(
    conn: &Connection,
    project_id: &str,
    draft: &ResearchEntryDraft,
    pending_entry_ids: &HashSet<String>,
) -> StoreResult<()> {
    if draft.text.trim().is_empty() {
        return Err("Research Entry text cannot be empty".to_string());
    }
    if draft.text.chars().count() > 4_000 {
        return Err("Research Entry text exceeds 4000 characters".to_string());
    }
    if draft.epistemic_status == EpistemicStatus::SourceSupported {
        if draft.kind != ResearchEntryKind::Finding {
            return Err("Only a Finding may be source-supported".to_string());
        }
        if draft.evidence.is_empty() {
            return Err("A source-supported Finding requires source evidence".to_string());
        }
    } else if !draft.evidence.is_empty() {
        return Err("Direct source evidence belongs only to source-supported Findings".to_string());
    }
    if draft.kind == ResearchEntryKind::Gap
        && !matches!(
            draft.epistemic_status,
            EpistemicStatus::AgentSynthesis | EpistemicStatus::Speculative
        )
    {
        return Err("A Gap must be agent synthesis or speculative".to_string());
    }
    if matches!(
        draft.kind,
        ResearchEntryKind::Hypothesis | ResearchEntryKind::ExperimentIdea
    ) && draft.epistemic_status != EpistemicStatus::Speculative
    {
        return Err("Hypotheses and Experiment Ideas must be speculative".to_string());
    }
    if draft.epistemic_status == EpistemicStatus::AgentSynthesis
        && !draft
            .relations
            .iter()
            .any(|relation| relation.kind == EntryRelationKind::DerivedFrom)
    {
        return Err("Agent synthesis requires a derived-from Research Entry".to_string());
    }

    let mut chunks = std::collections::HashSet::new();
    for evidence in &draft.evidence {
        if !chunks.insert(evidence.chunk_id.trim()) {
            return Err("A Research Entry cannot cite the same chunk twice".to_string());
        }
        let chunk = resolve_evidence_chunk(conn, project_id, evidence)?;
        if let Some(excerpt) = evidence.excerpt.as_deref() {
            if excerpt.trim().is_empty() || !chunk.text.contains(excerpt) {
                return Err(format!(
                    "Evidence excerpt is not an exact substring of chunk: {}",
                    evidence.chunk_id
                ));
            }
        }
    }
    for relation in &draft.relations {
        let owner: Option<String> = conn
            .query_row(
                "select project_id from research_entries where id = ?1",
                params![relation.target_entry_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|error| error.to_string())?;
        if owner.as_deref() != Some(project_id)
            && !pending_entry_ids.contains(&relation.target_entry_id)
        {
            return Err(format!(
                "Related Research Entry is not in this Project: {}",
                relation.target_entry_id
            ));
        }
    }
    for context in &draft.context {
        validate_research_context(conn, project_id, context)?;
    }
    Ok(())
}

fn resolve_local_relation_targets(
    relations: &mut [EntryRelationDraft],
    created_ids: &HashMap<String, String>,
) {
    for relation in relations {
        if let Some(entry_id) = created_ids.get(&relation.target_entry_id) {
            relation.target_entry_id = entry_id.clone();
        }
    }
}

fn normalize_state_statement(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

fn validate_evidence_relationships(
    draft: &ResearchEntryDraft,
    relationships: &[String],
) -> StoreResult<()> {
    if relationships.len() != draft.evidence.len() {
        return Err("Every evidence link requires one relationship".to_string());
    }
    if relationships.iter().any(|value| {
        !matches!(
            value.as_str(),
            "supports" | "contradicts" | "context" | "unspecified"
        )
    }) {
        return Err(
            "Evidence relationship must be supports, contradicts, context, or unspecified"
                .to_string(),
        );
    }
    Ok(())
}

fn set_evidence_relationships(
    conn: &Connection,
    entry_id: &str,
    revision: i64,
    relationships: &[String],
) -> StoreResult<()> {
    for (index, relationship) in relationships.iter().enumerate() {
        conn.execute(
            "update research_evidence_links set relationship = ?4
             where id = ?1 and entry_id = ?2 and state_revision = ?3",
            params![
                format!("{entry_id}:r{revision}:e{index}"),
                entry_id,
                revision,
                relationship
            ],
        )
        .map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn validate_research_context(
    conn: &Connection,
    project_id: &str,
    context: &ResearchContextLinkDraft,
) -> StoreResult<()> {
    if context.context_id.trim().is_empty() || context.label.trim().is_empty() {
        return Err("Research context requires an id and label".to_string());
    }
    let valid = match context.kind {
        ResearchContextKind::ProjectInstruction => context.context_id == project_id,
        ResearchContextKind::ProjectDocument => conn
            .query_row(
                "select exists(
                   select 1 from project_documents where id = ?1 and project_id = ?2
                 )",
                params![context.context_id, project_id],
                |row| row.get::<_, bool>(0),
            )
            .map_err(|error| error.to_string())?,
        ResearchContextKind::ReaderNote => conn
            .query_row(
                "select exists(
                   select 1 from highlights h
                   join vault_papers vp on vp.paper_id = h.paper_id
                   join vaults v on v.id = vp.vault_id
                   where h.id = ?1 and h.note is not null and trim(h.note) <> ''
                     and v.project_id = ?2
                 )",
                params![context.context_id, project_id],
                |row| row.get::<_, bool>(0),
            )
            .map_err(|error| error.to_string())?,
        ResearchContextKind::ChatTurn => conn
            .query_row(
                "select exists(
                   select 1 from chat_entries e
                   join chat_threads t on t.id = e.thread_id
                   where e.id = ?1 and (
                     (t.scope_kind = 'paper' and exists(
                       select 1 from vault_papers vp join vaults v on v.id = vp.vault_id
                       where vp.paper_id = t.scope_id and v.project_id = ?2
                     )) or
                     (t.scope_kind = 'vault' and exists(
                       select 1 from vaults v where v.id = t.scope_id and v.project_id = ?2
                     )) or
                     (t.scope_kind = 'project' and t.scope_id = ?2)
                   )
                 )",
                params![context.context_id, project_id],
                |row| row.get::<_, bool>(0),
            )
            .map_err(|error| error.to_string())?,
    };
    if !valid {
        return Err(format!(
            "Research context does not resolve inside this Project: {}",
            context.context_id
        ));
    }
    Ok(())
}

fn resolve_evidence_chunk(
    conn: &Connection,
    project_id: &str,
    draft: &EvidenceLinkDraft,
) -> StoreResult<DocumentChunk> {
    let chunks = read_chunks(
        conn,
        "where c.id = ?1 and exists(
           select 1 from vault_papers vp join vaults v on v.id = vp.vault_id
           where vp.paper_id = c.paper_id and v.project_id = ?2
         )",
        params![draft.chunk_id.trim(), project_id],
    )?;
    chunks.into_iter().next().ok_or_else(|| {
        format!(
            "Evidence chunk does not resolve inside this Project Vault: {}",
            draft.chunk_id
        )
    })
}

fn insert_new_research_entry(
    conn: &Connection,
    entry_id: &str,
    project_id: &str,
    revision: i64,
    run_id: Option<&str>,
    draft: &ResearchEntryDraft,
) -> StoreResult<()> {
    insert_research_entry_header(conn, entry_id, project_id, revision, run_id, draft)?;
    insert_research_entry_version(
        conn,
        entry_id,
        project_id,
        revision,
        EntryLifecycle::Active,
        run_id,
        draft,
    )
}

/// Insert the stable identity for a new entry before its revision payload.
fn insert_research_entry_header(
    conn: &Connection,
    entry_id: &str,
    project_id: &str,
    revision: i64,
    run_id: Option<&str>,
    draft: &ResearchEntryDraft,
) -> StoreResult<()> {
    conn.execute(
        "insert into research_entries (
           id, project_id, kind, epistemic_status, text, lifecycle,
           first_revision, last_revision, origin_run_id, created_at, updated_at
         ) values (?1, ?2, ?3, ?4, ?5, 'active', ?6, ?6, ?7, datetime('now'), datetime('now'))",
        params![
            entry_id,
            project_id,
            draft.kind.as_str(),
            draft.epistemic_status.as_str(),
            draft.text.trim(),
            revision,
            run_id
        ],
    )
    .map_err(|error| error.to_string())?;
    Ok(())
}

fn insert_research_entry_version(
    conn: &Connection,
    entry_id: &str,
    project_id: &str,
    revision: i64,
    lifecycle: EntryLifecycle,
    run_id: Option<&str>,
    draft: &ResearchEntryDraft,
) -> StoreResult<()> {
    conn.execute(
        "insert into research_entry_revisions (
           entry_id, project_id, state_revision, kind, epistemic_status, text,
           lifecycle, origin_run_id, reason, created_at
         ) values (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, datetime('now'))",
        params![
            entry_id,
            project_id,
            revision,
            draft.kind.as_str(),
            draft.epistemic_status.as_str(),
            draft.text.trim(),
            lifecycle.as_str(),
            run_id,
            manual_reason(draft.reason.as_deref())
        ],
    )
    .map_err(|error| error.to_string())?;

    for (index, evidence) in draft.evidence.iter().enumerate() {
        let chunk = resolve_evidence_chunk(conn, project_id, evidence)?;
        let excerpt: String = evidence
            .excerpt
            .clone()
            .unwrap_or_else(|| chunk.text.chars().take(2_000).collect());
        let excerpt_byte_start = chunk.text.find(&excerpt).unwrap_or(0);
        let excerpt_char_start = chunk.text[..excerpt_byte_start].chars().count() as i64;
        let source_start = chunk.source_start + excerpt_char_start;
        let source_end = source_start + excerpt.chars().count() as i64;
        conn.execute(
            "insert into research_evidence_links (
               id, entry_id, state_revision, paper_id, source_id, extraction_id,
               chunk_id, excerpt, source_start, source_end, page_start, page_end,
               support_note
             ) values (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            params![
                format!("{entry_id}:r{revision}:e{index}"),
                entry_id,
                revision,
                chunk.paper_id,
                chunk.source_id,
                chunk.extraction_id,
                chunk.id,
                excerpt,
                source_start,
                source_end,
                chunk.page_start,
                chunk.page_end,
                evidence.support_note.as_deref().map(str::trim)
            ],
        )
        .map_err(|error| error.to_string())?;
    }
    for (index, relation) in draft.relations.iter().enumerate() {
        conn.execute(
            "insert into research_entry_relations (
               id, entry_id, state_revision, target_entry_id, kind
             ) values (?1, ?2, ?3, ?4, ?5)",
            params![
                format!("{entry_id}:r{revision}:relation:{index}"),
                entry_id,
                revision,
                relation.target_entry_id,
                relation.kind.as_str()
            ],
        )
        .map_err(|error| error.to_string())?;
    }
    for (index, context) in draft.context.iter().enumerate() {
        conn.execute(
            "insert into research_context_links (
               id, entry_id, state_revision, kind, context_id, label
             ) values (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                format!("{entry_id}:r{revision}:context:{index}"),
                entry_id,
                revision,
                context.kind.as_str(),
                context.context_id.trim(),
                context.label.trim()
            ],
        )
        .map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn read_research_state(
    conn: &Connection,
    project_id: &str,
    requested_revision: Option<i64>,
) -> StoreResult<ResearchStateSnapshot> {
    let current_revision: i64 = conn
        .query_row(
            "select current_revision from research_state_heads where project_id = ?1",
            params![project_id],
            |row| row.get(0),
        )
        .map_err(|error| {
            if matches!(error, rusqlite::Error::QueryReturnedNoRows) {
                format!("Research State not found for Project: {project_id}")
            } else {
                error.to_string()
            }
        })?;
    let revision = requested_revision.unwrap_or(current_revision);
    let exists: bool = conn
        .query_row(
            "select exists(
               select 1 from research_state_revisions where project_id = ?1 and revision = ?2
             )",
            params![project_id, revision],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    if !exists {
        return Err(format!(
            "Research State revision not found for Project {project_id}: {revision}"
        ));
    }

    let mut revisions_statement = conn
        .prepare(
            "select project_id, revision, run_id, reason, created_at
             from research_state_revisions where project_id = ?1
             order by revision desc",
        )
        .map_err(|error| error.to_string())?;
    let revision_rows = revisions_statement
        .query_map(params![project_id], |row| {
            Ok(ResearchStateRevision {
                project_id: row.get(0)?,
                revision: row.get(1)?,
                run_id: row.get(2)?,
                reason: row.get(3)?,
                created_at: row.get(4)?,
            })
        })
        .map_err(|error| error.to_string())?;
    let revisions = collect_rows(revision_rows)?;
    let entries = read_research_entry_summaries(conn, project_id, revision)?;
    Ok(ResearchStateSnapshot {
        project_id: project_id.to_string(),
        revision,
        current_revision,
        revisions,
        entries,
    })
}

const RESEARCH_ENTRY_SUMMARY_COLUMNS: &str =
    "e.id, e.project_id, r.kind, r.epistemic_status, r.text, r.lifecycle,
     e.first_revision, r.state_revision, r.origin_run_id,
     (select count(*) from research_evidence_links l
      where l.entry_id = e.id and l.state_revision = r.state_revision),
     (select count(*) from research_entry_relations l
      where l.entry_id = e.id and l.state_revision = r.state_revision),
     (select count(*) from research_context_links l
      where l.entry_id = e.id and l.state_revision = r.state_revision),
     e.created_at, r.created_at";

fn read_research_entry_summaries(
    conn: &Connection,
    project_id: &str,
    revision: i64,
) -> StoreResult<Vec<ResearchEntrySummary>> {
    let sql = format!(
        "select {RESEARCH_ENTRY_SUMMARY_COLUMNS}
         from research_entries e
         join research_entry_revisions r on r.entry_id = e.id
          and r.state_revision = (
            select max(latest.state_revision) from research_entry_revisions latest
            where latest.entry_id = e.id and latest.state_revision <= ?2
          )
         where e.project_id = ?1 and e.first_revision <= ?2
         order by r.state_revision desc, e.id"
    );
    let mut statement = conn.prepare(&sql).map_err(|error| error.to_string())?;
    let rows = statement
        .query_map(
            params![project_id, revision],
            research_entry_summary_from_row,
        )
        .map_err(|error| error.to_string())?;
    collect_rows(rows)
}

fn read_current_research_entry(
    conn: &Connection,
    entry_id: &str,
) -> StoreResult<ResearchEntrySummary> {
    let revision: i64 = conn
        .query_row(
            "select h.current_revision from research_entries e
             join research_state_heads h on h.project_id = e.project_id
             where e.id = ?1",
            params![entry_id],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    read_research_entry_summary_at(conn, entry_id, revision)
}

fn read_research_entry_summary_at(
    conn: &Connection,
    entry_id: &str,
    revision: i64,
) -> StoreResult<ResearchEntrySummary> {
    let sql = format!(
        "select {RESEARCH_ENTRY_SUMMARY_COLUMNS}
         from research_entries e
         join research_entry_revisions r on r.entry_id = e.id
          and r.state_revision = (
            select max(latest.state_revision) from research_entry_revisions latest
            where latest.entry_id = e.id and latest.state_revision <= ?2
          )
         where e.id = ?1 and e.first_revision <= ?2"
    );
    conn.query_row(
        &sql,
        params![entry_id, revision],
        research_entry_summary_from_row,
    )
    .map_err(|error| {
        if matches!(error, rusqlite::Error::QueryReturnedNoRows) {
            format!("Research Entry not found at revision {revision}: {entry_id}")
        } else {
            error.to_string()
        }
    })
}

fn research_entry_summary_from_row(row: &rusqlite::Row) -> rusqlite::Result<ResearchEntrySummary> {
    let kind: String = row.get(2)?;
    let epistemic_status: String = row.get(3)?;
    let lifecycle: String = row.get(5)?;
    Ok(ResearchEntrySummary {
        id: row.get(0)?,
        project_id: row.get(1)?,
        kind: parse_sql_enum(2, &kind, ResearchEntryKind::parse)?,
        epistemic_status: parse_sql_enum(3, &epistemic_status, EpistemicStatus::parse)?,
        text: row.get(4)?,
        lifecycle: parse_sql_enum(5, &lifecycle, EntryLifecycle::parse)?,
        first_revision: row.get(6)?,
        last_revision: row.get(7)?,
        origin_run_id: row.get(8)?,
        evidence_count: row.get(9)?,
        relation_count: row.get(10)?,
        context_count: row.get(11)?,
        created_at: row.get(12)?,
        updated_at: row.get(13)?,
    })
}

fn parse_sql_enum<T>(
    column: usize,
    value: &str,
    parse: impl FnOnce(&str) -> Result<T, String>,
) -> rusqlite::Result<T> {
    parse(value).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(
            column,
            rusqlite::types::Type::Text,
            Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, error)),
        )
    })
}

fn read_research_entry_detail(
    conn: &Connection,
    entry_id: &str,
    requested_revision: Option<i64>,
) -> StoreResult<ResearchEntryDetail> {
    read_research_entry_detail_from_conn(conn, entry_id, requested_revision)
}

fn read_research_entry_detail_from_conn(
    conn: &Connection,
    entry_id: &str,
    requested_revision: Option<i64>,
) -> StoreResult<ResearchEntryDetail> {
    let current = read_current_research_entry(conn, entry_id)?;
    let revision = match requested_revision {
        Some(revision) => revision,
        None => conn
            .query_row(
                "select current_revision from research_state_heads where project_id = ?1",
                params![current.project_id],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?,
    };
    let entry = read_research_entry_summary_at(conn, entry_id, revision)?;
    let version = entry.last_revision;

    let mut evidence_statement = conn
        .prepare(
            "select link.id, link.entry_id, link.state_revision, link.paper_id,
                    paper.title, link.source_id, link.extraction_id, link.chunk_id,
                    link.excerpt, link.source_start, link.source_end, link.page_start,
                    link.page_end, link.support_note, link.relationship
             from research_evidence_links link
             join papers paper on paper.id = link.paper_id
             where link.entry_id = ?1 and link.state_revision = ?2
             order by link.id",
        )
        .map_err(|error| error.to_string())?;
    let evidence_rows = evidence_statement
        .query_map(params![entry_id, version], |row| {
            Ok(ResearchEvidenceLink {
                id: row.get(0)?,
                entry_id: row.get(1)?,
                state_revision: row.get(2)?,
                paper_id: row.get(3)?,
                paper_title: row.get(4)?,
                source_id: row.get(5)?,
                extraction_id: row.get(6)?,
                chunk_id: row.get(7)?,
                excerpt: row.get(8)?,
                source_start: row.get(9)?,
                source_end: row.get(10)?,
                page_start: row.get(11)?,
                page_end: row.get(12)?,
                support_note: row.get(13)?,
                relationship: row.get(14)?,
            })
        })
        .map_err(|error| error.to_string())?;
    let evidence = collect_rows(evidence_rows)?;

    let mut relation_statement = conn
        .prepare(
            "select id, entry_id, state_revision, target_entry_id, kind
             from research_entry_relations where entry_id = ?1 and state_revision = ?2
             order by id",
        )
        .map_err(|error| error.to_string())?;
    let relation_rows = relation_statement
        .query_map(params![entry_id, version], |row| {
            let kind: String = row.get(4)?;
            Ok(EntryRelation {
                id: row.get(0)?,
                entry_id: row.get(1)?,
                state_revision: row.get(2)?,
                target_entry_id: row.get(3)?,
                kind: parse_sql_enum(4, &kind, EntryRelationKind::parse)?,
            })
        })
        .map_err(|error| error.to_string())?;
    let relations = collect_rows(relation_rows)?;

    let mut context_statement = conn
        .prepare(
            "select id, entry_id, state_revision, kind, context_id, label
             from research_context_links where entry_id = ?1 and state_revision = ?2
             order by id",
        )
        .map_err(|error| error.to_string())?;
    let context_rows = context_statement
        .query_map(params![entry_id, version], |row| {
            let kind: String = row.get(3)?;
            Ok(ResearchContextLink {
                id: row.get(0)?,
                entry_id: row.get(1)?,
                state_revision: row.get(2)?,
                kind: parse_sql_enum(3, &kind, ResearchContextKind::parse)?,
                context_id: row.get(4)?,
                label: row.get(5)?,
            })
        })
        .map_err(|error| error.to_string())?;
    let context = collect_rows(context_rows)?;

    let mut history_statement = conn
        .prepare(
            "select entry_id, state_revision, kind, epistemic_status, text, lifecycle,
                    origin_run_id, reason, created_at
             from research_entry_revisions
             where entry_id = ?1 and state_revision <= ?2 order by state_revision desc",
        )
        .map_err(|error| error.to_string())?;
    let history_rows = history_statement
        .query_map(params![entry_id, revision], |row| {
            let kind: String = row.get(2)?;
            let status: String = row.get(3)?;
            let lifecycle: String = row.get(5)?;
            Ok(ResearchEntryVersion {
                entry_id: row.get(0)?,
                state_revision: row.get(1)?,
                kind: parse_sql_enum(2, &kind, ResearchEntryKind::parse)?,
                epistemic_status: parse_sql_enum(3, &status, EpistemicStatus::parse)?,
                text: row.get(4)?,
                lifecycle: parse_sql_enum(5, &lifecycle, EntryLifecycle::parse)?,
                origin_run_id: row.get(6)?,
                reason: row.get(7)?,
                created_at: row.get(8)?,
            })
        })
        .map_err(|error| error.to_string())?;
    let history = collect_rows(history_rows)?;

    Ok(ResearchEntryDetail {
        entry,
        evidence,
        relations,
        context,
        history,
    })
}

fn append_harness_event(
    conn: &Connection,
    run_id: &str,
    kind: &str,
    summary: &str,
) -> StoreResult<()> {
    let bounded_summary: String = summary.chars().take(500).collect();
    append_structured_harness_event(
        conn,
        run_id,
        kind,
        &bounded_summary,
        None,
        None,
        None,
        None,
        "harness",
    )
}

/// Appends one bounded, typed Activity event in Run sequence order.
fn append_structured_harness_event(
    conn: &Connection,
    run_id: &str,
    kind: &str,
    summary: &str,
    detail: Option<serde_json::Value>,
    phase: Option<&str>,
    progress_current: Option<i64>,
    progress_total: Option<i64>,
    actor: &str,
) -> StoreResult<()> {
    if kind.is_empty() || kind.chars().count() > 80 {
        return Err("Activity event kind must contain at most 80 characters".to_string());
    }
    if summary.is_empty() || summary.chars().count() > 500 {
        return Err("Activity event summary must contain at most 500 characters".to_string());
    }
    if phase.is_some_and(|value| value.is_empty() || value.chars().count() > 80) {
        return Err("Activity phase must contain at most 80 characters".to_string());
    }
    if !matches!(actor, "researcher" | "harness" | "scheduler" | "system") {
        return Err(format!("Unknown Activity actor: {actor}"));
    }
    if progress_current.is_some_and(|value| value < 0)
        || progress_total.is_some_and(|value| value < 0)
        || matches!((progress_current, progress_total), (Some(current), Some(total)) if current > total)
    {
        return Err(
            "Activity progress must be non-negative and current cannot exceed total".to_string(),
        );
    }
    let detail_json = detail
        .as_ref()
        .map(|value| serde_json::to_string(&value).map_err(|error| error.to_string()))
        .transpose()?;
    if detail_json
        .as_ref()
        .is_some_and(|value| value.chars().count() > 4_000)
    {
        return Err("Activity detail must contain at most 4000 characters".to_string());
    }
    validate_harness_event_detail(kind, detail.as_ref())?;
    conn.busy_timeout(Duration::from_secs(5))
        .map_err(|error| error.to_string())?;
    conn.execute(
        "with next_event(sequence) as (
           select coalesce(max(sequence), 0) + 1
           from harness_events where run_id = ?1
         )
         insert into harness_events (
           id, run_id, sequence, kind, summary, detail_json, phase,
           progress_current, progress_total, actor, occurred_at
         )
         select ?1 || ':event:' || sequence, ?1, sequence, ?2, ?3, ?4,
                ?5, ?6, ?7, ?8, datetime('now')
         from next_event",
        params![
            run_id,
            kind,
            summary,
            detail_json,
            phase,
            progress_current,
            progress_total,
            actor
        ],
    )
    .map_err(|error| error.to_string())?;
    Ok(())
}

/// Validates the schema of bounded structured detail for known Activity kinds.
fn validate_harness_event_detail(
    kind: &str,
    detail: Option<&serde_json::Value>,
) -> StoreResult<()> {
    let Some(detail) = detail else {
        return Ok(());
    };
    let fields: &[(&str, &str)] = match kind {
        "iteration_planned" => &[("iteration", "integer")],
        "provider_query_started" => &[("provider", "string"), ("query", "string")],
        "provider_query_completed" => &[("provider", "string"), ("candidateCount", "integer")],
        "provider_query_failed" => &[("provider", "string")],
        "candidates_deduplicated" => &[("uniqueCandidates", "integer")],
        "candidates_inspected" | "candidate_metadata_resolving" | "candidates_ranking" => {
            &[("candidateCount", "integer")]
        }
        "candidate_decided" => &[
            ("candidateId", "string"),
            ("decision", "string"),
            ("reason", "string"),
            ("relevanceConfidence", "number"),
            ("withinScope", "boolean"),
        ],
        "vault_membership_changed" => &[("paperIds", "string_array"), ("vaultRevision", "integer")],
        "change_set_applied" => &[
            ("changeSetId", "string"),
            ("resultingStateRevision", "integer"),
            ("resultingVaultRevision", "integer"),
            ("acceptedCandidateCount", "integer"),
            ("researchEntryCount", "integer"),
        ],
        "usage_recorded" => &[
            ("providerQueries", "integer"),
            ("llmCalls", "integer"),
            ("iterations", "integer"),
            ("inspectedCandidates", "integer"),
        ],
        "reconciliation_usage_recorded" => &[("llmCalls", "integer"), ("totalLlmCalls", "integer")],
        "stop_decided" => &[
            ("status", "string"),
            ("stopReason", "nullable_string"),
            ("complete", "boolean"),
            ("converged", "boolean"),
        ],
        "checkpoint_restored" => &[
            ("checkpointStateRevision", "integer"),
            ("previousCurrentRevision", "integer"),
            ("resultingRevision", "integer"),
            ("supersededEntryIds", "string_array"),
        ],
        "operational_observation" => &[
            ("observationId", "string"),
            ("kind", "string"),
            ("signature", "string"),
            ("severity", "number"),
            ("confidence", "number"),
            ("target", "nullable_string"),
            ("proposalEligible", "boolean"),
        ],
        "harness_reflection_recorded" => &[
            ("reflectionId", "string"),
            ("observationCount", "integer"),
            ("nextDirection", "nullable_string"),
        ],
        "agent_passages_delivered" => &[("paperId", "string"), ("passageRefs", "string_array")],
        "agent_evidence_question_started" => {
            &[("paperCount", "integer"), ("returnedTextChars", "integer")]
        }
        "agent_evidence_question_answered" => &[("passageRefs", "string_array")],
        "agent_state_committed" => &[
            ("revision", "integer"),
            ("affectedEntryIds", "string_array"),
        ],
        "agent_state_unchanged" => &[("reason", "string")],
        "agent_papers_assessed" => &[
            ("attempted", "integer"),
            ("read", "integer"),
            ("unavailable", "integer"),
            ("removed", "integer"),
            ("retained", "integer"),
        ],
        _ => {
            return Err(format!(
                "Activity event kind does not define structured detail: {kind}"
            ));
        }
    };
    let object = detail
        .as_object()
        .ok_or_else(|| format!("Activity detail for {kind} must be an object"))?;
    for (field, expected) in fields {
        let value = object
            .get(*field)
            .ok_or_else(|| format!("Activity detail for {kind} is missing {field}"))?;
        let valid = match *expected {
            "string" => value.as_str().is_some_and(|text| !text.is_empty()),
            "nullable_string" => value.is_null() || value.as_str().is_some(),
            "integer" => value.as_i64().is_some() || value.as_u64().is_some(),
            "number" => value.as_f64().is_some(),
            "boolean" => value.as_bool().is_some(),
            "string_array" => value.as_array().is_some_and(|items| {
                items
                    .iter()
                    .all(|item| item.as_str().is_some_and(|text| !text.is_empty()))
            }),
            _ => false,
        };
        if !valid {
            return Err(format!(
                "Activity detail field {field} for {kind} must be {expected}"
            ));
        }
    }
    Ok(())
}

fn append_harness_control_event(
    conn: &Connection,
    project_id: &str,
    kind: &str,
    summary: &str,
) -> StoreResult<()> {
    let sequence: i64 = conn
        .query_row(
            "select coalesce(max(sequence), 0) + 1 from harness_control_events where project_id = ?1",
            params![project_id],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    let id = format!("{project_id}:control:{sequence}");
    conn.execute(
        "insert into harness_control_events
         (id, project_id, sequence, kind, summary, occurred_at)
         values (?1, ?2, ?3, ?4, ?5, datetime('now'))",
        params![id, project_id, sequence, kind, summary],
    )
    .map_err(|error| error.to_string())?;
    Ok(())
}

fn terminal_stop_reason(
    conn: &Connection,
    project_id: &str,
    configuration: &HarnessConfiguration,
    now: DateTime<Utc>,
) -> StoreResult<Option<String>> {
    let (cycles, unproductive): (i64, i64) = conn
        .query_row(
            "select completed_cycle_count, consecutive_unproductive_runs
             from research_harnesses where project_id = ?1",
            params![project_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|error| error.to_string())?;
    let limits = &configuration.stop_conditions;
    if limits.maximum_cycles.is_some_and(|limit| cycles >= limit) {
        return Ok(Some("maximum_cycles_reached".to_string()));
    }
    if limits
        .maximum_unproductive_runs
        .is_some_and(|limit| unproductive >= limit)
    {
        return Ok(Some("maximum_unproductive_runs_reached".to_string()));
    }
    if let Some(end_at) = limits.end_at.as_deref() {
        let end = DateTime::parse_from_rfc3339(end_at)
            .map_err(|_| "Harness end time must be RFC 3339".to_string())?
            .with_timezone(&Utc);
        if now >= end {
            return Ok(Some("end_at_reached".to_string()));
        }
    }
    if limits.stop_on_convergence {
        let converged: bool = conn
            .query_row(
                "select exists(select 1 from harness_runs where project_id = ?1
                 and status = 'ready' and stop_reason in ('converged','target_reached'))",
                params![project_id],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        if converged {
            return Ok(Some("convergence_reached".to_string()));
        }
    }
    Ok(None)
}

fn settle_harness_after_run(
    conn: &Connection,
    project_id: &str,
    run_id: &str,
    productive: bool,
) -> StoreResult<()> {
    conn.execute(
        "update research_harnesses
         set status = requested_post_run_status,
             completed_cycle_count = completed_cycle_count + 1,
             consecutive_unproductive_runs = case when ?2 then 0
               else consecutive_unproductive_runs + 1 end,
             updated_at = datetime('now') where project_id = ?1",
        params![project_id, productive],
    )
    .map_err(|error| error.to_string())?;
    let configuration_json: String = conn
        .query_row(
            "select configuration_json from research_harnesses where project_id = ?1",
            params![project_id],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    let configuration: HarnessConfiguration =
        serde_json::from_str(&configuration_json).map_err(|error| error.to_string())?;
    if let Some(reason) = terminal_stop_reason(conn, project_id, &configuration, Utc::now())? {
        conn.execute(
            "update research_harnesses set status = 'stopped', schedule_enabled = 0,
             next_run_at = null, requested_post_run_status = 'stopped',
             terminal_stop_reason = ?2, updated_at = datetime('now') where project_id = ?1",
            params![project_id, reason],
        )
        .map_err(|error| error.to_string())?;
        append_harness_event(conn, run_id, "harness_stopped", &reason)?;
    }
    Ok(())
}

fn validate_reflection_draft(draft: &HarnessReflectionDraft) -> StoreResult<()> {
    if draft.summary.trim().is_empty() || draft.summary.chars().count() > 2_000 {
        return Err("Harness reflection summary must contain 1 to 2000 characters".to_string());
    }
    if draft.observations.len() > 20 {
        return Err("Harness reflection exceeds 20 observations".to_string());
    }
    serde_json::from_str::<serde_json::Value>(&draft.metrics_json)
        .map_err(|_| "Harness reflection metrics must be valid JSON".to_string())?;
    for observation in &draft.observations {
        if normalize_signature(&observation.signature).is_empty()
            || observation.description.trim().is_empty()
            || observation.description.chars().count() > 1_000
            || !(0.0..=1.0).contains(&observation.severity)
            || !(0.0..=1.0).contains(&observation.confidence)
        {
            return Err("Harness observation is outside its bounded schema".to_string());
        }
        serde_json::from_str::<serde_json::Value>(&observation.metrics_json)
            .map_err(|_| "Harness observation metrics must be valid JSON".to_string())?;
        if observation.proposal_eligible {
            let target = observation
                .target
                .ok_or_else(|| "Proposal-eligible observation requires a target".to_string())?;
            let value = observation.proposed_value.as_ref().ok_or_else(|| {
                "Proposal-eligible observation requires an exact proposed value".to_string()
            })?;
            normalize_improvement_value(target, value)?;
        } else if observation.target.is_some() != observation.proposed_value.is_some() {
            return Err("Observation target and proposed value must appear together".to_string());
        }
    }
    Ok(())
}

fn validate_research_run_outcome_shape(outcome: &ResearchRunOutcome) -> StoreResult<()> {
    validate_outcome_text("summary", &outcome.summary, 2_000)?;
    if outcome.display_items.len() > 20
        || outcome.paper_dispositions.len() > 100
        || outcome.task_outcomes.len() > 20
        || outcome.unanswered_questions.len() > 20
    {
        return Err("Research outcome exceeds its item limits".to_string());
    }
    for item in &outcome.display_items {
        validate_outcome_text("display item", &item.text, 1_000)?;
    }
    for decision in &outcome.paper_dispositions {
        validate_outcome_text("paper id", &decision.paper_id, 500)?;
        validate_outcome_text("paper disposition reason", &decision.reason, 500)?;
    }
    for question in &outcome.unanswered_questions {
        validate_outcome_text("unanswered question", question, 1_000)?;
    }
    if let Some(direction) = &outcome.next_direction {
        validate_outcome_text("next direction", direction, 1_000)?;
    }
    for task in &outcome.task_outcomes {
        if task.learned_points.is_empty()
            || task.search_run_ids.len() > 12
            || task.motivating_entry_ids.len() > 20
            || task.learned_points.len() > 20
            || task.cited_passage_refs.len() > 40
        {
            return Err("Research task outcome is outside its item limits".to_string());
        }
        for id in task
            .search_run_ids
            .iter()
            .chain(&task.motivating_entry_ids)
            .chain(&task.cited_passage_refs)
        {
            validate_outcome_text("reference", id, 500)?;
        }
        for point in &task.learned_points {
            validate_outcome_text("learned point", point, 1_000)?;
        }
    }
    let serialized_chars = serde_json::to_string(outcome)
        .map_err(|error| error.to_string())?
        .chars()
        .count();
    if serialized_chars > crate::services::research::synthesis::MAX_PROPOSAL_BYTES * 2 {
        return Err("Research outcome exceeds storage limit".to_string());
    }
    Ok(())
}

fn validate_research_state_synthesis_shape(
    synthesis: &ResearchStateSynthesis,
    next_direction: Option<&str>,
) -> StoreResult<()> {
    crate::services::research::synthesis::validate_shape(synthesis, next_direction)
}

fn validate_outcome_text(label: &str, value: &str, maximum_chars: usize) -> StoreResult<()> {
    if value.trim().is_empty() || value.chars().count() > maximum_chars {
        return Err(format!(
            "Research outcome {label} must contain 1 to {maximum_chars} characters"
        ));
    }
    Ok(())
}

/// Reject a new managed-agent mutation after cancellation has closed admission.
fn require_agent_run_write_admission(
    conn: &Connection,
    run_id: Option<&str>,
    project_id: Option<&str>,
) -> StoreResult<()> {
    let Some(run_id) = run_id else {
        return Ok(());
    };
    let admitted: bool = conn
        .query_row(
            "select exists(select 1 from harness_runs
             where id = ?1 and execution_kind = 'codex_agent'
               and (?2 is null or project_id = ?2)
               and status in ('queued','planning','searching','assessing','ranking','reconciling'))",
            params![run_id, project_id],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    if !admitted {
        return Err("Managed Research Run no longer accepts writes".to_string());
    }
    Ok(())
}

fn normalize_signature(value: &str) -> String {
    value
        .trim()
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join("-")
        .chars()
        .filter(|character| character.is_ascii_alphanumeric() || *character == '-')
        .take(120)
        .collect()
}

fn normalize_concepts(values: &[String]) -> StoreResult<Vec<String>> {
    let mut concepts = Vec::new();
    for value in values {
        let normalized = value.trim().to_lowercase();
        if normalized.is_empty() || normalized.chars().count() > 80 {
            return Err("Harness improvement concepts must contain 1 to 80 characters".to_string());
        }
        if !concepts.contains(&normalized) {
            concepts.push(normalized);
        }
    }
    if concepts.len() > 30 {
        return Err("Harness improvement exceeds 30 concepts".to_string());
    }
    Ok(concepts)
}

fn normalize_improvement_value(
    target: HarnessImprovementTarget,
    value: &HarnessImprovementValue,
) -> StoreResult<HarnessImprovementValue> {
    match (target, value) {
        (
            HarnessImprovementTarget::PreferredConcepts
            | HarnessImprovementTarget::ExcludedConcepts,
            HarnessImprovementValue::Concepts(values),
        ) => Ok(HarnessImprovementValue::Concepts(normalize_concepts(
            values,
        )?)),
        (
            HarnessImprovementTarget::MetadataResolvers,
            HarnessImprovementValue::MetadataResolvers(values),
        ) => {
            let mut normalized = values
                .iter()
                .map(|value| value.trim().to_lowercase())
                .filter(|value| !value.is_empty())
                .collect::<Vec<_>>();
            normalized.sort();
            normalized.dedup();
            if normalized
                .iter()
                .any(|value| !matches!(value.as_str(), "open_alex" | "arxiv"))
            {
                return Err("Only OpenAlex and arXiv metadata resolvers may be changed".to_string());
            }
            Ok(HarnessImprovementValue::MetadataResolvers(normalized))
        }
        _ => Err("Harness improvement value does not match its allow-listed target".to_string()),
    }
}

fn configuration_value(
    configuration: &HarnessConfiguration,
    target: HarnessImprovementTarget,
) -> HarnessImprovementValue {
    match target {
        HarnessImprovementTarget::PreferredConcepts => {
            HarnessImprovementValue::Concepts(configuration.preferred_concepts.clone())
        }
        HarnessImprovementTarget::ExcludedConcepts => {
            HarnessImprovementValue::Concepts(configuration.excluded_concepts.clone())
        }
        HarnessImprovementTarget::MetadataResolvers => HarnessImprovementValue::MetadataResolvers(
            configuration
                .sources
                .iter()
                .filter(|source| matches!(source.as_str(), "open_alex" | "arxiv"))
                .cloned()
                .collect(),
        ),
    }
}

fn apply_improvement_value(
    configuration: &mut HarnessConfiguration,
    target: HarnessImprovementTarget,
    value: &HarnessImprovementValue,
) -> StoreResult<()> {
    let normalized = normalize_improvement_value(target, value)?;
    match (target, normalized) {
        (
            HarnessImprovementTarget::PreferredConcepts,
            HarnessImprovementValue::Concepts(values),
        ) => {
            configuration.preferred_concepts = values;
        }
        (HarnessImprovementTarget::ExcludedConcepts, HarnessImprovementValue::Concepts(values)) => {
            configuration.excluded_concepts = values;
        }
        (
            HarnessImprovementTarget::MetadataResolvers,
            HarnessImprovementValue::MetadataResolvers(values),
        ) => {
            configuration.sources = std::iter::once("browser".to_string())
                .chain(values)
                .collect();
        }
        _ => unreachable!("normalization checked target/value pairing"),
    }
    Ok(())
}

fn improvement_preview(
    target: HarnessImprovementTarget,
    value: &HarnessImprovementValue,
) -> String {
    let values = match value {
        HarnessImprovementValue::Concepts(values)
        | HarnessImprovementValue::MetadataResolvers(values) => values.join(", "),
    };
    match target {
        HarnessImprovementTarget::PreferredConcepts => {
            format!("Future queries will foreground: {values}")
        }
        HarnessImprovementTarget::ExcludedConcepts => {
            format!("Future queries will avoid: {values}")
        }
        HarnessImprovementTarget::MetadataResolvers => {
            format!("Browser results will resolve metadata with: {values}")
        }
    }
}

fn detect_harness_improvement(
    conn: &Connection,
    project_id: &str,
    signature: &str,
    target: HarnessImprovementTarget,
) -> StoreResult<()> {
    let mut statement = conn
        .prepare(
            "select o.id, o.run_id, o.description, o.proposed_value_json
             from harness_observations o
             where o.project_id = ?1 and o.signature = ?2 and o.target = ?3
               and o.proposal_eligible = 1
               and o.run_id in (
                 select id from harness_runs where project_id = ?1 and status = 'ready'
                 order by finished_at desc, id desc limit 10
               )
               and not exists (
                 select 1 from harness_improvement_observations link
                 where link.observation_id = o.id
               )
             order by o.created_at desc, o.id desc",
        )
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map(params![project_id, signature, target.as_str()], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        })
        .map_err(|error| error.to_string())?;
    let observations = collect_rows(rows)?;
    let run_ids = observations
        .iter()
        .map(|item| item.1.clone())
        .collect::<std::collections::HashSet<_>>();
    if run_ids.len() < 3 {
        return Ok(());
    }
    let proposed_json = &observations[0].3;
    if observations.iter().any(|item| &item.3 != proposed_json) {
        return Ok(());
    }
    let proposed: HarnessImprovementValue =
        serde_json::from_str(proposed_json).map_err(|error| error.to_string())?;
    let proposed = normalize_improvement_value(target, &proposed)?;
    let harness = read_research_harness(conn, project_id)?
        .ok_or_else(|| "Research Harness not found".to_string())?;
    let before = configuration_value(&harness.configuration, target);
    if before == proposed {
        return Ok(());
    }
    let fingerprint_source = format!(
        "{project_id}:{signature}:{}:{}:{}",
        target.as_str(),
        harness.configuration_version,
        serde_json::to_string(&proposed).map_err(|error| error.to_string())?
    );
    let fingerprint = short_sha256(&fingerprint_source);
    let id = format!("harness_improvement:{fingerprint}");
    let inserted = conn
        .execute(
            "insert into harness_improvements
             (id, project_id, status, target, base_configuration_version,
              before_value_json, proposed_value_json, rationale, expected_effect,
              policy_version, fingerprint, created_at)
             values (?1, ?2, 'proposed', ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, datetime('now'))
             on conflict(fingerprint) do nothing",
            params![
                id,
                project_id,
                target.as_str(),
                harness.configuration_version,
                serde_json::to_string(&before).map_err(|error| error.to_string())?,
                serde_json::to_string(&proposed).map_err(|error| error.to_string())?,
                observations[0].2,
                improvement_preview(target, &proposed),
                PROPOSAL_POLICY_VERSION,
                fingerprint
            ],
        )
        .map_err(|error| error.to_string())?;
    if inserted == 0 {
        return Ok(());
    }
    for (observation_id, run_id, _, _) in &observations {
        conn.execute(
            "insert or ignore into harness_improvement_observations values (?1, ?2)",
            params![id, observation_id],
        )
        .map_err(|error| error.to_string())?;
        conn.execute(
            "insert or ignore into harness_improvement_runs values (?1, ?2)",
            params![id, run_id],
        )
        .map_err(|error| error.to_string())?;
    }
    append_harness_control_event(
        conn,
        project_id,
        "harness_improvement_proposed",
        &format!("Harness improvement {id} needs review"),
    )
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
    use std::sync::{Arc, Barrier};
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;
    use crate::domain::harness::ResearchTaskOutcome;
    use crate::domain::reconciliation::{
        CandidateDecision, PlannedEvidence, PlannedHarnessObservation, PlannedHarnessReflection,
        PlannedResearchEntry,
    };

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

    fn agent_question(operation_key: &str, text: &str) -> AgentStateChange {
        AgentStateChange::Create {
            operation_key: operation_key.to_string(),
            draft: ResearchEntryDraft {
                kind: ResearchEntryKind::Question,
                epistemic_status: EpistemicStatus::Speculative,
                text: text.to_string(),
                evidence: Vec::new(),
                relations: Vec::new(),
                context: Vec::new(),
                reason: Some("Agent State test".to_string()),
            },
            evidence_relationships: Vec::new(),
        }
    }

    fn seed_harness_improvement(db: &TestDb, signature: &str) -> StoreResult<String> {
        for index in 0..3 {
            let search = db.store.create_search(&sample_search_draft())?;
            let run = db
                .store
                .create_harness_run("project:attention", &search.id)?;
            let conn = Connection::open(&db.store.db_path).map_err(|error| error.to_string())?;
            conn.execute(
                "update harness_runs set status = 'ready', finished_at = datetime('now') where id = ?1",
                params![run.id],
            )
            .map_err(|error| error.to_string())?;
            conn.execute(
                "update research_harnesses set status = 'idle' where project_id = 'project:attention'",
                [],
            )
            .map_err(|error| error.to_string())?;
            db.store.persist_harness_reflection(
                &run.id,
                &HarnessReflectionDraft {
                    summary: format!("Operational reflection {index}"),
                    next_direction: None,
                    metrics_json: "{}".to_string(),
                    observations: vec![HarnessObservationDraft {
                        kind: HarnessObservationKind::Terminology,
                        signature: signature.to_string(),
                        severity: 0.5,
                        confidence: 0.9,
                        description: "Mechanistic terminology improved query precision."
                            .to_string(),
                        metrics_json: "{}".to_string(),
                        target: Some(HarnessImprovementTarget::PreferredConcepts),
                        proposed_value: Some(HarnessImprovementValue::Concepts(vec![
                            "mechanistic".to_string(),
                        ])),
                        proposal_eligible: true,
                    }],
                },
            )?;
        }
        db.store
            .list_harness_improvements(
                "project:attention",
                Some(HarnessImprovementStatus::Proposed),
            )?
            .into_iter()
            .next()
            .map(|proposal| proposal.id)
            .ok_or_else(|| "expected seeded Harness improvement".to_string())
    }

    fn ready_reconciliation_run(
        db: &TestDb,
        autonomy: HarnessAutonomy,
        may_add_papers: bool,
    ) -> StoreResult<(HarnessRun, SearchCandidate)> {
        let configuration = HarnessConfiguration {
            goal: "Improve LoRA interpretability".to_string(),
            autonomy,
            may_add_papers,
            paper_budget: 2,
            ..HarnessConfiguration::default()
        };
        db.store
            .save_harness_configuration("project:attention", &configuration)?;
        let search = db.store.create_search(&sample_search_draft())?;
        let harness_run = db
            .store
            .create_harness_run("project:attention", &search.id)?;
        let search_run = db.store.create_search_run(&search.id, "project_harness")?;
        db.store
            .attach_harness_search_run(&harness_run.id, &search_run.id)?;
        db.store
            .append_new_candidates(&search.id, &search_run.id, &[ranked("LoRA", None, 1)])?;
        db.store.set_search_run_status(
            &search_run.id,
            SearchRunStatus::Ready,
            1,
            Some("converged"),
            None,
            true,
        )?;
        db.store
            .record_automatic_harness_reflection(&harness_run.id)?;
        db.store.finalize_harness_run(&search_run.id)?;
        let run = db
            .store
            .get_harness_snapshot("project:attention")?
            .runs
            .into_iter()
            .find(|run| run.id == harness_run.id)
            .expect("ready Harness Run");
        let candidate = db
            .store
            .harness_reconciliation_candidates(&run.id)?
            .into_iter()
            .next()
            .expect("Run candidate");
        Ok((run, candidate))
    }

    fn reconciliation_plan(
        candidate_id: &str,
        decision: CandidateDecisionKind,
    ) -> RunReconciliationPlan {
        let accepted = decision == CandidateDecisionKind::Accept;
        RunReconciliationPlan {
            candidate_decisions: vec![CandidateDecision {
                candidate_id: candidate_id.to_string(),
                decision,
                reason: if accepted {
                    "Mechanistically relevant".to_string()
                } else {
                    "Outside the current emphasis".to_string()
                },
                relevance_confidence: 0.9,
                within_scope: accepted,
            }],
            entries: vec![if accepted {
                PlannedResearchEntry {
                    handle: "finding:1".to_string(),
                    kind: ResearchEntryKind::Finding,
                    epistemic_status: EpistemicStatus::SourceSupported,
                    text: "The candidate reports an inspected result.".to_string(),
                    evidence: vec![PlannedEvidence {
                        candidate_id: candidate_id.to_string(),
                        excerpt: "An abstract.".to_string(),
                        support_note: Some("Provider abstract evidence".to_string()),
                    }],
                    relations: Vec::new(),
                }
            } else {
                PlannedResearchEntry {
                    handle: "gap:1".to_string(),
                    kind: ResearchEntryKind::Gap,
                    epistemic_status: EpistemicStatus::Speculative,
                    text: "This bounded search did not resolve the mechanistic question."
                        .to_string(),
                    evidence: Vec::new(),
                    relations: Vec::new(),
                }
            }],
            next_direction: "Search for causal intervention studies".to_string(),
            operational_reflection: None,
        }
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
        for project in &snapshot.projects {
            assert_eq!(
                db.store
                    .get_harness_snapshot(&project.id)?
                    .harness
                    .project_id,
                project.id
            );
            let state = db.store.get_research_state(&project.id, None)?;
            assert_eq!(state.revision, 0);
            assert!(state.entries.is_empty());
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
        assert_eq!(created.content_revision, 1);

        let updated = db.store.update_project_document(&ProjectDocumentUpdate {
            id: created.id.clone(),
            title: "LoRA related work.md".to_string(),
            content: "# LoRA\n\nRevised synthesis.".to_string(),
            harness_writable: true,
        })?;
        assert_eq!(updated.title, "LoRA related work.md");
        assert_eq!(updated.content, "# LoRA\n\nRevised synthesis.");
        assert!(updated.harness_writable);
        assert_eq!(updated.content_revision, 2);

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
    fn harness_configuration_versions_and_run_snapshots_are_immutable() -> StoreResult<()> {
        let db = test_db()?;
        let configuration = HarnessConfiguration {
            goal: "Improve LoRA interpretability".to_string(),
            research_instructions: "Prioritize mechanistic studies".to_string(),
            scope: "Mechanistic explanations of low-rank adaptation".to_string(),
            exclusions: "Application-only benchmarks".to_string(),
            preferred_concepts: vec!["subspace".to_string()],
            excluded_concepts: vec!["application-only".to_string()],
            sources: vec![
                "browser".to_string(),
                "open_alex".to_string(),
                "arxiv".to_string(),
            ],
            depth: Depth::Quick,
            paper_budget: 6,
            autonomy: HarnessAutonomy::Automatic,
            may_add_papers: true,
            ..HarnessConfiguration::default()
        };
        let saved = db
            .store
            .save_harness_configuration("project:attention", &configuration)?;
        assert_eq!(saved.harness.configuration_version, 2);

        let search = db.store.create_search(&sample_search_draft())?;
        let run = db
            .store
            .create_harness_run("project:attention", &search.id)?;
        assert_eq!(run.configuration_snapshot, configuration);
        assert_eq!(run.configuration_version, 2);
        assert_eq!(run.policy_version, HARNESS_POLICY_VERSION);
        assert_eq!(
            run.effective_instructions.structured_settings,
            configuration
        );
        assert_eq!(
            run.effective_instructions.project_research_instructions,
            "Prioritize mechanistic studies"
        );
        assert_eq!(run.effective_instructions.run_context.vault_id, "attention");
        assert!(!run
            .effective_instructions
            .run_context
            .vault_paper_ids
            .is_empty());
        assert_ne!(
            run.effective_instructions.run_context.vault_revision,
            "legacy"
        );

        let mut changed = configuration.clone();
        changed.preferred_concepts.push("ablation".to_string());
        let saved = db
            .store
            .save_harness_configuration("project:attention", &changed)?;
        assert_eq!(saved.harness.configuration_version, 3);
        assert_eq!(saved.runs[0].configuration_snapshot, configuration);
        assert_eq!(
            db.store.get_harness_run_instructions(&run.id)?,
            run.effective_instructions
        );
        let versions = db
            .store
            .list_harness_configuration_versions("project:attention")?;
        assert_eq!(
            versions.iter().map(|item| item.version).collect::<Vec<_>>(),
            vec![3, 2, 1]
        );
        assert_eq!(versions[1].configuration, configuration);

        let second_search = db.store.create_search(&sample_search_draft())?;
        let error = db
            .store
            .create_harness_run("project:attention", &second_search.id)
            .expect_err("only one active Run may exist");
        assert_eq!(error, "A Research Run is already active for this Project");
        Ok(())
    }

    #[test]
    fn harness_search_uses_immutable_project_scoped_orientation() -> StoreResult<()> {
        let db = test_db()?;
        db.store.create_research_entry(
            "project:attention",
            0,
            &ResearchEntryDraft {
                kind: ResearchEntryKind::Question,
                epistemic_status: EpistemicStatus::ResearcherContext,
                text: "Reader note asks whether LoRA rank components specialize.".to_string(),
                evidence: Vec::new(),
                relations: Vec::new(),
                context: Vec::new(),
                reason: Some("Promoted working context".to_string()),
            },
        )?;
        db.store.create_project(&ProjectDraft {
            title: "Foreign project".to_string(),
            goal: None,
        })?;
        db.store.create_research_entry(
            "project:foreign-project",
            0,
            &ResearchEntryDraft {
                kind: ResearchEntryKind::Question,
                epistemic_status: EpistemicStatus::ResearcherContext,
                text: "FOREIGN_CONTEXT_MUST_NOT_LEAK".to_string(),
                evidence: Vec::new(),
                relations: Vec::new(),
                context: Vec::new(),
                reason: None,
            },
        )?;

        let (prior_run, _) = ready_reconciliation_run(&db, HarnessAutonomy::Manual, false)?;
        let conn = Connection::open(&db.store.db_path).map_err(|error| error.to_string())?;
        conn.execute(
            "update harness_reflections set next_direction = ?2 where run_id = ?1",
            params![prior_run.id, "Investigate component-level causal ablations"],
        )
        .map_err(|error| error.to_string())?;
        conn.execute(
            "insert into harness_observations
             (id, reflection_id, run_id, project_id, kind, signature, severity,
              confidence, description, metrics_json, proposal_eligible, created_at)
             values (?1, ?2, ?3, 'project:attention', 'query_quality', 'narrow-terms',
                     0.5, 0.9, 'Broad explanation queries favored application papers.',
                     '{}', 0, datetime('now'))",
            params![
                format!("{}:orientation-observation", prior_run.id),
                format!("{}:reflection", prior_run.id),
                prior_run.id
            ],
        )
        .map_err(|error| error.to_string())?;

        let search = db.store.create_search(&sample_search_draft())?;
        let run = db
            .store
            .create_harness_run("project:attention", &search.id)?;
        let oriented = db.store.get_search(&search.id)?.goal;
        assert!(oriented.contains("Investigate component-level causal ablations"));
        assert!(oriented.contains("Reader note asks whether LoRA rank components specialize."));
        assert!(oriented.contains("Broad explanation queries favored application papers."));
        assert!(oriented.contains("researcher_context"));
        assert!(oriented.contains("may guide discovery but may not support factual claims"));
        assert!(!oriented.contains("FOREIGN_CONTEXT_MUST_NOT_LEAK"));
        assert!(run
            .effective_instructions
            .run_context
            .prior_observations
            .iter()
            .any(|observation| observation.kind == "query_quality"));

        conn.execute(
            "update harness_reflections set next_direction = 'CHANGED_AFTER_RUN' where run_id = ?1",
            params![prior_run.id],
        )
        .map_err(|error| error.to_string())?;
        db.store.create_research_entry(
            "project:attention",
            1,
            &ResearchEntryDraft {
                kind: ResearchEntryKind::Question,
                epistemic_status: EpistemicStatus::Speculative,
                text: "ADDED_AFTER_RUN".to_string(),
                evidence: Vec::new(),
                relations: Vec::new(),
                context: Vec::new(),
                reason: None,
            },
        )?;
        assert_eq!(db.store.get_search(&search.id)?.goal, oriented);
        let persisted = db.store.get_harness_run_instructions(&run.id)?;
        assert_eq!(persisted, run.effective_instructions);
        assert!(!persisted
            .run_context
            .active_entries
            .iter()
            .any(|entry| entry.text == "ADDED_AFTER_RUN"));
        assert_ne!(
            persisted.run_context.prior_next_direction.as_deref(),
            Some("CHANGED_AFTER_RUN")
        );
        Ok(())
    }

    #[test]
    fn production_reconciliation_reflections_create_one_reviewable_improvement() -> StoreResult<()>
    {
        let db = test_db()?;
        for _ in 0..3 {
            let (run, candidate) = ready_reconciliation_run(&db, HarnessAutonomy::Manual, false)?;
            let conn = Connection::open(&db.store.db_path).map_err(|error| error.to_string())?;
            conn.execute(
                "delete from harness_reflections where run_id = ?1",
                params![run.id],
            )
            .map_err(|error| error.to_string())?;
            let mut plan = reconciliation_plan(&candidate.id, CandidateDecisionKind::Reject);
            plan.operational_reflection = Some(PlannedHarnessReflection {
                summary: "Application-heavy queries reduced mechanistic precision.".to_string(),
                next_direction: Some("Use mechanistic terminology next Run.".to_string()),
                observations: vec![PlannedHarnessObservation {
                    kind: HarnessObservationKind::QueryQuality,
                    signature: "application-heavy-query-results".to_string(),
                    severity: 0.6,
                    confidence: 0.9,
                    description:
                        "Executed queries repeatedly favored application papers over mechanisms."
                            .to_string(),
                    target: Some(HarnessImprovementTarget::PreferredConcepts),
                    proposed_value: Some(HarnessImprovementValue::Concepts(vec![
                        "mechanistic".to_string()
                    ])),
                    proposal_eligible: true,
                }],
            });
            db.store.create_harness_change_set(&run.id, &plan)?;
            db.store.record_automatic_harness_reflection(&run.id)?;
            db.store.record_automatic_harness_reflection(&run.id)?;
        }

        let improvements = db.store.list_harness_improvements(
            "project:attention",
            Some(HarnessImprovementStatus::Proposed),
        )?;
        assert_eq!(improvements.len(), 1);
        assert_eq!(improvements[0].run_ids.len(), 3);
        assert_eq!(
            improvements[0].proposed_value,
            HarnessImprovementValue::Concepts(vec!["mechanistic".to_string()])
        );
        let snapshot = db.store.get_harness_snapshot("project:attention")?;
        assert_eq!(
            snapshot
                .events
                .iter()
                .filter(|event| {
                    event.kind == "operational_observation"
                        && event
                            .detail
                            .as_ref()
                            .and_then(|detail| detail["kind"].as_str())
                            == Some("query_quality")
                })
                .count(),
            3
        );
        Ok(())
    }

    #[test]
    fn reconciliation_review_applies_paper_abstract_and_state_atomically() -> StoreResult<()> {
        let db = test_db()?;
        let (run, candidate) = ready_reconciliation_run(&db, HarnessAutonomy::Manual, true)?;
        let plan = reconciliation_plan(&candidate.id, CandidateDecisionKind::Accept);
        let proposed = db.store.create_harness_change_set(&run.id, &plan)?;
        assert_eq!(proposed.status, HarnessChangeSetStatus::Proposed);
        assert_eq!(
            db.store
                .get_research_state("project:attention", None)?
                .revision,
            0
        );
        assert!(!has_membership(
            &db.store.get_library()?,
            "attention",
            &candidate.candidate.id
        ));

        let applied = db.store.apply_harness_change_set(&proposed.id)?;
        assert_eq!(applied.status, HarnessChangeSetStatus::Applied);
        assert_eq!(applied.resulting_state_revision, Some(1));
        let library = db.store.get_library()?;
        assert!(has_membership(
            &library,
            "attention",
            &candidate.candidate.id
        ));
        assert!(library.document_sources.iter().any(|source| {
            source.paper_id == candidate.candidate.id && source.source_kind == "metadata_abstract"
        }));
        let state = db.store.get_research_state("project:attention", None)?;
        assert_eq!(state.revision, 1);
        let finding = state
            .entries
            .iter()
            .find(|entry| entry.kind == ResearchEntryKind::Finding)
            .expect("applied Finding");
        let detail = db.store.get_research_entry(&finding.id, None)?;
        assert_eq!(detail.evidence[0].excerpt, "An abstract.");
        assert!(detail.evidence[0]
            .support_note
            .as_deref()
            .unwrap_or_default()
            .contains("abstract"));
        Ok(())
    }

    #[test]
    fn checkpoint_captures_usage_decisions_and_monotonic_vault_revision() -> StoreResult<()> {
        let db = test_db()?;
        let (run, candidate) = ready_reconciliation_run(&db, HarnessAutonomy::Manual, true)?;
        let search_run_id = run.search_run_id.as_deref().expect("linked search Run");
        db.store.set_search_run_usage(search_run_id, 3, 4, 1, 1)?;
        db.store.add_search_run_llm_calls(search_run_id, 2)?;
        let proposed = db.store.create_harness_change_set(
            &run.id,
            &reconciliation_plan(&candidate.id, CandidateDecisionKind::Accept),
        )?;
        db.store.apply_harness_change_set(&proposed.id)?;

        let checkpoint = db.store.get_research_checkpoint(&run.id)?;
        assert_eq!(checkpoint.resulting_state_revision, Some(1));
        assert_eq!(checkpoint.applied_change_set_id, Some(proposed.id));
        assert_eq!(checkpoint.accepted_candidate_count, 1);
        assert_eq!(checkpoint.rejected_candidate_count, 0);
        assert_eq!(checkpoint.added_paper_ids, vec![candidate.candidate.id]);
        assert_eq!(checkpoint.usage.provider_queries, 3);
        assert_eq!(checkpoint.usage.llm_calls, 6);
        assert_eq!(checkpoint.usage.iterations, 1);
        assert_eq!(checkpoint.usage.inspected_candidates, 1);
        assert_eq!(
            checkpoint.next_direction.as_deref(),
            Some("Search for causal intervention studies")
        );
        assert!(checkpoint.complete);
        assert!(checkpoint.converged);
        assert!(checkpoint.restore_available);
        assert!(checkpoint.resulting_vault_revision > Some(checkpoint.starting_vault_revision));
        assert_eq!(
            db.store.list_research_checkpoints("project:attention")?[0].run_id,
            run.id
        );
        let reopened = LibraryStore::for_test(db.store.db_path.clone());
        let persisted = reopened.get_research_checkpoint(&run.id)?;
        assert_eq!(
            persisted.applied_change_set_id,
            checkpoint.applied_change_set_id
        );
        assert_eq!(persisted.next_direction, checkpoint.next_direction);
        Ok(())
    }

    #[test]
    fn checkpoint_restoration_appends_state_and_supersedes_later_entries() -> StoreResult<()> {
        let db = test_db()?;
        let (run, candidate) = ready_reconciliation_run(&db, HarnessAutonomy::Manual, true)?;
        let proposed = db.store.create_harness_change_set(
            &run.id,
            &reconciliation_plan(&candidate.id, CandidateDecisionKind::Accept),
        )?;
        db.store.apply_harness_change_set(&proposed.id)?;
        let later = db.store.create_research_entry(
            "project:attention",
            1,
            &ResearchEntryDraft {
                kind: ResearchEntryKind::Question,
                epistemic_status: EpistemicStatus::Speculative,
                text: "What changed after the checkpoint?".to_string(),
                evidence: Vec::new(),
                relations: Vec::new(),
                context: Vec::new(),
                reason: Some("Later manual work".to_string()),
            },
        )?;
        let later_id = later.entry.entry.id;

        let restored = db
            .store
            .restore_research_checkpoint("project:attention", &run.id, 2)?;
        assert_eq!(restored.revision, 3);
        assert_eq!(restored.current_revision, 3);
        assert_eq!(
            restored
                .entries
                .iter()
                .find(|entry| entry.id == later_id)
                .expect("later entry retained in history")
                .lifecycle,
            EntryLifecycle::Superseded
        );
        assert!(restored.entries.iter().any(|entry| {
            entry.kind == ResearchEntryKind::Finding && entry.lifecycle == EntryLifecycle::Active
        }));
        assert!(db
            .store
            .restore_research_checkpoint("project:attention", &run.id, 2)
            .is_err());
        let events = db.store.get_harness_snapshot("project:attention")?.events;
        let restored_event = events
            .iter()
            .find(|event| event.kind == "checkpoint_restored")
            .expect("restoration event");
        assert_eq!(restored_event.actor, "researcher");
        assert_eq!(
            restored_event
                .detail
                .as_ref()
                .and_then(|detail| detail["resultingRevision"].as_i64()),
            Some(3)
        );
        Ok(())
    }

    #[test]
    fn checkpoint_restoration_rejects_a_run_from_another_project() -> StoreResult<()> {
        let db = test_db()?;
        let (run, _) = ready_reconciliation_run(&db, HarnessAutonomy::Manual, false)?;
        db.store.create_project(&ProjectDraft {
            title: "Foreign project".to_string(),
            goal: None,
        })?;

        let error = db
            .store
            .restore_research_checkpoint("project:foreign-project", &run.id, 0)
            .expect_err("a foreign Run must not be restored through the active Project");
        assert_eq!(
            error,
            "Research checkpoint does not belong to the active Project"
        );
        assert_eq!(
            db.store
                .get_research_state("project:attention", None)?
                .current_revision,
            0
        );
        assert_eq!(
            db.store
                .get_research_state("project:foreign-project", None)?
                .current_revision,
            0
        );
        Ok(())
    }

    #[test]
    fn structured_activity_rejects_unbounded_or_invalid_records() -> StoreResult<()> {
        let db = test_db()?;
        let (run, _) = ready_reconciliation_run(&db, HarnessAutonomy::Manual, false)?;
        let conn = Connection::open(&db.store.db_path).map_err(|error| error.to_string())?;
        assert!(append_structured_harness_event(
            &conn,
            &run.id,
            "query",
            &"x".repeat(501),
            None,
            None,
            None,
            None,
            "harness",
        )
        .is_err());
        assert!(append_structured_harness_event(
            &conn,
            &run.id,
            "query",
            "Invalid progress",
            None,
            Some("searching"),
            Some(2),
            Some(1),
            "harness",
        )
        .is_err());
        assert!(append_structured_harness_event(
            &conn,
            &run.id,
            "provider_query_completed",
            "Malformed provider result",
            Some(serde_json::json!({ "provider": "browser", "candidateCount": "many" })),
            Some("searching"),
            None,
            None,
            "harness",
        )
        .is_err());
        assert!(append_structured_harness_event(
            &conn,
            &run.id,
            "unregistered_detail_kind",
            "Unknown structured payload",
            Some(serde_json::json!({ "value": 1 })),
            None,
            None,
            None,
            "harness",
        )
        .is_err());
        Ok(())
    }

    #[test]
    fn stale_or_rejected_reconciliation_never_mutates_project_knowledge() -> StoreResult<()> {
        let db = test_db()?;
        let (run, candidate) = ready_reconciliation_run(&db, HarnessAutonomy::Propose, false)?;
        let plan = reconciliation_plan(&candidate.id, CandidateDecisionKind::Reject);
        let proposed = db.store.create_harness_change_set(&run.id, &plan)?;
        db.store.create_research_entry(
            "project:attention",
            0,
            &ResearchEntryDraft {
                kind: ResearchEntryKind::Question,
                epistemic_status: EpistemicStatus::Speculative,
                text: "Which intervention distinguishes competing mechanisms?".to_string(),
                evidence: Vec::new(),
                relations: Vec::new(),
                context: Vec::new(),
                reason: Some("Researcher update".to_string()),
            },
        )?;
        let superseded = db.store.apply_harness_change_set(&proposed.id)?;
        assert_eq!(superseded.status, HarnessChangeSetStatus::Superseded);
        assert_eq!(
            db.store
                .get_research_state("project:attention", None)?
                .revision,
            1
        );

        let db = test_db()?;
        let (run, candidate) = ready_reconciliation_run(&db, HarnessAutonomy::Propose, false)?;
        let plan = reconciliation_plan(&candidate.id, CandidateDecisionKind::Reject);
        let proposed = db.store.create_harness_change_set(&run.id, &plan)?;
        let rejected = db
            .store
            .reject_harness_change_set(&proposed.id, "The bounded gap is not useful")?;
        assert_eq!(rejected.status, HarnessChangeSetStatus::Rejected);
        assert_eq!(rejected.plan, Some(plan));
        assert_eq!(
            db.store
                .get_research_state("project:attention", None)?
                .revision,
            0
        );
        Ok(())
    }

    #[test]
    fn failed_reconciliation_application_rolls_back_paper_and_state() -> StoreResult<()> {
        let db = test_db()?;
        let (run, candidate) = ready_reconciliation_run(&db, HarnessAutonomy::Manual, true)?;
        let plan = reconciliation_plan(&candidate.id, CandidateDecisionKind::Accept);
        let proposed = db.store.create_harness_change_set(&run.id, &plan)?;
        let mut invalid = plan;
        invalid.entries[0].evidence[0].excerpt = "Invented unsupported quote".to_string();
        let conn = Connection::open(&db.store.db_path).map_err(|error| error.to_string())?;
        conn.execute(
            "update harness_change_sets set plan_json = ?2 where id = ?1",
            params![
                proposed.id,
                serde_json::to_string(&invalid).map_err(|error| error.to_string())?
            ],
        )
        .map_err(|error| error.to_string())?;
        drop(conn);
        assert!(db.store.apply_harness_change_set(&proposed.id).is_err());
        let failed = db.store.get_harness_change_set(&run.id)?;
        assert_eq!(failed.status, HarnessChangeSetStatus::Failed);
        assert_eq!(
            db.store
                .get_research_state("project:attention", None)?
                .revision,
            0
        );
        assert!(!has_membership(
            &db.store.get_library()?,
            "attention",
            &candidate.candidate.id
        ));
        Ok(())
    }

    #[test]
    fn harness_authority_defaults_to_project_enrichment_and_stays_project_bounded(
    ) -> StoreResult<()> {
        let db = test_db()?;
        let defaults = db
            .store
            .get_harness_snapshot("project:attention")?
            .harness
            .configuration;
        assert_eq!(defaults.autonomy, HarnessAutonomy::Automatic);
        assert!(!defaults.schedule.enabled);
        assert!(defaults.may_add_papers);
        assert!(defaults.writable_document_ids.is_empty());

        let mut manual_schedule = HarnessConfiguration {
            goal: "Map mechanisms".to_string(),
            autonomy: HarnessAutonomy::Manual,
            ..HarnessConfiguration::default()
        };
        manual_schedule.schedule.enabled = true;
        assert_eq!(
            db.store
                .save_harness_configuration("project:attention", &manual_schedule)
                .expect_err("manual autonomy must not schedule"),
            "Manual autonomy cannot enable scheduled Runs"
        );

        db.store.create_project(&ProjectDraft {
            title: "Foreign project".to_string(),
            goal: None,
        })?;
        let foreign = db.store.create_project_document(&ProjectDocumentDraft {
            project_id: "project:foreign-project".to_string(),
            title: "Foreign.md".to_string(),
            content: String::new(),
        })?;
        let configuration = HarnessConfiguration {
            goal: "Map mechanisms".to_string(),
            writable_document_ids: vec![foreign.id.clone()],
            ..HarnessConfiguration::default()
        };
        assert_eq!(
            db.store
                .save_harness_configuration("project:attention", &configuration)
                .expect_err("foreign Documents must not grant authority"),
            format!(
                "Writable document does not belong to this Project: {}",
                foreign.id
            )
        );
        Ok(())
    }

    #[test]
    fn legacy_harness_authority_migrates_to_simple_project_enrichment() -> StoreResult<()> {
        let db = test_db()?;
        let conn = Connection::open(&db.store.db_path).map_err(|error| error.to_string())?;
        let raw: String = conn
            .query_row(
                "select configuration_json from research_harnesses where project_id = 'project:attention'",
                [],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        let mut value: serde_json::Value =
            serde_json::from_str(&raw).map_err(|error| error.to_string())?;
        let object = value.as_object_mut().expect("configuration object");
        object.remove("autonomy");
        object.remove("scope");
        object.remove("exclusions");
        object.remove("mayAddPapers");
        object.remove("writableDocumentIds");
        object
            .get_mut("schedule")
            .and_then(serde_json::Value::as_object_mut)
            .expect("schedule object")
            .insert("enabled".to_string(), serde_json::Value::Bool(true));
        conn.execute(
            "update research_harnesses set configuration_json = ?1, schedule_enabled = 1,
             next_run_at = '2026-09-03T07:00:00+00:00' where project_id = 'project:attention'",
            params![serde_json::to_string(&value).map_err(|error| error.to_string())?],
        )
        .map_err(|error| error.to_string())?;
        conn.execute(
            "delete from harness_configuration_versions where project_id = 'project:attention'",
            [],
        )
        .map_err(|error| error.to_string())?;
        drop(conn);

        db.store.init()?;
        let migrated = db.store.get_harness_snapshot("project:attention")?.harness;
        assert_eq!(migrated.configuration.autonomy, HarnessAutonomy::Automatic);
        assert!(!migrated.configuration.schedule.enabled);
        assert!(migrated.next_run_at.is_none());
        assert!(migrated.configuration.may_add_papers);
        assert!(migrated.configuration.writable_document_ids.is_empty());
        let versions = db
            .store
            .list_harness_configuration_versions("project:attention")?;
        assert_eq!(versions.len(), 1);
        assert_eq!(versions[0].actor, "migration");
        Ok(())
    }

    #[test]
    fn simple_research_migration_is_idempotent_and_preserves_all_instructions() -> StoreResult<()> {
        let db = test_db()?;
        let legacy = HarnessConfiguration {
            goal: "Map LoRA mechanisms".to_string(),
            research_instructions: "Prefer causal evidence".to_string(),
            scope: "Transformer adapters".to_string(),
            exclusions: "Benchmark-only studies".to_string(),
            preferred_concepts: vec!["ablation".to_string()],
            excluded_concepts: vec!["survey".to_string()],
            autonomy: HarnessAutonomy::Manual,
            may_add_papers: false,
            ..HarnessConfiguration::default()
        };
        let raw = serde_json::to_string(&legacy).map_err(|error| error.to_string())?;
        let conn = Connection::open(&db.store.db_path).map_err(|error| error.to_string())?;
        conn.execute(
            "update research_harnesses set configuration_json = ?1
             where project_id = 'project:attention'",
            params![raw],
        )
        .map_err(|error| error.to_string())?;
        drop(conn);

        db.store.init()?;
        let first = db.store.get_harness_snapshot("project:attention")?;
        let instructions = &first.harness.configuration.research_instructions;
        for expected in [
            "Map LoRA mechanisms",
            "Prefer causal evidence",
            "Transformer adapters",
            "Benchmark-only studies",
            "ablation",
            "survey",
        ] {
            assert!(instructions.contains(expected));
        }
        assert!(first.harness.configuration.goal.is_empty());
        assert_eq!(
            first.harness.configuration.autonomy,
            HarnessAutonomy::Automatic
        );
        assert!(first.harness.configuration.may_add_papers);
        let event_count = first.events.len();
        let version_count = db
            .store
            .list_harness_configuration_versions("project:attention")?
            .len();

        db.store.init()?;
        let second = db.store.get_harness_snapshot("project:attention")?;
        assert_eq!(second.harness.configuration, first.harness.configuration);
        assert_eq!(second.events.len(), event_count);
        assert_eq!(
            db.store
                .list_harness_configuration_versions("project:attention")?
                .len(),
            version_count
        );
        Ok(())
    }

    #[test]
    fn clearing_configuration_history_keeps_current_version_and_run_snapshot() -> StoreResult<()> {
        let db = test_db()?;
        let mut configuration = HarnessConfiguration::default();
        configuration.research_instructions = "First direction".to_string();
        db.store
            .save_harness_configuration("project:attention", &configuration)?;
        let search = db.store.create_search(&sample_search_draft())?;
        let run = db
            .store
            .create_harness_run("project:attention", &search.id)?;
        configuration.research_instructions = "Current direction".to_string();
        let current = db
            .store
            .save_harness_configuration("project:attention", &configuration)?;

        assert_eq!(
            db.store
                .clear_harness_configuration_history("project:attention")?,
            2
        );
        let versions = db
            .store
            .list_harness_configuration_versions("project:attention")?;
        assert_eq!(versions.len(), 1);
        assert_eq!(versions[0].version, current.harness.configuration_version);
        assert_eq!(
            db.store.get_harness_run_instructions(&run.id)?,
            run.effective_instructions
        );
        Ok(())
    }

    #[test]
    fn repeated_stop_does_not_append_control_activity() -> StoreResult<()> {
        let db = test_db()?;
        let first = db.store.stop_research_harness("project:attention")?;
        let second = db.store.stop_research_harness("project:attention")?;

        assert_eq!(first.events.len(), second.events.len());
        assert_eq!(
            second
                .events
                .iter()
                .filter(|event| event.kind == "harness_stopped")
                .count(),
            1
        );
        Ok(())
    }

    #[test]
    fn harness_activity_mirrors_linked_search_lifecycle() -> StoreResult<()> {
        let db = test_db()?;
        let mut configuration = HarnessConfiguration::default();
        configuration.goal = "Map LoRA mechanisms".to_string();
        db.store
            .save_harness_configuration("project:attention", &configuration)?;
        let search = db.store.create_search(&sample_search_draft())?;
        let harness_run = db
            .store
            .create_harness_run("project:attention", &search.id)?;
        let search_run = db.store.create_search_run(&search.id, "project_harness")?;
        db.store
            .attach_harness_search_run(&harness_run.id, &search_run.id)?;
        db.store.set_search_run_status(
            &search_run.id,
            SearchRunStatus::Planning,
            0,
            None,
            None,
            false,
        )?;
        db.store.set_search_run_status(
            &search_run.id,
            SearchRunStatus::Ready,
            1,
            Some("target_reached"),
            None,
            true,
        )?;
        db.store
            .record_automatic_harness_reflection(&harness_run.id)?;
        db.store.finalize_harness_run(&search_run.id)?;

        let snapshot = db.store.get_harness_snapshot("project:attention")?;
        assert_eq!(snapshot.harness.status, "idle");
        assert_eq!(snapshot.runs[0].status, "ready");
        assert_eq!(
            snapshot.runs[0].stop_reason.as_deref(),
            Some("target_reached")
        );
        let mut sequences: Vec<i64> = snapshot
            .events
            .iter()
            .filter(|event| event.run_id == harness_run.id)
            .map(|event| event.sequence)
            .collect();
        sequences.sort_unstable();
        assert_eq!(sequences, (1..=sequences.len() as i64).collect::<Vec<_>>());
        assert!(snapshot.events.iter().any(|event| event.kind == "planning"));
        assert!(snapshot.events.iter().any(|event| event.kind == "ready"));
        assert!(snapshot
            .events
            .iter()
            .any(|event| event.kind == "stop_decided"));
        assert!(snapshot
            .events
            .iter()
            .any(|event| event.kind == "harness_reflection_recorded"));
        Ok(())
    }

    #[test]
    fn concurrent_harness_activity_receives_unique_ordered_sequences() -> StoreResult<()> {
        const WRITERS: usize = 24;

        let db = test_db()?;
        let run = managed_harness_run(&db, &AgentRunLimits::default())?;
        let initial_count = db
            .store
            .get_harness_snapshot("project:attention")?
            .events
            .into_iter()
            .filter(|event| event.run_id == run.id)
            .count();
        let barrier = Arc::new(Barrier::new(WRITERS));
        let handles = (0..WRITERS)
            .map(|index| {
                let store = db.store.clone();
                let run_id = run.id.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    store.record_harness_activity(
                        &run_id,
                        "concurrent_test",
                        &format!("Concurrent event {index}"),
                        Some("test"),
                    )
                })
            })
            .collect::<Vec<_>>();

        for handle in handles {
            handle.join().expect("activity writer panicked")?;
        }
        let mut sequences = db
            .store
            .get_harness_snapshot("project:attention")?
            .events
            .into_iter()
            .filter(|event| event.run_id == run.id)
            .map(|event| event.sequence)
            .collect::<Vec<_>>();
        assert_eq!(sequences.len(), initial_count + WRITERS);
        sequences.sort_unstable();
        assert!(sequences.windows(2).all(|pair| pair[1] == pair[0] + 1));
        Ok(())
    }

    #[test]
    fn harness_run_finalizes_only_after_reconciliation_usage_and_reflection() -> StoreResult<()> {
        let db = test_db()?;
        let configuration = HarnessConfiguration {
            goal: "Map LoRA mechanisms".to_string(),
            paper_budget: 2,
            ..HarnessConfiguration::default()
        };
        db.store
            .save_harness_configuration("project:attention", &configuration)?;
        let search = db.store.create_search(&sample_search_draft())?;
        let harness_run = db
            .store
            .create_harness_run("project:attention", &search.id)?;
        let search_run = db.store.create_search_run(&search.id, "project_harness")?;
        db.store
            .attach_harness_search_run(&harness_run.id, &search_run.id)?;
        db.store
            .append_new_candidates(&search.id, &search_run.id, &[ranked("LoRA", None, 1)])?;
        db.store.set_search_run_usage(&search_run.id, 2, 3, 1, 1)?;
        db.store.set_search_run_status(
            &search_run.id,
            SearchRunStatus::Ready,
            1,
            Some("converged"),
            None,
            true,
        )?;

        let pending = db.store.get_harness_snapshot("project:attention")?;
        assert_eq!(pending.harness.status, "running");
        assert_eq!(pending.runs[0].status, "reconciling");
        assert!(
            !db.store
                .get_research_checkpoint(&harness_run.id)?
                .restore_available
        );
        let second_search = db.store.create_search(&sample_search_draft())?;
        assert!(db
            .store
            .create_harness_run("project:attention", &second_search.id)
            .expect_err("reconciling blocks a second Run")
            .contains("already active"));

        let candidate = db
            .store
            .harness_reconciliation_candidates(&harness_run.id)?
            .into_iter()
            .next()
            .expect("candidate");
        db.store.create_harness_change_set(
            &harness_run.id,
            &reconciliation_plan(&candidate.id, CandidateDecisionKind::Reject),
        )?;
        db.store.add_search_run_llm_calls(&search_run.id, 1)?;
        db.store
            .record_automatic_harness_reflection(&harness_run.id)?;
        db.store.finalize_harness_run(&search_run.id)?;

        let completed = db.store.get_harness_snapshot("project:attention")?;
        assert_eq!(completed.harness.status, "idle");
        assert_eq!(completed.runs[0].status, "ready");
        assert!(completed.runs[0].finished_at.is_some());
        let mut events = completed
            .events
            .iter()
            .filter(|event| event.run_id == harness_run.id)
            .collect::<Vec<_>>();
        events.sort_by_key(|event| event.sequence);
        let sequence = |kind: &str| {
            events
                .iter()
                .position(|event| event.kind == kind)
                .expect("event kind")
        };
        assert!(sequence("change_set_proposed") < sequence("reconciliation_usage_recorded"));
        assert!(
            sequence("reconciliation_usage_recorded") < sequence("harness_reflection_recorded")
        );
        assert!(sequence("harness_reflection_recorded") < sequence("ready"));
        assert!(sequence("ready") < sequence("stop_decided"));
        Ok(())
    }

    #[test]
    fn restart_recovery_finalizes_a_search_completed_reconciling_run() -> StoreResult<()> {
        let db = test_db()?;
        let configuration = HarnessConfiguration {
            goal: "Map LoRA mechanisms".to_string(),
            ..HarnessConfiguration::default()
        };
        db.store
            .save_harness_configuration("project:attention", &configuration)?;
        let search = db.store.create_search(&sample_search_draft())?;
        let harness_run = db
            .store
            .create_harness_run("project:attention", &search.id)?;
        let search_run = db.store.create_search_run(&search.id, "project_harness")?;
        db.store
            .attach_harness_search_run(&harness_run.id, &search_run.id)?;
        db.store.set_search_run_status(
            &search_run.id,
            SearchRunStatus::Ready,
            1,
            Some("converged"),
            None,
            true,
        )?;

        assert_eq!(db.store.recover_interrupted_harness_runs()?, 1);
        let recovered = db.store.get_harness_snapshot("project:attention")?;
        assert_eq!(recovered.harness.status, "idle");
        assert_eq!(recovered.runs[0].status, "ready");
        assert!(recovered.runs[0].resulting_state_revision.is_some());
        assert!(recovered.events.iter().any(|event| {
            event.run_id == harness_run.id && event.kind == "reconciliation_recovered"
        }));
        assert!(
            db.store
                .get_research_checkpoint(&harness_run.id)?
                .restore_available
        );
        Ok(())
    }

    #[test]
    fn scheduled_harness_claims_one_occurrence_and_skips_missed_intervals() -> StoreResult<()> {
        use chrono::TimeZone;

        let db = test_db()?;
        let mut configuration = HarnessConfiguration::default();
        configuration.goal = "Map LoRA mechanisms".to_string();
        configuration.autonomy = HarnessAutonomy::Propose;
        configuration.schedule.enabled = true;
        configuration.schedule.timezone = "Europe/Berlin".to_string();
        configuration.schedule.local_time = "09:00".to_string();
        let saved = db.store.save_harness_configuration_at(
            "project:attention",
            &configuration,
            Utc.with_ymd_and_hms(2026, 9, 2, 6, 0, 0).unwrap(),
        )?;
        assert_eq!(
            saved.harness.next_run_at.as_deref(),
            Some("2026-09-02T07:00:00+00:00")
        );

        let claim = db
            .store
            .claim_due_harness(Utc.with_ymd_and_hms(2026, 9, 5, 12, 0, 0).unwrap(), true)?
            .expect("one overdue schedule is claimed");
        assert_eq!(claim.project_id, "project:attention");
        assert_eq!(claim.trigger, HarnessRunTrigger::StartupCatchUp);
        assert_eq!(claim.scheduled_for, "2026-09-02T07:00:00+00:00");
        assert!(db
            .store
            .claim_due_harness(Utc.with_ymd_and_hms(2026, 9, 5, 12, 0, 0).unwrap(), true)?
            .is_none());
        assert_eq!(
            db.store
                .get_harness_snapshot("project:attention")?
                .harness
                .next_run_at
                .as_deref(),
            Some("2026-09-06T07:00:00+00:00")
        );
        Ok(())
    }

    #[test]
    fn pause_resume_stop_and_recovery_are_durable() -> StoreResult<()> {
        let db = test_db()?;
        let mut configuration = HarnessConfiguration::default();
        configuration.goal = "Map LoRA mechanisms".to_string();
        configuration.autonomy = HarnessAutonomy::Propose;
        configuration.schedule.enabled = true;
        db.store
            .save_harness_configuration("project:attention", &configuration)?;
        assert_eq!(
            db.store
                .pause_research_harness("project:attention")?
                .harness
                .status,
            "paused"
        );
        assert_eq!(
            db.store
                .resume_research_harness("project:attention")?
                .harness
                .status,
            "idle"
        );

        let search = db.store.create_search(&sample_search_draft())?;
        let run = db.store.create_harness_run_with_trigger(
            "project:attention",
            &search.id,
            HarnessRunTrigger::Scheduled,
            Some("2026-09-03T07:00:00+00:00"),
        )?;
        db.store.pause_research_harness("project:attention")?;
        assert_eq!(db.store.recover_interrupted_harness_runs()?, 1);
        let recovered = db.store.get_harness_snapshot("project:attention")?;
        assert_eq!(recovered.harness.status, "paused");
        assert_eq!(recovered.runs[0].id, run.id);
        assert_eq!(recovered.runs[0].status, "failed");
        assert_eq!(
            recovered.runs[0].stop_reason.as_deref(),
            Some("application_restarted")
        );
        assert_eq!(
            db.store
                .stop_research_harness("project:attention")?
                .harness
                .status,
            "stopped"
        );
        Ok(())
    }

    #[test]
    fn manual_runs_preserve_schedule_and_terminal_limits_stop_future_runs() -> StoreResult<()> {
        use chrono::TimeZone;

        let db = test_db()?;
        let mut configuration = HarnessConfiguration::default();
        configuration.goal = "Map LoRA mechanisms".to_string();
        configuration.autonomy = HarnessAutonomy::Propose;
        configuration.schedule.enabled = true;
        configuration.stop_conditions.maximum_cycles = Some(1);
        let scheduled = db.store.save_harness_configuration_at(
            "project:attention",
            &configuration,
            Utc.with_ymd_and_hms(2026, 9, 2, 6, 0, 0).unwrap(),
        )?;
        let next_run_at = scheduled.harness.next_run_at.clone();
        let search = db.store.create_search(&sample_search_draft())?;
        let harness_run = db
            .store
            .create_harness_run("project:attention", &search.id)?;
        assert_eq!(
            db.store
                .get_harness_snapshot("project:attention")?
                .harness
                .next_run_at,
            next_run_at
        );
        let search_run = db.store.create_search_run(&search.id, "project_harness")?;
        db.store
            .attach_harness_search_run(&harness_run.id, &search_run.id)?;
        db.store.set_search_run_status(
            &search_run.id,
            SearchRunStatus::Ready,
            1,
            Some("coverage_sufficient"),
            None,
            true,
        )?;
        db.store
            .record_automatic_harness_reflection(&harness_run.id)?;
        db.store.finalize_harness_run(&search_run.id)?;
        let stopped = db.store.get_harness_snapshot("project:attention")?;
        assert_eq!(stopped.harness.completed_cycle_count, 1);
        assert_eq!(stopped.harness.status, "stopped");
        assert_eq!(
            stopped.harness.terminal_stop_reason.as_deref(),
            Some("maximum_cycles_reached")
        );
        assert!(db
            .store
            .ensure_harness_can_start("project:attention")
            .is_err());
        Ok(())
    }

    #[test]
    fn recurring_operational_observations_create_one_reviewable_improvement() -> StoreResult<()> {
        let db = test_db()?;
        let mut configuration = HarnessConfiguration::default();
        configuration.goal = "Map LoRA mechanisms".to_string();
        db.store
            .save_harness_configuration("project:attention", &configuration)?;
        let mut run_ids = Vec::new();
        for index in 0..3 {
            let search = db.store.create_search(&sample_search_draft())?;
            let run = db
                .store
                .create_harness_run("project:attention", &search.id)?;
            let conn = Connection::open(&db.store.db_path).map_err(|error| error.to_string())?;
            conn.execute(
                "update harness_runs set status = 'ready', finished_at = datetime('now') where id = ?1",
                params![run.id],
            )
            .map_err(|error| error.to_string())?;
            conn.execute(
                "update research_harnesses set status = 'idle' where project_id = 'project:attention'",
                [],
            )
            .map_err(|error| error.to_string())?;
            db.store.persist_harness_reflection(
                &run.id,
                &HarnessReflectionDraft {
                    summary: format!(
                        "Run {} repeatedly found application-heavy results",
                        index + 1
                    ),
                    next_direction: None,
                    metrics_json: "{\"irrelevant\":4}".to_string(),
                    observations: vec![HarnessObservationDraft {
                        kind: HarnessObservationKind::CoverageBias,
                        signature: "application-heavy-results".to_string(),
                        severity: 0.7,
                        confidence: 0.8,
                        description: "Queries overrepresented application papers.".to_string(),
                        metrics_json: "{\"irrelevant\":4}".to_string(),
                        target: Some(HarnessImprovementTarget::PreferredConcepts),
                        proposed_value: Some(HarnessImprovementValue::Concepts(vec![
                            "mechanistic".to_string(),
                            "subspace".to_string(),
                        ])),
                        proposal_eligible: true,
                    }],
                },
            )?;
            run_ids.push(run.id);
        }

        let proposals = db.store.list_harness_improvements(
            "project:attention",
            Some(HarnessImprovementStatus::Proposed),
        )?;
        assert_eq!(proposals.len(), 1);
        assert_eq!(proposals[0].run_ids.len(), 3);
        assert_eq!(proposals[0].observation_ids.len(), 3);
        assert_eq!(
            proposals[0].target,
            HarnessImprovementTarget::PreferredConcepts
        );
        assert!(proposals[0].expected_effect.contains("mechanistic"));

        let edited = db.store.edit_harness_improvement(
            &proposals[0].id,
            &HarnessImprovementValue::Concepts(vec![
                " Mechanistic ".to_string(),
                "intrinsic dimension".to_string(),
                "mechanistic".to_string(),
            ]),
        )?;
        assert_eq!(
            edited.proposed_value,
            HarnessImprovementValue::Concepts(vec![
                "mechanistic".to_string(),
                "intrinsic dimension".to_string()
            ])
        );
        let accepted = db.store.accept_harness_improvement(&proposals[0].id)?;
        assert_eq!(accepted.status, HarnessImprovementStatus::Accepted);
        let harness = db.store.get_harness_snapshot("project:attention")?.harness;
        assert_eq!(
            harness.configuration.preferred_concepts,
            vec!["mechanistic", "intrinsic dimension"]
        );
        assert_eq!(
            accepted.resulting_configuration_version,
            Some(harness.configuration_version)
        );
        let versions = db
            .store
            .list_harness_configuration_versions("project:attention")?;
        let improvement_version = versions
            .iter()
            .find(|version| version.version == harness.configuration_version)
            .expect("accepted improvement records a version");
        assert_eq!(improvement_version.actor, "improvement");
        assert_eq!(
            improvement_version.source_improvement_id.as_deref(),
            Some(proposals[0].id.as_str())
        );
        for run_id in run_ids {
            assert!(db
                .store
                .get_harness_snapshot("project:attention")?
                .runs
                .iter()
                .find(|run| run.id == run_id)
                .expect("contributing Run remains")
                .configuration_snapshot
                .preferred_concepts
                .is_empty());
        }
        Ok(())
    }

    #[test]
    fn stale_harness_improvement_is_superseded_instead_of_rebased() -> StoreResult<()> {
        let db = test_db()?;
        let mut configuration = HarnessConfiguration::default();
        configuration.goal = "Map LoRA mechanisms".to_string();
        db.store
            .save_harness_configuration("project:attention", &configuration)?;
        let proposal_id = seed_harness_improvement(&db, "stale-proposal")?;
        configuration.preferred_concepts = vec!["causal".to_string()];
        db.store
            .save_harness_configuration("project:attention", &configuration)?;
        let superseded = db.store.accept_harness_improvement(&proposal_id)?;
        assert_eq!(superseded.status, HarnessImprovementStatus::Superseded);
        assert_eq!(
            db.store
                .get_harness_snapshot("project:attention")?
                .harness
                .configuration
                .preferred_concepts,
            vec!["causal"]
        );
        Ok(())
    }

    #[test]
    fn rejected_harness_improvement_retains_contributors_and_reason() -> StoreResult<()> {
        let db = test_db()?;
        let mut configuration = HarnessConfiguration::default();
        configuration.goal = "Map LoRA mechanisms".to_string();
        db.store
            .save_harness_configuration("project:attention", &configuration)?;
        let proposal_id = seed_harness_improvement(&db, "reject-proposal")?;
        let rejected = db.store.reject_harness_improvement(
            &proposal_id,
            Some("Vocabulary is too narrow for this Project"),
        )?;
        assert_eq!(rejected.status, HarnessImprovementStatus::Rejected);
        assert_eq!(rejected.run_ids.len(), 3);
        assert_eq!(
            rejected.decision_reason.as_deref(),
            Some("Vocabulary is too narrow for this Project")
        );
        assert!(db
            .store
            .get_harness_snapshot("project:attention")?
            .harness
            .configuration
            .preferred_concepts
            .is_empty());
        Ok(())
    }

    #[test]
    fn harness_improvement_allow_list_preserves_browser_and_rejects_mismatches() {
        assert!(HarnessImprovementTarget::parse("goal").is_err());
        assert!(normalize_improvement_value(
            HarnessImprovementTarget::MetadataResolvers,
            &HarnessImprovementValue::Concepts(vec!["unsafe".to_string()]),
        )
        .is_err());
        let mut configuration = HarnessConfiguration::default();
        apply_improvement_value(
            &mut configuration,
            HarnessImprovementTarget::MetadataResolvers,
            &HarnessImprovementValue::MetadataResolvers(Vec::new()),
        )
        .expect("empty optional resolver set is valid");
        assert_eq!(configuration.sources, vec!["browser"]);
    }

    #[test]
    fn source_supported_state_requires_project_vault_evidence() -> StoreResult<()> {
        let db = test_db()?;
        let extraction = extracted_paper(
            &db,
            "research-state-paper",
            &[(
                "paragraph",
                "Low-rank updates constrain adaptation to a learned subspace.",
            )],
        )?;
        let chunk = db.store.chunks_for_extraction(&extraction.id)?.remove(0);
        let draft = ResearchEntryDraft {
            kind: ResearchEntryKind::Finding,
            epistemic_status: EpistemicStatus::SourceSupported,
            text: "Low-rank updates constrain adaptation to a learned subspace.".to_string(),
            evidence: vec![EvidenceLinkDraft {
                chunk_id: chunk.id.clone(),
                excerpt: None,
                support_note: Some("Direct statement".to_string()),
            }],
            relations: Vec::new(),
            context: Vec::new(),
            reason: Some("Curated finding".to_string()),
        };

        let created = db
            .store
            .create_research_entry("project:attention", 0, &draft)?;
        assert_eq!(created.state.revision, 1);
        assert_eq!(created.entry.entry.evidence_count, 1);
        assert_eq!(created.entry.evidence[0].paper_id, "research-state-paper");
        assert_eq!(created.entry.evidence[0].chunk_id, chunk.id);
        assert!(created.entry.evidence[0]
            .excerpt
            .contains("learned subspace"));

        let missing = ResearchEntryDraft {
            evidence: Vec::new(),
            ..draft.clone()
        };
        assert_eq!(
            db.store
                .create_research_entry("project:attention", 1, &missing)
                .expect_err("source-supported entry must cite evidence"),
            "A source-supported Finding requires source evidence"
        );
        let wrong_project = db
            .store
            .create_research_entry("project:self-supervised", 0, &draft)
            .expect_err("evidence cannot cross the Project boundary");
        assert!(wrong_project.contains("does not resolve inside this Project Vault"));
        assert_eq!(
            db.store
                .get_research_state("project:self-supervised", None)?
                .revision,
            0
        );
        Ok(())
    }

    #[test]
    fn research_evidence_survives_chunk_regeneration() -> StoreResult<()> {
        let db = test_db()?;
        let extraction = extracted_paper(
            &db,
            "durable-evidence-paper",
            &[(
                "paragraph",
                "The cited passage must remain auditable after rechunking.",
            )],
        )?;
        let chunk = db.store.chunks_for_extraction(&extraction.id)?.remove(0);
        let created = db.store.create_research_entry(
            "project:attention",
            0,
            &ResearchEntryDraft {
                kind: ResearchEntryKind::Finding,
                epistemic_status: EpistemicStatus::SourceSupported,
                text: "The cited passage remains auditable.".to_string(),
                evidence: vec![EvidenceLinkDraft {
                    chunk_id: chunk.id,
                    excerpt: None,
                    support_note: None,
                }],
                relations: Vec::new(),
                context: Vec::new(),
                reason: None,
            },
        )?;

        let conn = Connection::open(&db.store.db_path).map_err(|error| error.to_string())?;
        clear_extraction_chunks(&conn, &extraction.id)?;

        let reloaded = db.store.get_research_entry(&created.entry.entry.id, None)?;
        assert_eq!(reloaded.evidence.len(), 1);
        assert!(reloaded.evidence[0]
            .excerpt
            .contains("remain auditable after rechunking"));
        Ok(())
    }

    #[test]
    fn research_state_rejects_invalid_kind_status_combinations() -> StoreResult<()> {
        let db = test_db()?;
        for (kind, status) in [
            (ResearchEntryKind::Gap, EpistemicStatus::SourceSupported),
            (
                ResearchEntryKind::Hypothesis,
                EpistemicStatus::AgentSynthesis,
            ),
            (
                ResearchEntryKind::ExperimentIdea,
                EpistemicStatus::ResearcherContext,
            ),
        ] {
            let error = db
                .store
                .create_research_entry(
                    "project:attention",
                    0,
                    &ResearchEntryDraft {
                        kind,
                        epistemic_status: status,
                        text: "Invalid combination".to_string(),
                        evidence: Vec::new(),
                        relations: Vec::new(),
                        context: Vec::new(),
                        reason: None,
                    },
                )
                .expect_err("invalid epistemic combination must not commit");
            assert!(!error.is_empty());
        }
        assert_eq!(
            db.store
                .get_research_state("project:attention", None)?
                .revision,
            0
        );
        Ok(())
    }

    #[test]
    fn research_context_never_satisfies_source_evidence() -> StoreResult<()> {
        let db = test_db()?;
        let document = db.store.create_project_document(&ProjectDocumentDraft {
            project_id: "project:attention".to_string(),
            title: "Working notes.md".to_string(),
            content: "This is working context, not evidence.".to_string(),
        })?;
        let draft = ResearchEntryDraft {
            kind: ResearchEntryKind::Finding,
            epistemic_status: EpistemicStatus::SourceSupported,
            text: "A working claim".to_string(),
            evidence: Vec::new(),
            relations: Vec::new(),
            context: vec![ResearchContextLinkDraft {
                kind: ResearchContextKind::ProjectDocument,
                context_id: document.id,
                label: "Working notes".to_string(),
            }],
            reason: None,
        };
        let error = db
            .store
            .create_research_entry("project:attention", 0, &draft)
            .expect_err("context is qualitatively different from evidence");
        assert_eq!(error, "A source-supported Finding requires source evidence");
        Ok(())
    }

    #[test]
    fn research_state_revisions_are_historical_and_optimistic() -> StoreResult<()> {
        let db = test_db()?;
        let premise = db.store.create_research_entry(
            "project:attention",
            0,
            &ResearchEntryDraft {
                kind: ResearchEntryKind::Question,
                epistemic_status: EpistemicStatus::Speculative,
                text: "Do rank components specialize?".to_string(),
                evidence: Vec::new(),
                relations: Vec::new(),
                context: Vec::new(),
                reason: None,
            },
        )?;
        let premise_id = premise.entry.entry.id.clone();
        let synthesis = db.store.create_research_entry(
            "project:attention",
            1,
            &ResearchEntryDraft {
                kind: ResearchEntryKind::Finding,
                epistemic_status: EpistemicStatus::AgentSynthesis,
                text: "Existing work leaves component specialization unresolved.".to_string(),
                evidence: Vec::new(),
                relations: vec![EntryRelationDraft {
                    target_entry_id: premise_id.clone(),
                    kind: EntryRelationKind::DerivedFrom,
                }],
                context: Vec::new(),
                reason: Some("Synthesis from open question".to_string()),
            },
        )?;
        let synthesis_id = synthesis.entry.entry.id.clone();
        assert_eq!(synthesis.state.revision, 2);
        assert_eq!(synthesis.entry.relations.len(), 1);

        let revised = db.store.revise_research_entry(
            2,
            &ResearchEntryUpdate {
                id: synthesis_id.clone(),
                epistemic_status: EpistemicStatus::AgentSynthesis,
                text: "The bounded review leaves component specialization unresolved.".to_string(),
                evidence: Vec::new(),
                relations: vec![EntryRelationDraft {
                    target_entry_id: premise_id,
                    kind: EntryRelationKind::DerivedFrom,
                }],
                context: Vec::new(),
                reason: Some("Qualify bounded coverage".to_string()),
            },
        )?;
        assert_eq!(revised.state.revision, 3);
        assert_eq!(revised.entry.history.len(), 2);

        let historical = db.store.get_research_state("project:attention", Some(2))?;
        let old = historical
            .entries
            .iter()
            .find(|entry| entry.id == synthesis_id)
            .expect("entry existed at revision 2");
        assert_eq!(
            old.text,
            "Existing work leaves component specialization unresolved."
        );
        let current = db.store.get_research_state("project:attention", None)?;
        assert_eq!(current.revision, 3);
        assert!(current
            .entries
            .iter()
            .any(|entry| entry.text.starts_with("The bounded review")));

        let stale = db
            .store
            .create_research_entry(
                "project:attention",
                2,
                &ResearchEntryDraft {
                    kind: ResearchEntryKind::Hypothesis,
                    epistemic_status: EpistemicStatus::Speculative,
                    text: "Components may specialize.".to_string(),
                    evidence: Vec::new(),
                    relations: Vec::new(),
                    context: Vec::new(),
                    reason: None,
                },
            )
            .expect_err("stale edit must not overwrite revision 3");
        assert!(stale.contains("expected revision 2, current revision is 3"));

        let contested = db.store.set_research_entry_lifecycle(
            &synthesis_id,
            3,
            EntryLifecycle::Contested,
            "New evidence challenges the synthesis",
        )?;
        assert_eq!(contested.state.revision, 4);
        assert_eq!(contested.entry.entry.lifecycle, EntryLifecycle::Contested);
        assert_eq!(contested.entry.history.len(), 3);
        assert_eq!(
            db.store
                .get_research_entry(&synthesis_id, Some(2))?
                .entry
                .lifecycle,
            EntryLifecycle::Active
        );
        Ok(())
    }

    #[test]
    fn agent_state_update_is_atomic_and_idempotent() -> StoreResult<()> {
        let db = test_db()?;
        let invalid_batch = vec![
            agent_question("valid", "Which mechanism explains the result?"),
            agent_question("invalid", ""),
        ];
        db.store
            .apply_agent_state_update(
                "project:attention",
                0,
                "codex-test",
                None,
                "invalid-batch",
                "invalid-hash",
                invalid_batch,
            )
            .expect_err("one invalid operation must reject the batch");
        assert_eq!(
            db.store
                .get_research_state("project:attention", None)?
                .revision,
            0
        );

        let first = db.store.apply_agent_state_update(
            "project:attention",
            0,
            "codex-test",
            None,
            "request-1",
            "payload-a",
            vec![agent_question(
                "question",
                "Which mechanism explains the result?",
            )],
        )?;
        let retry = db.store.apply_agent_state_update(
            "project:attention",
            0,
            "codex-test",
            None,
            "request-1",
            "payload-a",
            vec![agent_question(
                "question",
                "Which mechanism explains the result?",
            )],
        )?;
        assert_eq!(first.revision, retry.revision);
        assert_eq!(first.created_entry_ids, retry.created_entry_ids);

        let changed_retry = db
            .store
            .apply_agent_state_update(
                "project:attention",
                0,
                "codex-test",
                None,
                "request-1",
                "payload-b",
                vec![agent_question("question", "A different question")],
            )
            .expect_err("a request id cannot be reused for another payload");
        assert!(changed_retry.contains("different payload"));
        let reopened = LibraryStore::for_test(db.store.db_path.clone());
        let state = reopened.get_research_state("project:attention", None)?;
        assert_eq!(state.revision, 1);
        assert_eq!(state.entries.len(), 1);
        Ok(())
    }

    #[test]
    fn concurrent_agent_state_updates_allow_one_revision() -> StoreResult<()> {
        let db = test_db()?;
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
        let handles = ["first", "second"].map(|request_id| {
            let store = db.store.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                store.apply_agent_state_update(
                    "project:attention",
                    0,
                    "codex-test",
                    None,
                    request_id,
                    request_id,
                    vec![agent_question(
                        request_id,
                        "What should we investigate next?",
                    )],
                )
            })
        });
        let results = handles.map(|handle| handle.join().expect("State writer panicked"));
        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
        let conflict = results
            .iter()
            .find_map(|result| result.as_ref().err())
            .expect("one writer must conflict");
        assert!(conflict.contains("expected revision 0, current revision is 1"));
        assert_eq!(
            db.store
                .get_research_state("project:attention", None)?
                .revision,
            1
        );
        Ok(())
    }

    #[test]
    fn agent_state_update_revises_content_and_lifecycle_without_changing_kind() -> StoreResult<()> {
        let db = test_db()?;
        let created = db.store.apply_agent_state_update(
            "project:attention",
            0,
            "codex-test",
            None,
            "create-question",
            "create-question",
            vec![agent_question(
                "question",
                "What explains the observed behavior?",
            )],
        )?;
        let entry_id = created.created_entry_ids["question"].clone();

        db.store.apply_agent_state_update(
            "project:attention",
            1,
            "codex-test",
            None,
            "revise-question",
            "revise-question",
            vec![AgentStateChange::Revise {
                update: ResearchEntryUpdate {
                    id: entry_id.clone(),
                    epistemic_status: EpistemicStatus::Speculative,
                    text: "Under which conditions does the behavior occur?".to_string(),
                    evidence: Vec::new(),
                    relations: Vec::new(),
                    context: Vec::new(),
                    reason: Some("Narrow the question".to_string()),
                },
                evidence_relationships: Vec::new(),
            }],
        )?;
        db.store.apply_agent_state_update(
            "project:attention",
            2,
            "codex-test",
            None,
            "contest-question",
            "contest-question",
            vec![AgentStateChange::SetLifecycle {
                entry_id: entry_id.clone(),
                lifecycle: EntryLifecycle::Contested,
                reason: "The premise is now disputed".to_string(),
            }],
        )?;

        let detail = db.store.get_research_entry(&entry_id, None)?;
        assert_eq!(detail.entry.kind, ResearchEntryKind::Question);
        assert_eq!(detail.entry.lifecycle, EntryLifecycle::Contested);
        assert_eq!(
            detail.entry.text,
            "Under which conditions does the behavior occur?"
        );
        assert_eq!(detail.history.len(), 3);
        Ok(())
    }

    #[test]
    fn every_research_document_shape_creates_an_ordinary_markdown_document() -> StoreResult<()> {
        let db = test_db()?;
        let created = db.store.create_research_entry(
            "project:attention",
            0,
            &ResearchEntryDraft {
                kind: ResearchEntryKind::Hypothesis,
                epistemic_status: EpistemicStatus::Speculative,
                text: "Rank components may specialize into distinct functions.".to_string(),
                evidence: Vec::new(),
                relations: Vec::new(),
                context: Vec::new(),
                reason: Some("Candidate explanation".to_string()),
            },
        )?;
        let entry_id = created.entry.entry.id;
        for shape in [
            ResearchDocumentShape::Survey,
            ResearchDocumentShape::RelatedWork,
            ResearchDocumentShape::ResearchGapAnalysis,
            ResearchDocumentShape::HypothesisReport,
            ResearchDocumentShape::ExperimentPlan,
            ResearchDocumentShape::Custom,
        ] {
            let generation = db.store.create_research_document_generation(
                &CreateFromResearchRequest {
                    project_id: "project:attention".to_string(),
                    state_revision: 1,
                    selected_entry_ids: vec![entry_id.clone()],
                    shape,
                    title: format!("{} output", shape.as_str()),
                    custom_instruction: None,
                    originating_run_id: None,
                    include_non_active: false,
                },
                None,
            )?;
            let ready = db
                .store
                .execute_research_document_generation(&generation.id)?;
            assert_eq!(ready.status, "ready");
            let document = db.store.get_project_document(
                ready
                    .resulting_document_id
                    .as_deref()
                    .expect("ready document id"),
            )?;
            assert_eq!(document.format, "markdown");
            assert!(!document.harness_writable);
            assert_eq!(document.created_from_state_revision, Some(1));
            assert_eq!(document.output_shape.as_deref(), Some(shape.as_str()));
            assert!(document
                .content
                .contains("Generated from Research State revision 1"));
            for heading in shape_outline(shape) {
                assert!(document.content.contains(&format!("## {heading}")));
            }
        }
        Ok(())
    }

    #[test]
    fn generation_pins_revision_and_cancelled_jobs_create_no_document() -> StoreResult<()> {
        let db = test_db()?;
        let first = db.store.create_research_entry(
            "project:attention",
            0,
            &ResearchEntryDraft {
                kind: ResearchEntryKind::Question,
                epistemic_status: EpistemicStatus::Speculative,
                text: "Which rank components carry interpretable functions?".to_string(),
                evidence: Vec::new(),
                relations: Vec::new(),
                context: Vec::new(),
                reason: None,
            },
        )?;
        let request = CreateFromResearchRequest {
            project_id: "project:attention".to_string(),
            state_revision: 1,
            selected_entry_ids: vec![first.entry.entry.id],
            shape: ResearchDocumentShape::Survey,
            title: "Pinned survey".to_string(),
            custom_instruction: None,
            originating_run_id: None,
            include_non_active: false,
        };
        let pinned = db
            .store
            .create_research_document_generation(&request, None)?;
        db.store.create_research_entry(
            "project:attention",
            1,
            &ResearchEntryDraft {
                kind: ResearchEntryKind::Gap,
                epistemic_status: EpistemicStatus::Speculative,
                text: "A later gap must not enter the pinned request.".to_string(),
                evidence: Vec::new(),
                relations: Vec::new(),
                context: Vec::new(),
                reason: None,
            },
        )?;
        let ready = db.store.execute_research_document_generation(&pinned.id)?;
        let document = db.store.get_project_document(
            ready
                .resulting_document_id
                .as_deref()
                .expect("ready document id"),
        )?;
        assert!(!document.content.contains("later gap"));
        assert_eq!(document.created_from_state_revision, Some(1));

        let mut cancelled_request = request;
        cancelled_request.title = "Cancelled survey".to_string();
        let cancelled = db
            .store
            .create_research_document_generation(&cancelled_request, None)?;
        db.store
            .cancel_research_document_generation(&cancelled.id)?;
        let final_state = db
            .store
            .execute_research_document_generation(&cancelled.id)?;
        assert_eq!(final_state.status, "cancelled");
        assert!(final_state.resulting_document_id.is_none());
        assert_eq!(db.store.get_library()?.project_documents.len(), 1);
        Ok(())
    }

    #[test]
    fn generated_citations_are_local_unambiguous_and_metadata_stable() -> StoreResult<()> {
        let db = test_db()?;
        let mut entry_ids = Vec::new();
        for (index, paper_id) in ["citation-paper-a", "citation-paper-b"].iter().enumerate() {
            let extraction = extracted_paper(
                &db,
                paper_id,
                &[(
                    "paragraph",
                    "Low-rank adaptation constrains the update subspace.",
                )],
            )?;
            let chunk = db.store.chunks_for_extraction(&extraction.id)?.remove(0);
            let created = db.store.create_research_entry(
                "project:attention",
                index as i64,
                &ResearchEntryDraft {
                    kind: ResearchEntryKind::Finding,
                    epistemic_status: EpistemicStatus::SourceSupported,
                    text: format!("Source-supported finding {}.", index + 1),
                    evidence: vec![EvidenceLinkDraft {
                        chunk_id: chunk.id,
                        excerpt: None,
                        support_note: None,
                    }],
                    relations: Vec::new(),
                    context: Vec::new(),
                    reason: None,
                },
            )?;
            entry_ids.push(created.entry.entry.id);
        }
        let generation = db.store.create_research_document_generation(
            &CreateFromResearchRequest {
                project_id: "project:attention".to_string(),
                state_revision: 2,
                selected_entry_ids: entry_ids,
                shape: ResearchDocumentShape::Survey,
                title: "Cited survey".to_string(),
                custom_instruction: None,
                originating_run_id: None,
                include_non_active: false,
            },
            None,
        )?;
        let ready = db
            .store
            .execute_research_document_generation(&generation.id)?;
        let document_id = ready.resulting_document_id.expect("ready document id");
        let before = db.store.get_project_document(&document_id)?;
        assert_eq!(before.citations.len(), 2);
        assert_ne!(
            before.citations[0].citation_key,
            before.citations[1].citation_key
        );
        assert!(before
            .citations
            .iter()
            .all(|citation| !citation.evidence_link_ids.is_empty()));

        let conn = db.store.open_connection()?;
        conn.execute(
            "update papers set title = 'Changed canonical title', authors_json = '[\"Different Author\"]', year = 2030 where id = 'citation-paper-a'",
            [],
        )
        .map_err(|error| error.to_string())?;
        let after = db.store.get_project_document(&document_id)?;
        assert_eq!(after.citations, before.citations);
        Ok(())
    }

    #[test]
    fn generation_validation_rejects_unknown_and_missing_citations() {
        let empty_entries = Vec::new();
        let empty_citations = Vec::new();
        assert!(validate_generated_document(
            "A claim [@invented].",
            &empty_entries,
            &empty_citations,
        )
        .is_err());
    }

    #[test]
    fn failed_generation_transaction_leaves_no_partial_document() -> StoreResult<()> {
        let db = test_db()?;
        let extraction = extracted_paper(
            &db,
            "disappearing-citation-paper",
            &[(
                "paragraph",
                "This evidence will become unresolvable before generation.",
            )],
        )?;
        let chunk = db.store.chunks_for_extraction(&extraction.id)?.remove(0);
        let entry = db.store.create_research_entry(
            "project:attention",
            0,
            &ResearchEntryDraft {
                kind: ResearchEntryKind::Finding,
                epistemic_status: EpistemicStatus::SourceSupported,
                text: "A factual claim needs its resolvable source.".to_string(),
                evidence: vec![EvidenceLinkDraft {
                    chunk_id: chunk.id,
                    excerpt: None,
                    support_note: None,
                }],
                relations: Vec::new(),
                context: Vec::new(),
                reason: None,
            },
        )?;
        let generation = db.store.create_research_document_generation(
            &CreateFromResearchRequest {
                project_id: "project:attention".to_string(),
                state_revision: 1,
                selected_entry_ids: vec![entry.entry.entry.id],
                shape: ResearchDocumentShape::Survey,
                title: "Must fail atomically".to_string(),
                custom_instruction: None,
                originating_run_id: None,
                include_non_active: false,
            },
            None,
        )?;
        db.store
            .delete_paper_globally("disappearing-citation-paper")?;
        let error = db
            .store
            .execute_research_document_generation(&generation.id)
            .expect_err("missing source evidence must fail before document creation");
        assert!(error.contains("lacks evidence") || error.contains("no longer resolves"));
        assert!(db.store.get_library()?.project_documents.is_empty());
        assert!(db
            .store
            .get_research_document_generation(&generation.id)?
            .resulting_document_id
            .is_none());
        Ok(())
    }

    #[test]
    fn only_ready_harness_runs_publish_state_atomically() -> StoreResult<()> {
        let db = test_db()?;
        db.store.save_harness_configuration(
            "project:attention",
            &HarnessConfiguration {
                goal: "Understand rank component specialization".to_string(),
                ..HarnessConfiguration::default()
            },
        )?;
        let search = db.store.create_search(&sample_search_draft())?;
        let harness_run = db
            .store
            .create_harness_run("project:attention", &search.id)?;
        let draft = ResearchEntryDraft {
            kind: ResearchEntryKind::Hypothesis,
            epistemic_status: EpistemicStatus::Speculative,
            text: "Rank components may learn distinct functions.".to_string(),
            evidence: Vec::new(),
            relations: Vec::new(),
            context: Vec::new(),
            reason: Some("Run synthesis".to_string()),
        };
        assert!(db
            .store
            .publish_harness_run_research_state(&harness_run.id, 0, &[draft.clone()])
            .is_err());
        assert_eq!(
            db.store
                .get_research_state("project:attention", None)?
                .revision,
            0
        );

        let search_run = db.store.create_search_run(&search.id, "project_harness")?;
        db.store
            .attach_harness_search_run(&harness_run.id, &search_run.id)?;
        db.store.set_search_run_status(
            &search_run.id,
            SearchRunStatus::Ready,
            1,
            Some("converged"),
            None,
            true,
        )?;
        let published =
            db.store
                .publish_harness_run_research_state(&harness_run.id, 0, &[draft])?;
        assert_eq!(published.revision, 1);
        assert_eq!(published.entries.len(), 1);
        let harness = db.store.get_harness_snapshot("project:attention")?;
        assert_eq!(harness.runs[0].starting_state_revision, 0);
        assert_eq!(harness.runs[0].resulting_state_revision, Some(1));
        assert!(harness
            .events
            .iter()
            .any(|event| event.kind == "research_state_published"));
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

    fn quick_agent_search_draft() -> SearchDraft {
        SearchDraft {
            title: "Focused child search".to_string(),
            goal: "Find direct evidence for one focused question".to_string(),
            constraints: SearchConstraints {
                target_count: 10,
                ..sample_constraints()
            },
            strategy: Depth::Quick.budget(),
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

    #[test]
    fn agent_search_start_is_idempotent_and_reserves_parent_budget() -> StoreResult<()> {
        let db = test_db()?;
        let library = db.store.get_library()?;
        let vault = library.vaults.first().expect("seed vault");
        let mut configuration = db
            .store
            .get_harness_snapshot(&vault.project_id)?
            .harness
            .configuration;
        configuration.research_instructions = "Investigate the fixture topic".to_string();
        db.store
            .save_harness_configuration(&vault.project_id, &configuration)?;
        let parent_search = db.store.create_search(&sample_search_draft())?;
        let parent = db
            .store
            .create_harness_run(&vault.project_id, &parent_search.id)?;
        let draft = quick_agent_search_draft();

        let first = db.store.create_agent_search_run(
            &vault.project_id,
            &vault.id,
            "codex-test",
            Some(&parent.id),
            "request-1",
            "payload-1",
            None,
            &draft,
            10,
        )?;
        let retry = db.store.create_agent_search_run(
            &vault.project_id,
            &vault.id,
            "codex-test",
            Some(&parent.id),
            "request-1",
            "payload-1",
            None,
            &draft,
            10,
        )?;
        assert_eq!(first.run_id, retry.run_id);
        assert!(db
            .store
            .create_agent_search_run(
                &vault.project_id,
                &vault.id,
                "codex-test",
                Some(&parent.id),
                "request-1",
                "changed-payload",
                None,
                &draft,
                10,
            )
            .expect_err("changed retry must conflict")
            .contains("different payload"));

        db.store.create_agent_search_run(
            &vault.project_id,
            &vault.id,
            "codex-test",
            Some(&parent.id),
            "request-2",
            "payload-2",
            None,
            &draft,
            10,
        )?;
        assert!(db
            .store
            .create_agent_search_run(
                &vault.project_id,
                &vault.id,
                "codex-test",
                Some(&parent.id),
                "request-3",
                "payload-3",
                None,
                &draft,
                10,
            )
            .expect_err("third reservation exceeds the parent provider budget")
            .contains("budget is exhausted"));
        Ok(())
    }

    #[test]
    fn agent_search_events_page_stably_and_cancel_only_once() -> StoreResult<()> {
        let db = test_db()?;
        let library = db.store.get_library()?;
        let vault = library.vaults.first().expect("seed vault");
        let receipt = db.store.create_agent_search_run(
            &vault.project_id,
            &vault.id,
            "external-test",
            None,
            "request-events",
            "payload-events",
            None,
            &quick_agent_search_draft(),
            2,
        )?;
        db.store.append_agent_search_candidates(
            &receipt.run_id,
            "preview",
            &[
                sample_candidate("First", Some("10.1/first")),
                sample_candidate("Second", Some("10.1/second")),
            ],
        )?;
        let high_water = db
            .store
            .agent_search_candidate_high_water(&receipt.run_id)?;
        let first_page =
            db.store
                .list_agent_search_candidate_events(&receipt.run_id, 0, high_water, 1)?;
        db.store.append_agent_search_candidates(
            &receipt.run_id,
            "ranked",
            &[sample_candidate("Third", Some("10.1/third"))],
        )?;
        let second_page = db.store.list_agent_search_candidate_events(
            &receipt.run_id,
            first_page[0].sequence,
            high_water,
            10,
        )?;
        assert_eq!(first_page[0].candidate.title, "First");
        assert_eq!(second_page.len(), 1);
        assert_eq!(second_page[0].candidate.title, "Second");

        assert!(
            db.store
                .request_agent_search_cancel(&vault.project_id, &receipt.run_id)?
                .1
        );
        assert!(
            !db.store
                .request_agent_search_cancel(&vault.project_id, &receipt.run_id)?
                .1
        );
        assert_eq!(
            db.store
                .list_agent_search_activity(&receipt.run_id, 10)?
                .iter()
                .filter(|event| event.kind == "cancellation_requested")
                .count(),
            1
        );
        assert!(db
            .store
            .get_agent_search_run("project:missing", &receipt.run_id)
            .is_err());
        Ok(())
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

    fn managed_harness_run(db: &TestDb, limits: &AgentRunLimits) -> StoreResult<HarnessRun> {
        let mut configuration = db
            .store
            .get_harness_snapshot("project:attention")?
            .harness
            .configuration;
        configuration.research_instructions = "Investigate grounded evidence".to_string();
        db.store
            .save_harness_configuration("project:attention", &configuration)?;
        let search = db.store.create_search(&sample_search_draft())?;
        let run = db
            .store
            .create_harness_run("project:attention", &search.id)?;
        db.store
            .configure_codex_harness_run(&run.id, "test-model", limits)
    }

    fn research_outcome(summary: &str) -> ResearchRunOutcome {
        ResearchRunOutcome {
            summary: summary.to_string(),
            display_items: Vec::new(),
            paper_dispositions: Vec::new(),
            state_synthesis: Some(crate::domain::harness::ResearchStateSynthesis {
                changes: Vec::new(),
                unresolved_entry_ids: Vec::new(),
                next_direction_entry_ids: Vec::new(),
                no_change_reason: Some("No additional State entry was warranted".to_string()),
                resulting_revision: None,
                created_entry_ids: HashMap::new(),
            }),
            task_outcomes: Vec::new(),
            unanswered_questions: vec!["What should be tested next?".to_string()],
            next_direction: None,
        }
    }

    /// Return an exact extracted source anchor for delivery contract tests.
    fn passage_anchor_fixture(
        db: &TestDb,
        paper_id: &str,
        reference: &str,
    ) -> StoreResult<HashMap<String, crate::services::mcp::PassageAnchor>> {
        let extraction = extracted_paper(db, paper_id, &[("paragraph", "Exact source text.")])?;
        let chunk = db.store.chunks_for_extraction(&extraction.id)?.remove(0);
        Ok(HashMap::from([(
            reference.into(),
            crate::services::mcp::PassageAnchor {
                paper_id: chunk.paper_id,
                source_id: chunk.source_id,
                extraction_id: Some(chunk.extraction_id),
                chunk_id: Some(chunk.id),
                page_start: Some(1),
                page_end: Some(1),
                source_start: chunk.source_start,
                source_end: chunk.source_end,
                quote: chunk.text,
            },
        )]))
    }

    /// Upgrade an already populated anchor table, not only a fresh database.
    #[test]
    fn passage_delivery_migrates_legacy_anchor_versions() -> StoreResult<()> {
        let (db, run, proposal, anchors) = synthesis_fixture()?;
        db.store
            .open_connection()?
            .execute_batch("alter table agent_passage_anchors drop column source_version;")
            .map_err(|e| e.to_string())?;
        db.store.init()?;
        db.store.init()?;
        let conn = db.store.open_connection()?;
        let version: String = conn
            .query_row(
                "select source_version from agent_passage_anchors where run_id=?1",
                [&run.id],
                |row| row.get(0),
            )
            .map_err(|e| e.to_string())?;
        assert_eq!(version, "", "Do not invent provenance for existing anchors");
        db.store
            .save_synthesis_attempt(&run.id, 0, &proposal.to_string(), &anchors)?;
        assert!(db
            .store
            .preflight_synthesis(&run.id, 0)
            .unwrap_err()
            .contains("Source/project conflict"));
        Ok(())
    }

    /// Anchor failures roll back successes, while attempted work remains charged.
    #[test]
    fn passage_delivery_rolls_back_reader_and_question_successes() -> StoreResult<()> {
        for question in [false, true] {
            let db = test_db()?;
            let anchors = passage_anchor_fixture(&db, "vaswani2017", "passage:atomic")?;
            let run = managed_harness_run(&db, &AgentRunLimits::default())?;
            let conn = db.store.open_connection()?;
            conn.execute_batch(
                "create trigger reject_anchor before insert on agent_passage_anchors
                begin select raise(abort, 'injected anchor failure'); end;",
            )
            .map_err(|e| e.to_string())?;
            let chars = anchors["passage:atomic"].quote.chars().count() as u64;
            let result = if question {
                db.store.admit_agent_evidence_question(
                    &run.id,
                    &run.project_id,
                    &[AgentQuestionDelivery {
                        paper_id: "vaswani2017".into(),
                        returned_text_chars: chars,
                    }],
                )?;
                db.store
                    .record_agent_question_citations(&run.id, &run.project_id, &anchors)
            } else {
                db.store.record_agent_reader_delivery(
                    &run.id,
                    &run.project_id,
                    "vaswani2017",
                    chars,
                    &anchors,
                )
            };
            assert!(result.unwrap_err().contains("injected anchor failure"));
            assert_eq!(read_agent_delivery_counts(&conn, &run.id)?, (1, 0));
            let checkpoint = db.store.get_research_checkpoint(&run.id)?;
            assert_eq!(checkpoint.read_paper_count, 0);
            for table in ["agent_reader_passages", "agent_passage_anchors"] {
                let count: i64 = conn
                    .query_row(
                        &format!("select count(*) from {table} where run_id=?1"),
                        [&run.id],
                        |row| row.get(0),
                    )
                    .map_err(|e| e.to_string())?;
                assert_eq!(count, 0, "{table} leaked a successful delivery");
            }
            let events = db.store.get_harness_snapshot(&run.project_id)?.events;
            assert!(!events.iter().any(|e| matches!(
                e.kind.as_str(),
                "agent_passages_delivered" | "agent_evidence_question_answered"
            )));
            let charged: i64 = conn
                .query_row(
                    "select returned_text_chars from agent_reader_usage where run_id=?1",
                    [&run.id],
                    |row| row.get(0),
                )
                .map_err(|e| e.to_string())?;
            assert_eq!(charged, chars as i64);
            assert_eq!(
                db.store.get_harness_run(&run.id)?.llm_call_count,
                u32::from(question)
            );

            conn.execute_batch("drop trigger reject_anchor;")
                .map_err(|e| e.to_string())?;
            if question {
                db.store
                    .record_agent_question_citations(&run.id, &run.project_id, &anchors)?;
            } else {
                db.store.record_agent_reader_delivery(
                    &run.id,
                    &run.project_id,
                    "vaswani2017",
                    chars,
                    &anchors,
                )?;
            }
            assert_eq!(read_agent_delivery_counts(&conn, &run.id)?, (1, 1));
            assert_eq!(
                db.store.captured_run_evidence(&run.id)?["passage:atomic"].quote,
                anchors["passage:atomic"].quote
            );
            db.store.begin_codex_harness_finalization(&run.id)?;
            let retention = db.store.finalize_agent_paper_retention(&run.id, &[])?;
            assert_eq!((retention.attempted, retention.read), (1, 1));
        }
        Ok(())
    }

    /// Build model JSON and a genuinely delivered PDF passage for finalization tests.
    fn synthesis_fixture() -> StoreResult<(
        TestDb,
        HarnessRun,
        serde_json::Value,
        HashMap<String, crate::services::mcp::PassageAnchor>,
    )> {
        let db = test_db()?;
        let extraction = extracted_paper(
            &db,
            "vaswani2017",
            &[("paragraph", "The effect was evaluated on two benchmarks.")],
        )?;
        let chunk = db.store.chunks_for_extraction(&extraction.id)?.remove(0);
        let run = managed_harness_run(&db, &AgentRunLimits::default())?;
        let reference = "passage:delivered".to_string();
        let anchors = HashMap::from([(
            reference.clone(),
            crate::services::mcp::PassageAnchor {
                paper_id: chunk.paper_id,
                source_id: chunk.source_id,
                extraction_id: Some(chunk.extraction_id),
                chunk_id: Some(chunk.id),
                page_start: Some(1),
                page_end: Some(1),
                source_start: chunk.source_start,
                source_end: chunk.source_end,
                quote: chunk.text,
            },
        )]);
        db.store.record_agent_reader_delivery(
            &run.id,
            &run.project_id,
            "vaswani2017",
            anchors[&reference].quote.chars().count() as u64,
            &anchors,
        )?;
        let mut proposal = serde_json::to_value(research_outcome(
            "A bounded finding and a follow-up question",
        ))
        .map_err(|e| e.to_string())?;
        proposal["stateSynthesis"] = serde_json::json!({
            "changes":[
                {"operation":"create","handle":"f","kind":"finding","epistemicStatus":"source_supported","statement":"The effect was evaluated on two benchmarks.","evidence":[{"passageRef":reference,"relationship":"supports","explanation":"The source states its evaluation scope"}],"relations":[],"reason":"New evidence"},
                {"operation":"create","handle":"g","kind":"gap","epistemicStatus":"agent_synthesis","statement":"Beyond these two benchmarks, generalization remains untested in this corpus.","evidence":[],"relations":[{"target":"f","kind":"derived_from"}],"reason":"Bounded synthesis"}
            ],"unresolvedEntryIds":["g"],"nextDirectionEntryIds":["g"],"noChangeReason":null
        });
        proposal["nextDirection"] = serde_json::json!("Investigate other benchmarks");
        proposal["taskOutcomes"] = serde_json::json!([{"searchRunIds":[],"motivatingEntryIds":["g"],"learnedPoints":["The evidence has bounded coverage"],"citedPassageRefs":[reference]}]);
        db.store.begin_codex_harness_finalization(&run.id)?;
        Ok((db, run, proposal, anchors))
    }

    #[test]
    fn synthesis_finalization_preflights_commits_and_replays_once() -> StoreResult<()> {
        let (db, run, proposal, anchors) = synthesis_fixture()?;
        db.store
            .save_synthesis_attempt(&run.id, 0, &proposal.to_string(), &anchors)?;
        db.store.preflight_synthesis(&run.id, 0)?;
        assert_eq!(
            db.store.get_research_state(&run.project_id, None)?.revision,
            0
        );
        assert_eq!(db.store.get_harness_run(&run.id)?.status, "reconciling");
        let complete = db.store.finalize_synthesis(&run.id, 0, true)?;
        assert_eq!(complete.status, "ready");
        assert_eq!(complete.resulting_state_revision, Some(1));
        assert_eq!(
            db.store.finalize_synthesis(&run.id, 0, true)?.id,
            complete.id
        );
        let history = db
            .store
            .list_recent_agent_run_outcomes(&run.project_id, 3, 12000)?;
        assert_eq!(history.len(), 1);
        assert_ne!(
            history[0].outcome.task_outcomes[0].motivating_entry_ids[0],
            "g"
        );
        assert_eq!(
            db.store
                .get_research_state(&run.project_id, None)?
                .entries
                .len(),
            2
        );
        Ok(())
    }

    #[test]
    fn synthesis_rejects_realistic_invalid_proposals_before_any_commit() -> StoreResult<()> {
        for case in [
            "direct_evidence_on_gap",
            "missing_evidence",
            "missing_premise",
            "unknown_ref",
            "cycle",
            "duplicate_handle",
            "duplicate_statement",
            "orphan_direction",
            "unread_passage",
            "duplicate_chunk",
            "oversize_text",
            "whitespace",
            "invented_disposition",
            "backend_facts",
            "unknown_enum",
            "self_reference",
            "conflicting_operations",
            "context_only",
        ] {
            let (db, run, mut proposal, anchors) = synthesis_fixture()?;
            match case {
                "direct_evidence_on_gap" => {
                    proposal["stateSynthesis"]["changes"][1]["evidence"] =
                        proposal["stateSynthesis"]["changes"][0]["evidence"].clone()
                }
                "missing_evidence" => {
                    proposal["stateSynthesis"]["changes"][0]["evidence"] = serde_json::json!([])
                }
                "missing_premise" => {
                    proposal["stateSynthesis"]["changes"][1]["relations"] = serde_json::json!([])
                }
                "unknown_ref" => {
                    proposal["stateSynthesis"]["nextDirectionEntryIds"] =
                        serde_json::json!(["invented"])
                }
                "cycle" => {
                    proposal["stateSynthesis"]["changes"][0]["relations"] =
                        serde_json::json!([{"target":"g","kind":"derived_from"}])
                }
                "duplicate_handle" => {
                    proposal["stateSynthesis"]["changes"][1]["handle"] = serde_json::json!("f")
                }
                "duplicate_statement" => {
                    proposal["stateSynthesis"]["changes"][1]["statement"] =
                        proposal["stateSynthesis"]["changes"][0]["statement"].clone()
                }
                "orphan_direction" => proposal["nextDirection"] = serde_json::Value::Null,
                "unread_passage" => {
                    proposal["stateSynthesis"]["changes"][0]["evidence"][0]["passageRef"] =
                        serde_json::json!("invented")
                }
                "duplicate_chunk" => {
                    let e = proposal["stateSynthesis"]["changes"][0]["evidence"][0].clone();
                    proposal["stateSynthesis"]["changes"][0]["evidence"]
                        .as_array_mut()
                        .unwrap()
                        .push(e);
                }
                "oversize_text" => proposal["nextDirection"] = serde_json::json!("x".repeat(1001)),
                "whitespace" => proposal["summary"] = serde_json::json!("  "),
                "invented_disposition" => {
                    proposal["paperDispositions"] = serde_json::json!([{"paperId":"invented","disposition":"background","reason":"Example"}])
                }
                "backend_facts" => {
                    proposal["stateSynthesis"]["resultingRevision"] = serde_json::json!(42)
                }
                "unknown_enum" => {
                    proposal["stateSynthesis"]["changes"][0]["epistemicStatus"] =
                        serde_json::json!("proven")
                }
                "self_reference" => {
                    proposal["stateSynthesis"]["changes"][1]["relations"] =
                        serde_json::json!([{"target":"g","kind":"derived_from"}])
                }
                "conflicting_operations" => {
                    proposal["stateSynthesis"]["changes"] = serde_json::json!([
                        {"operation":"set_lifecycle","entryId":"same","lifecycle":"contested","reason":"First"},
                        {"operation":"set_lifecycle","entryId":"same","lifecycle":"superseded","reason":"Second"}
                    ])
                }
                "context_only" => {}
                _ => unreachable!(),
            }
            let mut anchors = anchors;
            if case == "context_only" {
                anchors.values_mut().next().unwrap().chunk_id = None;
            }
            db.store
                .save_synthesis_attempt(&run.id, 0, &proposal.to_string(), &anchors)?;
            assert!(
                db.store.preflight_synthesis(&run.id, 0).is_err(),
                "{case} must fail"
            );
            assert_eq!(
                db.store.get_research_state(&run.project_id, None)?.revision,
                0,
                "{case}"
            );
            assert_eq!(db.store.get_harness_run(&run.id)?.status, "reconciling");
        }
        Ok(())
    }

    #[test]
    fn synthesis_transaction_rolls_back_at_every_write_stage() -> StoreResult<()> {
        for (table, operation) in [
            ("research_state_revisions", "insert"),
            ("research_entries", "insert"),
            ("research_entry_revisions", "insert"),
            ("research_evidence_links", "insert"),
            ("research_entry_relations", "insert"),
            ("harness_reflections", "insert"),
            ("harness_runs", "update"),
            ("research_synthesis_attempts", "update"),
        ] {
            let (db, run, proposal, anchors) = synthesis_fixture()?;
            db.store
                .save_synthesis_attempt(&run.id, 0, &proposal.to_string(), &anchors)?;
            db.store.preflight_synthesis(&run.id, 0)?;
            db.store.open_connection()?.execute_batch(&format!("create trigger fail_finalization before {operation} on {table} begin select raise(abort,'injected failure'); end;")).map_err(|e|e.to_string())?;
            assert!(
                db.store.finalize_synthesis(&run.id, 0, true).is_err(),
                "{table}"
            );
            assert_eq!(
                db.store.get_research_state(&run.project_id, None)?.revision,
                0,
                "{table}"
            );
            assert_eq!(db.store.get_harness_run(&run.id)?.status, "reconciling");
            let reflections: i64 = db
                .store
                .open_connection()?
                .query_row(
                    "select count(*) from harness_reflections where run_id=?1",
                    [&run.id],
                    |r| r.get(0),
                )
                .map_err(|e| e.to_string())?;
            assert_eq!(reflections, 0, "{table}");
        }
        Ok(())
    }

    #[test]
    fn synthesis_restart_recovers_only_preflighted_proposals() -> StoreResult<()> {
        let (db, run, proposal, anchors) = synthesis_fixture()?;
        db.store
            .save_synthesis_attempt(&run.id, 0, &proposal.to_string(), &anchors)?;
        db.store.preflight_synthesis(&run.id, 0)?;
        db.store.recover_interrupted_harness_runs()?;
        assert_eq!(db.store.get_harness_run(&run.id)?.status, "ready");
        db.store.recover_interrupted_harness_runs()?;
        assert_eq!(
            db.store.get_research_state(&run.project_id, None)?.revision,
            1
        );
        Ok(())
    }

    #[test]
    fn synthesis_cancelled_after_preflight_cannot_commit() -> StoreResult<()> {
        let (db, run, proposal, anchors) = synthesis_fixture()?;
        db.store
            .save_synthesis_attempt(&run.id, 0, &proposal.to_string(), &anchors)?;
        db.store.preflight_synthesis(&run.id, 0)?;
        db.store
            .request_codex_harness_cancellation(&run.project_id, "cancelled_by_user")?;
        assert!(db.store.finalize_synthesis(&run.id, 0, true).is_err());
        assert_eq!(
            db.store.get_research_state(&run.project_id, None)?.revision,
            0
        );
        Ok(())
    }

    #[test]
    fn synthesis_rechecks_source_and_vault_versions_before_commit() -> StoreResult<()> {
        for conflict in ["extraction", "source", "vault", "state"] {
            let (db, run, proposal, anchors) = synthesis_fixture()?;
            db.store
                .save_synthesis_attempt(&run.id, 0, &proposal.to_string(), &anchors)?;
            db.store.preflight_synthesis(&run.id, 0)?;
            let conn = db.store.open_connection()?;
            let anchor = anchors.values().next().unwrap();
            match conflict {
                "extraction" => {
                    conn.execute(
                        "update document_extractions set updated_at='changed' where id=?1",
                        [anchor.extraction_id.as_ref().unwrap()],
                    )
                    .map_err(|e| e.to_string())?;
                }
                "source" => {
                    conn.execute(
                        "update document_sources set status='failed' where id=?1",
                        [&anchor.source_id],
                    )
                    .map_err(|e| e.to_string())?;
                }
                "vault" => {
                    conn.execute("update vaults set membership_revision=membership_revision+1 where project_id=?1",[&run.project_id]).map_err(|e|e.to_string())?;
                }
                "state" => {
                    db.store.create_research_entry(
                        &run.project_id,
                        0,
                        &ResearchEntryDraft {
                            kind: ResearchEntryKind::Question,
                            epistemic_status: EpistemicStatus::ResearcherContext,
                            text: "A new user question".into(),
                            evidence: vec![],
                            relations: vec![],
                            context: vec![],
                            reason: None,
                        },
                    )?;
                }
                _ => unreachable!(),
            }
            assert!(
                db.store.finalize_synthesis(&run.id, 0, true).is_err(),
                "{conflict}"
            );
            assert_eq!(
                db.store.get_research_state(&run.project_id, None)?.revision,
                if conflict == "state" { 1 } else { 0 }
            );
            assert_eq!(
                db.store.synthesis_attempt_reports(&run.id)?[0]["committed"],
                false
            );
            if conflict == "vault" {
                db.store
                    .save_synthesis_attempt(&run.id, 1, &proposal.to_string(), &anchors)?;
                assert!(db
                    .store
                    .preflight_synthesis(&run.id, 1)
                    .unwrap_err()
                    .contains("Vault changed"));
            }
        }
        Ok(())
    }

    #[test]
    fn synthesis_large_valid_artifact_keeps_a_small_continuation() -> StoreResult<()> {
        let (db, run, mut proposal, anchors) = synthesis_fixture()?;
        for index in 0..12 {
            proposal["stateSynthesis"]["changes"].as_array_mut().unwrap().push(serde_json::json!({
                "operation":"create","handle":format!("q{index}"),"kind":"question","epistemicStatus":"speculative",
                "statement":format!("Question {index}: {}", "bounded context ".repeat(200)),"evidence":[],"relations":[],"reason":"Explicit unresolved question"
            }));
        }
        let raw = proposal.to_string();
        assert!(
            raw.len() > 12000
                && raw.len() < crate::services::research::synthesis::MAX_PROPOSAL_BYTES
        );
        db.store
            .save_synthesis_attempt(&run.id, 0, &raw, &anchors)?;
        db.store.preflight_synthesis(&run.id, 0)?;
        db.store.finalize_synthesis(&run.id, 0, true)?;
        let history = db
            .store
            .list_recent_agent_run_outcomes(&run.project_id, 3, 12000)?;
        assert_eq!(history.len(), 1);
        assert!(history[0].outcome.state_synthesis.is_none());
        assert_eq!(
            db.store
                .get_research_state(&run.project_id, None)?
                .entries
                .len(),
            14
        );
        assert!(db
            .store
            .get_research_checkpoint(&run.id)?
            .outcome
            .unwrap()
            .state_synthesis
            .is_some());
        Ok(())
    }

    #[test]
    fn synthesis_no_change_and_interrupted_unvalidated_proposals_are_explicit() -> StoreResult<()> {
        for prepared in [false, true] {
            let db = test_db()?;
            let run = managed_harness_run(&db, &AgentRunLimits::default())?;
            db.store.begin_codex_harness_finalization(&run.id)?;
            let proposal =
                serde_json::to_string(&research_outcome("Evidence was insufficient")).unwrap();
            db.store
                .save_synthesis_attempt(&run.id, 0, &proposal, &HashMap::new())?;
            if prepared {
                db.store.preflight_synthesis(&run.id, 0)?;
            }
            db.store.recover_interrupted_harness_runs()?;
            assert_eq!(
                db.store.get_harness_run(&run.id)?.status,
                if prepared { "ready" } else { "failed" }
            );
            assert_eq!(
                db.store.get_research_state(&run.project_id, None)?.revision,
                0
            );
            assert_eq!(
                db.store.get_research_checkpoint(&run.id)?.outcome.is_some(),
                prepared
            );
        }
        Ok(())
    }

    #[test]
    fn synthesis_retention_rolls_back_with_report_failure_and_replays_once() -> StoreResult<()> {
        for (table, operation) in [
            ("vault_papers", "delete"),
            ("vaults", "update"),
            ("agent_paper_dispositions", "insert"),
            ("harness_reflections", "insert"),
        ] {
            let db = test_db()?;
            let run = managed_harness_run(&db, &AgentRunLimits::default())?;
            let paper =
                paper_draft_with_pdf("temporary-addition", "https://example.test/paper.pdf");
            db.store.add_agent_search_candidate_to_vault(
                &run.project_id,
                "attention",
                "codex-test",
                Some(&run.id),
                "addition",
                "payload",
                &paper,
            )?;
            db.store.record_agent_reader_delivery(
                &run.id,
                &run.project_id,
                &paper.id,
                0,
                &HashMap::new(),
            )?;
            db.store.begin_codex_harness_finalization(&run.id)?;
            let mut proposal = research_outcome("The candidate was not relevant");
            proposal.paper_dispositions.push(ResearchPaperDisposition {
                paper_id: paper.id.clone(),
                disposition: PaperDispositionKind::Irrelevant,
                reason: "Outside scope".into(),
            });
            db.store.save_synthesis_attempt(
                &run.id,
                0,
                &serde_json::to_string(&proposal).unwrap(),
                &HashMap::new(),
            )?;
            db.store.preflight_synthesis(&run.id, 0)?;
            let conn = db.store.open_connection()?;
            conn.execute_batch(&format!("create trigger fail_retention before {operation} on {table} begin select raise(abort,'injected retention failure'); end;")).map_err(|e|e.to_string())?;
            assert!(
                db.store.finalize_synthesis(&run.id, 0, true).is_err(),
                "{table}"
            );
            assert!(db
                .store
                .get_library()?
                .vault_papers
                .iter()
                .any(|p| p.paper_id == paper.id && p.vault_id == "attention"));
            let disposition_count: i64 = conn
                .query_row(
                    "select count(*) from agent_paper_dispositions where run_id=?1",
                    [&run.id],
                    |r| r.get(0),
                )
                .map_err(|e| e.to_string())?;
            assert_eq!(disposition_count, 0, "{table}");
            conn.execute_batch("drop trigger fail_retention")
                .map_err(|e| e.to_string())?;
            assert_eq!(
                db.store.finalize_synthesis(&run.id, 0, true)?.status,
                "ready"
            );
            assert_eq!(
                db.store.finalize_synthesis(&run.id, 0, true)?.status,
                "ready"
            );
            assert!(!db
                .store
                .get_library()?
                .vault_papers
                .iter()
                .any(|p| p.paper_id == paper.id && p.vault_id == "attention"));
        }
        Ok(())
    }

    #[test]
    fn synthesis_revision_checks_existing_kind_and_duplicate_statement() -> StoreResult<()> {
        for wrong_kind in [false, true] {
            let db = test_db()?;
            let mut entry_ids = Vec::new();
            for index in 0..2 {
                let created = db.store.create_research_entry(
                    "project:attention",
                    index,
                    &ResearchEntryDraft {
                        kind: ResearchEntryKind::Question,
                        epistemic_status: EpistemicStatus::ResearcherContext,
                        text: format!("Question {index}"),
                        evidence: vec![],
                        relations: vec![],
                        context: vec![],
                        reason: None,
                    },
                )?;
                entry_ids.push(created.entry.entry.id);
            }
            let run = managed_harness_run(&db, &AgentRunLimits::default())?;
            db.store.begin_codex_harness_finalization(&run.id)?;
            let mut proposal = serde_json::to_value(research_outcome("Revised question")).unwrap();
            proposal["stateSynthesis"]["noChangeReason"] = serde_json::Value::Null;
            proposal["stateSynthesis"]["changes"] = serde_json::json!([{
                "operation":"revise","entryId":entry_ids[0],"epistemicStatus":if wrong_kind {"source_supported"} else {"speculative"},
                "statement":"Question 1","evidence":if wrong_kind {serde_json::json!([{"passageRef":"unused","relationship":"supports","explanation":"Example"}])} else {serde_json::json!([])},"relations":[],"reason":"Revision"
            }]);
            db.store
                .save_synthesis_attempt(&run.id, 0, &proposal.to_string(), &HashMap::new())?;
            let error = db.store.preflight_synthesis(&run.id, 0).unwrap_err();
            assert!(
                error.contains(if wrong_kind {
                    "Only a Finding"
                } else {
                    "Equivalent active"
                }),
                "{error}"
            );
            assert_eq!(
                db.store.get_research_state(&run.project_id, None)?.revision,
                2
            );
        }
        Ok(())
    }

    /// A contest link is not a premise for a revised hypothesis.
    #[test]
    fn synthesis_revision_requires_a_real_premise() -> StoreResult<()> {
        let db = test_db()?;
        let created = db.store.create_research_entry(
            "project:attention",
            0,
            &ResearchEntryDraft {
                kind: ResearchEntryKind::Hypothesis,
                epistemic_status: EpistemicStatus::Speculative,
                text: "Initial researcher hypothesis".into(),
                evidence: vec![],
                relations: vec![],
                context: vec![],
                reason: None,
            },
        )?;
        let run = managed_harness_run(&db, &AgentRunLimits::default())?;
        db.store.begin_codex_harness_finalization(&run.id)?;
        let mut proposal = serde_json::to_value(research_outcome("Revised hypothesis")).unwrap();
        proposal["stateSynthesis"]["noChangeReason"] = serde_json::Value::Null;
        proposal["stateSynthesis"]["changes"] = serde_json::json!([{
            "operation":"revise", "entryId":created.entry.entry.id,
            "epistemicStatus":"speculative", "statement":"Refined hypothesis",
            "evidence":[], "relations":[{"kind":"contests", "target":created.entry.entry.id}],
            "reason":"Consider a competing interpretation"
        }]);
        db.store
            .save_synthesis_attempt(&run.id, 0, &proposal.to_string(), &HashMap::new())?;
        assert!(db
            .store
            .preflight_synthesis(&run.id, 0)
            .unwrap_err()
            .contains("requires premise"));
        assert_eq!(
            db.store.get_research_state(&run.project_id, None)?.revision,
            1
        );
        Ok(())
    }

    #[test]
    fn stored_synthesis_allows_an_unlinked_next_direction() {
        let synthesis = research_outcome("Completed investigation")
            .state_synthesis
            .expect("synthesis fixture");

        assert!(
            validate_research_state_synthesis_shape(&synthesis, Some("Broaden the corpus")).is_ok()
        );
    }

    #[test]
    fn stored_synthesis_rejects_entry_ids_without_direction_text() {
        let mut synthesis = research_outcome("Completed investigation")
            .state_synthesis
            .expect("synthesis fixture");
        synthesis
            .next_direction_entry_ids
            .push("state-entry:gap".to_string());

        let error = validate_research_state_synthesis_shape(&synthesis, None)
            .expect_err("entry links without direction text must fail");
        assert!(error.contains("Next direction entry IDs require a next direction"));
    }

    #[test]
    fn managed_run_snapshots_runtime_limits_and_remains_singleton() -> StoreResult<()> {
        let db = test_db()?;
        let limits = AgentRunLimits {
            maximum_provider_queries: 9,
            ..AgentRunLimits::default()
        };
        let run = managed_harness_run(&db, &limits)?;
        assert_eq!(run.status, "planning");
        assert_eq!(run.execution_kind, "codex_agent");
        assert_eq!(run.runtime_model.as_deref(), Some("test-model"));
        assert_eq!(run.agent_limits, Some(limits));
        assert_eq!(
            run.effective_instructions
                .run_context
                .maximum_provider_queries,
            9
        );

        let search = db.store.create_search(&sample_search_draft())?;
        assert!(db
            .store
            .create_harness_run("project:attention", &search.id)
            .expect_err("a second active Run must fail atomically")
            .contains("already active"));
        Ok(())
    }

    #[test]
    fn managed_cancellation_closes_writes_once_and_preserves_prior_state() -> StoreResult<()> {
        let db = test_db()?;
        let run = managed_harness_run(&db, &AgentRunLimits::default())?;
        let child = db.store.create_agent_search_run(
            "project:attention",
            "attention",
            "codex-test",
            Some(&run.id),
            "search-before-cancel",
            "search-before-cancel-payload",
            None,
            &quick_agent_search_draft(),
            2,
        )?;
        let committed = db.store.apply_agent_state_update(
            "project:attention",
            0,
            "codex-test",
            Some(&run.id),
            "before-cancel",
            "before-cancel-payload",
            vec![agent_question("known-gap", "What remains uncertain?")],
        )?;
        assert_eq!(committed.revision, 1);

        let (canceling, changed) = db
            .store
            .request_codex_harness_cancellation("project:attention", "cancelled_by_user")?;
        assert!(changed);
        assert_eq!(canceling.status, "canceling");
        let (_, repeated) = db
            .store
            .request_codex_harness_cancellation("project:attention", "cancelled_by_user")?;
        assert!(!repeated);
        assert!(
            db.store
                .get_agent_search_run("project:attention", &child.run_id)?
                .1
        );
        assert!(db
            .store
            .create_agent_search_run(
                "project:attention",
                "attention",
                "codex-test",
                Some(&run.id),
                "search-after-cancel",
                "search-after-cancel-payload",
                None,
                &quick_agent_search_draft(),
                2,
            )
            .expect_err("canceled Runs must reject new child Searches")
            .contains("no longer active"));
        assert!(db
            .store
            .apply_agent_state_update(
                "project:attention",
                1,
                "codex-test",
                Some(&run.id),
                "after-cancel",
                "after-cancel-payload",
                vec![agent_question("late-gap", "Should this be rejected?")],
            )
            .expect_err("canceled Runs must reject new State writes")
            .contains("no longer accepts writes"));
        assert!(db
            .store
            .add_agent_note_idempotent(
                "paper",
                "vaswani2017",
                &ThreadAnchor::Document,
                "Late note",
                "codex-test",
                Some(&run.id),
                "late-note",
                "late-note-payload",
            )
            .expect_err("canceled Runs must reject new notes")
            .contains("no longer accepts writes"));
        assert_eq!(
            db.store
                .get_research_state("project:attention", None)?
                .current_revision,
            1
        );
        let events = db.store.get_harness_snapshot("project:attention")?.events;
        assert_eq!(
            events
                .iter()
                .filter(|event| event.run_id == run.id && event.kind == "cancellation_requested")
                .count(),
            1
        );
        Ok(())
    }

    #[test]
    fn managed_completion_wins_over_a_late_cancel() -> StoreResult<()> {
        let db = test_db()?;
        let run = managed_harness_run(&db, &AgentRunLimits::default())?;
        db.store.begin_codex_harness_finalization(&run.id)?;
        db.store
            .record_agent_state_unchanged(&run.id, "No evidence was gathered")?;
        db.store
            .persist_agent_run_outcome(&run.id, &research_outcome("No evidence was gathered"))?;
        db.store
            .finish_codex_harness_run(&run.id, "ready", "agent_completed")?;

        let (completed, changed) = db
            .store
            .request_codex_harness_cancellation("project:attention", "cancelled_by_user")?;
        assert!(!changed);
        assert_eq!(completed.status, "ready");
        assert!(!db
            .store
            .get_harness_snapshot("project:attention")?
            .events
            .iter()
            .any(|event| event.run_id == run.id && event.kind == "cancellation_requested"));
        Ok(())
    }

    #[test]
    fn restart_interrupts_managed_finalization_and_keeps_saved_sources() -> StoreResult<()> {
        let db = test_db()?;
        let run = managed_harness_run(&db, &AgentRunLimits::default())?;
        db.store.begin_codex_harness_finalization(&run.id)?;
        let source_ids_before: Vec<String> = db
            .store
            .get_document_sources("vaswani2017")?
            .into_iter()
            .map(|source| source.id)
            .collect();

        assert_eq!(db.store.recover_interrupted_harness_runs()?, 1);
        let recovered = db.store.get_harness_run(&run.id)?;
        assert_eq!(recovered.status, "failed");
        assert_eq!(
            recovered.stop_reason.as_deref(),
            Some("application_restarted")
        );
        let source_ids_after: Vec<String> = db
            .store
            .get_document_sources("vaswani2017")?
            .into_iter()
            .map(|source| source.id)
            .collect();
        assert_eq!(source_ids_after, source_ids_before);
        assert!(db
            .store
            .get_harness_snapshot("project:attention")?
            .events
            .iter()
            .any(|event| event.run_id == run.id && event.kind == "interrupted"));
        Ok(())
    }

    #[test]
    fn managed_reader_budget_counts_repeats_and_distinct_papers() -> StoreResult<()> {
        let db = test_db()?;
        let limits = AgentRunLimits {
            maximum_distinct_papers_read: 1,
            maximum_returned_text_chars: 10,
            ..AgentRunLimits::default()
        };
        let run = managed_harness_run(&db, &limits)?;
        db.store.record_agent_reader_delivery(
            &run.id,
            "project:attention",
            "vaswani2017",
            6,
            &HashMap::new(),
        )?;
        db.store.record_agent_reader_delivery(
            &run.id,
            "project:attention",
            "vaswani2017",
            4,
            &HashMap::new(),
        )?;
        assert!(db
            .store
            .record_agent_reader_delivery(
                &run.id,
                "project:attention",
                "vaswani2017",
                1,
                &HashMap::new()
            )
            .expect_err("repeat reads still consume returned-text budget")
            .contains("returned-text"));
        assert!(db
            .store
            .record_agent_reader_delivery(
                &run.id,
                "project:attention",
                "caron2021",
                1,
                &HashMap::new()
            )
            .expect_err("new papers consume the distinct-paper budget")
            .contains("paper-read"));
        Ok(())
    }

    #[test]
    fn managed_evidence_question_accounts_text_model_call_and_citations() -> StoreResult<()> {
        let db = test_db()?;
        let run = managed_harness_run(&db, &AgentRunLimits::default())?;
        db.store.admit_agent_evidence_question(
            &run.id,
            "project:attention",
            &[AgentQuestionDelivery {
                paper_id: "vaswani2017".to_string(),
                returned_text_chars: 42,
            }],
        )?;
        db.store.record_agent_question_citations(
            &run.id,
            "project:attention",
            &passage_anchor_fixture(&db, "vaswani2017", "passage:question")?,
        )?;
        db.store.begin_codex_harness_finalization(&run.id)?;
        db.store
            .record_agent_state_unchanged(&run.id, "No State change was warranted")?;
        db.store.persist_agent_run_outcome(
            &run.id,
            &ResearchRunOutcome {
                summary: "Delegated question answered".to_string(),
                display_items: Vec::new(),
                paper_dispositions: Vec::new(),
                state_synthesis: Some(crate::domain::harness::ResearchStateSynthesis {
                    changes: Vec::new(),
                    unresolved_entry_ids: Vec::new(),
                    next_direction_entry_ids: Vec::new(),
                    no_change_reason: Some("No State change was warranted".to_string()),
                    resulting_revision: None,
                    created_entry_ids: HashMap::new(),
                }),
                task_outcomes: vec![crate::domain::harness::ResearchTaskOutcome {
                    search_run_ids: Vec::new(),
                    motivating_entry_ids: Vec::new(),
                    learned_points: vec!["Evidence inspected".to_string()],
                    cited_passage_refs: vec!["passage:question".to_string()],
                }],
                unanswered_questions: Vec::new(),
                next_direction: None,
            },
        )?;
        db.store
            .finish_codex_harness_run(&run.id, "ready", "agent_completed")?;

        let checkpoint = db.store.get_research_checkpoint(&run.id)?;
        assert_eq!(checkpoint.usage.llm_calls, 1);
        assert!(db
            .store
            .get_harness_snapshot("project:attention")?
            .events
            .iter()
            .any(|event| event.kind == "agent_evidence_question_answered"));
        Ok(())
    }

    #[test]
    fn managed_search_budget_limits_total_child_searches() -> StoreResult<()> {
        let db = test_db()?;
        let limits = AgentRunLimits {
            maximum_child_searches: 1,
            ..AgentRunLimits::default()
        };
        let run = managed_harness_run(&db, &limits)?;
        let draft = quick_agent_search_draft();
        db.store.create_agent_search_run(
            "project:attention",
            "attention",
            "codex-test",
            Some(&run.id),
            "child-request-1",
            "child-payload-1",
            None,
            &draft,
            2,
        )?;

        assert!(db
            .store
            .create_agent_search_run(
                "project:attention",
                "attention",
                "codex-test",
                Some(&run.id),
                "child-request-2",
                "child-payload-2",
                None,
                &draft,
                2,
            )
            .expect_err("the parent child-search budget must be enforced")
            .contains("child-search limit"));
        Ok(())
    }

    #[test]
    fn managed_checkpoint_attributes_only_its_own_vault_additions() -> StoreResult<()> {
        let db = test_db()?;
        let run = managed_harness_run(&db, &AgentRunLimits::default())?;
        let paper = paper_draft_with_pdf("managed-addition", "https://example.test/paper.pdf");
        db.store.add_agent_search_candidate_to_vault(
            "project:attention",
            "attention",
            "codex-test",
            Some(&run.id),
            "vault-request-1",
            "vault-payload-1",
            &paper,
        )?;
        db.store.record_agent_reader_delivery(
            &run.id,
            "project:attention",
            &paper.id,
            20,
            &passage_anchor_fixture(&db, &paper.id, "passage:background")?,
        )?;
        db.store.begin_codex_harness_finalization(&run.id)?;
        db.store.finalize_agent_paper_retention(
            &run.id,
            &[ResearchPaperDisposition {
                paper_id: paper.id.clone(),
                disposition: PaperDispositionKind::Background,
                reason: "Relevant background".to_string(),
            }],
        )?;
        db.store
            .record_agent_state_unchanged(&run.id, "No State change was warranted")?;
        db.store.persist_agent_run_outcome(
            &run.id,
            &research_outcome("Retained assessed background"),
        )?;
        db.store
            .finish_codex_harness_run(&run.id, "ready", "agent_completed")?;

        let checkpoint = db.store.get_research_checkpoint(&run.id)?;
        assert_eq!(checkpoint.accepted_candidate_count, 1);
        assert_eq!(checkpoint.added_paper_ids, vec![paper.id]);
        assert_eq!(checkpoint.attempted_paper_count, 1);
        assert_eq!(checkpoint.read_paper_count, 1);
        assert_eq!(checkpoint.retained_paper_count, 1);
        Ok(())
    }

    #[test]
    fn managed_synthesis_creates_related_entries_atomically_and_rejects_duplicates(
    ) -> StoreResult<()> {
        let db = test_db()?;
        let extraction = extracted_paper(
            &db,
            "vaswani2017",
            &[(
                "paragraph",
                "The measured effect was limited to two benchmarks.",
            )],
        )?;
        let chunk = db.store.chunks_for_extraction(&extraction.id)?.remove(0);
        let run = managed_harness_run(&db, &AgentRunLimits::default())?;
        db.store.begin_codex_harness_finalization(&run.id)?;

        let receipt = db.store.apply_agent_state_update(
            "project:attention",
            0,
            "codex-test",
            Some(&run.id),
            "synthesis-batch",
            "synthesis-batch-payload",
            vec![
                AgentStateChange::Create {
                    operation_key: "finding".to_string(),
                    draft: ResearchEntryDraft {
                        kind: ResearchEntryKind::Finding,
                        epistemic_status: EpistemicStatus::SourceSupported,
                        text: "The measured effect was evaluated on two benchmarks.".to_string(),
                        evidence: vec![EvidenceLinkDraft {
                            chunk_id: chunk.id,
                            excerpt: Some(
                                "The measured effect was limited to two benchmarks.".to_string(),
                            ),
                            support_note: Some("Defines the observed evaluation scope".to_string()),
                        }],
                        relations: Vec::new(),
                        context: Vec::new(),
                        reason: Some("New evidence from this Run".to_string()),
                    },
                    evidence_relationships: vec!["supports".to_string()],
                },
                AgentStateChange::Create {
                    operation_key: "gap".to_string(),
                    draft: ResearchEntryDraft {
                        kind: ResearchEntryKind::Gap,
                        epistemic_status: EpistemicStatus::AgentSynthesis,
                        text: "Within the bounded corpus, evaluation beyond those two benchmarks remains absent."
                            .to_string(),
                        evidence: Vec::new(),
                        relations: vec![EntryRelationDraft {
                            target_entry_id: "finding".to_string(),
                            kind: EntryRelationKind::DerivedFrom,
                        }],
                        context: Vec::new(),
                        reason: Some("Bounded post-Run synthesis".to_string()),
                    },
                    evidence_relationships: Vec::new(),
                },
            ],
        )?;

        assert_eq!(receipt.revision, 1);
        let finding_id = &receipt.created_entry_ids["finding"];
        let gap = db
            .store
            .get_research_entry(&receipt.created_entry_ids["gap"], None)?;
        assert_eq!(gap.relations[0].target_entry_id, *finding_id);
        assert!(db
            .store
            .apply_agent_state_update(
                "project:attention",
                1,
                "codex-test",
                Some(&run.id),
                "duplicate-synthesis",
                "duplicate-synthesis-payload",
                vec![AgentStateChange::Create {
                    operation_key: "duplicate".to_string(),
                    draft: ResearchEntryDraft {
                        kind: ResearchEntryKind::Gap,
                        epistemic_status: EpistemicStatus::Speculative,
                        text: "Within the bounded corpus, evaluation beyond those two benchmarks remains absent."
                            .to_string(),
                        evidence: Vec::new(),
                        relations: Vec::new(),
                        context: Vec::new(),
                        reason: Some("Duplicate attempt".to_string()),
                    },
                    evidence_relationships: Vec::new(),
                }],
            )
            .expect_err("equivalent active claims must not be duplicated")
            .contains("Equivalent active Research Entry"));
        Ok(())
    }

    #[test]
    fn managed_no_change_synthesis_finishes_without_a_new_revision() -> StoreResult<()> {
        let db = test_db()?;
        let run = managed_harness_run(&db, &AgentRunLimits::default())?;
        db.store.begin_codex_harness_finalization(&run.id)?;
        db.store.record_agent_state_unchanged(
            &run.id,
            "The Run found no evidence warranting a State mutation",
        )?;
        db.store
            .persist_agent_run_outcome(&run.id, &research_outcome("No State change"))?;
        let completed = db
            .store
            .finish_codex_harness_run(&run.id, "ready", "agent_completed")?;

        assert_eq!(completed.resulting_state_revision, Some(0));
        assert_eq!(
            db.store
                .get_research_state("project:attention", None)?
                .current_revision,
            0
        );
        Ok(())
    }

    #[test]
    fn managed_retention_requires_reader_attempts_and_applies_each_disposition() -> StoreResult<()>
    {
        let db = test_db()?;
        let run = managed_harness_run(&db, &AgentRunLimits::default())?;
        let cases = [
            ("evidence-paper", PaperDispositionKind::EvidenceUsed, 20_u64),
            ("background-paper", PaperDispositionKind::Background, 20),
            (
                "contradictory-paper",
                PaperDispositionKind::Contradictory,
                20,
            ),
            ("unavailable-paper", PaperDispositionKind::Unavailable, 0),
            ("irrelevant-paper", PaperDispositionKind::Irrelevant, 20),
        ];
        for (paper_id, _, _) in &cases {
            let paper = paper_draft_with_pdf(paper_id, "https://example.test/paper.pdf");
            db.store.add_agent_search_candidate_to_vault(
                "project:attention",
                "attention",
                "codex-test",
                Some(&run.id),
                &format!("add-{paper_id}"),
                &format!("payload-{paper_id}"),
                &paper,
            )?;
        }
        db.store.begin_codex_harness_finalization(&run.id)?;
        let dispositions: Vec<ResearchPaperDisposition> = cases
            .iter()
            .map(|(paper_id, disposition, _)| ResearchPaperDisposition {
                paper_id: (*paper_id).to_string(),
                disposition: *disposition,
                reason: format!("Assessment for {paper_id}"),
            })
            .collect();
        assert!(db
            .store
            .finalize_agent_paper_retention(&run.id, &dispositions)
            .expect_err("unread additions must prevent finalization")
            .contains("not assessed through Reader"));

        // Return to the active phase only to finish arranging this test fixture.
        db.store
            .open_connection()?
            .execute(
                "update harness_runs set status = 'assessing' where id = ?1",
                params![run.id],
            )
            .map_err(|error| error.to_string())?;
        for (paper_id, _, returned_chars) in &cases {
            let anchors = if *returned_chars > 0 {
                passage_anchor_fixture(&db, paper_id, &format!("passage:{paper_id}"))?
            } else {
                HashMap::new()
            };
            db.store.record_agent_reader_delivery(
                &run.id,
                "project:attention",
                paper_id,
                *returned_chars,
                &anchors,
            )?;
        }
        db.store.begin_codex_harness_finalization(&run.id)?;
        let summary = db
            .store
            .finalize_agent_paper_retention(&run.id, &dispositions)?;
        assert_eq!(summary.attempted, 5);
        assert_eq!(summary.read, 4);
        assert_eq!(summary.unavailable, 1);
        assert_eq!(summary.removed, 1);
        assert_eq!(summary.retained, 4);

        let snapshot = db.store.get_library()?;
        let vault = snapshot
            .vaults
            .iter()
            .find(|vault| vault.project_id == "project:attention")
            .expect("project vault");
        assert!(!has_membership(&snapshot, &vault.id, "irrelevant-paper"));
        for retained in [
            "evidence-paper",
            "background-paper",
            "contradictory-paper",
            "unavailable-paper",
        ] {
            assert!(has_membership(&snapshot, &vault.id, retained));
        }
        Ok(())
    }

    #[test]
    fn managed_outcome_rejects_a_passage_the_run_did_not_read() -> StoreResult<()> {
        let db = test_db()?;
        let run = managed_harness_run(&db, &AgentRunLimits::default())?;
        db.store.record_agent_reader_delivery(
            &run.id,
            "project:attention",
            "vaswani2017",
            20,
            &passage_anchor_fixture(&db, "vaswani2017", "passage:observed")?,
        )?;
        db.store.begin_codex_harness_finalization(&run.id)?;
        let mut outcome = research_outcome("The paper supports the bounded observation.");
        outcome.task_outcomes.push(ResearchTaskOutcome {
            search_run_ids: vec!["search-run:invented".to_string()],
            motivating_entry_ids: Vec::new(),
            learned_points: vec!["The reported observation is conditional.".to_string()],
            cited_passage_refs: Vec::new(),
        });

        assert!(db
            .store
            .persist_agent_run_outcome(&run.id, &outcome)
            .expect_err("an unrelated child Search must not enter continuation history")
            .contains("unrelated Search Run"));
        outcome.task_outcomes[0].search_run_ids.clear();
        outcome.task_outcomes[0]
            .motivating_entry_ids
            .push("state-entry:invented".to_string());
        assert!(db
            .store
            .persist_agent_run_outcome(&run.id, &outcome)
            .expect_err("an unknown State entry must not enter continuation history")
            .contains("unknown State entry"));
        outcome.task_outcomes[0].motivating_entry_ids.clear();
        outcome.task_outcomes[0]
            .cited_passage_refs
            .push("passage:invented".to_string());
        assert!(db
            .store
            .persist_agent_run_outcome(&run.id, &outcome)
            .expect_err("an unread passage must not enter continuation history")
            .contains("did not read"));
        db.store
            .record_agent_state_unchanged(&run.id, "Outcome validation failed")?;
        assert!(db
            .store
            .finish_codex_harness_run(&run.id, "ready", "agent_completed")
            .unwrap_err()
            .contains("no persisted outcome"));
        db.store
            .finish_codex_harness_run(&run.id, "failed", "invalid_outcome")?;
        assert!(db
            .store
            .list_recent_agent_run_outcomes("project:attention", 3, 12_000)?
            .is_empty());
        Ok(())
    }

    #[test]
    fn missing_synthesis_prevents_a_ready_run() -> StoreResult<()> {
        let db = test_db()?;
        let run = managed_harness_run(&db, &AgentRunLimits::default())?;
        db.store.begin_codex_harness_finalization(&run.id)?;
        db.store
            .save_synthesis_attempt(&run.id, 0, "invalid JSON", &HashMap::new())?;
        assert!(db.store.preflight_synthesis(&run.id, 0).is_err());
        db.store.record_harness_activity(
            &run.id,
            "summary_failed",
            "Research outcome was not retained: invalid JSON",
            Some("complete"),
        )?;
        assert!(db
            .store
            .finish_codex_harness_run(&run.id, "ready", "agent_completed")
            .expect_err("ready requires a completed synthesis")
            .contains("no completed State synthesis"));
        db.store
            .finish_codex_harness_run(&run.id, "failed", "invalid_outcome")?;

        assert_eq!(
            db.store
                .get_research_state("project:attention", None)?
                .current_revision,
            0,
            "invalid final proposals must not create partial State writes"
        );
        assert!(db
            .store
            .list_recent_agent_run_outcomes("project:attention", 3, 12_000)?
            .is_empty());
        Ok(())
    }

    #[test]
    fn continuation_history_is_recent_and_character_bounded() -> StoreResult<()> {
        let db = test_db()?;
        let mut latest_run_id = String::new();
        for index in 0..4 {
            let run = managed_harness_run(&db, &AgentRunLimits::default())?;
            latest_run_id = run.id.clone();
            db.store.begin_codex_harness_finalization(&run.id)?;
            db.store
                .record_agent_state_unchanged(&run.id, "No State change was warranted")?;
            db.store.persist_agent_run_outcome(
                &run.id,
                &research_outcome(&format!("Outcome {index}")),
            )?;
            db.store
                .finish_codex_harness_run(&run.id, "ready", "agent_completed")?;
        }

        let recent = db
            .store
            .list_recent_agent_run_outcomes("project:attention", 3, 12_000)?;
        assert_eq!(recent.len(), 3);
        assert_eq!(recent[0].outcome.summary, "Outcome 3");
        assert_eq!(
            db.store
                .get_research_checkpoint(&latest_run_id)?
                .outcome
                .expect("checkpoint outcome")
                .summary,
            "Outcome 3"
        );
        let one_outcome_chars = serde_json::to_string(&recent[0].outcome)
            .map_err(|error| error.to_string())?
            .chars()
            .count();
        assert_eq!(
            db.store
                .list_recent_agent_run_outcomes(
                    "project:attention",
                    3,
                    one_outcome_chars.saturating_sub(1),
                )?
                .len(),
            0
        );
        Ok(())
    }

    #[test]
    fn managed_activity_orders_passage_delivery_before_state_commit() -> StoreResult<()> {
        let db = test_db()?;
        let run = managed_harness_run(&db, &AgentRunLimits::default())?;
        db.store.record_agent_reader_delivery(
            &run.id,
            "project:attention",
            "vaswani2017",
            20,
            &passage_anchor_fixture(&db, "vaswani2017", "passage:observed")?,
        )?;
        db.store.apply_agent_state_update(
            "project:attention",
            0,
            "codex-test",
            Some(&run.id),
            "ordered-state-request",
            "ordered-state-payload",
            vec![agent_question(
                "ordered-gap",
                "Which setting changes the result?",
            )],
        )?;

        let snapshot = db.store.get_harness_snapshot("project:attention")?;
        let read_sequence = snapshot
            .events
            .iter()
            .find(|event| event.run_id == run.id && event.kind == "agent_passages_delivered")
            .map(|event| event.sequence)
            .expect("Reader activity");
        let state_sequence = snapshot
            .events
            .iter()
            .find(|event| event.run_id == run.id && event.kind == "agent_state_committed")
            .map(|event| event.sequence)
            .expect("State activity");
        assert!(read_sequence < state_sequence);
        Ok(())
    }

    #[test]
    fn managed_finalization_uses_committed_state_and_preserves_failed_work() -> StoreResult<()> {
        let db = test_db()?;
        let run = managed_harness_run(&db, &AgentRunLimits::default())?;
        db.store.attach_codex_turn(&run.id, "thread-1", "turn-1")?;
        let receipt = db.store.apply_agent_state_update(
            "project:attention",
            0,
            "codex-test",
            Some(&run.id),
            "state-request-1",
            "state-payload-1",
            vec![agent_question("gap", "Which conditions change the result?")],
        )?;
        assert_eq!(receipt.revision, 1);

        let finished = db
            .store
            .finish_codex_harness_run(&run.id, "failed", "agent_failed")?;
        assert_eq!(finished.status, "failed");
        assert_eq!(finished.resulting_state_revision, Some(1));
        assert_eq!(finished.iteration_count, 1);
        assert!(finished
            .summary
            .as_deref()
            .unwrap_or_default()
            .contains("1 State revisions"));
        assert_eq!(
            db.store
                .get_research_state("project:attention", None)?
                .current_revision,
            1,
            "validated writes survive a later agent failure"
        );
        Ok(())
    }
}
