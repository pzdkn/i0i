//! Managed Codex orchestration for one project research Run.
//!
//! The controller owns lifecycle and limits. Codex chooses research actions,
//! but can only act through the project-scoped MCP grant issued here.

use std::collections::HashMap;
use std::fs;
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
};
use crate::domain::research::{SearchConstraints, SearchDraft};
use crate::services::codex_runtime::{CodexEvent, CodexRuntime, CodexRuntimeConfig, CodexTurn};
use crate::services::mcp::{
    LocalMcpServer, READER_ADD_NOTE, READER_LIST_NOTES, READER_READ, SEARCH_CANCEL, SEARCH_GET,
    SEARCH_START, STATE_READ, STATE_UPDATE, VAULT_ADD_PAPER, VAULT_GET_PAPER, VAULT_LIST,
    VAULT_LIST_PAPERS,
};
use crate::services::research::manager::SearchManager;
use crate::storage::library_store::LibraryStore;

const CHILD_SETTLE_SECONDS: u64 = 30;

#[derive(Clone)]
struct ActiveAgentRun {
    run_id: String,
    cancellation: CancellationToken,
}

/// Starts and supervises managed Codex research without duplicating tool logic.
#[derive(Clone)]
pub struct ProjectResearchController {
    app: AppHandle,
    store: LibraryStore,
    search_manager: SearchManager,
    mcp_server: LocalMcpServer,
    runtime: Arc<Mutex<Option<CodexRuntime>>>,
    active_runs: Arc<StdMutex<HashMap<String, ActiveAgentRun>>>,
}

impl ProjectResearchController {
    /// Construct the app-wide controller. Codex itself starts lazily on first Run.
    pub fn new(
        app: AppHandle,
        store: LibraryStore,
        search_manager: SearchManager,
        mcp_server: LocalMcpServer,
    ) -> Self {
        Self {
            app,
            store,
            search_manager,
            mcp_server,
            runtime: Arc::new(Mutex::new(None)),
            active_runs: Arc::new(StdMutex::new(HashMap::new())),
        }
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

        let runtime_config = CodexRuntimeConfig::load();
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

    /// Signal the active managed Run for one project; the worker owns cleanup.
    pub async fn cancel(&self, project_id: &str) -> Result<(), String> {
        let active = self
            .active_runs
            .lock()
            .expect("active Research Run lock")
            .get(project_id)
            .cloned()
            .ok_or_else(|| "No Research Run is active for this Project".to_string())?;
        active.cancellation.cancel();
        self.store.record_harness_activity(
            &active.run_id,
            "cancellation_requested",
            "Research Run cancellation requested",
            Some("complete"),
        )
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
            let _ = self.store.record_harness_activity(
                &run_id,
                "agent_failed",
                &bounded_error,
                Some("complete"),
            );
            let finalized = match self.store.get_harness_run(&run_id) {
                Ok(run) => self
                    .settle_child_searches(&run.project_id, &run_id)
                    .await
                    .and_then(|()| {
                        self.store
                            .finish_codex_harness_run(&run_id, "failed", "agent_failed")
                    }),
                Err(store_error) => Err(store_error),
            };
            if finalized.is_err() {
                let _ = self.store.fail_harness_run(&run_id, &bounded_error);
            }
        }
        self.mcp_server.revoke_run(&run_id).await;
        let _ = self
            .app
            .emit("research_harness_updated", json!({"runId": run_id}));
    }

    async fn execute_inner(
        &self,
        run_id: &str,
        runtime_config: CodexRuntimeConfig,
        cancellation: CancellationToken,
    ) -> Result<(), String> {
        let run = self.store.get_harness_run(run_id)?;
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
                    READER_READ,
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
        let work_dir = self
            .app
            .path()
            .app_data_dir()
            .map_err(|error| error.to_string())?
            .join("research-runtime");
        fs::create_dir_all(&work_dir).map_err(|error| error.to_string())?;
        let thread_id = runtime
            .start_thread(
                &work_dir,
                RESEARCH_AGENT_INSTRUCTIONS,
                codex_thread_config(&grant.endpoint, &grant.bearer_token),
            )
            .await?;
        let prompt = render_research_prompt(&run.effective_instructions, &limits)?;
        let turn = runtime.start_turn(&thread_id, &prompt).await?;
        self.store
            .attach_codex_turn(run_id, &turn.thread_id, &turn.turn_id)?;

        let terminal = tokio::select! {
            result = wait_for_turn(&mut events, &turn, &self.store, run_id) => result,
            _ = cancellation.cancelled() => {
                runtime.interrupt(&turn).await?;
                Ok("cancelled".to_string())
            }
            _ = tokio::time::sleep(Duration::from_secs(limits.maximum_run_seconds)) => {
                runtime.interrupt(&turn).await?;
                Ok("timed_out".to_string())
            }
        }?;

        self.settle_child_searches(&run.project_id, run_id).await?;
        let (status, reason) = match terminal.as_str() {
            "completed" => ("ready", "agent_completed"),
            "interrupted" | "cancelled" => ("cancelled", "cancelled_by_user"),
            "timed_out" => ("failed", "maximum_run_seconds_reached"),
            "failed" => ("failed", "agent_failed"),
            other => return Err(format!("Unknown Codex turn status: {other}")),
        };
        self.store
            .finish_codex_harness_run(run_id, status, reason)?;
        Ok(())
    }

    async fn ensure_runtime(&self, config: CodexRuntimeConfig) -> Result<CodexRuntime, String> {
        let mut runtime = self.runtime.lock().await;
        if let Some(existing) = runtime.as_ref() {
            return Ok(existing.clone());
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
            let (_, changed) = self
                .store
                .request_agent_search_cancel(project_id, &child.id)?;
            if changed {
                self.search_manager.cancel_run(&child.id);
            }
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
}

const RESEARCH_AGENT_INSTRUCTIONS: &str = r#"You are i0i's bounded literature research agent. Work only through the i0i MCP tools. Do not use shell, filesystem, built-in web search, or unrelated MCP servers. Inspect Research State and the Vault before choosing work. State the purpose of each focused search. Save useful candidates, wait for acquisition when needed, and read relevant passages before citing them. Compare evidence with existing entries and actively look for conflicting results and conditions. Update Research State only with passage references actually returned by reader_read. Distinguish source claims, synthesis, speculation, abstract-only coverage, and unavailable full text. Continue only while another step can materially improve the project; otherwise finish with a concise summary and remaining questions."#;

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
                "enabled": true
            }
        },
        "web_search": "disabled"
    })
}

fn render_research_prompt(
    stack: &EffectiveInstructionStack,
    limits: &AgentRunLimits,
) -> Result<String, String> {
    let snapshot = serde_json::to_string_pretty(&json!({
        "projectInstructions": stack.project_research_instructions,
        "stateRevision": stack.run_context.starting_state_revision,
        "stateEntries": stack.run_context.active_entries,
        "vaultId": stack.run_context.vault_id,
        "vaultRevision": stack.run_context.vault_revision,
        "vaultPaperIds": stack.run_context.vault_paper_ids,
        "priorNextDirection": stack.run_context.prior_next_direction,
        "priorObservations": stack.run_context.prior_observations,
        "paperTarget": stack.run_context.paper_budget,
        "limits": limits,
    }))
    .map_err(|error| error.to_string())?;
    Ok(format!(
        "Run this project's literature research procedure. Treat the snapshot as orientation, then verify current data with i0i tools before writing.\n\n{snapshot}"
    ))
}

async fn wait_for_turn(
    events: &mut broadcast::Receiver<CodexEvent>,
    turn: &CodexTurn,
    store: &LibraryStore,
    run_id: &str,
) -> Result<String, String> {
    loop {
        match events.recv().await {
            Ok(event) => {
                if let Some((kind, summary)) = observable_activity(&event, turn) {
                    store.record_harness_activity(run_id, kind, &summary, Some("agent"))?;
                }
                if event.method == "turn/completed"
                    && event.params["threadId"].as_str() == Some(&turn.thread_id)
                    && event.params["turn"]["id"].as_str() == Some(&turn.turn_id)
                {
                    return event.params["turn"]["status"]
                        .as_str()
                        .map(str::to_string)
                        .ok_or_else(|| "Codex completion omitted turn status".to_string());
                }
            }
            Err(broadcast::error::RecvError::Lagged(_)) => continue,
            Err(broadcast::error::RecvError::Closed) => {
                return Err("Codex event stream closed before turn completion".to_string());
            }
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
