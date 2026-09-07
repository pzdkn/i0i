//! Managed Codex orchestration for one project research Run.
//!
//! The controller owns lifecycle and limits. Codex chooses research actions,
//! but can only act through the project-scoped MCP grant issued here.

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex as StdMutex;
use std::time::Duration;

use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::{broadcast, Mutex};
use tokio_util::sync::CancellationToken;

use crate::domain::discovery::DiscoveryProviderChoice;
use crate::domain::harness::{
    AgentRunLimits, EffectiveInstructionStack, HarnessConfiguration, HarnessRun, HarnessRunTrigger,
    PriorResearchRunOutcome, ResearchRunOutcome,
};
use crate::domain::research::{SearchConstraints, SearchDraft};
use crate::services::codex_runtime::{CodexEvent, CodexRuntime, CodexRuntimeConfig, CodexTurn};
use crate::services::mcp::{
    LocalMcpServer, READER_ADD_NOTE, READER_ASK, READER_LIST_NOTES, READER_READ, SEARCH_CANCEL,
    SEARCH_GET, SEARCH_START, STATE_READ, STATE_UPDATE, VAULT_ADD_PAPER, VAULT_ASK,
    VAULT_GET_PAPER, VAULT_LIST, VAULT_LIST_PAPERS, VAULT_SUMMARY,
};
use crate::services::research::manager::SearchManager;
use crate::storage::library_store::LibraryStore;

const CHILD_SETTLE_SECONDS: u64 = 30;
const DEFAULT_RECENT_OUTCOMES: usize = 3;
const DEFAULT_OUTCOME_HISTORY_CHARS: usize = 12_000;

#[derive(Clone)]
struct ActiveAgentRun {
    run_id: String,
    cancellation: CancellationToken,
}

#[derive(Debug, Clone, Copy)]
struct ContinuationConfig {
    maximum_recent_outcomes: usize,
    maximum_history_chars: usize,
}

impl ContinuationConfig {
    fn load() -> Self {
        Self {
            maximum_recent_outcomes: bounded_preference(
                "research.continuation_outcomes",
                DEFAULT_RECENT_OUTCOMES,
                10,
            ),
            maximum_history_chars: bounded_preference(
                "research.continuation_history_chars",
                DEFAULT_OUTCOME_HISTORY_CHARS,
                50_000,
            ),
        }
    }
}

struct TurnCompletion {
    status: String,
    final_message: Option<String>,
}

/// Starts and supervises managed Codex research without duplicating tool logic.
#[derive(Clone)]
pub struct ProjectResearchController {
    app: Option<AppHandle>,
    runtime_dir: PathBuf,
    store: LibraryStore,
    search_manager: SearchManager,
    mcp_server: LocalMcpServer,
    runtime_config_override: Option<CodexRuntimeConfig>,
    runtime: Arc<Mutex<Option<CodexRuntime>>>,
    active_runs: Arc<StdMutex<HashMap<String, ActiveAgentRun>>>,
    tool_trace: Option<Arc<StdMutex<Vec<Value>>>>,
}

impl ProjectResearchController {
    /// Construct the app-wide controller. Codex itself starts lazily on first Run.
    pub fn new(
        app: AppHandle,
        store: LibraryStore,
        search_manager: SearchManager,
        mcp_server: LocalMcpServer,
    ) -> Self {
        let runtime_dir = app
            .path()
            .app_data_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join("research-runtime");
        Self {
            app: Some(app),
            runtime_dir,
            store,
            search_manager,
            mcp_server,
            runtime_config_override: None,
            runtime: Arc::new(Mutex::new(None)),
            active_runs: Arc::new(StdMutex::new(HashMap::new())),
            tool_trace: None,
        }
    }

    /// Construct the production controller without native-window events.
    #[cfg(test)]
    pub(crate) fn for_evaluation(
        runtime_dir: PathBuf,
        store: LibraryStore,
        search_manager: SearchManager,
        mcp_server: LocalMcpServer,
        runtime_config: CodexRuntimeConfig,
    ) -> Self {
        Self {
            app: None,
            runtime_dir,
            store,
            search_manager,
            mcp_server,
            runtime_config_override: Some(runtime_config),
            runtime: Arc::new(Mutex::new(None)),
            active_runs: Arc::new(StdMutex::new(HashMap::new())),
            tool_trace: Some(Arc::new(StdMutex::new(Vec::new()))),
        }
    }

    /// Return the tool lifecycle observed from managed Codex during evaluation.
    #[cfg(test)]
    pub(crate) fn evaluation_tool_trace(&self) -> Vec<Value> {
        self.tool_trace
            .as_ref()
            .expect("evaluation controller trace")
            .lock()
            .expect("evaluation tool trace lock")
            .clone()
    }

    /// Persist a complete Run snapshot, dispatch Codex, and return immediately.
    pub fn start(
        &self,
        project_id: &str,
        trigger: HarnessRunTrigger,
        scheduled_for: Option<&str>,
    ) -> Result<HarnessRun, String> {
        if trigger != HarnessRunTrigger::Manual {
            self.store.ensure_harness_can_start(project_id)?;
        }
        let snapshot = self.store.get_harness_snapshot(project_id)?;
        let configuration = snapshot.harness.configuration;
        let instructions = configuration.canonical_instructions();
        if instructions.trim().is_empty() {
            return Err("Research instructions cannot be empty".to_string());
        }

        let runtime_config = self
            .runtime_config_override
            .clone()
            .unwrap_or_else(CodexRuntimeConfig::load);
        let limits = effective_agent_limits(&configuration);
        let search = self.store.create_search(&SearchDraft {
            title: instructions
                .lines()
                .next()
                .unwrap_or("Project research")
                .chars()
                .take(160)
                .collect(),
            goal: instructions,
            constraints: SearchConstraints {
                year_from: None,
                year_to: None,
                providers: configured_providers(&configuration.sources),
                open_access: false,
                target_count: configuration.paper_budget,
                venues: Vec::new(),
                authors: Vec::new(),
                fields_of_study: Vec::new(),
                seed_paper_ids: Vec::new(),
            },
            strategy: configuration.depth.budget(),
            schedule: None,
        })?;
        let run = self.store.create_harness_run_with_trigger(
            project_id,
            &search.id,
            trigger,
            scheduled_for,
        )?;
        let run =
            match self
                .store
                .configure_codex_harness_run(&run.id, &runtime_config.model, &limits)
            {
                Ok(run) => run,
                Err(error) => {
                    self.store.fail_harness_run(&run.id, &error)?;
                    return Err(error);
                }
            };

        let cancellation = CancellationToken::new();
        let active = ActiveAgentRun {
            run_id: run.id.clone(),
            cancellation: cancellation.clone(),
        };
        let controller = self.clone();
        let project_key = project_id.to_string();
        let run_id = run.id.clone();
        self.active_runs
            .lock()
            .expect("active Research Run lock")
            .insert(project_key.clone(), active);
        self.emit_update(project_id, &run.id);
        tauri::async_runtime::spawn(async move {
            controller
                .execute(run_id, runtime_config, cancellation)
                .await;
            controller
                .active_runs
                .lock()
                .expect("active Research Run lock")
                .remove(&project_key);
        });
        Ok(run)
    }

    /// Close one managed Run to new work and interrupt its active workers.
    pub async fn cancel(&self, project_id: &str) -> Result<HarnessRun, String> {
        self.cancel_with_reason(project_id, "cancelled_by_user")
            .await
    }

    /// Stop managed research before the native app exits.
    pub async fn shutdown(&self) {
        let projects: Vec<String> = self
            .active_runs
            .lock()
            .expect("active Research Run lock")
            .keys()
            .cloned()
            .collect();
        for project_id in projects {
            let _ = self
                .cancel_with_reason(&project_id, "application_shutdown")
                .await;
        }
        if let Some(runtime) = self.runtime.lock().await.take() {
            let _ = runtime.shutdown().await;
        }
        let _ = self.store.recover_interrupted_harness_runs();
    }

    async fn execute(
        &self,
        run_id: String,
        runtime_config: CodexRuntimeConfig,
        cancellation: CancellationToken,
    ) {
        let result = self
            .execute_inner(&run_id, runtime_config, cancellation)
            .await;
        if let Err(error) = result {
            eprintln!("[research-controller] run_id={run_id} failed: {error}");
            let bounded_error: String = error.chars().take(500).collect();
            let finalized = match self.store.get_harness_run(&run_id) {
                Ok(run) => {
                    let cancellation_reason = run.stop_reason.clone();
                    if run.status != "canceling" {
                        let _ = self.store.record_harness_activity(
                            &run_id,
                            "agent_failed",
                            &bounded_error,
                            Some("complete"),
                        );
                    }
                    self.settle_child_searches(&run.project_id, &run_id)
                        .await
                        .and_then(|()| {
                            if run.status == "canceling" {
                                self.store.finish_codex_harness_run(
                                    &run_id,
                                    "cancelled",
                                    cancellation_reason
                                        .as_deref()
                                        .unwrap_or("cancelled_by_user"),
                                )
                            } else {
                                self.store.finish_codex_harness_run(
                                    &run_id,
                                    "failed",
                                    runtime_failure_reason(&error),
                                )
                            }
                        })
                }
                Err(store_error) => Err(store_error),
            };
            if finalized.is_err() {
                let _ = self.store.fail_harness_run(&run_id, &bounded_error);
            }
        }
        self.mcp_server.revoke_run(&run_id).await;
        let project_id = self
            .store
            .get_harness_run(&run_id)
            .ok()
            .map(|run| run.project_id);
        if let Some(project_id) = project_id.as_deref() {
            self.emit_update(project_id, &run_id);
        }
    }

    async fn execute_inner(
        &self,
        run_id: &str,
        runtime_config: CodexRuntimeConfig,
        cancellation: CancellationToken,
    ) -> Result<(), String> {
        let run = self.store.get_harness_run(run_id)?;
        if run.status == "canceling" {
            self.settle_child_searches(&run.project_id, run_id).await?;
            self.store.finish_codex_harness_run(
                run_id,
                "cancelled",
                run.stop_reason.as_deref().unwrap_or("cancelled_by_user"),
            )?;
            return Ok(());
        }
        let limits = run
            .agent_limits
            .clone()
            .ok_or("Managed Research Run has no limit snapshot")?;
        let vault_id = run.effective_instructions.run_context.vault_id.clone();
        let caller = format!("codex-agent:{run_id}");
        let grant = self
            .mcp_server
            .issue_grant(
                &self.store,
                &run.project_id,
                &vault_id,
                &caller,
                Some(run_id),
                [
                    VAULT_LIST,
                    VAULT_LIST_PAPERS,
                    VAULT_GET_PAPER,
                    VAULT_ADD_PAPER,
                    VAULT_ASK,
                    VAULT_SUMMARY,
                    READER_READ,
                    READER_ASK,
                    READER_ADD_NOTE,
                    READER_LIST_NOTES,
                    STATE_READ,
                    STATE_UPDATE,
                    SEARCH_START,
                    SEARCH_GET,
                    SEARCH_CANCEL,
                ],
            )
            .await?;
        let runtime = self.ensure_runtime(runtime_config).await?;
        let mut events = runtime.subscribe();
        let continuation = ContinuationConfig::load();
        let prior_outcomes = self.store.list_recent_agent_run_outcomes(
            &run.project_id,
            continuation.maximum_recent_outcomes,
            continuation.maximum_history_chars,
        )?;
        let work_dir = self.runtime_dir.clone();
        fs::create_dir_all(&work_dir).map_err(|error| error.to_string())?;
        let thread_id = runtime
            .start_thread(
                &work_dir,
                RESEARCH_AGENT_INSTRUCTIONS,
                codex_thread_config(&grant.endpoint, &grant.bearer_token),
            )
            .await?;
        let prompt = render_research_prompt(
            &run.project_id,
            &run.effective_instructions,
            &limits,
            &prior_outcomes,
        )?;
        let turn = runtime
            .start_turn(&thread_id, &prompt, Some(research_outcome_schema()))
            .await?;
        self.store
            .attach_codex_turn(run_id, &turn.thread_id, &turn.turn_id)?;

        let completion = tokio::select! {
            result = wait_for_turn(
                &mut events,
                &turn,
                &self.store,
                self.app.as_ref(),
                &run.project_id,
                run_id,
                self.tool_trace.as_ref(),
            ) => result,
            _ = cancellation.cancelled() => {
                runtime.interrupt(&turn).await?;
                Ok(TurnCompletion { status: "cancelled".to_string(), final_message: None })
            }
            _ = tokio::time::sleep(Duration::from_secs(limits.maximum_run_seconds)) => {
                self.cancel_with_reason(&run.project_id, "time_limit").await?;
                runtime.interrupt(&turn).await?;
                Ok(TurnCompletion { status: "timed_out".to_string(), final_message: None })
            }
        }?;

        self.settle_child_searches(&run.project_id, run_id).await?;
        let (status, reason): (&str, String) = match completion.status.as_str() {
            "completed" => {
                self.store.begin_codex_harness_finalization(run_id)?;
                match parse_research_outcome(completion.final_message.as_deref())
                    .and_then(|outcome| self.store.persist_agent_run_outcome(run_id, &outcome))
                {
                    Ok(_) => {}
                    Err(error) => {
                        self.store.record_harness_activity(
                            run_id,
                            "summary_failed",
                            &format!("Research outcome was not retained: {error}"),
                            Some("complete"),
                        )?;
                    }
                }
                ("ready", "agent_completed".to_string())
            }
            "cancelled" => {
                let reason = self
                    .store
                    .get_harness_run(run_id)?
                    .stop_reason
                    .unwrap_or_else(|| "cancelled_by_user".to_string());
                ("cancelled", reason)
            }
            "timed_out" => ("cancelled", "time_limit".to_string()),
            "interrupted" | "runtime_exited" => ("failed", "agent_process_interrupted".to_string()),
            "failed" => ("failed", "agent_failed".to_string()),
            other => return Err(format!("Unknown Codex turn status: {other}")),
        };
        self.store
            .finish_codex_harness_run(run_id, status, &reason)?;
        Ok(())
    }

    async fn ensure_runtime(&self, config: CodexRuntimeConfig) -> Result<CodexRuntime, String> {
        let mut runtime = self.runtime.lock().await;
        if let Some(existing) = runtime.as_ref() {
            if existing.is_running().await? {
                return Ok(existing.clone());
            }
            *runtime = None;
        }
        let started = CodexRuntime::start(config).await?;
        *runtime = Some(started.clone());
        Ok(started)
    }

    async fn settle_child_searches(
        &self,
        project_id: &str,
        parent_run_id: &str,
    ) -> Result<(), String> {
        let children = self
            .store
            .list_agent_search_runs_for_parent(project_id, parent_run_id)?;
        for child in children.iter().filter(|run| !is_terminal(&run.status)) {
            self.store
                .request_agent_search_cancel(project_id, &child.id)?;
            self.search_manager.cancel_run(&child.id);
        }
        tokio::time::timeout(Duration::from_secs(CHILD_SETTLE_SECONDS), async {
            loop {
                let children = self
                    .store
                    .list_agent_search_runs_for_parent(project_id, parent_run_id)?;
                if children.iter().all(|run| is_terminal(&run.status)) {
                    return Ok::<(), String>(());
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        })
        .await
        .map_err(|_| "Child searches did not stop before finalization".to_string())?
    }

    async fn cancel_with_reason(
        &self,
        project_id: &str,
        reason: &str,
    ) -> Result<HarnessRun, String> {
        let (run, changed) = self
            .store
            .request_codex_harness_cancellation(project_id, reason)?;
        if !changed {
            return Ok(run);
        }
        self.mcp_server.revoke_run(&run.id).await;
        self.signal_child_searches(project_id, &run.id)?;
        if let Some(active) = self
            .active_runs
            .lock()
            .expect("active Research Run lock")
            .get(project_id)
            .filter(|active| active.run_id == run.id)
            .cloned()
        {
            active.cancellation.cancel();
        }
        self.emit_update(project_id, &run.id);
        Ok(run)
    }

    fn emit_update(&self, project_id: &str, run_id: &str) {
        if let Some(app) = &self.app {
            let _ = app.emit(
                "research_harness_updated",
                json!({"projectId": project_id, "runId": run_id}),
            );
        }
    }

    fn signal_child_searches(&self, project_id: &str, parent_run_id: &str) -> Result<(), String> {
        for child in self
            .store
            .list_agent_search_runs_for_parent(project_id, parent_run_id)?
            .iter()
            .filter(|run| !is_terminal(&run.status))
        {
            self.store
                .request_agent_search_cancel(project_id, &child.id)?;
            self.search_manager.cancel_run(&child.id);
        }
        Ok(())
    }
}

const RESEARCH_AGENT_INSTRUCTIONS: &str = r#"You are i0i's bounded literature research agent. Work only through the i0i MCP tools. Do not use shell, filesystem, built-in web search, or unrelated MCP servers. Inspect Research State and the Vault before choosing work. Use vault_summary only when a collection overview is useful; it is sampled context, not proof that every paper was read. State the purpose of each focused search. Save useful candidates, wait for acquisition when needed, and read relevant passages before citing them. You may delegate a bounded evidence question to reader_ask or vault_ask, but direct reading remains the primary path. Compare evidence with existing entries and actively look for conflicting results and conditions. Update Research State only with passage references actually returned by reader_read, reader_ask, vault_ask, or vault_summary. Distinguish source claims, model interpretation, speculation, abstract-only coverage, and unavailable full text. Continue only while another step can materially improve the project; otherwise finish with a concise summary and remaining questions."#;

fn effective_agent_limits(configuration: &HarnessConfiguration) -> AgentRunLimits {
    let mut limits = AgentRunLimits::default();
    if let Some(value) = configuration.stop_conditions.maximum_run_seconds {
        if value > 0 {
            limits.maximum_run_seconds = value as u64;
        }
    }
    if let Some(value) = configuration.stop_conditions.maximum_provider_queries {
        limits.maximum_provider_queries = value;
    }
    if let Some(value) = configuration.stop_conditions.maximum_llm_calls {
        limits.maximum_llm_calls = value;
    }
    limits
}

fn configured_providers(sources: &[String]) -> Vec<DiscoveryProviderChoice> {
    let mut providers = Vec::new();
    if sources.iter().any(|source| source == "open_alex") {
        providers.push(DiscoveryProviderChoice::OpenAlex);
    }
    if sources.iter().any(|source| source == "arxiv") {
        providers.push(DiscoveryProviderChoice::Arxiv);
    }
    providers
}

fn codex_thread_config(endpoint: &str, bearer_token: &str) -> Value {
    json!({
        "mcp_servers": {
            "ioi": {
                "url": endpoint,
                "http_headers": {"Authorization": format!("Bearer {bearer_token}")},
                "default_tools_approval_mode": "approve",
                "enabled": true
            }
        },
        "web_search": "disabled"
    })
}

fn render_research_prompt(
    project_id: &str,
    stack: &EffectiveInstructionStack,
    limits: &AgentRunLimits,
    prior_outcomes: &[PriorResearchRunOutcome],
) -> Result<String, String> {
    let snapshot = serde_json::to_string_pretty(&json!({
        "projectId": project_id,
        "projectInstructions": stack.project_research_instructions,
        "stateRevision": stack.run_context.starting_state_revision,
        "stateEntries": stack.run_context.active_entries,
        "vaultId": stack.run_context.vault_id,
        "vaultRevision": stack.run_context.vault_revision,
        "vaultPaperIds": stack.run_context.vault_paper_ids,
        "priorNextDirection": stack.run_context.prior_next_direction,
        "priorObservations": stack.run_context.prior_observations,
        "recentRunOutcomes": prior_outcomes,
        "paperTarget": stack.run_context.paper_budget,
        "limits": limits,
    }))
    .map_err(|error| error.to_string())?;
    Ok(format!(
        "Run this project's literature research procedure. Treat the snapshot as orientation, then verify current data with i0i tools before writing. Current evidence and the user's instructions outrank recommendations from prior Runs. Do not repeat an earlier search unless changed evidence, broader coverage, or an external failure justifies it. A search with no results does not prove that no such research exists. In the final structured response, report only IDs and passage references returned by this Run's tools.\n\n{snapshot}"
    ))
}

async fn wait_for_turn(
    events: &mut broadcast::Receiver<CodexEvent>,
    turn: &CodexTurn,
    store: &LibraryStore,
    app: Option<&AppHandle>,
    project_id: &str,
    run_id: &str,
    tool_trace: Option<&Arc<StdMutex<Vec<Value>>>>,
) -> Result<TurnCompletion, String> {
    let mut final_message = None;
    loop {
        match events.recv().await {
            Ok(event) => {
                if matches!(event.method.as_str(), "item/started" | "item/completed")
                    && event.params["threadId"].as_str() == Some(&turn.thread_id)
                    && event.params["turnId"].as_str() == Some(&turn.turn_id)
                    && event.params["item"]["type"].as_str() == Some("mcpToolCall")
                    && event.params["item"]["server"].as_str() == Some("ioi")
                {
                    if let Some(trace) = tool_trace {
                        trace
                            .lock()
                            .expect("evaluation tool trace lock")
                            .push(json!({
                                "event": event.method,
                                "item": event.params["item"],
                            }));
                    }
                }
                if event.method == "runtime/exited" {
                    return Ok(TurnCompletion {
                        status: "runtime_exited".to_string(),
                        final_message: None,
                    });
                }
                if event.method == "item/completed"
                    && event.params["threadId"].as_str() == Some(&turn.thread_id)
                    && event.params["turnId"].as_str() == Some(&turn.turn_id)
                    && event.params["item"]["type"].as_str() == Some("agentMessage")
                {
                    final_message = event.params["item"]["text"].as_str().map(str::to_string);
                }
                if let Some((kind, summary)) = observable_activity(&event, turn) {
                    store.record_harness_activity(run_id, kind, &summary, Some("agent"))?;
                    if let Some(app) = app {
                        let _ = app.emit(
                            "research_harness_updated",
                            json!({"projectId": project_id, "runId": run_id}),
                        );
                    }
                }
                if event.method == "turn/completed"
                    && event.params["threadId"].as_str() == Some(&turn.thread_id)
                    && event.params["turn"]["id"].as_str() == Some(&turn.turn_id)
                {
                    let status = event.params["turn"]["status"]
                        .as_str()
                        .map(str::to_string)
                        .ok_or_else(|| "Codex completion omitted turn status".to_string())?;
                    return Ok(TurnCompletion {
                        status,
                        final_message,
                    });
                }
            }
            Err(broadcast::error::RecvError::Lagged(_)) => continue,
            Err(broadcast::error::RecvError::Closed) => {
                return Err("Codex event stream closed before turn completion".to_string());
            }
        }
    }
}

fn research_outcome_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["summary", "taskOutcomes", "unansweredQuestions", "nextDirection"],
        "properties": {
            "summary": {"type": "string", "minLength": 1, "maxLength": 2000},
            "taskOutcomes": {
                "type": "array",
                "maxItems": 20,
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["searchRunIds", "motivatingEntryIds", "learnedPoints", "citedPassageRefs"],
                    "properties": {
                        "searchRunIds": {"type": "array", "maxItems": 12, "items": {"type": "string"}},
                        "motivatingEntryIds": {"type": "array", "maxItems": 20, "items": {"type": "string"}},
                        "learnedPoints": {"type": "array", "minItems": 1, "maxItems": 20, "items": {"type": "string"}},
                        "citedPassageRefs": {"type": "array", "maxItems": 40, "items": {"type": "string"}}
                    }
                }
            },
            "unansweredQuestions": {"type": "array", "maxItems": 20, "items": {"type": "string"}},
            "nextDirection": {"type": ["string", "null"]}
        }
    })
}

fn parse_research_outcome(message: Option<&str>) -> Result<ResearchRunOutcome, String> {
    let message = message.ok_or("Codex completed without a final Research outcome")?;
    serde_json::from_str(message).map_err(|error| format!("Invalid Research outcome JSON: {error}"))
}

fn bounded_preference(key: &str, default: usize, maximum: usize) -> usize {
    crate::services::settings::preference(key)
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(default)
        .min(maximum)
}

fn observable_activity<'a>(
    event: &'a CodexEvent,
    turn: &CodexTurn,
) -> Option<(&'static str, String)> {
    if !matches!(event.method.as_str(), "item/started" | "item/completed")
        || event.params["threadId"].as_str() != Some(&turn.thread_id)
        || event.params["turnId"].as_str() != Some(&turn.turn_id)
        || event.params["item"]["type"].as_str() != Some("mcpToolCall")
        || event.params["item"]["server"].as_str() != Some("ioi")
    {
        return None;
    }
    let tool = event.params["item"]["tool"].as_str()?;
    if event.method == "item/started" {
        Some(("agent_tool_started", format!("Started {tool}")))
    } else {
        let status = event.params["item"]["status"]
            .as_str()
            .unwrap_or("completed");
        Some(("agent_tool_completed", format!("Finished {tool}: {status}")))
    }
}

fn is_terminal(status: &str) -> bool {
    matches!(status, "ready" | "failed" | "cancelled")
}

fn runtime_failure_reason(error: &str) -> &'static str {
    if error.contains("app-server closed") || error.contains("interruption timed out") {
        "agent_process_interrupted"
    } else {
        "agent_failed"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn outcome_json() -> String {
        serde_json::json!({
            "summary": "The evidence narrows the question.",
            "taskOutcomes": [{
                "searchRunIds": [],
                "motivatingEntryIds": [],
                "learnedPoints": ["The result depends on the evaluation setting."],
                "citedPassageRefs": []
            }],
            "unansweredQuestions": ["Does the result generalize?"],
            "nextDirection": "Test a broader setting"
        })
        .to_string()
    }

    #[test]
    fn configured_stop_conditions_override_only_public_limits() {
        let mut configuration = HarnessConfiguration::default();
        configuration.stop_conditions.maximum_run_seconds = Some(90);
        configuration.stop_conditions.maximum_provider_queries = Some(7);
        let limits = effective_agent_limits(&configuration);
        assert_eq!(limits.maximum_run_seconds, 90);
        assert_eq!(limits.maximum_provider_queries, 7);
        assert_eq!(limits.maximum_child_searches, 6);
        assert_eq!(limits.maximum_returned_text_chars, 120_000);
    }

    #[test]
    fn managed_i0i_tools_are_preapproved_for_unattended_runs() {
        let config = codex_thread_config("http://127.0.0.1:1234/mcp", "secret");
        assert_eq!(
            config["mcp_servers"]["ioi"]["default_tools_approval_mode"],
            "approve"
        );
        assert_eq!(config["web_search"], "disabled");
    }

    #[test]
    fn activity_projection_exposes_tool_lifecycle_but_not_arguments() {
        let turn = CodexTurn {
            thread_id: "thread-1".to_string(),
            turn_id: "turn-1".to_string(),
        };
        let event = CodexEvent {
            method: "item/completed".to_string(),
            params: json!({
                "threadId": "thread-1",
                "turnId": "turn-1",
                "item": {
                    "type": "mcpToolCall",
                    "server": "ioi",
                    "tool": "reader_read",
                    "status": "completed",
                    "arguments": {"secret": "not persisted"}
                }
            }),
        };
        let (_, summary) = observable_activity(&event, &turn).expect("observable tool event");
        assert_eq!(summary, "Finished reader_read: completed");
        assert!(!summary.contains("secret"));
    }

    #[test]
    fn final_outcome_parser_requires_the_structured_contract() {
        let outcome = parse_research_outcome(Some(&outcome_json())).expect("valid outcome");
        assert_eq!(outcome.task_outcomes.len(), 1);
        assert!(parse_research_outcome(Some("not json")).is_err());
        assert!(parse_research_outcome(None).is_err());
    }

    #[tokio::test]
    async fn turn_wait_retains_the_final_agent_message() {
        let (sender, mut receiver) = broadcast::channel(4);
        let turn = CodexTurn {
            thread_id: "thread-1".to_string(),
            turn_id: "turn-1".to_string(),
        };
        sender
            .send(CodexEvent {
                method: "item/completed".to_string(),
                params: json!({
                    "threadId": "thread-1",
                    "turnId": "turn-1",
                    "item": {"type": "agentMessage", "text": outcome_json()}
                }),
            })
            .expect("send message");
        sender
            .send(CodexEvent {
                method: "turn/completed".to_string(),
                params: json!({
                    "threadId": "thread-1",
                    "turn": {"id": "turn-1", "status": "completed"}
                }),
            })
            .expect("send completion");
        let store = LibraryStore::for_test(std::env::temp_dir().join(format!(
            "i0i-controller-outcome-{}.sqlite",
            uuid::Uuid::new_v4().simple()
        )));

        let completion = wait_for_turn(
            &mut receiver,
            &turn,
            &store,
            None,
            "project:test",
            "unused-run",
            None,
        )
        .await
        .expect("wait for completion");
        let expected = outcome_json();
        assert_eq!(completion.status, "completed");
        assert_eq!(completion.final_message.as_deref(), Some(expected.as_str()));
    }

    #[tokio::test]
    async fn turn_wait_reports_process_eof_as_interrupted() {
        let (sender, mut receiver) = broadcast::channel(2);
        let turn = CodexTurn {
            thread_id: "thread-1".to_string(),
            turn_id: "turn-1".to_string(),
        };
        sender
            .send(CodexEvent {
                method: "runtime/exited".to_string(),
                params: Value::Null,
            })
            .expect("send process exit");
        let store = LibraryStore::for_test(std::env::temp_dir().join(format!(
            "i0i-controller-exit-{}.sqlite",
            uuid::Uuid::new_v4().simple()
        )));

        let completion = wait_for_turn(
            &mut receiver,
            &turn,
            &store,
            None,
            "project:test",
            "unused-run",
            None,
        )
        .await
        .expect("process exit is observable");
        assert_eq!(completion.status, "runtime_exited");
        assert_eq!(
            runtime_failure_reason("Codex app-server closed its output"),
            "agent_process_interrupted"
        );
    }
}
