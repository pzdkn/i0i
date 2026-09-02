//! Background execution for search runs (RFC 0037).
//!
//! Mirrors `PdfExtractionManager`: a queue keyed by search id, an
//! `async_runtime::spawn` worker, a persisted status lifecycle, `search_updated`
//! events, and a cancellation map. The loop itself lives in `agent`; this owns
//! the side effects (build seams, persist, emit, cancel).

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use reqwest::Client;
use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::commands::discovery::providers::{arxiv::ArxivProvider, openalex::OpenAlexProvider};
use crate::domain::discovery::PaperCandidate;
use crate::domain::harness::HarnessRun;
use crate::domain::research::{candidate_dedup_key, SearchRunStatus};
use crate::services::chat::config::ChatConfig;
use crate::services::embedding::EmbeddingReranker;
use crate::services::research::agent::{self, Progress, RunInputs};
use crate::services::research::planner::OpenRouterPlanner;
use crate::services::research::reconciliation::{
    plan_reconciliation_with_retry_counted, render_reconciliation_prompt,
    should_apply_automatically,
};
use crate::services::research::source::BrowserCandidateSource;
use crate::services::source_acquisition::SourceAcquisitionService;
use crate::storage::library_store::LibraryStore;

type Cancellations = Arc<Mutex<HashMap<String, Arc<AtomicBool>>>>;

/// Owns the search-run queue, worker, cancellation tokens, and events.
#[derive(Clone)]
pub struct SearchManager {
    app: AppHandle,
    store: LibraryStore,
    /// Local biencoder for semantic ranking (RFC 0057). Cheap to clone
    /// (`Option<Arc<…>>`); disabled when the model isn't built/available, in
    /// which case deep research falls back to legacy ranking.
    reranker: EmbeddingReranker,
    source_acquisition: SourceAcquisitionService,
    queued_or_active: Arc<Mutex<HashSet<String>>>,
    cancellations: Cancellations,
    timed_out: Arc<Mutex<HashSet<String>>>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchUpdated {
    pub search_id: String,
    pub run_id: String,
    pub status: String,
    pub message: String,
    pub iteration: u32,
    pub found: u32,
    pub unique: u32,
    pub new: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchCandidatesPreview {
    pub search_id: String,
    pub run_id: String,
    pub candidates: Vec<PaperCandidate>,
    pub unique: u32,
}

#[derive(Debug, Clone, Serialize)]
struct RunSummary {
    new: usize,
    total: usize,
    complete: bool,
    stop_reason: String,
}

impl SearchManager {
    pub fn new(
        app: AppHandle,
        store: LibraryStore,
        reranker: EmbeddingReranker,
        source_acquisition: SourceAcquisitionService,
    ) -> Self {
        Self {
            app,
            store,
            reranker,
            source_acquisition,
            queued_or_active: Arc::new(Mutex::new(HashSet::new())),
            cancellations: Arc::new(Mutex::new(HashMap::new())),
            timed_out: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    /// Reset runs left mid-flight by a previous session to `failed`.
    pub fn recover_and_queue_startup_runs(&self) -> Result<(), String> {
        // Phase 1: no automatic re-queue. Stale `running` runs are marked failed
        // so the UI never shows a perpetually-spinning run. (Scheduling catch-up
        // lands with Phase 2.)
        Ok(())
    }

    /// Enqueue a run for a search and return its run id immediately.
    pub fn run_search(&self, search_id: String, mode: Option<String>) -> Result<String, String> {
        let mode = mode.unwrap_or_else(|| "deep".to_string());
        let run = self.store.create_search_run(&search_id, &mode)?;
        let run_id = run.id.clone();
        self.enqueue_run(search_id, run_id.clone(), None)?;
        Ok(run_id)
    }

    /// Creates and links the Search Run before its background task can execute.
    pub fn run_harness_search_with_timeout(
        &self,
        search_id: String,
        mode: Option<String>,
        maximum_seconds: Option<i64>,
        harness_run_id: &str,
    ) -> Result<HarnessRun, String> {
        let mode = mode.unwrap_or_else(|| "project_harness".to_string());
        let run = self.store.create_search_run(&search_id, &mode)?;
        let linked = self
            .store
            .attach_harness_search_run(harness_run_id, &run.id)?;
        self.enqueue_run(search_id, run.id, maximum_seconds)?;
        Ok(linked)
    }

    fn enqueue_run(
        &self,
        search_id: String,
        run_id: String,
        maximum_seconds: Option<i64>,
    ) -> Result<(), String> {
        if !self.mark_queued(&search_id) {
            return Ok(());
        }

        let cancel = Arc::new(AtomicBool::new(false));
        self.cancellations
            .lock()
            .expect("cancellation lock")
            .insert(run_id.clone(), cancel.clone());

        let manager = self.clone();
        let search_id_for_cleanup = search_id.clone();
        let run_id_for_job = run_id.clone();
        tauri::async_runtime::spawn(async move {
            let result = manager.execute(&search_id, &run_id_for_job, cancel).await;
            manager.mark_finished(&search_id_for_cleanup);
            manager
                .cancellations
                .lock()
                .expect("cancellation lock")
                .remove(&run_id_for_job);
            manager
                .timed_out
                .lock()
                .expect("timeout lock")
                .remove(&run_id_for_job);
            if let Err(error) = result {
                manager.fail_run(&search_id_for_cleanup, &run_id_for_job, &error);
            }
        });

        if let Some(seconds) = maximum_seconds.filter(|seconds| *seconds > 0) {
            let manager = self.clone();
            let timed_run_id = run_id;
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_secs(seconds as u64)).await;
                let token = manager
                    .cancellations
                    .lock()
                    .expect("cancellation lock")
                    .get(&timed_run_id)
                    .cloned();
                if let Some(token) = token {
                    manager
                        .timed_out
                        .lock()
                        .expect("timeout lock")
                        .insert(timed_run_id);
                    token.store(true, Ordering::Relaxed);
                }
            });
        }
        Ok(())
    }

    /// Trip the cancellation token for an in-flight run.
    pub fn cancel_run(&self, run_id: &str) {
        if let Some(flag) = self
            .cancellations
            .lock()
            .expect("cancellation lock")
            .get(run_id)
        {
            flag.store(true, Ordering::Relaxed);
        }
    }

    async fn execute(
        &self,
        search_id: &str,
        run_id: &str,
        cancel: Arc<AtomicBool>,
    ) -> Result<(), String> {
        eprintln!("[research] run start search_id={search_id} run_id={run_id}");
        let search = self.store.get_search(search_id)?;
        self.store.set_search_run_status(
            run_id,
            SearchRunStatus::Planning,
            0,
            None,
            None,
            false,
        )?;
        self.store
            .set_search_status(search_id, SearchRunStatus::Planning, None, None)?;

        // Build the seams. Provider/LLM config is loaded per run (runs are rare),
        // so a `model.planner` override (Settings) applies without restart; it
        // falls back to the chat model (RFC 0055).
        let chat = ChatConfig::load()?;
        let api_key = chat.resolve_api_key()?;
        let planner_model = crate::services::settings::preference("model.planner")
            .unwrap_or_else(|| chat.model.clone());
        let planner =
            OpenRouterPlanner::new(Client::new(), chat.url.clone(), api_key, planner_model);
        let source = BrowserCandidateSource::from_app(
            &self.app,
            self.source_acquisition.clone(),
            OpenAlexProvider::from_app_config().map_err(|e| e.to_string())?,
            ArxivProvider::from_app_config().map_err(|e| e.to_string())?,
        );

        let existing_keys: HashSet<String> = self
            .store
            .list_search_candidates(search_id)?
            .iter()
            .map(|c| candidate_dedup_key(&c.candidate))
            .collect();

        let inputs = RunInputs {
            goal: &search.goal,
            constraints: &search.constraints,
            strategy: &search.strategy,
            existing_keys,
        };

        let app = self.app.clone();
        let sid = search_id.to_string();
        let rid = run_id.to_string();
        let progress_store = self.store.clone();
        let progress_run_id = run_id.to_string();
        let query_expansions = Arc::new(Mutex::new(Vec::<String>::new()));
        let query_expansions_for_progress = query_expansions.clone();
        let on = move |progress: Progress| {
            let (event_kind, event_summary, event_detail, event_phase, current, total) =
                activity_event(&progress);
            let _ = progress_store.record_harness_search_progress(
                &progress_run_id,
                event_kind,
                &event_summary,
                event_detail,
                Some(event_phase),
                current,
                total,
            );
            if let Progress::Searching { text, .. } = &progress {
                let mut expansions = query_expansions_for_progress
                    .lock()
                    .expect("query expansions lock");
                if !expansions.iter().any(|existing| existing == text) {
                    expansions.push(text.clone());
                }
            }

            if let Progress::CandidatePreview { candidates } = progress {
                let unique = candidates.len() as u32;
                let _ = app.emit(
                    "search_candidates_preview",
                    SearchCandidatesPreview {
                        search_id: sid.clone(),
                        run_id: rid.clone(),
                        candidates,
                        unique,
                    },
                );
                return;
            }

            let (status, message, counts) = describe(&progress);
            let _ = app.emit(
                "search_updated",
                SearchUpdated {
                    search_id: sid.clone(),
                    run_id: rid.clone(),
                    status: status.as_str().to_string(),
                    message,
                    iteration: 0,
                    found: counts.0,
                    unique: counts.1,
                    new: counts.2,
                },
            );
        };

        let outcome = agent::run(&planner, &source, &self.reranker, inputs, &cancel, on)
            .await
            .map_err(|e| e.to_string())?;
        let query_expansions_json = {
            let expansions = query_expansions.lock().expect("query expansions lock");
            serde_json::to_string(&*expansions).map_err(|e| e.to_string())?
        };
        self.store
            .set_search_run_query_expansions(run_id, &query_expansions_json)?;

        let added = self
            .store
            .append_new_candidates(search_id, run_id, &outcome.ranked)?;
        let total = self.store.list_search_candidates(search_id)?.len();
        let timed_out = self.timed_out.lock().expect("timeout lock").remove(run_id);
        let stop = if timed_out {
            "maximum_run_seconds"
        } else {
            outcome.stop_reason.as_str()
        };
        let summary = serde_json::to_string(&RunSummary {
            new: added,
            total,
            complete: outcome.complete,
            stop_reason: stop.to_string(),
        })
        .map_err(|e| e.to_string())?;
        let final_status = if timed_out
            || outcome.stop_reason == crate::services::research::budget::StopReason::Cancelled
        {
            SearchRunStatus::Cancelled
        } else {
            SearchRunStatus::Ready
        };

        self.store.set_search_run_usage(
            run_id,
            outcome.usage.provider_queries,
            outcome.usage.llm_calls,
            outcome.iterations,
            outcome.ranked.len() as u32,
        )?;

        self.store.set_search_run_status(
            run_id,
            final_status,
            outcome.iterations as i32,
            Some(stop),
            None,
            true,
        )?;
        self.store
            .set_search_status(search_id, final_status, Some(stop), Some(&summary))?;

        if final_status == SearchRunStatus::Ready {
            let reconciliation_calls = self.reconcile_harness_run(run_id, &planner).await?;
            self.store
                .add_search_run_llm_calls(run_id, reconciliation_calls)?;
            if let Some(harness_run) = self.store.get_harness_run_for_search_run(run_id)? {
                if let Err(error) = self
                    .store
                    .record_automatic_harness_reflection(&harness_run.id)
                {
                    self.store
                        .record_harness_reflection_failure(&harness_run.id, &error)?;
                }
                self.store.finalize_harness_run(run_id)?;
            }
        }

        let _ = self.app.emit(
            "search_updated",
            SearchUpdated {
                search_id: search_id.to_string(),
                run_id: run_id.to_string(),
                status: final_status.as_str().to_string(),
                message: final_message(outcome.complete, stop, added, total),
                iteration: outcome.iterations,
                found: 0,
                unique: total as u32,
                new: added as u32,
            },
        );
        Ok(())
    }

    async fn reconcile_harness_run(
        &self,
        search_run_id: &str,
        planner: &OpenRouterPlanner,
    ) -> Result<u32, String> {
        let Some(run) = self.store.get_harness_run_for_search_run(search_run_id)? else {
            return Ok(0);
        };
        let candidates = self.store.harness_reconciliation_candidates(&run.id)?;
        let telemetry = self.store.harness_reconciliation_telemetry(&run.id)?;
        let prompt =
            render_reconciliation_prompt(&run.effective_instructions, &candidates, &telemetry)?;
        let (planned, call_count) = plan_reconciliation_with_retry_counted(
            planner,
            &prompt,
            &run.effective_instructions,
            &candidates,
        )
        .await;
        let plan = match planned {
            Ok(plan) => plan,
            Err(error) => {
                self.store.fail_harness_reconciliation(&run.id, &error)?;
                return Ok(call_count);
            }
        };
        let change_set = match self.store.create_harness_change_set(&run.id, &plan) {
            Ok(change_set) => change_set,
            Err(error) => {
                self.store.fail_harness_reconciliation(&run.id, &error)?;
                return Ok(call_count);
            }
        };
        if should_apply_automatically(&run.effective_instructions) {
            let _ = self.store.apply_harness_change_set(&change_set.id);
        }
        Ok(call_count)
    }

    fn fail_run(&self, search_id: &str, run_id: &str, error: &str) {
        eprintln!("[research] run failed search_id={search_id} run_id={run_id} error={error}");
        let _ = self.store.set_search_run_status(
            run_id,
            SearchRunStatus::Failed,
            0,
            None,
            Some(error),
            true,
        );
        let _ = self
            .store
            .set_search_status(search_id, SearchRunStatus::Failed, None, None);
        let _ = self.app.emit(
            "search_updated",
            SearchUpdated {
                search_id: search_id.to_string(),
                run_id: run_id.to_string(),
                status: "failed".to_string(),
                message: error.to_string(),
                iteration: 0,
                found: 0,
                unique: 0,
                new: 0,
            },
        );
    }

    fn mark_queued(&self, search_id: &str) -> bool {
        self.queued_or_active
            .lock()
            .expect("search queue lock")
            .insert(search_id.to_string())
    }

    fn mark_finished(&self, search_id: &str) {
        self.queued_or_active
            .lock()
            .expect("search queue lock")
            .remove(search_id);
    }
}

/// Maps transient agent signals to bounded persisted Activity facts.
fn activity_event(
    progress: &Progress,
) -> (
    &'static str,
    String,
    Option<serde_json::Value>,
    &'static str,
    Option<i64>,
    Option<i64>,
) {
    match progress {
        Progress::Planning { iteration } => (
            "iteration_planned",
            format!("Planning research iteration {}", iteration + 1),
            Some(serde_json::json!({ "iteration": iteration + 1 })),
            "planning",
            Some(i64::from(*iteration + 1)),
            None,
        ),
        Progress::Searching { provider, text } => (
            "provider_query_started",
            format!("Searching {provider}"),
            Some(serde_json::json!({
                "provider": provider,
                "query": text.chars().take(500).collect::<String>(),
            })),
            "searching",
            None,
            None,
        ),
        Progress::SearchResult { provider, count } => (
            "provider_query_completed",
            format!("{provider} returned {count} candidates"),
            Some(serde_json::json!({ "provider": provider, "candidateCount": count })),
            "searching",
            Some(*count as i64),
            None,
        ),
        Progress::SearchFailed { provider, .. } => (
            "provider_query_failed",
            format!("{provider} query failed"),
            Some(serde_json::json!({ "provider": provider })),
            "searching",
            None,
            None,
        ),
        Progress::Deduped { unique } => (
            "candidates_deduplicated",
            format!("Retained {unique} unique candidates"),
            Some(serde_json::json!({ "uniqueCandidates": unique })),
            "searching",
            Some(*unique as i64),
            None,
        ),
        Progress::CandidatePreview { candidates } => (
            "candidates_inspected",
            format!("Inspected {} candidates", candidates.len()),
            Some(serde_json::json!({ "candidateCount": candidates.len() })),
            "searching",
            Some(candidates.len() as i64),
            None,
        ),
        Progress::Resolving { count } => (
            "candidate_metadata_resolving",
            format!("Resolving metadata for {count} candidates"),
            Some(serde_json::json!({ "candidateCount": count })),
            "searching",
            Some(*count as i64),
            None,
        ),
        Progress::Assessing => (
            "coverage_assessing",
            "Assessing research coverage".to_string(),
            None,
            "assessing",
            None,
            None,
        ),
        Progress::Ranking { count } => (
            "candidates_ranking",
            format!("Ranking {count} candidates"),
            Some(serde_json::json!({ "candidateCount": count })),
            "ranking",
            Some(*count as i64),
            None,
        ),
    }
}

/// Describe whether a run converged or returned the best pool its budget bought.
fn final_message(complete: bool, stop_reason: &str, added: usize, total: usize) -> String {
    if complete {
        format!("{stop_reason} · {added} new · {total} total")
    } else {
        format!("stopped early ({stop_reason}) · {added} new · {total} total")
    }
}

/// Map a progress signal to (status, human message, (found, unique, new)).
fn describe(progress: &Progress) -> (SearchRunStatus, String, (u32, u32, u32)) {
    match progress {
        Progress::Planning { iteration } => (
            SearchRunStatus::Planning,
            format!("planning (iteration {iteration})"),
            (0, 0, 0),
        ),
        Progress::Searching { provider, text } => (
            SearchRunStatus::Searching,
            format!("search({provider}, \"{text}\")"),
            (0, 0, 0),
        ),
        Progress::SearchResult { provider, count } => (
            SearchRunStatus::Searching,
            format!("{provider} → {count}"),
            (*count as u32, 0, 0),
        ),
        Progress::SearchFailed { provider, error } => (
            SearchRunStatus::Searching,
            format!("{provider} failed: {error}"),
            (0, 0, 0),
        ),
        Progress::Deduped { unique } => (
            SearchRunStatus::Searching,
            format!("deduped → {unique} unique"),
            (0, *unique as u32, 0),
        ),
        Progress::CandidatePreview { candidates } => (
            SearchRunStatus::Searching,
            format!("preview → {} unique", candidates.len()),
            (0, candidates.len() as u32, candidates.len() as u32),
        ),
        Progress::Resolving { count } => (
            SearchRunStatus::Searching,
            format!("resolving metadata for {count} papers"),
            (0, *count as u32, 0),
        ),
        Progress::Assessing => (
            SearchRunStatus::Assessing,
            "assessing coverage".to_string(),
            (0, 0, 0),
        ),
        Progress::Ranking { count } => (
            SearchRunStatus::Ranking,
            format!("ranking {count} candidates"),
            (0, 0, *count as u32),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn final_message_distinguishes_convergence_from_exhaustion() {
        assert_eq!(
            final_message(true, "converged", 4, 12),
            "converged · 4 new · 12 total"
        );
        assert_eq!(
            final_message(false, "max_iterations", 4, 12),
            "stopped early (max_iterations) · 4 new · 12 total"
        );
    }
}
