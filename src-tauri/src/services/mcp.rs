//! Authenticated local MCP access to i0i capabilities.
//!
//! The HTTP transport is deliberately thin: the official `rmcp` SDK owns MCP
//! framing and sessions, while this module owns app-session grants and maps
//! tool calls onto the same `LibraryStore` used by Tauri commands.

use std::collections::{BTreeSet, HashMap};
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
use tokio::sync::{Mutex, RwLock};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::domain::library::{LibrarySnapshot, Paper, Vault};
use crate::storage::library_store::LibraryStore;

const DEFAULT_PAGE_SIZE: usize = 25;
const MAX_PAGE_SIZE: usize = 100;
const DEFAULT_CURSOR_TTL: Duration = Duration::from_secs(10 * 60);

pub const VAULT_LIST: &str = "vault_list";
pub const VAULT_LIST_PAPERS: &str = "vault_list_papers";

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

#[derive(Clone)]
struct I0iMcpHandler {
    store: LibraryStore,
    cursors: Arc<Mutex<HashMap<String, CursorSnapshot>>>,
    cursor_ttl: Duration,
}

impl I0iMcpHandler {
    fn new(store: LibraryStore, cursor_ttl: Duration) -> Self {
        Self {
            store,
            cursors: Arc::new(Mutex::new(HashMap::new())),
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
    cancellation: CancellationToken,
    server_task: Arc<Mutex<Option<JoinHandle<()>>>>,
}

impl LocalMcpServer {
    /// Bind an authenticated Streamable HTTP MCP endpoint on an ephemeral port.
    pub async fn start(store: LibraryStore) -> Result<Self, String> {
        Self::start_with_cursor_ttl(store, DEFAULT_CURSOR_TTL).await
    }

    async fn start_with_cursor_ttl(
        store: LibraryStore,
        cursor_ttl: Duration,
    ) -> Result<Self, String> {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .map_err(|error| error.to_string())?;
        let address = listener.local_addr().map_err(|error| error.to_string())?;
        let endpoint = format!("http://{address}/mcp");
        let grants = GrantRegistry::new();
        let cancellation = CancellationToken::new();
        let handler = I0iMcpHandler::new(store, cursor_ttl);
        let service: StreamableHttpService<I0iMcpHandler, LocalSessionManager> =
            StreamableHttpService::new(
                move || Ok(handler.clone()),
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

    #[tokio::test]
    async fn real_client_lists_only_the_granted_vault_and_its_papers() {
        let store = test_store("listing");
        let snapshot = store.get_library().expect("read fixture library");
        let vault = snapshot.vaults.first().expect("seed vault");
        let server = LocalMcpServer::start(store.clone())
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
        let server = LocalMcpServer::start(store.clone())
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
        let server = LocalMcpServer::start_with_cursor_ttl(store.clone(), Duration::from_millis(1))
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
}
