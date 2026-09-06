//! Authenticated local MCP access to i0i capabilities.
//!
//! The HTTP transport is deliberately thin: the official `rmcp` SDK owns MCP
//! framing and sessions, while this module owns app-session grants and maps
//! tool calls onto the same `LibraryStore` used by Tauri commands.

use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::extract::{Request, State};
use axum::http::{header, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use rmcp::handler::server::wrapper::Parameters;
use rmcp::service::{RequestContext, RoleServer};
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::{StreamableHttpServerConfig, StreamableHttpService};
use rmcp::{schemars, tool, tool_router, Json};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::sync::{Mutex, RwLock};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::domain::chat::{ChatEntry, ThreadAnchor, ENTRY_NOTE};
use crate::domain::highlight::HighlightAuthor;
use crate::domain::library::{DocumentChunk, DocumentSource, LibrarySnapshot, Paper, Vault};
use crate::domain::research_state::{
    EntryLifecycle, EntryRelationDraft, EntryRelationKind, EpistemicStatus, EvidenceLinkDraft,
    ResearchEntryDraft, ResearchEntryKind, ResearchEntryUpdate,
};
use crate::pdf_extraction::PdfExtractionManager;
use crate::pdf_ingestion::PdfDownloadManager;
use crate::storage::library_store::{AgentStateChange, LibraryStore};
use tauri::{AppHandle, Emitter};

const DEFAULT_PAGE_SIZE: usize = 25;
const MAX_PAGE_SIZE: usize = 100;
const DEFAULT_CURSOR_TTL: Duration = Duration::from_secs(10 * 60);
const READER_TEXT_BUDGET: usize = 12_000;
const MAX_PASSAGE_CHARS: usize = 4_000;

pub const VAULT_LIST: &str = "vault_list";
pub const VAULT_LIST_PAPERS: &str = "vault_list_papers";
pub const VAULT_GET_PAPER: &str = "vault_get_paper";
pub const READER_READ: &str = "reader_read";
pub const READER_ADD_NOTE: &str = "reader_add_note";
pub const READER_LIST_NOTES: &str = "reader_list_notes";
pub const STATE_READ: &str = "state_read";
pub const STATE_UPDATE: &str = "state_update";

/// Caller identity and scope established by an opaque local credential.
#[derive(Debug, Clone)]
pub struct McpCallContext {
    pub project_id: String,
    pub vault_id: String,
    pub caller: String,
    pub run_id: Option<String>,
    tools: BTreeSet<String>,
}

impl McpCallContext {
    fn permits(&self, tool: &str) -> bool {
        self.tools.contains(tool)
    }
}

/// Connection details returned only to the caller that requested a grant.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpConnectionGrant {
    pub endpoint: String,
    pub bearer_token: String,
}

#[derive(Clone)]
struct GrantRegistry {
    grants: Arc<RwLock<HashMap<String, McpCallContext>>>,
}

impl GrantRegistry {
    fn new() -> Self {
        Self {
            grants: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    async fn issue(&self, context: McpCallContext) -> String {
        let token = Uuid::new_v4().simple().to_string();
        self.grants.write().await.insert(token.clone(), context);
        token
    }

    async fn resolve(&self, token: &str) -> Option<McpCallContext> {
        self.grants.read().await.get(token).cloned()
    }

    async fn revoke(&self, token: &str) -> bool {
        self.grants.write().await.remove(token).is_some()
    }

    async fn revoke_run(&self, run_id: &str) {
        self.grants
            .write()
            .await
            .retain(|_, grant| grant.run_id.as_deref() != Some(run_id));
    }
}

#[derive(Debug, Clone)]
struct CursorSnapshot {
    grant_token: String,
    vault_id: String,
    title: Option<String>,
    year: Option<i32>,
    membership_revision: i64,
    papers: Vec<McpPaper>,
    offset: usize,
    expires_at: Instant,
}

/// Canonical source location represented by an opaque passage reference.
#[derive(Debug, Clone)]
pub(crate) struct PassageAnchor {
    pub paper_id: String,
    pub source_id: String,
    pub extraction_id: Option<String>,
    pub chunk_id: Option<String>,
    pub page_start: Option<i32>,
    pub page_end: Option<i32>,
    pub source_start: i64,
    pub source_end: i64,
    pub quote: String,
}

#[derive(Debug, Clone)]
struct RegisteredPassage {
    grant_token: String,
    anchor: PassageAnchor,
}

#[derive(Debug, Clone)]
struct ReaderCursorSnapshot {
    grant_token: String,
    paper_id: String,
    source_version: String,
    page_start: Option<i32>,
    page_end: Option<i32>,
    passages: Vec<McpPassage>,
    offset: usize,
    expires_at: Instant,
}

#[derive(Debug, Clone)]
struct NoteCursorSnapshot {
    grant_token: String,
    paper_id: String,
    notes: Vec<McpReaderNote>,
    offset: usize,
    expires_at: Instant,
}

#[derive(Debug, Clone)]
struct StateCursorSnapshot {
    grant_token: String,
    project_id: String,
    revision: i64,
    entries: Vec<McpStateEntry>,
    offset: usize,
    expires_at: Instant,
}

#[derive(Clone)]
struct I0iMcpHandler {
    app: Option<AppHandle>,
    store: LibraryStore,
    pdf_downloads: Option<PdfDownloadManager>,
    pdf_extractions: Option<PdfExtractionManager>,
    cursors: Arc<Mutex<HashMap<String, CursorSnapshot>>>,
    reader_cursors: Arc<Mutex<HashMap<String, ReaderCursorSnapshot>>>,
    passage_anchors: Arc<RwLock<HashMap<String, RegisteredPassage>>>,
    note_cursors: Arc<Mutex<HashMap<String, NoteCursorSnapshot>>>,
    state_cursors: Arc<Mutex<HashMap<String, StateCursorSnapshot>>>,
    cursor_ttl: Duration,
}

impl I0iMcpHandler {
    fn new(
        app: Option<AppHandle>,
        store: LibraryStore,
        pdf_downloads: Option<PdfDownloadManager>,
        pdf_extractions: Option<PdfExtractionManager>,
        cursor_ttl: Duration,
    ) -> Self {
        Self {
            app,
            store,
            pdf_downloads,
            pdf_extractions,
            cursors: Arc::new(Mutex::new(HashMap::new())),
            reader_cursors: Arc::new(Mutex::new(HashMap::new())),
            passage_anchors: Arc::new(RwLock::new(HashMap::new())),
            note_cursors: Arc::new(Mutex::new(HashMap::new())),
            state_cursors: Arc::new(Mutex::new(HashMap::new())),
            cursor_ttl,
        }
    }

    fn context(
        context: &RequestContext<RoleServer>,
        tool: &str,
    ) -> Result<(McpCallContext, String), rmcp::ErrorData> {
        let parts = context
            .extensions
            .get::<axum::http::request::Parts>()
            .ok_or_else(|| rmcp::ErrorData::internal_error("HTTP call context missing", None))?;
        let grant = parts
            .extensions
            .get::<AuthorizedGrant>()
            .ok_or_else(|| rmcp::ErrorData::internal_error("Authorized grant missing", None))?;
        if !grant.context.permits(tool) {
            return Err(rmcp::ErrorData::invalid_request(
                "Tool is not permitted by this connection grant",
                Some(serde_json::json!({"code": "out_of_scope"})),
            ));
        }
        Ok((grant.context.clone(), grant.token.clone()))
    }

    fn snapshot(&self) -> Result<LibrarySnapshot, rmcp::ErrorData> {
        self.store.get_library().map_err(|error| {
            rmcp::ErrorData::internal_error(
                "Could not read the i0i library",
                Some(serde_json::json!({"code": "internal_failure", "detail": error})),
            )
        })
    }

    fn scoped_paper<'a>(
        snapshot: &'a LibrarySnapshot,
        grant: &McpCallContext,
        paper_id: &str,
    ) -> Result<&'a Paper, rmcp::ErrorData> {
        let belongs_to_vault = snapshot.vault_papers.iter().any(|membership| {
            membership.vault_id == grant.vault_id && membership.paper_id == paper_id
        });
        if !belongs_to_vault {
            return Err(out_of_scope());
        }
        snapshot
            .papers
            .iter()
            .find(|paper| paper.id == paper_id)
            .ok_or_else(out_of_scope)
    }

    fn active_source<'a>(
        snapshot: &'a LibrarySnapshot,
        paper: &Paper,
    ) -> Option<&'a DocumentSource> {
        paper
            .active_source_id
            .as_deref()
            .and_then(|id| {
                snapshot
                    .document_sources
                    .iter()
                    .find(|source| source.id == id)
            })
            .or_else(|| {
                snapshot
                    .document_sources
                    .iter()
                    .find(|source| source.paper_id == paper.id)
            })
    }

    async fn register_passage(&self, grant_token: &str, anchor: PassageAnchor) -> String {
        let passage_ref = format!("passage_{}", Uuid::new_v4().simple());
        self.passage_anchors.write().await.insert(
            passage_ref.clone(),
            RegisteredPassage {
                grant_token: grant_token.to_string(),
                anchor,
            },
        );
        passage_ref
    }

    async fn passages_from_chunks(
        &self,
        grant_token: &str,
        chunks: Vec<DocumentChunk>,
        page_start: Option<i32>,
        page_end: Option<i32>,
    ) -> Vec<McpPassage> {
        let mut passages = Vec::new();
        for chunk in chunks.into_iter().filter(|chunk| {
            page_start.is_none_or(|start| chunk.page_end + 1 >= start)
                && page_end.is_none_or(|end| chunk.page_start + 1 <= end)
        }) {
            for (relative_start, relative_end, text) in split_text(&chunk.text, MAX_PASSAGE_CHARS) {
                let anchor = PassageAnchor {
                    paper_id: chunk.paper_id.clone(),
                    source_id: chunk.source_id.clone(),
                    extraction_id: Some(chunk.extraction_id.clone()),
                    chunk_id: Some(chunk.id.clone()),
                    page_start: Some(chunk.page_start + 1),
                    page_end: Some(chunk.page_end + 1),
                    source_start: chunk.source_start + relative_start as i64,
                    source_end: chunk.source_start + relative_end as i64,
                    quote: text.clone(),
                };
                let passage_ref = self.register_passage(grant_token, anchor.clone()).await;
                passages.push(McpPassage::from_anchor(passage_ref, anchor));
            }
        }
        passages
    }

    async fn passages_from_flow_text(
        &self,
        grant_token: &str,
        paper_id: &str,
        source_id: &str,
        text: &str,
    ) -> Vec<McpPassage> {
        let mut passages = Vec::new();
        for (start, end, text) in split_text(text, MAX_PASSAGE_CHARS) {
            let anchor = PassageAnchor {
                paper_id: paper_id.to_string(),
                source_id: source_id.to_string(),
                extraction_id: None,
                chunk_id: None,
                page_start: None,
                page_end: None,
                source_start: start as i64,
                source_end: end as i64,
                quote: text.clone(),
            };
            let passage_ref = self.register_passage(grant_token, anchor.clone()).await;
            passages.push(McpPassage::from_anchor(passage_ref, anchor));
        }
        passages
    }
}

#[tool_router(server_handler)]
impl I0iMcpHandler {
    /// List the vault authorized by this connection grant.
    #[tool(description = "List the i0i vault available to this scoped connection")]
    async fn vault_list(
        &self,
        context: RequestContext<RoleServer>,
        Parameters(input): Parameters<PageInput>,
    ) -> Result<Json<VaultListResult>, rmcp::ErrorData> {
        let started = Instant::now();
        let (grant, _) = Self::context(&context, VAULT_LIST)?;
        let limit = validated_limit(input.limit)?;
        if input.cursor.is_some() {
            return Err(cursor_expired());
        }
        let snapshot = self.snapshot()?;
        let mut vaults = snapshot
            .vaults
            .into_iter()
            .filter(|vault| vault.id == grant.vault_id && vault.project_id == grant.project_id)
            .map(McpVault::from)
            .collect::<Vec<_>>();
        vaults.sort_by(|left, right| left.id.cmp(&right.id));
        vaults.truncate(limit);
        trace_tool(&grant, VAULT_LIST, started, "ok", vaults.len());
        Ok(Json(VaultListResult {
            vaults,
            next_cursor: None,
        }))
    }

    /// List papers in the authorized vault using a stable membership snapshot.
    #[tool(description = "List papers and metadata in an authorized i0i vault")]
    async fn vault_list_papers(
        &self,
        context: RequestContext<RoleServer>,
        Parameters(input): Parameters<VaultPapersInput>,
    ) -> Result<Json<VaultPapersResult>, rmcp::ErrorData> {
        let started = Instant::now();
        let (grant, grant_token) = Self::context(&context, VAULT_LIST_PAPERS)?;
        let limit = validated_limit(input.limit)?;
        if input.vault_id != grant.vault_id {
            return Err(out_of_scope());
        }

        let result = match input.cursor.as_deref() {
            Some(cursor) => {
                self.continue_paper_page(cursor, &grant_token, &input, limit)
                    .await?
            }
            None => {
                self.start_paper_page(&grant, &grant_token, &input, limit)
                    .await?
            }
        };
        trace_tool(
            &grant,
            VAULT_LIST_PAPERS,
            started,
            "ok",
            result.papers.len(),
        );
        Ok(Json(result))
    }

    /// Return metadata and honest document availability for one scoped paper.
    #[tool(description = "Get one i0i paper's metadata and document availability")]
    async fn vault_get_paper(
        &self,
        context: RequestContext<RoleServer>,
        Parameters(input): Parameters<GetPaperInput>,
    ) -> Result<Json<GetPaperResult>, rmcp::ErrorData> {
        let started = Instant::now();
        let (grant, _) = Self::context(&context, VAULT_GET_PAPER)?;
        if input.vault_id != grant.vault_id {
            return Err(out_of_scope());
        }
        let snapshot = self.snapshot()?;
        let paper = Self::scoped_paper(&snapshot, &grant, &input.paper_id)?;
        let source = Self::active_source(&snapshot, paper);
        let extraction = paper.active_extraction_id.as_deref().and_then(|id| {
            snapshot
                .document_extractions
                .iter()
                .find(|extraction| extraction.id == id)
        });
        let availability = document_availability(paper, source, extraction);
        trace_tool(&grant, VAULT_GET_PAPER, started, "ok", 1);
        Ok(Json(GetPaperResult {
            paper: McpPaper::from(paper),
            source: source.map(McpSource::from),
            extraction_id: extraction.map(|value| value.id.clone()),
            text_availability: availability.status,
            reason: availability.reason,
        }))
    }

    /// Read bounded exact passages from a scoped saved document.
    #[tool(description = "Read exact, referenceable passages from an i0i paper")]
    async fn reader_read(
        &self,
        context: RequestContext<RoleServer>,
        Parameters(input): Parameters<ReaderReadInput>,
    ) -> Result<Json<ReaderReadResult>, rmcp::ErrorData> {
        let started = Instant::now();
        let (grant, grant_token) = Self::context(&context, READER_READ)?;
        let (page_start, page_end) = validate_page_range(input.page_start, input.page_end)?;
        let result = if let Some(cursor) = input.cursor.as_deref() {
            self.continue_reader_page(cursor, &grant_token, &input.paper_id, page_start, page_end)
                .await?
        } else {
            self.start_reader_page(&grant, &grant_token, &input.paper_id, page_start, page_end)
                .await?
        };
        trace_tool(
            &grant,
            READER_READ,
            started,
            &result.availability,
            result.passages.len(),
        );
        Ok(Json(result))
    }

    /// Persist an agent-authored note at an opaque Reader passage reference.
    #[tool(description = "Add an agent-authored note to an exact i0i Reader passage")]
    async fn reader_add_note(
        &self,
        context: RequestContext<RoleServer>,
        Parameters(input): Parameters<ReaderAddNoteInput>,
    ) -> Result<Json<ReaderAddNoteResult>, rmcp::ErrorData> {
        let started = Instant::now();
        let (grant, grant_token) = Self::context(&context, READER_ADD_NOTE)?;
        let body = input.body.trim();
        let request_id = input.request_id.trim();
        if body.is_empty() || request_id.is_empty() {
            return Err(invalid_input("body and request_id must not be empty"));
        }
        let snapshot = self.snapshot()?;
        let paper = Self::scoped_paper(&snapshot, &grant, &input.paper_id)?;
        let anchor = match input.passage_ref.as_deref() {
            None => ThreadAnchor::Document,
            Some(passage_ref) => {
                let passage = self
                    .passage_anchors
                    .read()
                    .await
                    .get(passage_ref)
                    .filter(|registered| registered.grant_token == grant_token)
                    .map(|registered| registered.anchor.clone())
                    .ok_or_else(|| invalid_input("Passage reference is stale or unknown"))?;
                validate_passage_anchor(&snapshot, paper, &passage)?;
                ThreadAnchor::SourcePassage {
                    source_id: passage.source_id,
                    page_index: passage.page_start.map(|page| page - 1),
                    start_offset: passage.source_start,
                    end_offset: passage.source_end,
                    selected_text: passage.quote,
                }
            }
        };
        let payload_hash = sha256(
            &serde_json::to_string(&serde_json::json!({
                "paperId": input.paper_id,
                "passageRef": input.passage_ref,
                "body": body,
                "runId": grant.run_id,
            }))
            .map_err(|error| internal_failure(error.to_string()))?,
        );
        let receipt = self
            .store
            .add_agent_note_idempotent(
                "paper",
                &paper.id,
                &anchor,
                body,
                &grant.caller,
                grant.run_id.as_deref(),
                request_id,
                &payload_hash,
            )
            .map_err(write_failure)?;
        if let Some(app) = &self.app {
            let _ = app.emit(
                "chat_scope_updated",
                serde_json::json!({"paperId": paper.id}),
            );
        }
        trace_tool(&grant, READER_ADD_NOTE, started, "ok", 1);
        Ok(Json(ReaderAddNoteResult {
            thread_id: receipt.thread_id,
            entry_id: receipt.entry_id,
            anchor: serde_json::to_value(&anchor)
                .map_err(|error| internal_failure(error.to_string()))?,
            quote: anchor.selected_text().map(str::to_string),
            author: McpNoteAuthor {
                kind: "agent".to_string(),
                id: grant.caller,
                run_id: grant.run_id,
            },
        }))
    }

    /// List note entries from the scoped paper using a stable snapshot.
    #[tool(description = "List notes and their anchors from an i0i Reader paper")]
    async fn reader_list_notes(
        &self,
        context: RequestContext<RoleServer>,
        Parameters(input): Parameters<ReaderListNotesInput>,
    ) -> Result<Json<ReaderListNotesResult>, rmcp::ErrorData> {
        let started = Instant::now();
        let (grant, grant_token) = Self::context(&context, READER_LIST_NOTES)?;
        let limit = validated_limit(input.limit)?;
        let result = if let Some(cursor) = input.cursor.as_deref() {
            self.continue_note_page(cursor, &grant_token, &input.paper_id, limit)
                .await?
        } else {
            let snapshot = self.snapshot()?;
            Self::scoped_paper(&snapshot, &grant, &input.paper_id)?;
            let mut notes = Vec::new();
            for thread in self
                .store
                .list_chat_threads("paper", &input.paper_id)
                .map_err(internal_failure)?
            {
                let view = self
                    .store
                    .get_chat_thread(&thread.id)
                    .map_err(internal_failure)?;
                notes.extend(
                    view.entries
                        .into_iter()
                        .filter(|entry| entry.kind == ENTRY_NOTE)
                        .map(|entry| McpReaderNote::new(entry, &view.thread.anchor)),
                );
            }
            notes.extend(
                self.store
                    .list_highlights(&input.paper_id)
                    .map_err(internal_failure)?
                    .into_iter()
                    .filter_map(McpReaderNote::from_annotation),
            );
            notes.sort_by(|left, right| {
                right
                    .created_at
                    .cmp(&left.created_at)
                    .then_with(|| right.entry_id.cmp(&left.entry_id))
            });
            self.note_result_from_snapshot(
                NoteCursorSnapshot {
                    grant_token: grant_token.clone(),
                    paper_id: input.paper_id.clone(),
                    notes,
                    offset: 0,
                    expires_at: Instant::now() + self.cursor_ttl,
                },
                limit,
            )
            .await
        };
        trace_tool(&grant, READER_LIST_NOTES, started, "ok", result.notes.len());
        Ok(Json(result))
    }

    async fn continue_note_page(
        &self,
        cursor: &str,
        grant_token: &str,
        paper_id: &str,
        limit: usize,
    ) -> Result<ReaderListNotesResult, rmcp::ErrorData> {
        let snapshot = self
            .note_cursors
            .lock()
            .await
            .remove(cursor)
            .ok_or_else(cursor_expired)?;
        if snapshot.expires_at <= Instant::now()
            || snapshot.grant_token != grant_token
            || snapshot.paper_id != paper_id
        {
            return Err(cursor_expired());
        }
        Ok(self.note_result_from_snapshot(snapshot, limit).await)
    }

    async fn note_result_from_snapshot(
        &self,
        mut snapshot: NoteCursorSnapshot,
        limit: usize,
    ) -> ReaderListNotesResult {
        let end = (snapshot.offset + limit).min(snapshot.notes.len());
        let notes = snapshot.notes[snapshot.offset..end].to_vec();
        snapshot.offset = end;
        let next_cursor = if end < snapshot.notes.len() {
            let cursor = Uuid::new_v4().simple().to_string();
            self.note_cursors
                .lock()
                .await
                .insert(cursor.clone(), snapshot);
            Some(cursor)
        } else {
            None
        };
        ReaderListNotesResult { notes, next_cursor }
    }

    /// Read a revision-consistent projection of the scoped Project State.
    #[tool(description = "Read findings, hypotheses, and questions from i0i Research State")]
    async fn state_read(
        &self,
        context: RequestContext<RoleServer>,
        Parameters(input): Parameters<StateReadInput>,
    ) -> Result<Json<StateReadResult>, rmcp::ErrorData> {
        let started = Instant::now();
        let (grant, grant_token) = Self::context(&context, STATE_READ)?;
        if input.project_id != grant.project_id {
            return Err(out_of_scope());
        }
        let limit = validated_limit(input.limit)?;
        let result = if let Some(cursor) = input.cursor.as_deref() {
            self.continue_state_page(cursor, &grant_token, &input.project_id, limit)
                .await?
        } else {
            let state = self
                .store
                .get_research_state(&input.project_id, None)
                .map_err(internal_failure)?;
            let requested = input
                .entry_ids
                .as_ref()
                .map(|ids| ids.iter().map(String::as_str).collect::<BTreeSet<&str>>());
            let mut entries = Vec::new();
            for summary in state.entries.iter().filter(|entry| {
                requested
                    .as_ref()
                    .is_none_or(|ids| ids.contains(entry.id.as_str()))
            }) {
                let detail = self
                    .store
                    .get_research_entry(&summary.id, Some(state.revision))
                    .map_err(internal_failure)?;
                entries.push(McpStateEntry::from_detail(detail, &state.revisions));
            }
            if let Some(ids) = requested {
                if entries.len() != ids.len() {
                    return Err(invalid_input(
                        "One or more requested entries are outside this Project or revision",
                    ));
                }
            }
            self.state_result_from_snapshot(
                StateCursorSnapshot {
                    grant_token,
                    project_id: input.project_id.clone(),
                    revision: state.revision,
                    entries,
                    offset: 0,
                    expires_at: Instant::now() + self.cursor_ttl,
                },
                state.current_revision,
                limit,
            )
            .await
        };
        trace_tool(&grant, STATE_READ, started, "ok", result.entries.len());
        Ok(Json(result))
    }

    async fn continue_state_page(
        &self,
        cursor: &str,
        grant_token: &str,
        project_id: &str,
        limit: usize,
    ) -> Result<StateReadResult, rmcp::ErrorData> {
        let snapshot = self
            .state_cursors
            .lock()
            .await
            .remove(cursor)
            .ok_or_else(cursor_expired)?;
        if snapshot.expires_at <= Instant::now()
            || snapshot.grant_token != grant_token
            || snapshot.project_id != project_id
        {
            return Err(cursor_expired());
        }
        let current_revision = self
            .store
            .get_research_state(project_id, None)
            .map_err(internal_failure)?
            .current_revision;
        Ok(self
            .state_result_from_snapshot(snapshot, current_revision, limit)
            .await)
    }

    async fn state_result_from_snapshot(
        &self,
        mut snapshot: StateCursorSnapshot,
        current_revision: i64,
        limit: usize,
    ) -> StateReadResult {
        let end = (snapshot.offset + limit).min(snapshot.entries.len());
        let entries = snapshot.entries[snapshot.offset..end].to_vec();
        snapshot.offset = end;
        let revision = snapshot.revision;
        let next_cursor = if end < snapshot.entries.len() {
            let cursor = Uuid::new_v4().simple().to_string();
            self.state_cursors
                .lock()
                .await
                .insert(cursor.clone(), snapshot);
            Some(cursor)
        } else {
            None
        };
        StateReadResult {
            revision,
            current_revision,
            entries,
            next_cursor,
        }
    }

    /// Commit a bounded, evidence-validated State batch as one revision.
    #[tool(description = "Create, revise, or change lifecycle of i0i Research State entries")]
    async fn state_update(
        &self,
        context: RequestContext<RoleServer>,
        Parameters(input): Parameters<StateUpdateInput>,
    ) -> Result<Json<StateUpdateResult>, rmcp::ErrorData> {
        let started = Instant::now();
        let (grant, grant_token) = Self::context(&context, STATE_UPDATE)?;
        if input.project_id != grant.project_id {
            return Err(out_of_scope());
        }
        if input.changes.is_empty() || input.changes.len() > 20 {
            return Err(invalid_input(
                "changes must contain between 1 and 20 operations",
            ));
        }
        let request_id = input.request_id.trim();
        if request_id.is_empty() {
            return Err(invalid_input("request_id must not be empty"));
        }
        let payload_hash = sha256(
            &serde_json::to_string(&input).map_err(|error| internal_failure(error.to_string()))?,
        );
        if let Some(receipt) = self
            .store
            .find_agent_state_update_receipt(&grant.caller, request_id, &payload_hash)
            .map_err(state_write_failure)?
        {
            trace_tool(
                &grant,
                STATE_UPDATE,
                started,
                "retry",
                receipt.affected_entry_ids.len(),
            );
            return Ok(Json(StateUpdateResult {
                revision: receipt.revision,
                affected_entry_ids: receipt.affected_entry_ids,
                created_entry_ids: receipt.created_entry_ids,
            }));
        }
        let mut changes = Vec::with_capacity(input.changes.len());
        for change in input.changes {
            changes.push(
                self.prepare_state_change(&grant, &grant_token, change)
                    .await?,
            );
        }
        let receipt = self
            .store
            .apply_agent_state_update(
                &grant.project_id,
                input.base_revision,
                &grant.caller,
                grant.run_id.as_deref(),
                request_id,
                &payload_hash,
                changes,
            )
            .map_err(state_write_failure)?;
        if let Some(app) = &self.app {
            let _ = app.emit(
                "research_state_updated",
                serde_json::json!({
                    "projectId": grant.project_id,
                    "revision": receipt.revision,
                }),
            );
        }
        trace_tool(
            &grant,
            STATE_UPDATE,
            started,
            "ok",
            receipt.affected_entry_ids.len(),
        );
        Ok(Json(StateUpdateResult {
            revision: receipt.revision,
            affected_entry_ids: receipt.affected_entry_ids,
            created_entry_ids: receipt.created_entry_ids,
        }))
    }

    async fn prepare_state_change(
        &self,
        grant: &McpCallContext,
        grant_token: &str,
        change: StateChangeInput,
    ) -> Result<AgentStateChange, rmcp::ErrorData> {
        match change {
            StateChangeInput::Create {
                operation_key,
                kind,
                statement,
                epistemic_status,
                evidence,
                relationships,
                reason,
            } => {
                let reason = validate_state_reason(reason)?;
                let (evidence, evidence_relationships) = self
                    .prepare_state_evidence(grant, grant_token, None, evidence)
                    .await?;
                Ok(AgentStateChange::Create {
                    operation_key,
                    draft: ResearchEntryDraft {
                        kind: parse_minimal_kind(&kind)?,
                        epistemic_status: parse_epistemic_status(&epistemic_status)?,
                        text: statement,
                        evidence,
                        relations: parse_state_relations(relationships)?,
                        context: Vec::new(),
                        reason: Some(reason),
                    },
                    evidence_relationships,
                })
            }
            StateChangeInput::Revise {
                entry_id,
                statement,
                epistemic_status,
                evidence,
                relationships,
                reason,
            } => {
                let reason = validate_state_reason(reason)?;
                let (evidence, evidence_relationships) = self
                    .prepare_state_evidence(grant, grant_token, Some(&entry_id), evidence)
                    .await?;
                Ok(AgentStateChange::Revise {
                    update: ResearchEntryUpdate {
                        id: entry_id,
                        epistemic_status: parse_epistemic_status(&epistemic_status)?,
                        text: statement,
                        evidence,
                        relations: parse_state_relations(relationships)?,
                        context: Vec::new(),
                        reason: Some(reason),
                    },
                    evidence_relationships,
                })
            }
            StateChangeInput::SetLifecycle {
                entry_id,
                lifecycle,
                reason,
            } => Ok(AgentStateChange::SetLifecycle {
                entry_id,
                lifecycle: EntryLifecycle::parse(&lifecycle).map_err(|_| {
                    invalid_input("lifecycle must be active, contested, or superseded")
                })?,
                reason: validate_state_reason(reason)?,
            }),
        }
    }

    async fn prepare_state_evidence(
        &self,
        grant: &McpCallContext,
        grant_token: &str,
        revised_entry_id: Option<&str>,
        evidence: Vec<StateEvidenceInput>,
    ) -> Result<(Vec<EvidenceLinkDraft>, Vec<String>), rmcp::ErrorData> {
        let snapshot = self.snapshot()?;
        let retained = revised_entry_id
            .map(|entry_id| self.store.get_research_entry(entry_id, None))
            .transpose()
            .map_err(internal_failure)?;
        if retained
            .as_ref()
            .is_some_and(|detail| detail.entry.project_id != grant.project_id)
        {
            return Err(out_of_scope());
        }
        let mut drafts = Vec::new();
        let mut relationships = Vec::new();
        for item in evidence {
            match item {
                StateEvidenceInput::Passage {
                    passage_ref,
                    relationship,
                    explanation,
                } => {
                    validate_evidence_relationship(&relationship)?;
                    let explanation = explanation.trim().to_string();
                    if explanation.is_empty() {
                        return Err(invalid_input("Evidence explanation must not be empty"));
                    }
                    let passage = self
                        .passage_anchors
                        .read()
                        .await
                        .get(&passage_ref)
                        .filter(|registered| registered.grant_token == grant_token)
                        .map(|registered| registered.anchor.clone())
                        .ok_or_else(|| {
                            invalid_input("Evidence passage is stale or out of scope")
                        })?;
                    let paper = Self::scoped_paper(&snapshot, grant, &passage.paper_id)?;
                    validate_passage_anchor(&snapshot, paper, &passage)?;
                    let chunk_id = passage
                        .chunk_id
                        .ok_or_else(|| invalid_input("Passage has no citable source chunk"))?;
                    drafts.push(EvidenceLinkDraft {
                        chunk_id,
                        excerpt: Some(passage.quote),
                        support_note: Some(explanation),
                    });
                    relationships.push(relationship);
                }
                StateEvidenceInput::Retain { evidence_id } => {
                    let link = retained
                        .as_ref()
                        .and_then(|detail| {
                            detail.evidence.iter().find(|link| link.id == evidence_id)
                        })
                        .ok_or_else(|| {
                            invalid_input("Retained evidence does not belong to this entry")
                        })?;
                    drafts.push(EvidenceLinkDraft {
                        chunk_id: link.chunk_id.clone(),
                        excerpt: Some(link.excerpt.clone()),
                        support_note: link.support_note.clone(),
                    });
                    relationships.push(link.relationship.clone());
                }
            }
        }
        Ok((drafts, relationships))
    }

    async fn start_reader_page(
        &self,
        grant: &McpCallContext,
        grant_token: &str,
        paper_id: &str,
        page_start: Option<i32>,
        page_end: Option<i32>,
    ) -> Result<ReaderReadResult, rmcp::ErrorData> {
        let snapshot = self.snapshot()?;
        let paper = Self::scoped_paper(&snapshot, grant, paper_id)?;
        let source = Self::active_source(&snapshot, paper);

        let Some(source) = source else {
            return self.abstract_or_unavailable(grant_token, paper).await;
        };
        if source.source_kind == "html" {
            if page_start.is_some() {
                return Err(rmcp::ErrorData::invalid_params(
                    "HTML documents do not support PDF page ranges",
                    Some(serde_json::json!({"code": "invalid_input"})),
                ));
            }
            if source.status != "cached" {
                return Ok(unavailable_or_pending(source));
            }
            let text = read_html_source_text(source).map_err(internal_failure)?;
            let passages = self
                .passages_from_flow_text(grant_token, paper_id, &source.id, &text)
                .await;
            return self
                .first_reader_result(
                    grant_token,
                    paper_id,
                    &source.id,
                    None,
                    None,
                    "full_text",
                    passages,
                )
                .await;
        }

        if source.status == "remote_available" || source.status == "downloading" {
            if source.status == "remote_available" {
                if let Some(downloads) = &self.pdf_downloads {
                    downloads.queue_source(source.id.clone());
                }
            }
            return Ok(ReaderReadResult::pending(
                "PDF acquisition is still in progress",
                source.source_url.clone(),
            ));
        }
        if source.status == "failed" {
            return Ok(ReaderReadResult::unavailable(
                source.error.as_deref().unwrap_or("PDF acquisition failed"),
                source.source_url.clone(),
            ));
        }

        let extraction = snapshot.document_extractions.iter().find(|extraction| {
            extraction.source_id == source.id
                && extraction.status == "ready"
                && (paper.active_extraction_id.as_deref() == Some(extraction.id.as_str())
                    || paper.active_extraction_id.is_none())
        });
        let Some(extraction) = extraction else {
            if let Some(failed) = snapshot.document_extractions.iter().find(|extraction| {
                extraction.source_id == source.id && extraction.status == "failed"
            }) {
                return Ok(ReaderReadResult::unavailable(
                    failed.error.as_deref().unwrap_or("Text extraction failed"),
                    source.source_url.clone(),
                ));
            }
            if let Some(extractions) = &self.pdf_extractions {
                extractions.queue_source(source.id.clone(), false);
            }
            return Ok(ReaderReadResult::pending(
                "PDF text extraction is still in progress",
                source.source_url.clone(),
            ));
        };

        let chunks = self
            .store
            .chunks_for_extraction(&extraction.id)
            .map_err(internal_failure)?;
        let passages = self
            .passages_from_chunks(grant_token, chunks, page_start, page_end)
            .await;
        self.first_reader_result(
            grant_token,
            paper_id,
            &extraction.id,
            page_start,
            page_end,
            "full_text",
            passages,
        )
        .await
    }

    async fn abstract_or_unavailable(
        &self,
        grant_token: &str,
        paper: &Paper,
    ) -> Result<ReaderReadResult, rmcp::ErrorData> {
        let Some(_) = paper.abstract_text.as_deref() else {
            return Ok(ReaderReadResult::unavailable(
                "No readable source or abstract is available",
                None,
            ));
        };
        let chunk = self
            .store
            .materialize_paper_abstract(&paper.id)
            .map_err(internal_failure)?;
        let source_version = chunk.extraction_id.clone();
        let passages = self
            .passages_from_chunks(grant_token, vec![chunk], None, None)
            .await;
        Ok(ReaderReadResult {
            availability: "available".to_string(),
            coverage: "abstract_only".to_string(),
            source_version: Some(source_version),
            passages,
            next_cursor: None,
            has_more: false,
            reason: None,
            source_url: None,
        })
    }

    async fn first_reader_result(
        &self,
        grant_token: &str,
        paper_id: &str,
        source_version: &str,
        page_start: Option<i32>,
        page_end: Option<i32>,
        coverage: &str,
        passages: Vec<McpPassage>,
    ) -> Result<ReaderReadResult, rmcp::ErrorData> {
        self.reader_result_from_snapshot(
            ReaderCursorSnapshot {
                grant_token: grant_token.to_string(),
                paper_id: paper_id.to_string(),
                source_version: source_version.to_string(),
                page_start,
                page_end,
                passages,
                offset: 0,
                expires_at: Instant::now() + self.cursor_ttl,
            },
            coverage,
        )
        .await
    }

    async fn continue_reader_page(
        &self,
        cursor: &str,
        grant_token: &str,
        paper_id: &str,
        page_start: Option<i32>,
        page_end: Option<i32>,
    ) -> Result<ReaderReadResult, rmcp::ErrorData> {
        let snapshot = self
            .reader_cursors
            .lock()
            .await
            .remove(cursor)
            .ok_or_else(cursor_expired)?;
        if snapshot.expires_at <= Instant::now() {
            return Err(cursor_expired());
        }
        if snapshot.grant_token != grant_token
            || snapshot.paper_id != paper_id
            || snapshot.page_start != page_start
            || snapshot.page_end != page_end
        {
            return Err(rmcp::ErrorData::invalid_params(
                "Cursor does not belong to this paper, range, or connection",
                Some(serde_json::json!({"code": "invalid_input"})),
            ));
        }
        self.reader_result_from_snapshot(snapshot, "full_text")
            .await
    }

    async fn reader_result_from_snapshot(
        &self,
        mut snapshot: ReaderCursorSnapshot,
        coverage: &str,
    ) -> Result<ReaderReadResult, rmcp::ErrorData> {
        let start = snapshot.offset;
        let mut end = start;
        let mut characters = 0;
        while end < snapshot.passages.len() {
            let next = snapshot.passages[end].text.chars().count();
            if end > start && characters + next > READER_TEXT_BUDGET {
                break;
            }
            characters += next;
            end += 1;
        }
        let passages = snapshot.passages[start..end].to_vec();
        snapshot.offset = end;
        let source_version = snapshot.source_version.clone();
        let has_more = end < snapshot.passages.len();
        let next_cursor = if has_more {
            let cursor = Uuid::new_v4().simple().to_string();
            self.reader_cursors
                .lock()
                .await
                .insert(cursor.clone(), snapshot);
            Some(cursor)
        } else {
            None
        };
        Ok(ReaderReadResult {
            availability: "available".to_string(),
            coverage: coverage.to_string(),
            source_version: Some(source_version),
            passages,
            next_cursor,
            has_more,
            reason: None,
            source_url: None,
        })
    }

    async fn start_paper_page(
        &self,
        grant: &McpCallContext,
        grant_token: &str,
        input: &VaultPapersInput,
        limit: usize,
    ) -> Result<VaultPapersResult, rmcp::ErrorData> {
        let snapshot = self.snapshot()?;
        let vault = snapshot
            .vaults
            .iter()
            .find(|vault| vault.id == grant.vault_id && vault.project_id == grant.project_id)
            .ok_or_else(out_of_scope)?;
        let member_ids = snapshot
            .vault_papers
            .iter()
            .filter(|membership| membership.vault_id == grant.vault_id)
            .map(|membership| membership.paper_id.as_str())
            .collect::<BTreeSet<_>>();
        let normalized_title = input
            .title
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_lowercase);
        let mut papers = snapshot
            .papers
            .iter()
            .filter(|paper| member_ids.contains(paper.id.as_str()))
            .filter(|paper| input.year.is_none_or(|year| paper.year == year))
            .filter(|paper| {
                normalized_title
                    .as_ref()
                    .is_none_or(|title| paper.title.to_lowercase().contains(title))
            })
            .map(McpPaper::from)
            .collect::<Vec<_>>();
        papers.sort_by(|left, right| left.id.cmp(&right.id));

        self.page_from_snapshot(
            CursorSnapshot {
                grant_token: grant_token.to_string(),
                vault_id: grant.vault_id.clone(),
                title: normalized_title,
                year: input.year,
                membership_revision: vault.membership_revision,
                papers,
                offset: 0,
                expires_at: Instant::now() + self.cursor_ttl,
            },
            limit,
        )
        .await
    }

    async fn continue_paper_page(
        &self,
        cursor: &str,
        grant_token: &str,
        input: &VaultPapersInput,
        limit: usize,
    ) -> Result<VaultPapersResult, rmcp::ErrorData> {
        let mut cursors = self.cursors.lock().await;
        let snapshot = cursors.remove(cursor).ok_or_else(cursor_expired)?;
        let normalized_title = input
            .title
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_lowercase);
        if snapshot.expires_at <= Instant::now() {
            return Err(cursor_expired());
        }
        if snapshot.grant_token != grant_token
            || snapshot.vault_id != input.vault_id
            || snapshot.title != normalized_title
            || snapshot.year != input.year
        {
            return Err(rmcp::ErrorData::invalid_params(
                "Cursor does not belong to these filters or this connection",
                Some(serde_json::json!({"code": "invalid_input"})),
            ));
        }
        drop(cursors);
        self.page_from_snapshot(snapshot, limit).await
    }

    async fn page_from_snapshot(
        &self,
        mut snapshot: CursorSnapshot,
        limit: usize,
    ) -> Result<VaultPapersResult, rmcp::ErrorData> {
        let end = (snapshot.offset + limit).min(snapshot.papers.len());
        let papers = snapshot.papers[snapshot.offset..end].to_vec();
        snapshot.offset = end;
        let membership_revision = snapshot.membership_revision;
        let next_cursor = if end < snapshot.papers.len() {
            let cursor = Uuid::new_v4().simple().to_string();
            self.cursors.lock().await.insert(cursor.clone(), snapshot);
            Some(cursor)
        } else {
            None
        };
        Ok(VaultPapersResult {
            papers,
            membership_revision,
            next_cursor,
        })
    }
}

#[derive(Debug, Clone)]
struct AuthorizedGrant {
    token: String,
    context: McpCallContext,
}

/// An app-owned loopback MCP endpoint.
#[derive(Clone)]
pub struct LocalMcpServer {
    endpoint: String,
    grants: GrantRegistry,
    handler: I0iMcpHandler,
    cancellation: CancellationToken,
    server_task: Arc<Mutex<Option<JoinHandle<()>>>>,
}

impl LocalMcpServer {
    /// Bind an authenticated Streamable HTTP MCP endpoint on an ephemeral port.
    pub async fn start(
        app: AppHandle,
        store: LibraryStore,
        pdf_downloads: PdfDownloadManager,
        pdf_extractions: PdfExtractionManager,
    ) -> Result<Self, String> {
        Self::start_with_cursor_ttl(
            Some(app),
            store,
            Some(pdf_downloads),
            Some(pdf_extractions),
            DEFAULT_CURSOR_TTL,
        )
        .await
    }

    async fn start_with_cursor_ttl(
        app: Option<AppHandle>,
        store: LibraryStore,
        pdf_downloads: Option<PdfDownloadManager>,
        pdf_extractions: Option<PdfExtractionManager>,
        cursor_ttl: Duration,
    ) -> Result<Self, String> {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .map_err(|error| error.to_string())?;
        let address = listener.local_addr().map_err(|error| error.to_string())?;
        let endpoint = format!("http://{address}/mcp");
        let grants = GrantRegistry::new();
        let cancellation = CancellationToken::new();
        let handler = I0iMcpHandler::new(app, store, pdf_downloads, pdf_extractions, cursor_ttl);
        let session_handler = handler.clone();
        let service: StreamableHttpService<I0iMcpHandler, LocalSessionManager> =
            StreamableHttpService::new(
                move || Ok(session_handler.clone()),
                Default::default(),
                StreamableHttpServerConfig::default()
                    .with_allowed_hosts(vec![
                        "localhost".to_string(),
                        "127.0.0.1".to_string(),
                        format!("localhost:{}", address.port()),
                        address.to_string(),
                    ])
                    .with_allowed_origins(["tauri://localhost", "http://tauri.localhost"])
                    .with_sse_keep_alive(None)
                    .with_cancellation_token(cancellation.child_token()),
            );
        let router = axum::Router::new().nest_service("/mcp", service).layer(
            middleware::from_fn_with_state(grants.clone(), authorize_request),
        );
        let shutdown = cancellation.clone();
        let server_task = tokio::spawn(async move {
            if let Err(error) = axum::serve(listener, router)
                .with_graceful_shutdown(async move { shutdown.cancelled_owned().await })
                .await
            {
                eprintln!("[mcp] server stopped with error: {error}");
            }
        });
        eprintln!("[mcp] listening on {endpoint}");
        Ok(Self {
            endpoint,
            grants,
            handler,
            cancellation,
            server_task: Arc::new(Mutex::new(Some(server_task))),
        })
    }

    /// Issue an app-session grant for a project and its vault.
    pub async fn issue_grant(
        &self,
        store: &LibraryStore,
        project_id: &str,
        vault_id: &str,
        caller: &str,
        run_id: Option<&str>,
        tools: impl IntoIterator<Item = &'static str>,
    ) -> Result<McpConnectionGrant, String> {
        let snapshot = store.get_library()?;
        let scoped_vault_exists = snapshot
            .vaults
            .iter()
            .any(|vault| vault.id == vault_id && vault.project_id == project_id);
        if !scoped_vault_exists {
            return Err("Project vault not found".to_string());
        }
        let token = self
            .grants
            .issue(McpCallContext {
                project_id: project_id.to_string(),
                vault_id: vault_id.to_string(),
                caller: caller.to_string(),
                run_id: run_id.map(str::to_string),
                tools: tools.into_iter().map(str::to_string).collect(),
            })
            .await;
        Ok(McpConnectionGrant {
            endpoint: self.endpoint.clone(),
            bearer_token: token,
        })
    }

    /// Revoke one credential without revealing whether it ever existed.
    pub async fn revoke_grant(&self, token: &str) {
        self.grants.revoke(token).await;
    }

    /// Revoke every credential owned by a completed or canceled run.
    pub async fn revoke_run(&self, run_id: &str) {
        self.grants.revoke_run(run_id).await;
    }

    /// Resolve a passage issued by this app session for a later write tool.
    pub(crate) async fn resolve_passage(&self, passage_ref: &str) -> Option<PassageAnchor> {
        self.handler
            .passage_anchors
            .read()
            .await
            .get(passage_ref)
            .map(|registered| registered.anchor.clone())
    }

    /// Stop accepting calls and wait for the HTTP task to finish.
    pub async fn shutdown(&self) {
        self.cancellation.cancel();
        if let Some(task) = self.server_task.lock().await.take() {
            let _ = task.await;
        }
    }
}

async fn authorize_request(
    State(registry): State<GrantRegistry>,
    mut request: Request,
    next: Next,
) -> Response {
    let token = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .map(str::to_string);
    let Some(token) = token else {
        return unauthorized_response();
    };
    let Some(context) = registry.resolve(&token).await else {
        return unauthorized_response();
    };
    request
        .extensions_mut()
        .insert(AuthorizedGrant { token, context });
    next.run(request).await
}

fn unauthorized_response() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        [(header::WWW_AUTHENTICATE, "Bearer realm=\"i0i-mcp\"")],
        "Unauthorized",
    )
        .into_response()
}

fn validated_limit(limit: Option<usize>) -> Result<usize, rmcp::ErrorData> {
    let limit = limit.unwrap_or(DEFAULT_PAGE_SIZE);
    if !(1..=MAX_PAGE_SIZE).contains(&limit) {
        return Err(rmcp::ErrorData::invalid_params(
            format!("limit must be between 1 and {MAX_PAGE_SIZE}"),
            Some(serde_json::json!({"code": "invalid_input"})),
        ));
    }
    Ok(limit)
}

fn out_of_scope() -> rmcp::ErrorData {
    rmcp::ErrorData::invalid_request(
        "Vault is not available to this connection",
        Some(serde_json::json!({"code": "out_of_scope"})),
    )
}

fn cursor_expired() -> rmcp::ErrorData {
    rmcp::ErrorData::invalid_params(
        "Cursor is unknown or expired; start a new listing",
        Some(serde_json::json!({"code": "cursor_expired"})),
    )
}

fn invalid_input(message: &str) -> rmcp::ErrorData {
    rmcp::ErrorData::invalid_params(
        message.to_string(),
        Some(serde_json::json!({"code": "invalid_input"})),
    )
}

fn write_failure(error: String) -> rmcp::ErrorData {
    let code = if error.contains("Request ID") {
        "request_conflict"
    } else {
        "internal_failure"
    };
    rmcp::ErrorData::invalid_request(error, Some(serde_json::json!({"code": code})))
}

fn state_write_failure(error: String) -> rmcp::ErrorData {
    let code = if error.contains("Request ID") {
        "request_conflict"
    } else if error.contains("Research State changed") {
        "conflict"
    } else {
        "invalid_input"
    };
    rmcp::ErrorData::invalid_request(error, Some(serde_json::json!({"code": code})))
}

/// Confirm that an in-memory passage still names the currently persisted source.
fn validate_passage_anchor(
    snapshot: &LibrarySnapshot,
    paper: &Paper,
    passage: &PassageAnchor,
) -> Result<(), rmcp::ErrorData> {
    if passage.paper_id != paper.id || passage.source_end < passage.source_start {
        return Err(invalid_input(
            "Passage reference does not belong to this paper",
        ));
    }
    if passage.source_id.starts_with("metadata:") {
        let abstract_text = paper
            .abstract_text
            .as_deref()
            .ok_or_else(|| invalid_input("Abstract passage is no longer available"))?;
        let expected = format!("metadata:{}:{}", paper.id, &sha256(abstract_text)[..12]);
        if passage.source_id != expected {
            return Err(invalid_input("Abstract passage refers to an older version"));
        }
        return Ok(());
    }
    let source_exists = snapshot
        .document_sources
        .iter()
        .any(|source| source.id == passage.source_id && source.paper_id == paper.id);
    let extraction_exists = passage.extraction_id.as_ref().is_none_or(|extraction_id| {
        snapshot.document_extractions.iter().any(|extraction| {
            extraction.id == *extraction_id
                && extraction.source_id == passage.source_id
                && extraction.status == "ready"
        })
    });
    if !source_exists || !extraction_exists {
        return Err(invalid_input(
            "Passage reference names a stale document version",
        ));
    }
    Ok(())
}

fn parse_minimal_kind(value: &str) -> Result<ResearchEntryKind, rmcp::ErrorData> {
    match value {
        "finding" => Ok(ResearchEntryKind::Finding),
        "hypothesis" => Ok(ResearchEntryKind::Hypothesis),
        "question" => Ok(ResearchEntryKind::Question),
        _ => Err(invalid_input(
            "kind must be finding, hypothesis, or question",
        )),
    }
}

fn parse_epistemic_status(value: &str) -> Result<EpistemicStatus, rmcp::ErrorData> {
    EpistemicStatus::parse(value).map_err(|_| invalid_input("Invalid epistemic_status"))
}

fn parse_state_relations(
    values: Vec<StateRelationInput>,
) -> Result<Vec<EntryRelationDraft>, rmcp::ErrorData> {
    values
        .into_iter()
        .map(|value| {
            Ok(EntryRelationDraft {
                target_entry_id: value.target_entry_id,
                kind: EntryRelationKind::parse(&value.kind)
                    .map_err(|_| invalid_input("Invalid entry relationship kind"))?,
            })
        })
        .collect()
}

fn validate_evidence_relationship(value: &str) -> Result<(), rmcp::ErrorData> {
    if matches!(value, "supports" | "contradicts" | "context") {
        Ok(())
    } else {
        Err(invalid_input(
            "Evidence relationship must be supports, contradicts, or context",
        ))
    }
}

fn validate_state_reason(value: String) -> Result<String, rmcp::ErrorData> {
    let reason = value.trim();
    if reason.is_empty() {
        Err(invalid_input("State change reason must not be empty"))
    } else {
        Ok(reason.to_string())
    }
}

fn trace_tool(
    context: &McpCallContext,
    tool: &str,
    started: Instant,
    outcome: &str,
    result_count: usize,
) {
    eprintln!(
        "[mcp] caller={} run={} tool={} elapsed_ms={} outcome={} results={}",
        context.caller,
        context.run_id.as_deref().unwrap_or("none"),
        tool,
        started.elapsed().as_millis(),
        outcome,
        result_count
    );
}

fn validate_page_range(
    page_start: Option<i32>,
    page_end: Option<i32>,
) -> Result<(Option<i32>, Option<i32>), rmcp::ErrorData> {
    match (page_start, page_end) {
        (None, None) => Ok((None, None)),
        (Some(start), None) if start >= 1 => Ok((Some(start), Some(start))),
        (None, Some(end)) if end >= 1 => Ok((Some(1), Some(end))),
        (Some(start), Some(end)) if start >= 1 && end >= start => Ok((Some(start), Some(end))),
        _ => Err(rmcp::ErrorData::invalid_params(
            "Pages are one-based and page_end must not precede page_start",
            Some(serde_json::json!({"code": "invalid_input"})),
        )),
    }
}

fn split_text(text: &str, max_chars: usize) -> Vec<(usize, usize, String)> {
    let characters = text.chars().collect::<Vec<_>>();
    (0..characters.len())
        .step_by(max_chars)
        .map(|start| {
            let end = (start + max_chars).min(characters.len());
            (start, end, characters[start..end].iter().collect())
        })
        .collect()
}

fn sha256(text: &str) -> String {
    format!("{:x}", Sha256::digest(text.as_bytes()))
}

fn read_html_source_text(source: &DocumentSource) -> Result<String, String> {
    let html_path = source
        .local_path
        .as_deref()
        .ok_or_else(|| "Saved HTML source has no local path".to_string())?;
    let metadata = fs::read_to_string(Path::new(html_path).with_file_name("meta.json"))
        .map_err(|error| format!("Saved HTML metadata is missing: {error}"))?;
    serde_json::from_str::<serde_json::Value>(&metadata)
        .map_err(|error| format!("Saved HTML metadata is invalid: {error}"))?
        .get("source_text")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| "Saved HTML metadata has no source text".to_string())
}

fn internal_failure(error: String) -> rmcp::ErrorData {
    rmcp::ErrorData::internal_error(
        "Could not read document evidence",
        Some(serde_json::json!({"code": "internal_failure", "detail": error})),
    )
}

struct Availability {
    status: String,
    reason: Option<String>,
}

fn document_availability(
    paper: &Paper,
    source: Option<&DocumentSource>,
    extraction: Option<&crate::domain::library::DocumentExtraction>,
) -> Availability {
    let Some(source) = source else {
        return Availability {
            status: if paper.abstract_text.is_some() {
                "abstract_only"
            } else {
                "unavailable"
            }
            .to_string(),
            reason: None,
        };
    };
    if source.status == "failed" {
        return Availability {
            status: "unavailable".to_string(),
            reason: source.error.clone(),
        };
    }
    if source.source_kind == "html" && source.status == "cached" {
        return Availability {
            status: "full_text".to_string(),
            reason: None,
        };
    }
    if extraction.is_some_and(|value| value.status == "ready") {
        return Availability {
            status: "full_text".to_string(),
            reason: None,
        };
    }
    Availability {
        status: "pending".to_string(),
        reason: Some("Document acquisition or extraction is pending".to_string()),
    }
}

fn unavailable_or_pending(source: &DocumentSource) -> ReaderReadResult {
    if source.status == "failed" {
        ReaderReadResult::unavailable(
            source
                .error
                .as_deref()
                .unwrap_or("Document acquisition failed"),
            source.source_url.clone(),
        )
    } else {
        ReaderReadResult::pending(
            "Document acquisition is still in progress",
            source.source_url.clone(),
        )
    }
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
struct PageInput {
    cursor: Option<String>,
    limit: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct VaultPapersInput {
    vault_id: String,
    title: Option<String>,
    year: Option<i32>,
    cursor: Option<String>,
    limit: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct GetPaperInput {
    vault_id: String,
    paper_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct ReaderReadInput {
    paper_id: String,
    page_start: Option<i32>,
    page_end: Option<i32>,
    cursor: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct ReaderAddNoteInput {
    paper_id: String,
    passage_ref: Option<String>,
    body: String,
    request_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct ReaderListNotesInput {
    paper_id: String,
    cursor: Option<String>,
    limit: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct StateReadInput {
    project_id: String,
    entry_ids: Option<Vec<String>>,
    cursor: Option<String>,
    limit: Option<usize>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
struct StateUpdateInput {
    project_id: String,
    base_revision: i64,
    request_id: String,
    changes: Vec<StateChangeInput>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "operation", rename_all = "snake_case")]
enum StateChangeInput {
    Create {
        operation_key: String,
        kind: String,
        statement: String,
        epistemic_status: String,
        evidence: Vec<StateEvidenceInput>,
        relationships: Vec<StateRelationInput>,
        reason: String,
    },
    Revise {
        entry_id: String,
        statement: String,
        epistemic_status: String,
        evidence: Vec<StateEvidenceInput>,
        relationships: Vec<StateRelationInput>,
        reason: String,
    },
    SetLifecycle {
        entry_id: String,
        lifecycle: String,
        reason: String,
    },
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "source", rename_all = "snake_case")]
enum StateEvidenceInput {
    Passage {
        passage_ref: String,
        relationship: String,
        explanation: String,
    },
    Retain {
        evidence_id: String,
    },
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
struct StateRelationInput {
    target_entry_id: String,
    kind: String,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct ReaderAddNoteResult {
    thread_id: String,
    entry_id: String,
    anchor: serde_json::Value,
    quote: Option<String>,
    author: McpNoteAuthor,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct McpNoteAuthor {
    kind: String,
    id: String,
    run_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct McpReaderNote {
    kind: String,
    entry_id: String,
    thread_id: String,
    body: String,
    anchor: serde_json::Value,
    quote: Option<String>,
    author: McpNoteAuthor,
    created_at: String,
}

impl McpReaderNote {
    fn new(entry: ChatEntry, anchor: &ThreadAnchor) -> Self {
        Self {
            kind: "thread_note".to_string(),
            entry_id: entry.id,
            thread_id: entry.thread_id,
            body: entry.body,
            anchor: serde_json::to_value(anchor).unwrap_or(serde_json::Value::Null),
            quote: anchor.selected_text().map(str::to_string),
            author: McpNoteAuthor {
                kind: entry.author_kind,
                id: entry.author_id.unwrap_or_else(|| "researcher".to_string()),
                run_id: entry.run_id,
            },
            created_at: entry.created_at,
        }
    }

    fn from_annotation(highlight: crate::domain::highlight::Highlight) -> Option<Self> {
        let body = highlight.note?.trim().to_string();
        if body.is_empty() {
            return None;
        }
        let (author_kind, author_id) = match highlight.author {
            HighlightAuthor::User => ("user".to_string(), "researcher".to_string()),
            HighlightAuthor::Agent { model } => ("agent".to_string(), model),
        };
        Some(Self {
            kind: "annotation_note".to_string(),
            entry_id: highlight.id.clone(),
            thread_id: String::new(),
            body,
            anchor: serde_json::to_value(&highlight.locator).unwrap_or(serde_json::Value::Null),
            quote: Some(highlight.excerpt),
            author: McpNoteAuthor {
                kind: author_kind,
                id: author_id,
                run_id: None,
            },
            created_at: highlight.created_at,
        })
    }
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct ReaderListNotesResult {
    notes: Vec<McpReaderNote>,
    next_cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct McpStateEvidence {
    id: String,
    relationship: String,
    paper_id: String,
    source_id: String,
    extraction_id: String,
    chunk_id: String,
    excerpt: String,
    source_start: i64,
    source_end: i64,
    page_start: i32,
    page_end: i32,
    explanation: Option<String>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct McpStateRelation {
    target_entry_id: String,
    kind: String,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct McpStateContext {
    kind: String,
    context_id: String,
    label: String,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct McpStateEntry {
    id: String,
    kind: String,
    original_kind: String,
    statement: String,
    epistemic_status: String,
    lifecycle: String,
    first_revision: i64,
    last_revision: i64,
    origin_run_id: Option<String>,
    revision_run_id: Option<String>,
    evidence: Vec<McpStateEvidence>,
    related_entries: Vec<McpStateRelation>,
    context: Vec<McpStateContext>,
}

impl McpStateEntry {
    fn from_detail(
        detail: crate::domain::research_state::ResearchEntryDetail,
        revisions: &[crate::domain::research_state::ResearchStateRevision],
    ) -> Self {
        let original_kind = detail.entry.kind.as_str().to_string();
        let kind = match detail.entry.kind {
            crate::domain::research_state::ResearchEntryKind::Finding => "finding",
            crate::domain::research_state::ResearchEntryKind::Hypothesis => "hypothesis",
            crate::domain::research_state::ResearchEntryKind::Question
            | crate::domain::research_state::ResearchEntryKind::Gap
            | crate::domain::research_state::ResearchEntryKind::ExperimentIdea => "question",
        }
        .to_string();
        let revision_run_id = revisions
            .iter()
            .find(|revision| revision.revision == detail.entry.last_revision)
            .and_then(|revision| revision.run_id.clone());
        Self {
            id: detail.entry.id,
            kind,
            original_kind,
            statement: detail.entry.text,
            epistemic_status: detail.entry.epistemic_status.as_str().to_string(),
            lifecycle: detail.entry.lifecycle.as_str().to_string(),
            first_revision: detail.entry.first_revision,
            last_revision: detail.entry.last_revision,
            origin_run_id: detail.entry.origin_run_id,
            revision_run_id,
            evidence: detail
                .evidence
                .into_iter()
                .map(|evidence| McpStateEvidence {
                    id: evidence.id,
                    relationship: evidence.relationship,
                    paper_id: evidence.paper_id,
                    source_id: evidence.source_id,
                    extraction_id: evidence.extraction_id,
                    chunk_id: evidence.chunk_id,
                    excerpt: evidence.excerpt,
                    source_start: evidence.source_start,
                    source_end: evidence.source_end,
                    page_start: evidence.page_start,
                    page_end: evidence.page_end,
                    explanation: evidence.support_note,
                })
                .collect(),
            related_entries: detail
                .relations
                .into_iter()
                .map(|relation| McpStateRelation {
                    target_entry_id: relation.target_entry_id,
                    kind: relation.kind.as_str().to_string(),
                })
                .collect(),
            context: detail
                .context
                .into_iter()
                .map(|context| McpStateContext {
                    kind: context.kind.as_str().to_string(),
                    context_id: context.context_id,
                    label: context.label,
                })
                .collect(),
        }
    }
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct StateReadResult {
    revision: i64,
    current_revision: i64,
    entries: Vec<McpStateEntry>,
    next_cursor: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct StateUpdateResult {
    revision: i64,
    affected_entry_ids: Vec<String>,
    created_entry_ids: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct McpVault {
    id: String,
    project_id: String,
    name: String,
    membership_revision: i64,
}

impl From<Vault> for McpVault {
    fn from(vault: Vault) -> Self {
        Self {
            id: vault.id,
            project_id: vault.project_id,
            name: vault.title,
            membership_revision: vault.membership_revision,
        }
    }
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct McpPaper {
    id: String,
    title: String,
    authors: Vec<String>,
    venue: String,
    year: i32,
    citations: i32,
    status: String,
    has_abstract: bool,
    source_id: Option<String>,
    extraction_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct McpSource {
    id: String,
    kind: String,
    status: String,
    source_url: Option<String>,
    error: Option<String>,
}

impl From<&DocumentSource> for McpSource {
    fn from(source: &DocumentSource) -> Self {
        Self {
            id: source.id.clone(),
            kind: source.source_kind.clone(),
            status: source.status.clone(),
            source_url: source.source_url.clone(),
            error: source.error.clone(),
        }
    }
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct GetPaperResult {
    paper: McpPaper,
    source: Option<McpSource>,
    extraction_id: Option<String>,
    text_availability: String,
    reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct McpPassage {
    passage_ref: String,
    paper_id: String,
    source_id: String,
    extraction_id: Option<String>,
    chunk_id: Option<String>,
    page_start: Option<i32>,
    page_end: Option<i32>,
    source_start: i64,
    source_end: i64,
    text: String,
}

impl McpPassage {
    fn from_anchor(passage_ref: String, anchor: PassageAnchor) -> Self {
        Self {
            passage_ref,
            paper_id: anchor.paper_id,
            source_id: anchor.source_id,
            extraction_id: anchor.extraction_id,
            chunk_id: anchor.chunk_id,
            page_start: anchor.page_start,
            page_end: anchor.page_end,
            source_start: anchor.source_start,
            source_end: anchor.source_end,
            text: anchor.quote,
        }
    }
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct ReaderReadResult {
    availability: String,
    coverage: String,
    source_version: Option<String>,
    passages: Vec<McpPassage>,
    next_cursor: Option<String>,
    has_more: bool,
    reason: Option<String>,
    source_url: Option<String>,
}

impl ReaderReadResult {
    fn pending(reason: &str, source_url: Option<String>) -> Self {
        Self {
            availability: "pending".to_string(),
            coverage: "none".to_string(),
            source_version: None,
            passages: Vec::new(),
            next_cursor: None,
            has_more: false,
            reason: Some(reason.to_string()),
            source_url,
        }
    }

    fn unavailable(reason: &str, source_url: Option<String>) -> Self {
        Self {
            availability: "unavailable".to_string(),
            coverage: "none".to_string(),
            source_version: None,
            passages: Vec::new(),
            next_cursor: None,
            has_more: false,
            reason: Some(reason.to_string()),
            source_url,
        }
    }
}

impl From<&Paper> for McpPaper {
    fn from(paper: &Paper) -> Self {
        Self {
            id: paper.id.clone(),
            title: paper.title.clone(),
            authors: paper.authors.clone(),
            venue: paper.venue.clone(),
            year: paper.year,
            citations: paper.citations,
            status: paper.status.clone(),
            has_abstract: paper.abstract_text.is_some(),
            source_id: paper.active_source_id.clone(),
            extraction_id: paper.active_extraction_id.clone(),
        }
    }
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct VaultListResult {
    vaults: Vec<McpVault>,
    next_cursor: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct VaultPapersResult {
    papers: Vec<McpPaper>,
    membership_revision: i64,
    next_cursor: Option<String>,
}

#[cfg(test)]
mod tests {
    use crate::domain::library::{DocumentBlock, DocumentPage, PaperDraft, PaperSourceDraft};
    use crate::domain::research_state::{EpistemicStatus, ResearchEntryDraft, ResearchEntryKind};
    use rmcp::model::CallToolRequestParams;
    use rmcp::transport::streamable_http_client::{
        StreamableHttpClientTransport, StreamableHttpClientTransportConfig,
    };
    use rmcp::ServiceExt;

    use super::*;

    fn test_store(name: &str) -> LibraryStore {
        let path =
            std::env::temp_dir().join(format!("i0i-mcp-{name}-{}.sqlite", Uuid::new_v4().simple()));
        let store = LibraryStore::for_test(path);
        store.init().expect("initialize test library");
        store
    }

    async fn client_for(
        grant: &McpConnectionGrant,
    ) -> rmcp::service::RunningService<rmcp::RoleClient, ()> {
        let transport = StreamableHttpClientTransport::from_config(
            StreamableHttpClientTransportConfig::with_uri(grant.endpoint.clone())
                .auth_header(&grant.bearer_token),
        );
        ().serve(transport).await.expect("connect MCP client")
    }

    fn paper_draft(id: &str, abstract_text: Option<&str>) -> PaperDraft {
        PaperDraft {
            id: id.to_string(),
            title: format!("Fixture {id}"),
            authors: vec!["Ada Example".to_string()],
            venue: "Fixture Journal".to_string(),
            year: 2026,
            citations: 0,
            tags: Vec::new(),
            status: "saved".to_string(),
            abstract_text: abstract_text.map(str::to_string),
            sources: Vec::new(),
        }
    }

    fn state_draft(kind: ResearchEntryKind, text: &str) -> ResearchEntryDraft {
        ResearchEntryDraft {
            kind,
            epistemic_status: EpistemicStatus::Speculative,
            text: text.to_string(),
            evidence: Vec::new(),
            relations: Vec::new(),
            context: Vec::new(),
            reason: Some("MCP fixture".to_string()),
        }
    }

    fn add_extracted_pdf(store: &LibraryStore, vault_id: &str, paper_id: &str, text: &str) {
        let source_id = format!("pdf:{paper_id}:fixture");
        let paper = paper_draft(paper_id, None);
        store
            .add_local_pdf_to_vault(
                &paper,
                vault_id,
                &source_id,
                "file://fixture.pdf",
                "/tmp/i0i-fixture.pdf",
            )
            .expect("add fixture PDF");
        let extraction = store
            .start_document_extraction(
                &source_id,
                "pdfium_basic",
                "fixture",
                &format!("annotation:{source_id}"),
                false,
            )
            .expect("start fixture extraction");
        store
            .finish_document_extraction(
                &extraction.id,
                &[DocumentPage {
                    id: format!("page:{paper_id}:0"),
                    paper_id: paper_id.to_string(),
                    source_id: source_id.clone(),
                    extraction_id: extraction.id.clone(),
                    page_index: 0,
                    width: 100.0,
                    height: 100.0,
                }],
                &[DocumentBlock {
                    id: format!("block:{paper_id}:0"),
                    paper_id: paper_id.to_string(),
                    source_id,
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
                }],
                &[],
            )
            .expect("finish fixture extraction");
    }

    #[tokio::test]
    async fn real_client_lists_only_the_granted_vault_and_its_papers() {
        let store = test_store("listing");
        let snapshot = store.get_library().expect("read fixture library");
        let vault = snapshot.vaults.first().expect("seed vault");
        let server = LocalMcpServer::start_with_cursor_ttl(
            None,
            store.clone(),
            None,
            None,
            DEFAULT_CURSOR_TTL,
        )
        .await
        .expect("start MCP");
        let grant = server
            .issue_grant(
                &store,
                &vault.project_id,
                &vault.id,
                "test-client",
                Some("test-run"),
                [VAULT_LIST, VAULT_LIST_PAPERS],
            )
            .await
            .expect("issue grant");
        let client = client_for(&grant).await;

        let tools = client.list_all_tools().await.expect("list tools");
        assert!(tools.iter().any(|tool| tool.name == VAULT_LIST));
        let vaults = client
            .call_tool(CallToolRequestParams::new(VAULT_LIST))
            .await
            .expect("call vault_list");
        assert_eq!(
            vaults.structured_content.unwrap()["vaults"][0]["id"],
            vault.id
        );

        let papers = client
            .call_tool(
                CallToolRequestParams::new(VAULT_LIST_PAPERS).with_arguments(
                    serde_json::json!({"vault_id": vault.id, "limit": 1})
                        .as_object()
                        .unwrap()
                        .clone(),
                ),
            )
            .await
            .expect("call vault_list_papers");
        assert_eq!(
            papers.structured_content.unwrap()["membershipRevision"],
            vault.membership_revision
        );

        client.cancel().await.expect("stop client");
        server.shutdown().await;
    }

    #[tokio::test]
    async fn revoked_grants_are_rejected_by_http_transport() {
        let store = test_store("revoked");
        let snapshot = store.get_library().expect("read fixture library");
        let vault = snapshot.vaults.first().expect("seed vault");
        let server = LocalMcpServer::start_with_cursor_ttl(
            None,
            store.clone(),
            None,
            None,
            DEFAULT_CURSOR_TTL,
        )
        .await
        .expect("start MCP");
        let grant = server
            .issue_grant(
                &store,
                &vault.project_id,
                &vault.id,
                "test-client",
                None,
                [VAULT_LIST],
            )
            .await
            .expect("issue grant");
        server.revoke_grant(&grant.bearer_token).await;

        let transport = StreamableHttpClientTransport::from_config(
            StreamableHttpClientTransportConfig::with_uri(grant.endpoint.clone())
                .auth_header(&grant.bearer_token),
        );
        assert!(().serve(transport).await.is_err());
        server.shutdown().await;
    }

    #[tokio::test]
    async fn cursor_cannot_cross_connections_or_outlive_its_ttl() {
        let store = test_store("cursor");
        let snapshot = store.get_library().expect("read fixture library");
        let vault = snapshot.vaults.first().expect("seed vault");
        let server = LocalMcpServer::start_with_cursor_ttl(
            None,
            store.clone(),
            None,
            None,
            Duration::from_millis(1),
        )
        .await
        .expect("start MCP");
        let grant = server
            .issue_grant(
                &store,
                &vault.project_id,
                &vault.id,
                "test-client",
                None,
                [VAULT_LIST_PAPERS],
            )
            .await
            .expect("issue grant");
        let client = client_for(&grant).await;
        let first = client
            .call_tool(
                CallToolRequestParams::new(VAULT_LIST_PAPERS).with_arguments(
                    serde_json::json!({"vault_id": vault.id, "limit": 1})
                        .as_object()
                        .unwrap()
                        .clone(),
                ),
            )
            .await
            .expect("first page");
        if let Some(cursor) = first.structured_content.unwrap()["nextCursor"].as_str() {
            tokio::time::sleep(Duration::from_millis(3)).await;
            let expired = client
                .call_tool(
                    CallToolRequestParams::new(VAULT_LIST_PAPERS).with_arguments(
                        serde_json::json!({"vault_id": vault.id, "cursor": cursor})
                            .as_object()
                            .unwrap()
                            .clone(),
                    ),
                )
                .await;
            assert!(expired.is_err());
        }
        client.cancel().await.expect("stop client");
        server.shutdown().await;
    }

    #[tokio::test]
    async fn real_client_reads_exact_extracted_text_and_resolves_its_reference() {
        let store = test_store("reader-pdf");
        let snapshot = store.get_library().expect("read fixture library");
        let vault = snapshot.vaults.first().expect("seed vault");
        let expected = "Evidence on the first fixture page.";
        add_extracted_pdf(&store, &vault.id, "paper:reader-fixture", expected);
        let server = LocalMcpServer::start_with_cursor_ttl(
            None,
            store.clone(),
            None,
            None,
            DEFAULT_CURSOR_TTL,
        )
        .await
        .expect("start MCP");
        let grant = server
            .issue_grant(
                &store,
                &vault.project_id,
                &vault.id,
                "test-client",
                Some("test-run"),
                [VAULT_GET_PAPER, READER_READ],
            )
            .await
            .expect("issue grant");
        let client = client_for(&grant).await;

        let response = client
            .call_tool(
                CallToolRequestParams::new(READER_READ).with_arguments(
                    serde_json::json!({"paper_id": "paper:reader-fixture", "page_start": 1})
                        .as_object()
                        .unwrap()
                        .clone(),
                ),
            )
            .await
            .expect("read fixture");
        let content = response.structured_content.expect("structured result");
        assert_eq!(content["coverage"], "full_text");
        assert_eq!(content["passages"][0]["text"], expected);
        let passage_ref = content["passages"][0]["passageRef"]
            .as_str()
            .expect("passage ref");
        let anchor = server
            .resolve_passage(passage_ref)
            .await
            .expect("resolve passage");
        assert_eq!(anchor.quote, expected);
        assert_eq!(anchor.page_start, Some(1));

        client.cancel().await.expect("stop client");
        server.shutdown().await;
    }

    #[tokio::test]
    async fn real_client_adds_and_lists_one_idempotent_anchored_agent_note() {
        let store = test_store("reader-note");
        let snapshot = store.get_library().expect("read fixture library");
        let vault = snapshot.vaults.first().expect("seed vault");
        let paper_id = "paper:agent-note";
        let quote = "The repeated observation belongs to page one.";
        add_extracted_pdf(&store, &vault.id, paper_id, quote);
        let server = LocalMcpServer::start_with_cursor_ttl(
            None,
            store.clone(),
            None,
            None,
            DEFAULT_CURSOR_TTL,
        )
        .await
        .expect("start MCP");
        let grant = server
            .issue_grant(
                &store,
                &vault.project_id,
                &vault.id,
                "codex-test",
                Some("run-note-1"),
                [READER_READ, READER_ADD_NOTE, READER_LIST_NOTES],
            )
            .await
            .expect("issue grant");
        let client = client_for(&grant).await;
        let read = client
            .call_tool(
                CallToolRequestParams::new(READER_READ).with_arguments(
                    serde_json::json!({"paper_id": paper_id, "page_start": 1})
                        .as_object()
                        .unwrap()
                        .clone(),
                ),
            )
            .await
            .expect("read paper")
            .structured_content
            .expect("read result");
        let passage_ref = read["passages"][0]["passageRef"]
            .as_str()
            .expect("passage ref");
        let arguments = serde_json::json!({
            "paper_id": paper_id,
            "passage_ref": passage_ref,
            "body": "Keep this observation.",
            "request_id": "request-note-1"
        })
        .as_object()
        .unwrap()
        .clone();
        let first = client
            .call_tool(
                CallToolRequestParams::new(READER_ADD_NOTE).with_arguments(arguments.clone()),
            )
            .await
            .expect("add note")
            .structured_content
            .expect("add result");
        let retry = client
            .call_tool(CallToolRequestParams::new(READER_ADD_NOTE).with_arguments(arguments))
            .await
            .expect("retry note")
            .structured_content
            .expect("retry result");
        assert_eq!(first["entryId"], retry["entryId"]);
        assert_eq!(first["anchor"]["kind"], "sourcePassage");
        assert_eq!(first["anchor"]["pageIndex"], 0);

        let listed = client
            .call_tool(
                CallToolRequestParams::new(READER_LIST_NOTES).with_arguments(
                    serde_json::json!({"paper_id": paper_id, "limit": 25})
                        .as_object()
                        .unwrap()
                        .clone(),
                ),
            )
            .await
            .expect("list notes")
            .structured_content
            .expect("list result");
        assert_eq!(listed["notes"].as_array().unwrap().len(), 1);
        assert_eq!(listed["notes"][0]["quote"], quote);
        assert_eq!(listed["notes"][0]["author"]["id"], "codex-test");
        assert_eq!(listed["notes"][0]["author"]["runId"], "run-note-1");

        client.cancel().await.expect("stop client");
        server.shutdown().await;
    }

    #[test]
    fn agent_note_retry_is_atomic_and_preserves_existing_user_notes() {
        let store = test_store("reader-note-retry");
        let snapshot = store.get_library().expect("read fixture library");
        let paper = snapshot.papers.first().expect("seed paper");
        store
            .add_note_at_anchor("paper", &paper.id, &ThreadAnchor::Document, "User note")
            .expect("add user note");

        let first = store
            .add_agent_note_idempotent(
                "paper",
                &paper.id,
                &ThreadAnchor::Document,
                "Agent note",
                "codex-test",
                Some("run-1"),
                "request-1",
                "hash-1",
            )
            .expect("add agent note");
        let retry = store
            .add_agent_note_idempotent(
                "paper",
                &paper.id,
                &ThreadAnchor::Document,
                "Agent note",
                "codex-test",
                Some("run-1"),
                "request-1",
                "hash-1",
            )
            .expect("retry agent note");
        assert_eq!(first.entry_id, retry.entry_id);
        assert!(store
            .add_agent_note_idempotent(
                "paper",
                &paper.id,
                &ThreadAnchor::Document,
                "Different note",
                "codex-test",
                Some("run-1"),
                "request-1",
                "different-hash",
            )
            .unwrap_err()
            .contains("different payload"));

        let view = store
            .get_chat_thread(&first.thread_id)
            .expect("read document thread");
        assert_eq!(view.entries.len(), 2);
        assert_eq!(view.entries[0].author_kind, "user");
        assert_eq!(view.entries[1].author_kind, "agent");
        assert_eq!(view.entries[1].run_id.as_deref(), Some("run-1"));
    }

    #[tokio::test]
    async fn state_read_projects_kinds_and_keeps_one_revision_across_pages() {
        let store = test_store("state-read");
        let snapshot = store.get_library().expect("read fixture library");
        let vault = snapshot.vaults.first().expect("seed vault");
        store
            .create_research_entry(
                &vault.project_id,
                0,
                &state_draft(ResearchEntryKind::Finding, "A finding"),
            )
            .expect("create finding");
        store
            .create_research_entry(
                &vault.project_id,
                1,
                &state_draft(ResearchEntryKind::Hypothesis, "A hypothesis"),
            )
            .expect("create hypothesis");
        store
            .create_research_entry(
                &vault.project_id,
                2,
                &state_draft(ResearchEntryKind::Gap, "Missing comparison"),
            )
            .expect("create gap");
        store
            .create_research_entry(
                &vault.project_id,
                3,
                &state_draft(ResearchEntryKind::Question, "An open question"),
            )
            .expect("create question");
        store
            .create_research_entry(
                &vault.project_id,
                4,
                &state_draft(ResearchEntryKind::ExperimentIdea, "Try an ablation"),
            )
            .expect("create experiment idea");
        let server = LocalMcpServer::start_with_cursor_ttl(
            None,
            store.clone(),
            None,
            None,
            DEFAULT_CURSOR_TTL,
        )
        .await
        .expect("start MCP");
        let grant = server
            .issue_grant(
                &store,
                &vault.project_id,
                &vault.id,
                "codex-test",
                Some("state-run"),
                [STATE_READ],
            )
            .await
            .expect("issue grant");
        let client = client_for(&grant).await;
        let first = client
            .call_tool(
                CallToolRequestParams::new(STATE_READ).with_arguments(
                    serde_json::json!({"project_id": vault.project_id, "limit": 1})
                        .as_object()
                        .unwrap()
                        .clone(),
                ),
            )
            .await
            .expect("first state page")
            .structured_content
            .expect("state result");
        assert_eq!(first["revision"], 5);
        let cursor = first["nextCursor"].as_str().expect("next cursor");

        store
            .create_research_entry(
                &vault.project_id,
                5,
                &state_draft(ResearchEntryKind::Question, "Added concurrently"),
            )
            .expect("concurrent update");
        let second = client
            .call_tool(
                CallToolRequestParams::new(STATE_READ).with_arguments(
                    serde_json::json!({
                        "project_id": vault.project_id,
                        "cursor": cursor,
                        "limit": 100
                    })
                    .as_object()
                    .unwrap()
                    .clone(),
                ),
            )
            .await
            .expect("continued state page")
            .structured_content
            .expect("continued result");
        assert_eq!(second["revision"], 5);
        assert_eq!(second["currentRevision"], 6);
        assert_eq!(second["entries"].as_array().unwrap().len(), 4);
        let projected = first["entries"]
            .as_array()
            .unwrap()
            .iter()
            .chain(second["entries"].as_array().unwrap().iter())
            .map(|entry| {
                (
                    entry["originalKind"].as_str().unwrap(),
                    entry["kind"].as_str().unwrap(),
                )
            })
            .collect::<HashMap<_, _>>();
        assert_eq!(projected["finding"], "finding");
        assert_eq!(projected["hypothesis"], "hypothesis");
        assert_eq!(projected["question"], "question");
        assert_eq!(projected["gap"], "question");
        assert_eq!(projected["experiment_idea"], "question");

        client.cancel().await.expect("stop client");
        server.shutdown().await;
    }

    #[tokio::test]
    async fn state_update_commits_typed_evidence_once_and_reads_it_back() {
        let store = test_store("state-update");
        let snapshot = store.get_library().expect("read fixture library");
        let vault = snapshot.vaults.first().expect("seed vault");
        add_extracted_pdf(
            &store,
            &vault.id,
            "paper:support",
            "Measured accuracy improved after the intervention.",
        );
        add_extracted_pdf(
            &store,
            &vault.id,
            "paper:conflict",
            "A second benchmark showed no measurable improvement.",
        );
        store
            .add_paper_to_vaults(
                &paper_draft(
                    "paper:abstract-evidence",
                    Some("The provider abstract describes a related intervention."),
                ),
                std::slice::from_ref(&vault.id),
            )
            .expect("add abstract evidence paper");
        let server = LocalMcpServer::start_with_cursor_ttl(
            None,
            store.clone(),
            None,
            None,
            DEFAULT_CURSOR_TTL,
        )
        .await
        .expect("start MCP");
        let grant = server
            .issue_grant(
                &store,
                &vault.project_id,
                &vault.id,
                "codex-test",
                None,
                [READER_READ, STATE_READ, STATE_UPDATE],
            )
            .await
            .expect("issue grant");
        let client = client_for(&grant).await;
        let mut passage_refs = Vec::new();
        for paper_id in ["paper:support", "paper:conflict", "paper:abstract-evidence"] {
            let read = client
                .call_tool(
                    CallToolRequestParams::new(READER_READ).with_arguments(
                        serde_json::json!({"paper_id": paper_id})
                            .as_object()
                            .unwrap()
                            .clone(),
                    ),
                )
                .await
                .expect("read evidence")
                .structured_content
                .expect("read result");
            passage_refs.push(
                read["passages"][0]["passageRef"]
                    .as_str()
                    .unwrap()
                    .to_string(),
            );
        }
        let arguments = serde_json::json!({
            "project_id": vault.project_id,
            "base_revision": 0,
            "request_id": "state-request-1",
            "changes": [{
                "operation": "create",
                "operation_key": "finding-1",
                "kind": "finding",
                "statement": "The intervention has mixed benchmark evidence.",
                "epistemic_status": "source_supported",
                "evidence": [
                    {"source": "passage", "passage_ref": passage_refs[0], "relationship": "supports", "explanation": "Positive benchmark"},
                    {"source": "passage", "passage_ref": passage_refs[1], "relationship": "contradicts", "explanation": "Null benchmark"},
                    {"source": "passage", "passage_ref": passage_refs[2], "relationship": "context", "explanation": "Abstract-only context"}
                ],
                "relationships": [],
                "reason": "Record mixed evidence"
            }]
        })
        .as_object()
        .unwrap()
        .clone();
        let first = client
            .call_tool(CallToolRequestParams::new(STATE_UPDATE).with_arguments(arguments.clone()))
            .await
            .expect("update state")
            .structured_content
            .expect("update result");
        client.cancel().await.expect("stop first client");
        server.shutdown().await;

        let retry_server = LocalMcpServer::start_with_cursor_ttl(
            None,
            store.clone(),
            None,
            None,
            DEFAULT_CURSOR_TTL,
        )
        .await
        .expect("restart MCP");
        let retry_grant = retry_server
            .issue_grant(
                &store,
                &vault.project_id,
                &vault.id,
                "codex-test",
                None,
                [STATE_READ, STATE_UPDATE],
            )
            .await
            .expect("issue retry grant");
        let retry_client = client_for(&retry_grant).await;
        let retry = retry_client
            .call_tool(CallToolRequestParams::new(STATE_UPDATE).with_arguments(arguments.clone()))
            .await
            .expect("retry state")
            .structured_content
            .expect("retry result");
        assert_eq!(first, retry);
        assert_eq!(first["revision"], 1);

        let mut changed_arguments = arguments;
        changed_arguments["changes"][0]["statement"] =
            serde_json::json!("A changed retry must fail.");
        let changed_retry = retry_client
            .call_tool(CallToolRequestParams::new(STATE_UPDATE).with_arguments(changed_arguments))
            .await
            .expect_err("changed retry payload must conflict");
        assert!(format!("{changed_retry:?}").contains("request_conflict"));

        let state = retry_client
            .call_tool(
                CallToolRequestParams::new(STATE_READ).with_arguments(
                    serde_json::json!({"project_id": vault.project_id})
                        .as_object()
                        .unwrap()
                        .clone(),
                ),
            )
            .await
            .expect("read updated state")
            .structured_content
            .expect("state result");
        assert_eq!(state["entries"].as_array().unwrap().len(), 1);
        let relationships = state["entries"][0]["evidence"]
            .as_array()
            .unwrap()
            .iter()
            .map(|evidence| evidence["relationship"].as_str().unwrap())
            .collect::<BTreeSet<_>>();
        assert_eq!(
            relationships,
            BTreeSet::from(["context", "contradicts", "supports"])
        );
        assert!(state["entries"][0]["originRunId"].is_null());

        retry_client.cancel().await.expect("stop retry client");
        retry_server.shutdown().await;
    }

    #[tokio::test]
    async fn reader_reports_abstract_pending_and_failed_sources_honestly() {
        let store = test_store("reader-states");
        let snapshot = store.get_library().expect("read fixture library");
        let vault = snapshot.vaults.first().expect("seed vault");
        let abstract_paper = paper_draft("paper:abstract", Some("Only abstract evidence."));
        store
            .add_paper_to_vaults(&abstract_paper, std::slice::from_ref(&vault.id))
            .expect("add abstract paper");
        let mut pending_paper = paper_draft("paper:pending", None);
        pending_paper.sources.push(PaperSourceDraft {
            source_kind: "pdf".to_string(),
            source_url: "https://example.test/pending.pdf".to_string(),
            landing_url: None,
        });
        store
            .add_paper_to_vaults(&pending_paper, std::slice::from_ref(&vault.id))
            .expect("add pending paper");
        let pending_source = store
            .get_document_sources(&pending_paper.id)
            .expect("pending source")
            .remove(0);
        let failed_paper = paper_draft("paper:failed", None);
        store
            .add_local_pdf_to_vault(
                &failed_paper,
                &vault.id,
                "pdf:paper:failed:fixture",
                "https://example.test/failed.pdf",
                "/tmp/failed.pdf",
            )
            .expect("add failed paper");
        store
            .set_document_source_failed("pdf:paper:failed:fixture", "publisher denied access")
            .expect("fail source");

        let handler = I0iMcpHandler::new(None, store, None, None, DEFAULT_CURSOR_TTL);
        let saved_abstract = handler
            .store
            .get_paper(&abstract_paper.id)
            .expect("read abstract paper")
            .expect("abstract paper exists");
        let abstract_result = handler
            .abstract_or_unavailable("test-grant", &saved_abstract)
            .await
            .expect("abstract result");
        assert_eq!(abstract_result.coverage, "abstract_only");
        assert_eq!(
            unavailable_or_pending(&pending_source).availability,
            "pending"
        );
        let failed_source = handler
            .store
            .get_document_source("pdf:paper:failed:fixture")
            .expect("failed source");
        let failed_result = unavailable_or_pending(&failed_source);
        assert_eq!(failed_result.availability, "unavailable");
        assert_eq!(
            failed_result.reason.as_deref(),
            Some("publisher denied access")
        );
    }

    #[tokio::test]
    async fn reader_paginates_exact_html_text_and_rejects_pdf_page_ranges() {
        let store = test_store("reader-html");
        let snapshot = store.get_library().expect("read fixture library");
        let vault = snapshot.vaults.first().expect("seed vault");
        let paper = paper_draft("paper:html", None);
        let directory = std::env::temp_dir().join(format!("i0i-html-{}", Uuid::new_v4().simple()));
        fs::create_dir_all(&directory).expect("create HTML fixture directory");
        let html_path = directory.join("source.html");
        fs::write(&html_path, "<p>fixture</p>").expect("write HTML fixture");
        let expected = "évidence ".repeat(2_000);
        fs::write(
            directory.join("meta.json"),
            serde_json::json!({"source_text": expected}).to_string(),
        )
        .expect("write HTML metadata");
        store
            .add_local_html_to_vault(
                &paper,
                &vault.id,
                "html:paper:html:fixture",
                "https://example.test/article",
                html_path.to_str().expect("UTF-8 fixture path"),
            )
            .expect("add HTML fixture");
        let server = LocalMcpServer::start_with_cursor_ttl(
            None,
            store.clone(),
            None,
            None,
            DEFAULT_CURSOR_TTL,
        )
        .await
        .expect("start MCP");
        let grant = server
            .issue_grant(
                &store,
                &vault.project_id,
                &vault.id,
                "test-client",
                None,
                [READER_READ],
            )
            .await
            .expect("issue grant");
        let client = client_for(&grant).await;

        let first = client
            .call_tool(
                CallToolRequestParams::new(READER_READ).with_arguments(
                    serde_json::json!({"paper_id": paper.id})
                        .as_object()
                        .unwrap()
                        .clone(),
                ),
            )
            .await
            .expect("read HTML");
        let first_content = first.structured_content.expect("structured HTML result");
        assert_eq!(first_content["coverage"], "full_text");
        assert_eq!(first_content["hasMore"], true);
        assert!(first_content["nextCursor"].is_string());

        let invalid_range = client
            .call_tool(
                CallToolRequestParams::new(READER_READ).with_arguments(
                    serde_json::json!({"paper_id": paper.id, "page_start": 1})
                        .as_object()
                        .unwrap()
                        .clone(),
                ),
            )
            .await;
        assert!(invalid_range.is_err());

        client.cancel().await.expect("stop client");
        server.shutdown().await;
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn reader_text_splitting_uses_character_offsets() {
        let parts = split_text("aé日z", 2);
        assert_eq!(parts[0], (0, 2, "aé".to_string()));
        assert_eq!(parts[1], (2, 4, "日z".to_string()));
        assert!(validate_page_range(Some(0), None).is_err());
        assert!(validate_page_range(Some(3), Some(2)).is_err());
    }
}
