//! Vault-scoped paper recommendations (RFC 0091 and RFC 0094).
//!
//! The manager prepares a reviewable query agenda without contacting paper
//! providers. Once the user approves paths, it searches them concurrently,
//! filters owned and dismissed papers, adds citation neighbours, fuses the
//! existing vault ranking signals, and atomically replaces the inbox.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use futures_util::stream::{self, StreamExt};
use reqwest::Client;
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter};
use tokio::sync::Semaphore;

use crate::commands::discovery::orchestrator::{
    apply_semantic_floor, legacy_top_n, rank_candidates, SEMANTIC_RERANK_WINDOW,
};
use crate::commands::discovery::providers::{arxiv::ArxivProvider, openalex::OpenAlexProvider};
use crate::domain::discovery::{
    paper_candidate_dedup_key, DiscoveryProviderChoice, Lineage, PaperCandidate,
};
use crate::domain::research::SearchConstraints;
use crate::domain::vault_suggestion::{
    VaultSuggestion, VaultSuggestionOptions, VaultSuggestionPreview, VaultSuggestionQueryPath,
    VaultSuggestionQueryPlan, VaultSuggestionUpdated,
};
use crate::services::chat::config::ChatConfig;
use crate::services::embedding::{EmbeddingReranker, MODEL_NAME, MODEL_VERSION};
use crate::services::research::planner::{OpenRouterPlanner, Query};
use crate::services::research::source::{CandidateSource, RealCandidateSource};
use crate::services::search::fusion::reciprocal_rank_fusion_many;
use crate::storage::library_store::LibraryStore;

const MAX_PROFILE_CHARS: usize = 12_000;
const MAX_PAPERS_IN_PROFILE: usize = 24;
const MAX_CHUNKS_PER_PAPER: usize = 2;
const GRAPH_SEED_LIMIT: usize = 6;
const GRAPH_PER_SEED_LIMIT: i32 = 10;
const GRAPH_HOP_TWO_SEEDS: usize = 4;
const MAX_CONCURRENT_QUERY_SEARCHES: usize = 3;
const RETRIEVAL_POOL_LIMIT: i32 = 25;
/// Structural agreement is the distinctive vault signal. Repeating its rank
/// list makes RRF treat it as three independent arrivals without blending raw
/// counts with incomparable semantic and citation scores.
const GRAPH_RRF_WEIGHT: usize = 3;

type Cancellations = Arc<Mutex<HashMap<String, Arc<AtomicBool>>>>;

#[derive(Clone)]
struct PendingQueryPlan {
    plan: VaultSuggestionQueryPlan,
    options: VaultSuggestionOptions,
}

/// Owns manual suggestion runs and their transient progress events.
#[derive(Clone)]
pub struct VaultSuggestionManager {
    app: AppHandle,
    store: LibraryStore,
    reranker: EmbeddingReranker,
    active_vaults: Arc<Mutex<HashSet<String>>>,
    cancellations: Cancellations,
    plans: Arc<Mutex<HashMap<String, PendingQueryPlan>>>,
}

impl VaultSuggestionManager {
    /// Construct the manager around the app's shared store and embedding model.
    pub fn new(app: AppHandle, store: LibraryStore, reranker: EmbeddingReranker) -> Self {
        Self {
            app,
            store,
            reranker,
            active_vaults: Arc::new(Mutex::new(HashSet::new())),
            cancellations: Arc::new(Mutex::new(HashMap::new())),
            plans: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Preserve explicit query approval by never starting retrieval at startup.
    ///
    /// RFC 0091's weekly automatic run cannot choose paths on the user's behalf
    /// after RFC 0094. A future scheduler may prepare a visible proposal, but
    /// provider search remains manual.
    pub fn recover_and_queue_startup_run(&self) -> Result<(), String> {
        Ok(())
    }

    /// Prepare reviewable query paths without contacting paper providers.
    pub async fn prepare_queries(
        &self,
        vault_id: String,
        options: VaultSuggestionOptions,
    ) -> Result<VaultSuggestionQueryPlan, String> {
        let options = validate_options(options)?;
        let snapshot = self.store.get_library()?;
        let vault_papers = papers_for_vault(&snapshot, &vault_id);
        if vault_papers.is_empty() {
            return Err("No papers to match yet".to_string());
        }
        let profile = build_vault_profile(&self.store, &vault_papers, options.focus.as_deref())?;
        let queries = suggestion_planner()?
            .plan_vault_suggestion_queries(&profile, options.query_path_count)
            .await
            .map_err(|error| error.to_string())?;
        let revision = vault_revision(&vault_papers);
        let plan = VaultSuggestionQueryPlan {
            id: query_plan_id(&vault_id, &revision)?,
            vault_id: vault_id.clone(),
            vault_revision: revision,
            queries,
        };
        self.store
            .save_vault_suggestion_options(&vault_id, &options)?;
        let mut plans = self.plans.lock().expect("suggestion plan lock");
        plans.retain(|_, pending| pending.plan.vault_id != vault_id);
        plans.insert(
            plan.id.clone(),
            PendingQueryPlan {
                plan: plan.clone(),
                options,
            },
        );
        Ok(plan)
    }

    /// Validate and persist controls without preparing or running a search.
    pub fn save_options(
        &self,
        vault_id: &str,
        options: VaultSuggestionOptions,
    ) -> Result<(), String> {
        let options = validate_options(options)?;
        self.store.save_vault_suggestion_options(vault_id, &options)
    }

    /// Queue searches for the approved paths and return the durable run id.
    pub fn run(
        &self,
        vault_id: String,
        plan_id: String,
        selected_query_ids: Vec<String>,
        options: VaultSuggestionOptions,
    ) -> Result<String, String> {
        let options = validate_options(options)?;
        let snapshot = self.store.get_library()?;
        let vault_papers = papers_for_vault(&snapshot, &vault_id);
        let paper_count = snapshot
            .vault_papers
            .iter()
            .filter(|membership| membership.vault_id == vault_id)
            .count();
        if paper_count == 0 {
            return Err("No papers to match yet".to_string());
        }

        let pending = self
            .plans
            .lock()
            .expect("suggestion plan lock")
            .get(&plan_id)
            .cloned()
            .ok_or_else(|| "Suggestion queries expired; prepare them again".to_string())?;
        if pending.plan.vault_id != vault_id
            || pending.plan.vault_revision != vault_revision(&vault_papers)
        {
            return Err("Vault changed; prepare suggestion queries again".to_string());
        }
        if pending.options.focus != options.focus
            || pending.options.query_path_count != options.query_path_count
        {
            return Err("Search focus changed; prepare suggestion queries again".to_string());
        }
        let selected_paths = selected_query_paths(&pending.plan, &selected_query_ids)?;

        if !self
            .active_vaults
            .lock()
            .expect("suggestion run lock")
            .insert(vault_id.clone())
        {
            return Err("A suggestion run is already active for this vault".to_string());
        }
        self.store
            .save_vault_suggestion_options(&vault_id, &options)?;
        let run = match self.store.create_vault_suggestion_run_with_options(
            &vault_id,
            &options,
            &selected_paths,
        ) {
            Ok(run) => run,
            Err(error) => {
                self.active_vaults
                    .lock()
                    .expect("suggestion run lock")
                    .remove(&vault_id);
                return Err(error);
            }
        };
        let run_id = run.id.clone();

        let cancelled = Arc::new(AtomicBool::new(false));
        self.cancellations
            .lock()
            .expect("suggestion cancellation lock")
            .insert(run_id.clone(), cancelled.clone());

        let manager = self.clone();
        let run_id_for_job = run_id.clone();
        tauri::async_runtime::spawn(async move {
            let result = manager
                .execute(
                    &vault_id,
                    &run_id_for_job,
                    selected_paths,
                    options,
                    cancelled,
                )
                .await;
            manager
                .active_vaults
                .lock()
                .expect("suggestion run lock")
                .remove(&vault_id);
            manager
                .cancellations
                .lock()
                .expect("suggestion cancellation lock")
                .remove(&run_id_for_job);
            if let Err(error) = result {
                manager.fail(&vault_id, &run_id_for_job, &error);
            }
        });
        Ok(run_id)
    }

    /// Request cancellation between the loop's bounded operations.
    pub fn cancel(&self, run_id: &str) {
        if let Some(flag) = self
            .cancellations
            .lock()
            .expect("suggestion cancellation lock")
            .get(run_id)
        {
            flag.store(true, Ordering::Relaxed);
        }
    }

    async fn execute(
        &self,
        vault_id: &str,
        run_id: &str,
        query_paths: Vec<VaultSuggestionQueryPath>,
        options: VaultSuggestionOptions,
        cancelled: Arc<AtomicBool>,
    ) -> Result<(), String> {
        let sequence = AtomicU64::new(0);
        self.update(vault_id, run_id, "searching", "Starting searches", 0, false)?;
        let snapshot = self.store.get_library()?;
        let vault_papers = papers_for_vault(&snapshot, vault_id);
        let profile = build_vault_profile(&self.store, &vault_papers, options.focus.as_deref())?;
        let blocked = blocked_candidate_refs(
            &snapshot.papers,
            self.store.decided_vault_suggestion_refs(vault_id)?,
        );

        let source = RealCandidateSource::new(
            OpenAlexProvider::from_app_config().map_err(|error| error.to_string())?,
            ArxivProvider::from_app_config().map_err(|error| error.to_string())?,
        );
        let constraints = SearchConstraints {
            year_from: options.year_from,
            year_to: options.year_to,
            providers: Vec::new(),
            open_access: false,
            target_count: options.result_count as i32,
            venues: Vec::new(),
            authors: Vec::new(),
            fields_of_study: Vec::new(),
            seed_paper_ids: vault_papers.iter().map(|paper| paper.id.clone()).collect(),
        };

        // Fan each approved path across both providers under one shared cap.
        let providers = [
            DiscoveryProviderChoice::OpenAlex,
            DiscoveryProviderChoice::Arxiv,
        ];
        let jobs: Vec<(VaultSuggestionQueryPath, DiscoveryProviderChoice)> = query_paths
            .iter()
            .flat_map(|path| {
                providers
                    .iter()
                    .cloned()
                    .map(move |provider| (path.clone(), provider))
            })
            .collect();
        let arxiv_gate = Arc::new(Semaphore::new(1));
        let searches = stream::iter(jobs.into_iter().map(|(path, provider)| {
            let source = &source;
            let manager = self;
            let sequence = &sequence;
            let mut query_constraints = constraints.clone();
            query_constraints.providers = vec![provider.clone()];
            let query = Query {
                provider: provider.clone(),
                text: path.query.clone(),
            };
            let cancelled = cancelled.clone();
            let arxiv_gate = arxiv_gate.clone();
            async move {
                if cancelled.load(Ordering::Relaxed) {
                    return (path, provider, None);
                }
                let _provider_permit = if provider == DiscoveryProviderChoice::Arxiv {
                    Some(
                        arxiv_gate
                            .acquire_owned()
                            .await
                            .expect("arXiv suggestion gate"),
                    )
                } else {
                    None
                };
                manager.emit_progress(
                    vault_id,
                    run_id,
                    sequence,
                    "provider",
                    Some(path.id.clone()),
                    format!("{} · {}", provider_display_name(&provider), path.intent),
                    Some("Searching".to_string()),
                    0,
                );
                let result = source.search(&query, &query_constraints).await;
                (path, provider, Some(result))
            }
        }))
        .buffer_unordered(MAX_CONCURRENT_QUERY_SEARCHES);
        futures_util::pin_mut!(searches);

        let mut candidates = Vec::new();
        let mut seen_previews = HashSet::new();
        let mut completed_searches = 0_u32;
        while let Some((path, provider, result)) = searches.next().await {
            if cancelled.load(Ordering::Relaxed) {
                break;
            }
            completed_searches += 1;
            let provider_name = provider_display_name(&provider);
            match result {
                Some(Ok(found)) => {
                    self.emit_progress(
                        vault_id,
                        run_id,
                        &sequence,
                        "provider",
                        Some(path.id.clone()),
                        format!("{provider_name} · {}", path.intent),
                        Some(format!("{} found", found.len())),
                        found.len() as u32,
                    );
                    let preview: Vec<VaultSuggestion> = found
                        .iter()
                        .filter(|candidate| !candidate_is_blocked(candidate, &blocked))
                        .filter(|candidate| {
                            seen_previews.insert(paper_candidate_dedup_key(candidate))
                        })
                        .take(options.result_count as usize)
                        .cloned()
                        .map(|candidate| {
                            suggestion_from_candidate(vault_id, run_id, candidate, 0.0, None, None)
                        })
                        .collect();
                    if !preview.is_empty() {
                        let _ = self.app.emit(
                            "vault_suggestion_preview",
                            VaultSuggestionPreview {
                                vault_id: vault_id.to_string(),
                                run_id: run_id.to_string(),
                                suggestions: preview,
                            },
                        );
                    }
                    candidates.extend(found);
                }
                Some(Err(error)) => self.emit_progress(
                    vault_id,
                    run_id,
                    &sequence,
                    "provider",
                    Some(path.id),
                    format!("{provider_name} unavailable"),
                    Some(short_error(&error.to_string())),
                    0,
                ),
                None => {}
            }
            let message = format!("Searching {completed_searches}/{}", query_paths.len() * 2);
            self.store.set_vault_suggestion_run_status(
                run_id,
                "searching",
                &message,
                None,
                None,
                candidates.len() as i32,
                false,
            )?;
        }

        if cancelled.load(Ordering::Relaxed) {
            self.update(vault_id, run_id, "cancelled", "Cancelled", 0, true)?;
            return Ok(());
        }

        // Collapse provider duplicates, then preserve the existing deterministic
        // Deep Research ranking behavior before adding graph candidates.
        candidates = deduplicate_candidates(candidates);
        self.emit_progress(
            vault_id,
            run_id,
            &sequence,
            "filtering",
            None,
            "Filtering candidates".to_string(),
            Some(format!("{} unique", candidates.len())),
            candidates.len() as u32,
        );
        self.emit_progress(
            vault_id,
            run_id,
            &sequence,
            "ranking",
            None,
            "Ranking retrieved papers".to_string(),
            Some(format!("{} candidates", candidates.len())),
            candidates.len() as u32,
        );
        let windowed = if self.reranker.is_ready() && candidates.len() > SEMANTIC_RERANK_WINDOW {
            legacy_top_n(candidates, &profile, SEMANTIC_RERANK_WINDOW)
        } else {
            candidates
        };
        let semantic_scores = self.reranker.semantic_scores(&profile, &windowed).await;
        let (windowed, semantic_scores) = apply_semantic_floor(windowed, semantic_scores);
        let mut candidates: Vec<PaperCandidate> =
            rank_candidates(windowed, &profile, RETRIEVAL_POOL_LIMIT, &semantic_scores)
                .into_iter()
                .filter(|candidate| !candidate_is_blocked(candidate, &blocked))
                .collect();

        // Add bounded citation neighbours, then fuse them with vault similarity.
        self.update(
            vault_id,
            run_id,
            "ranking",
            "Expanding citation graph",
            0,
            false,
        )?;
        self.emit_progress(
            vault_id,
            run_id,
            &sequence,
            "graph",
            None,
            "Citation graph".to_string(),
            Some(format!(
                "{} vault seeds",
                vault_papers.len().min(GRAPH_SEED_LIMIT)
            )),
            0,
        );
        let graph = graph_candidates(&source, &vault_papers).await;
        let mut candidate_refs: HashSet<String> =
            candidates.iter().map(paper_candidate_dedup_key).collect();
        for (paper_ref, graph_candidate) in &graph.candidates {
            if candidate_in_year_range(graph_candidate, &options)
                && !candidate_is_blocked(graph_candidate, &blocked)
                && candidate_refs.insert(paper_ref.clone())
            {
                candidates.push(graph_candidate.clone());
            }
        }
        let centroid = self
            .store
            .vault_embedding_centroid(vault_id, MODEL_NAME, MODEL_VERSION)?;
        let semantic_scores = match centroid {
            Some(vector) => {
                self.reranker
                    .semantic_scores_against(&vector, &candidates)
                    .await
            }
            None => Vec::new(),
        };
        self.emit_progress(
            vault_id,
            run_id,
            &sequence,
            "ranking",
            None,
            "Ranking vault fit".to_string(),
            Some(format!("{} candidates", candidates.len())),
            candidates.len() as u32,
        );
        let suggestions = rank_suggestions(
            vault_id,
            run_id,
            candidates,
            &semantic_scores,
            &graph.arrivals,
            options.result_count as usize,
        );

        // Replace the inbox only after every stage succeeds, preserving old
        // suggestions when a refresh fails.
        self.store
            .replace_vault_suggestions(vault_id, run_id, &suggestions)?;
        let count = suggestions.len() as i32;
        let message = if count == 0 {
            "No new suggestions".to_string()
        } else {
            format!("{count} found")
        };
        self.store.set_vault_suggestion_run_status(
            run_id,
            "ready",
            &message,
            Some("selected_queries_completed"),
            None,
            count,
            true,
        )?;
        self.emit_progress(
            vault_id,
            run_id,
            &sequence,
            "complete",
            None,
            message.clone(),
            None,
            count as u32,
        );
        Ok(())
    }

    fn update(
        &self,
        vault_id: &str,
        run_id: &str,
        status: &str,
        message: &str,
        found: u32,
        finished: bool,
    ) -> Result<(), String> {
        self.store.set_vault_suggestion_run_status(
            run_id,
            status,
            message,
            None,
            None,
            found as i32,
            finished,
        )?;
        self.emit(vault_id, run_id, status, message, found);
        Ok(())
    }

    fn emit(&self, vault_id: &str, run_id: &str, status: &str, message: &str, found: u32) {
        let _ = self.app.emit(
            "vault_suggestion_updated",
            VaultSuggestionUpdated {
                vault_id: vault_id.to_string(),
                run_id: run_id.to_string(),
                status: status.to_string(),
                message: message.to_string(),
                found,
                sequence: 0,
                phase: status.to_string(),
                query_path: None,
                label: message.to_string(),
                detail: None,
            },
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn emit_progress(
        &self,
        vault_id: &str,
        run_id: &str,
        sequence: &AtomicU64,
        phase: &str,
        query_path: Option<String>,
        label: String,
        detail: Option<String>,
        found: u32,
    ) {
        let status = match phase {
            "complete" => "ready",
            "graph" | "filtering" | "ranking" => "ranking",
            _ => "searching",
        };
        let message = detail
            .as_ref()
            .map(|detail| format!("{label} · {detail}"))
            .unwrap_or_else(|| label.clone());
        let _ = self.app.emit(
            "vault_suggestion_updated",
            VaultSuggestionUpdated {
                vault_id: vault_id.to_string(),
                run_id: run_id.to_string(),
                status: status.to_string(),
                message,
                found,
                sequence: sequence.fetch_add(1, Ordering::Relaxed) + 1,
                phase: phase.to_string(),
                query_path,
                label,
                detail,
            },
        );
    }

    fn fail(&self, vault_id: &str, run_id: &str, error: &str) {
        eprintln!("[vault-suggestions] failed vault_id={vault_id} run_id={run_id} error={error}");
        let _ = self.store.set_vault_suggestion_run_status(
            run_id,
            "failed",
            "Suggestion run failed",
            None,
            Some(error),
            0,
            true,
        );
        self.emit(vault_id, run_id, "failed", error, 0);
    }
}

fn build_vault_profile(
    store: &LibraryStore,
    papers: &[&crate::domain::library::Paper],
    focus: Option<&str>,
) -> Result<String, String> {
    let mut profile = String::from("Find research papers related to this vault:\n");
    if let Some(focus) = focus.filter(|focus| !focus.trim().is_empty()) {
        profile.push_str("Focus within the vault: ");
        profile.push_str(focus.trim());
        profile.push('\n');
    }
    for paper in papers.iter().take(MAX_PAPERS_IN_PROFILE) {
        profile.push_str("\n- ");
        profile.push_str(&paper.title);
        if let Some(abstract_text) = paper.abstract_text.as_deref() {
            profile.push_str(": ");
            profile.extend(abstract_text.chars().take(500));
        }
        if let Some(extraction_id) = paper.active_extraction_id.as_deref() {
            for chunk in store
                .chunks_for_extraction(extraction_id)?
                .into_iter()
                .take(MAX_CHUNKS_PER_PAPER)
            {
                profile.push_str(" ");
                profile.extend(chunk.text.chars().take(350));
            }
        }
        if profile.len() >= MAX_PROFILE_CHARS {
            profile = profile.chars().take(MAX_PROFILE_CHARS).collect();
            break;
        }
    }
    Ok(profile)
}

fn papers_for_vault<'a>(
    snapshot: &'a crate::domain::library::LibrarySnapshot,
    vault_id: &str,
) -> Vec<&'a crate::domain::library::Paper> {
    let paper_ids: HashSet<&str> = snapshot
        .vault_papers
        .iter()
        .filter(|membership| membership.vault_id == vault_id)
        .map(|membership| membership.paper_id.as_str())
        .collect();
    snapshot
        .papers
        .iter()
        .filter(|paper| paper_ids.contains(paper.id.as_str()))
        .collect()
}

fn vault_revision(papers: &[&crate::domain::library::Paper]) -> String {
    let mut ids: Vec<&str> = papers.iter().map(|paper| paper.id.as_str()).collect();
    ids.sort_unstable();
    let digest = Sha256::digest(ids.join("\n"));
    digest[..8]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn query_plan_id(vault_id: &str, revision: &str) -> Result<String, String> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| error.to_string())?
        .as_nanos();
    let digest = Sha256::digest(format!("{vault_id}:{revision}:{now}"));
    Ok(format!(
        "vsp:{}",
        digest[..8]
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    ))
}

fn selected_query_paths(
    plan: &VaultSuggestionQueryPlan,
    selected_query_ids: &[String],
) -> Result<Vec<VaultSuggestionQueryPath>, String> {
    let selected_ids: HashSet<&str> = selected_query_ids.iter().map(String::as_str).collect();
    if selected_ids.is_empty() {
        return Err("Select at least one suggestion query".to_string());
    }
    let selected: Vec<VaultSuggestionQueryPath> = plan
        .queries
        .iter()
        .filter(|path| selected_ids.contains(path.id.as_str()))
        .cloned()
        .collect();
    if selected.len() != selected_ids.len() {
        return Err("One or more selected queries are not part of this plan".to_string());
    }
    Ok(selected)
}

fn suggestion_planner() -> Result<OpenRouterPlanner, String> {
    let chat = ChatConfig::load()?;
    Ok(OpenRouterPlanner::new(
        Client::new(),
        chat.url.clone(),
        chat.resolve_api_key()?,
        crate::services::settings::preference("model.planner")
            .unwrap_or_else(|| chat.model.clone()),
    ))
}

fn validate_options(mut options: VaultSuggestionOptions) -> Result<VaultSuggestionOptions, String> {
    options.focus = options
        .focus
        .map(|focus| focus.trim().to_string())
        .filter(|focus| !focus.is_empty());
    if options
        .focus
        .as_ref()
        .is_some_and(|focus| focus.len() > 500)
    {
        return Err("Suggestion focus must be 500 characters or fewer".to_string());
    }
    for year in [options.year_from, options.year_to].into_iter().flatten() {
        if !(1000..=2100).contains(&year) {
            return Err("Suggestion years must be between 1000 and 2100".to_string());
        }
    }
    if options
        .year_from
        .zip(options.year_to)
        .is_some_and(|(from, to)| from > to)
    {
        return Err("Suggestion start year cannot be after the end year".to_string());
    }
    if ![1, 3, 5].contains(&options.query_path_count) {
        return Err("Proposed query count must be 1, 3, or 5".to_string());
    }
    if ![3, 5, 10].contains(&options.result_count) {
        return Err("Suggestion result count must be 3, 5, or 10".to_string());
    }
    Ok(options)
}

fn provider_display_name(provider: &DiscoveryProviderChoice) -> &'static str {
    match provider {
        DiscoveryProviderChoice::OpenAlex => "OpenAlex",
        DiscoveryProviderChoice::Arxiv => "arXiv",
        DiscoveryProviderChoice::EuropePmc => "Europe PMC",
        DiscoveryProviderChoice::Core => "CORE",
    }
}

fn short_error(error: &str) -> String {
    error.chars().take(160).collect()
}

fn deduplicate_candidates(candidates: Vec<PaperCandidate>) -> Vec<PaperCandidate> {
    let mut seen = HashSet::new();
    candidates
        .into_iter()
        .filter(|candidate| seen.insert(paper_candidate_dedup_key(candidate)))
        .collect()
}

fn candidate_in_year_range(candidate: &PaperCandidate, options: &VaultSuggestionOptions) -> bool {
    candidate.year.is_none_or(|year| {
        options.year_from.is_none_or(|from| year >= from)
            && options.year_to.is_none_or(|to| year <= to)
    })
}

fn blocked_candidate_refs(
    papers: &[crate::domain::library::Paper],
    decided: Vec<String>,
) -> HashSet<String> {
    let mut blocked: HashSet<String> = decided.into_iter().collect();
    for paper in papers {
        blocked.insert(paper.id.to_lowercase());
        blocked.insert(normalized_title(&paper.title));
    }
    blocked
}

fn candidate_is_blocked(candidate: &PaperCandidate, blocked: &HashSet<String>) -> bool {
    blocked.contains(&paper_candidate_dedup_key(candidate))
        || blocked.contains(&candidate.id.to_lowercase())
        || blocked.contains(&normalized_title(&candidate.title))
}

fn normalized_title(title: &str) -> String {
    format!(
        "title:{}",
        title
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .to_lowercase()
    )
}

fn rank_suggestions(
    vault_id: &str,
    run_id: &str,
    candidates: Vec<PaperCandidate>,
    semantic_scores: &[f64],
    graph_arrivals: &HashMap<String, usize>,
    result_limit: usize,
) -> Vec<VaultSuggestion> {
    let deep: Vec<(String, f64)> = candidates
        .iter()
        .enumerate()
        .map(|(index, candidate)| (paper_candidate_dedup_key(candidate), -(index as f64)))
        .collect();
    let mut semantic: Vec<(String, f64)> = candidates
        .iter()
        .zip(semantic_scores.iter())
        .map(|(candidate, score)| (paper_candidate_dedup_key(candidate), *score))
        .collect();
    semantic.sort_by(|left, right| {
        right
            .1
            .partial_cmp(&left.1)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut citations: Vec<(String, f64)> = candidates
        .iter()
        .map(|candidate| {
            (
                paper_candidate_dedup_key(candidate),
                candidate.citation_count.unwrap_or(0) as f64,
            )
        })
        .collect();
    citations.sort_by(|left, right| {
        right
            .1
            .partial_cmp(&left.1)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut graph: Vec<(String, f64)> = graph_arrivals
        .iter()
        .map(|(paper_ref, arrivals)| (paper_ref.clone(), *arrivals as f64))
        .collect();
    graph.sort_by(|left, right| {
        right
            .1
            .partial_cmp(&left.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| left.0.cmp(&right.0))
    });
    let mut rankings = vec![deep, semantic];
    for _ in 0..GRAPH_RRF_WEIGHT {
        rankings.push(graph.clone());
    }
    rankings.push(citations);
    let fused = reciprocal_rank_fusion_many(&rankings);
    let semantic_by_ref: HashMap<String, f64> = candidates
        .iter()
        .zip(semantic_scores.iter())
        .map(|(candidate, score)| (paper_candidate_dedup_key(candidate), *score))
        .collect();
    let by_ref: HashMap<String, PaperCandidate> = candidates
        .into_iter()
        .map(|candidate| (paper_candidate_dedup_key(&candidate), candidate))
        .collect();

    fused
        .into_iter()
        .take(result_limit)
        .filter_map(|(paper_ref, score)| {
            let candidate = by_ref.get(&paper_ref)?.clone();
            Some(suggestion_from_candidate(
                vault_id,
                run_id,
                candidate,
                score,
                semantic_by_ref.get(&paper_ref).copied(),
                graph_arrivals.get(&paper_ref).copied(),
            ))
        })
        .collect()
}

fn suggestion_from_candidate(
    vault_id: &str,
    run_id: &str,
    candidate: PaperCandidate,
    score: f64,
    semantic_score: Option<f64>,
    graph_arrivals: Option<usize>,
) -> VaultSuggestion {
    let paper_ref = paper_candidate_dedup_key(&candidate);
    let reason = match (graph_arrivals, semantic_score) {
        (Some(arrivals), _) if arrivals > 1 => {
            format!("Reached from {arrivals} papers in this vault")
        }
        (Some(_), _) => "Citation neighbour of a paper in this vault".to_string(),
        (None, Some(similarity)) => format!("{:.0}% similar to this vault", similarity * 100.0),
        (None, None) => "Matches the topics and gaps in this vault".to_string(),
    };
    VaultSuggestion {
        id: suggestion_id(vault_id, &paper_ref),
        vault_id: vault_id.to_string(),
        run_id: run_id.to_string(),
        paper_ref,
        candidate,
        reason,
        score,
        state: "pending".to_string(),
        created_at: String::new(),
        updated_at: String::new(),
    }
}

#[derive(Default)]
struct GraphCandidates {
    candidates: HashMap<String, PaperCandidate>,
    arrivals: HashMap<String, usize>,
    origins: HashMap<String, HashSet<String>>,
}

/// Expand two bounded citation hops. Hop two starts only from papers reached
/// more than once, making repeated arrival both the quality signal and fan-out
/// guard described by RFC 0091.
async fn graph_candidates(
    source: &RealCandidateSource,
    papers: &[&crate::domain::library::Paper],
) -> GraphCandidates {
    let seed_ids: Vec<String> = papers
        .iter()
        .filter_map(|paper| openalex_work_id(&paper.id))
        .take(GRAPH_SEED_LIMIT)
        .collect();
    let mut graph = GraphCandidates::default();
    for seed_id in seed_ids {
        for lineage in [Lineage::References, Lineage::Citations] {
            match source
                .lineage(&seed_id, lineage, GRAPH_PER_SEED_LIMIT)
                .await
            {
                Ok(candidates) => {
                    absorb_graph_candidates(&mut graph, candidates, std::slice::from_ref(&seed_id))
                }
                Err(error) => eprintln!(
                    "[vault-suggestions] graph expansion failed seed={seed_id} error={error}"
                ),
            }
        }
    }

    let mut hop_two: Vec<(String, usize)> = graph
        .arrivals
        .iter()
        .filter(|(_, arrivals)| **arrivals > 1)
        .map(|(paper_ref, arrivals)| (paper_ref.clone(), *arrivals))
        .collect();
    hop_two.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
    let hop_two_ids: Vec<(String, Vec<String>)> = hop_two
        .into_iter()
        .take(GRAPH_HOP_TWO_SEEDS)
        .filter_map(|(paper_ref, _)| {
            let work_id = graph
                .candidates
                .get(&paper_ref)
                .and_then(|candidate| candidate.openalex_id.as_deref())
                .and_then(openalex_work_id)?;
            let origins = graph.origins.get(&paper_ref)?.iter().cloned().collect();
            Some((work_id, origins))
        })
        .collect();
    for (work_id, origins) in hop_two_ids {
        for lineage in [Lineage::References, Lineage::Citations] {
            if let Ok(candidates) = source.lineage(&work_id, lineage, 6).await {
                absorb_graph_candidates(&mut graph, candidates, &origins);
            }
        }
    }
    graph
}

fn absorb_graph_candidates(
    graph: &mut GraphCandidates,
    candidates: Vec<PaperCandidate>,
    origins: &[String],
) {
    for candidate in candidates {
        let paper_ref = paper_candidate_dedup_key(&candidate);
        let candidate_origins = graph.origins.entry(paper_ref.clone()).or_default();
        candidate_origins.extend(origins.iter().cloned());
        graph
            .arrivals
            .insert(paper_ref.clone(), candidate_origins.len());
        graph.candidates.entry(paper_ref).or_insert(candidate);
    }
}

fn openalex_work_id(value: &str) -> Option<String> {
    let tail = value
        .trim()
        .trim_start_matches("openalex:")
        .rsplit('/')
        .next()?;
    let upper = tail.to_ascii_uppercase();
    (upper.starts_with('W')
        && upper[1..]
            .chars()
            .all(|character| character.is_ascii_digit()))
    .then_some(upper)
}

fn suggestion_id(vault_id: &str, paper_ref: &str) -> String {
    let digest = Sha256::digest(format!("{vault_id}:{paper_ref}"));
    let short = digest[..8]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!("vs:{short}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::discovery::{CandidateMatch, PaperCandidate};

    fn candidate(id: &str, title: &str, citations: i32) -> PaperCandidate {
        PaperCandidate {
            id: id.to_string(),
            source_provider: "test".to_string(),
            source_id: id.to_string(),
            title: title.to_string(),
            authors: Vec::new(),
            abstract_text: None,
            year: None,
            publication_date: None,
            venue: None,
            citation_count: Some(citations),
            doi: None,
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
    fn owned_title_is_blocked_even_when_provider_id_differs() {
        let mut blocked = HashSet::new();
        blocked.insert(normalized_title("A Paper We Own"));
        assert!(candidate_is_blocked(
            &candidate("remote:1", "A Paper We Own", 0),
            &blocked
        ));
    }

    #[test]
    fn ranking_is_bounded_and_explained() {
        let candidates = (0..8)
            .map(|index| candidate(&format!("c{index}"), &format!("Paper {index}"), index))
            .collect();
        let suggestions = rank_suggestions("vault", "run", candidates, &[], &HashMap::new(), 5);
        assert_eq!(suggestions.len(), 5);
        assert!(suggestions.iter().all(|item| !item.reason.is_empty()));
    }

    #[test]
    fn repeated_graph_arrival_outranks_single_arrival() {
        let candidates = vec![
            candidate("single", "Single", 0),
            candidate("shared", "Shared", 0),
        ];
        let arrivals = HashMap::from([
            (normalized_title("Single"), 1_usize),
            (normalized_title("Shared"), 3_usize),
        ]);
        let suggestions = rank_suggestions("vault", "run", candidates, &[], &arrivals, 5);

        assert_eq!(suggestions[0].candidate.id, "shared");
        assert!(suggestions[0].reason.contains("3 papers"));
    }

    #[test]
    fn suggestion_options_reject_inverted_years() {
        let options = VaultSuggestionOptions {
            year_from: Some(2026),
            year_to: Some(2020),
            ..VaultSuggestionOptions::default()
        };

        assert!(validate_options(options).is_err());
    }

    #[test]
    fn candidate_deduplication_preserves_first_path_order() {
        let candidates = vec![
            candidate("first", "Shared", 1),
            candidate("duplicate", "Shared", 4),
            candidate("second", "Distinct", 2),
        ];

        let deduplicated = deduplicate_candidates(candidates);

        assert_eq!(deduplicated.len(), 2);
        assert_eq!(deduplicated[0].id, "first");
        assert_eq!(deduplicated[1].id, "second");
    }

    #[test]
    fn only_approved_query_paths_are_selected() {
        let plan = VaultSuggestionQueryPlan {
            id: "plan".to_string(),
            vault_id: "vault".to_string(),
            vault_revision: "revision".to_string(),
            queries: ["one", "two", "three"]
                .into_iter()
                .map(|id| VaultSuggestionQueryPath {
                    id: id.to_string(),
                    intent: id.to_string(),
                    query: format!("query {id}"),
                })
                .collect(),
        };

        let selected = selected_query_paths(&plan, &["two".to_string()]).expect("valid path");

        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].id, "two");
        assert!(selected_query_paths(&plan, &["unknown".to_string()]).is_err());
    }

    #[test]
    fn graph_arrivals_count_distinct_vault_papers() {
        let mut graph = GraphCandidates::default();
        absorb_graph_candidates(
            &mut graph,
            vec![candidate("shared", "Shared", 0)],
            &["seed-a".to_string()],
        );
        absorb_graph_candidates(
            &mut graph,
            vec![candidate("shared", "Shared", 0)],
            &["seed-a".to_string(), "seed-b".to_string()],
        );

        assert_eq!(graph.arrivals[&normalized_title("Shared")], 2);
    }
}
