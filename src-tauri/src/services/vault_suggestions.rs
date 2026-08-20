//! Vault-scoped paper recommendations (RFC 0091, RFC 0094 and RFC 0095).
//!
//! The manager prepares a reviewable query agenda without contacting paper
//! providers. Once the user approves paths, it searches them concurrently,
//! retains path evidence, adds citation neighbours, applies strict relevance
//! gates and atomically replaces the inbox.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use futures_util::stream::{self, StreamExt};
use reqwest::Client;
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter, Manager};
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
use crate::services::embedding::{cosine_similarity, EmbeddingReranker, MODEL_NAME, MODEL_VERSION};
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
const DEFAULT_NEAREST_PAPER_SIMILARITY_FLOOR: f64 = 0.35;
const DEFAULT_FOCUS_SIMILARITY_FLOOR: f64 = 0.30;

type Cancellations = Arc<Mutex<HashMap<String, Arc<AtomicBool>>>>;

#[derive(Clone)]
struct PendingQueryPlan {
    plan: VaultSuggestionQueryPlan,
    options: VaultSuggestionOptions,
}

/// Relevance thresholds loaded from `[vault_suggestions]` in `i0i.config.toml`.
#[derive(Debug, Clone)]
struct SuggestionRankingConfig {
    nearest_paper_similarity_floor: f64,
    focus_similarity_floor: f64,
}

impl Default for SuggestionRankingConfig {
    fn default() -> Self {
        Self {
            nearest_paper_similarity_floor: DEFAULT_NEAREST_PAPER_SIMILARITY_FLOOR,
            focus_similarity_floor: DEFAULT_FOCUS_SIMILARITY_FLOOR,
        }
    }
}

/// One candidate occurrence within one provider/path result list.
#[derive(Debug, Clone)]
struct RetrievalHit {
    candidate: PaperCandidate,
    path_id: String,
    path_intent: String,
    search_key: String,
    rank: usize,
}

/// One candidate's rank within a stable provider/path result list.
#[derive(Debug, Clone)]
struct RetrievalEvidence {
    search_key: String,
    rank: usize,
}

/// The strongest semantic relationship between a candidate and one vault paper.
#[derive(Debug, Clone)]
struct PaperSimilarity {
    paper_id: String,
    paper_title: String,
    score: f64,
}

/// All deterministic evidence retained for one deduplicated candidate.
#[derive(Debug, Clone)]
struct CandidateEvidence {
    candidate: PaperCandidate,
    retrieval_hits: Vec<RetrievalEvidence>,
    path_intents: BTreeMap<String, String>,
    graph_origins: BTreeSet<String>,
    nearest_paper: Option<PaperSimilarity>,
    focus_similarity: Option<f64>,
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

        let mut retrieval_hits = Vec::new();
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
                        .filter(|candidate| candidate_in_year_range(candidate, &options))
                        .filter(|candidate| {
                            options.include_reviews || !is_review_title(&candidate.title)
                        })
                        .filter(|candidate| {
                            seen_previews.insert(paper_candidate_dedup_key(candidate))
                        })
                        .take(options.result_count as usize)
                        .cloned()
                        .map(|candidate| preview_suggestion(vault_id, run_id, candidate))
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
                    let search_key = format!("{}:{}", path.id, provider_name.to_lowercase());
                    retrieval_hits.extend(found.into_iter().enumerate().map(
                        |(rank, candidate)| RetrievalHit {
                            candidate,
                            path_id: path.id.clone(),
                            path_intent: path.intent.clone(),
                            search_key: search_key.clone(),
                            rank,
                        },
                    ));
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
                retrieval_hits.len() as i32,
                false,
            )?;
        }

        if cancelled.load(Ordering::Relaxed) {
            self.update(vault_id, run_id, "cancelled", "Cancelled", 0, true)?;
            return Ok(());
        }

        // Collapse provider duplicates, then preserve the existing deterministic
        // Deep Research ranking behavior before adding graph candidates.
        let mut evidence = merge_retrieval_hits(retrieval_hits);
        self.emit_progress(
            vault_id,
            run_id,
            &sequence,
            "filtering",
            None,
            "Filtering candidates".to_string(),
            Some(format!("{} unique", evidence.len())),
            evidence.len() as u32,
        );
        self.emit_progress(
            vault_id,
            run_id,
            &sequence,
            "ranking",
            None,
            "Ranking retrieved papers".to_string(),
            Some(format!("{} candidates", evidence.len())),
            evidence.len() as u32,
        );
        let candidates: Vec<PaperCandidate> =
            evidence.iter().map(|item| item.candidate.clone()).collect();
        let windowed = if self.reranker.is_ready() && candidates.len() > SEMANTIC_RERANK_WINDOW {
            legacy_top_n(candidates, &profile, SEMANTIC_RERANK_WINDOW)
        } else {
            candidates
        };
        let semantic_scores = self.reranker.semantic_scores(&profile, &windowed).await;
        let (windowed, semantic_scores) = apply_semantic_floor(windowed, semantic_scores);
        let ranked_candidates: Vec<PaperCandidate> =
            rank_candidates(windowed, &profile, RETRIEVAL_POOL_LIMIT, &semantic_scores)
                .into_iter()
                .filter(|candidate| !candidate_is_blocked(candidate, &blocked))
                .collect();
        let mut evidence_by_ref: HashMap<String, CandidateEvidence> = evidence
            .drain(..)
            .map(|item| (paper_candidate_dedup_key(&item.candidate), item))
            .collect();
        evidence = ranked_candidates
            .into_iter()
            .filter(|candidate| candidate_in_year_range(candidate, &options))
            .filter_map(|candidate| evidence_by_ref.remove(&paper_candidate_dedup_key(&candidate)))
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
        let mut candidate_refs: HashSet<String> = evidence
            .iter()
            .map(|item| paper_candidate_dedup_key(&item.candidate))
            .collect();
        for (paper_ref, graph_candidate) in &graph.candidates {
            if !candidate_in_year_range(graph_candidate, &options)
                || candidate_is_blocked(graph_candidate, &blocked)
            {
                continue;
            }
            let origins: BTreeSet<String> = graph
                .origins
                .get(paper_ref)
                .into_iter()
                .flatten()
                .cloned()
                .collect();
            if let Some(item) = evidence
                .iter_mut()
                .find(|item| paper_candidate_dedup_key(&item.candidate) == *paper_ref)
            {
                item.graph_origins.extend(origins);
            } else if candidate_refs.insert(paper_ref.clone()) {
                evidence.push(CandidateEvidence {
                    candidate: graph_candidate.clone(),
                    retrieval_hits: Vec::new(),
                    path_intents: BTreeMap::new(),
                    graph_origins: origins,
                    nearest_paper: None,
                    focus_similarity: None,
                });
            }
        }

        let candidates: Vec<PaperCandidate> =
            evidence.iter().map(|item| item.candidate.clone()).collect();
        let paper_centroids =
            self.store
                .vault_paper_embedding_centroids(vault_id, MODEL_NAME, MODEL_VERSION)?;
        let candidate_embeddings = self.reranker.candidate_embeddings(&candidates).await;
        if candidate_embeddings.len() == candidates.len() && !paper_centroids.is_empty() {
            let titles: HashMap<&str, &str> = vault_papers
                .iter()
                .map(|paper| (paper.id.as_str(), paper.title.as_str()))
                .collect();
            for (item, candidate_embedding) in evidence.iter_mut().zip(&candidate_embeddings) {
                item.nearest_paper =
                    nearest_paper_similarity(candidate_embedding, &paper_centroids, &titles);
            }
        } else {
            self.emit_progress(
                vault_id,
                run_id,
                &sequence,
                "ranking",
                None,
                "Semantic evidence unavailable".to_string(),
                Some("Using graph and query-path agreement".to_string()),
                evidence.len() as u32,
            );
        }

        let focus = options
            .focus
            .as_deref()
            .filter(|value| !value.trim().is_empty());
        if let Some(focus) = focus {
            let Some(focus_embedding) = self.reranker.text_embedding(focus).await else {
                return Err(
                    "Focus similarity is unavailable; previous suggestions were preserved"
                        .to_string(),
                );
            };
            if candidate_embeddings.len() != evidence.len() {
                return Err(
                    "Focus similarity is unavailable; previous suggestions were preserved"
                        .to_string(),
                );
            }
            for (item, candidate_embedding) in evidence.iter_mut().zip(&candidate_embeddings) {
                item.focus_similarity =
                    Some(cosine_similarity(&focus_embedding, candidate_embedding).clamp(0.0, 1.0));
            }
        }
        self.emit_progress(
            vault_id,
            run_id,
            &sequence,
            "ranking",
            None,
            "Ranking vault fit".to_string(),
            Some(format!("{} candidates", evidence.len())),
            evidence.len() as u32,
        );
        let ranking_config = SuggestionRankingConfig::load(&self.app);
        let suggestions = rank_suggestions(
            vault_id,
            run_id,
            evidence,
            &ranking_config,
            focus,
            options.include_reviews,
            options.result_count as usize,
        );

        // Replace the inbox only after every stage succeeds, preserving old
        // suggestions when a refresh fails.
        self.store
            .replace_vault_suggestions(vault_id, run_id, &suggestions)?;
        let count = suggestions.len() as i32;
        let message = if count == 0 {
            "No strong suggestions".to_string()
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

impl SuggestionRankingConfig {
    /// Load optional ranking thresholds from the normal application config.
    fn load(app: &AppHandle) -> Self {
        let mut config = Self::default();
        for path in suggestion_config_paths(app) {
            if let Ok(contents) = fs::read_to_string(path) {
                config.apply_toml_like_overrides(&contents);
                break;
            }
        }
        config
    }

    /// Apply valid threshold values from the vault suggestion section.
    fn apply_toml_like_overrides(&mut self, contents: &str) {
        let mut in_section = false;
        for line in contents.lines().map(str::trim) {
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if line.starts_with('[') && line.ends_with(']') {
                in_section = line == "[vault_suggestions]";
                continue;
            }
            if !in_section {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let Ok(value) = value.trim().trim_matches('"').parse::<f64>() else {
                continue;
            };
            if !(0.0..=1.0).contains(&value) {
                continue;
            }
            match key.trim() {
                "nearest_paper_similarity_floor" => {
                    self.nearest_paper_similarity_floor = value;
                }
                "focus_similarity_floor" => self.focus_similarity_floor = value,
                _ => {}
            }
        }
    }
}

/// Resolve local and platform application config locations in priority order.
fn suggestion_config_paths(app: &AppHandle) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Ok(cwd) = std::env::current_dir() {
        paths.push(cwd.join("i0i.config.toml"));
    }
    if let Ok(config_dir) = app.path().app_config_dir() {
        paths.push(config_dir.join("i0i.config.toml"));
    }
    paths
}

/// Merge provider duplicates without losing path-level retrieval evidence.
fn merge_retrieval_hits(mut hits: Vec<RetrievalHit>) -> Vec<CandidateEvidence> {
    hits.sort_by(|left, right| {
        paper_candidate_dedup_key(&left.candidate)
            .cmp(&paper_candidate_dedup_key(&right.candidate))
            .then_with(|| left.search_key.cmp(&right.search_key))
            .then_with(|| left.rank.cmp(&right.rank))
            .then_with(|| left.candidate.id.cmp(&right.candidate.id))
    });

    let mut merged: BTreeMap<String, CandidateEvidence> = BTreeMap::new();
    for hit in hits {
        let paper_ref = paper_candidate_dedup_key(&hit.candidate);
        let item = merged
            .entry(paper_ref)
            .or_insert_with(|| CandidateEvidence {
                candidate: hit.candidate.clone(),
                retrieval_hits: Vec::new(),
                path_intents: BTreeMap::new(),
                graph_origins: BTreeSet::new(),
                nearest_paper: None,
                focus_similarity: None,
            });
        merge_candidate_metadata(&mut item.candidate, &hit.candidate);
        item.path_intents
            .insert(hit.path_id.clone(), hit.path_intent);
        if !item
            .retrieval_hits
            .iter()
            .any(|evidence| evidence.search_key == hit.search_key)
        {
            item.retrieval_hits.push(RetrievalEvidence {
                search_key: hit.search_key,
                rank: hit.rank,
            });
        }
    }
    merged.into_values().collect()
}

/// Keep the richest useful metadata while merging duplicate provider records.
fn merge_candidate_metadata(target: &mut PaperCandidate, incoming: &PaperCandidate) {
    if incoming.authors.len() > target.authors.len() {
        target.authors.clone_from(&incoming.authors);
    }
    if incoming.abstract_text.as_ref().map_or(0, String::len)
        > target.abstract_text.as_ref().map_or(0, String::len)
    {
        target.abstract_text.clone_from(&incoming.abstract_text);
    }
    if target.year.is_none() {
        target.year = incoming.year;
    }
    if target.venue.is_none() {
        target.venue.clone_from(&incoming.venue);
    }
    target.citation_count = target.citation_count.max(incoming.citation_count);
    if target.doi.is_none() {
        target.doi.clone_from(&incoming.doi);
    }
    if target.openalex_id.is_none() {
        target.openalex_id.clone_from(&incoming.openalex_id);
    }
    if target.arxiv_id.is_none() {
        target.arxiv_id.clone_from(&incoming.arxiv_id);
    }
    if target.external_url.is_none() {
        target.external_url.clone_from(&incoming.external_url);
    }
    if target.pdf_url.is_none() {
        target.pdf_url.clone_from(&incoming.pdf_url);
    }
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

/// Filter and fuse candidate evidence into the final bounded recommendation list.
fn rank_suggestions(
    vault_id: &str,
    run_id: &str,
    evidence: Vec<CandidateEvidence>,
    config: &SuggestionRankingConfig,
    focus: Option<&str>,
    include_reviews: bool,
    result_limit: usize,
) -> Vec<VaultSuggestion> {
    let eligible: Vec<CandidateEvidence> = evidence
        .into_iter()
        .filter(|item| include_reviews || !is_review_title(&item.candidate.title))
        .filter(|item| candidate_clears_relevance_floor(item, config, focus.is_some()))
        .collect();
    if eligible.is_empty() {
        return Vec::new();
    }

    let mut rankings = vec![merged_retrieval_rank(&eligible)];
    rankings.push(sorted_rank(&eligible, |item| {
        item.nearest_paper
            .as_ref()
            .map(|similarity| similarity.score)
    }));
    if focus.is_some() {
        rankings.push(sorted_rank(&eligible, |item| item.focus_similarity));
    }
    rankings.push(sorted_rank(&eligible, |item| {
        (!item.graph_origins.is_empty()).then_some(item.graph_origins.len() as f64)
    }));
    if eligible.iter().any(|item| item.path_intents.len() > 1) {
        rankings.push(sorted_rank(&eligible, |item| {
            (!item.path_intents.is_empty()).then_some(item.path_intents.len() as f64)
        }));
    }
    rankings.retain(|ranking| !ranking.is_empty());

    let fused = reciprocal_rank_fusion_many(&rankings);
    let by_ref: HashMap<String, CandidateEvidence> = eligible
        .into_iter()
        .map(|item| (paper_candidate_dedup_key(&item.candidate), item))
        .collect();

    fused
        .into_iter()
        .take(result_limit)
        .filter_map(|(paper_ref, score)| {
            let item = by_ref.get(&paper_ref)?;
            Some(suggestion_from_evidence(
                vault_id, run_id, item, config, focus, score,
            ))
        })
        .collect()
}

/// Convert ranked evidence into the durable suggestion shown by the UI.
fn suggestion_from_evidence(
    vault_id: &str,
    run_id: &str,
    evidence: &CandidateEvidence,
    config: &SuggestionRankingConfig,
    focus: Option<&str>,
    score: f64,
) -> VaultSuggestion {
    let candidate = evidence.candidate.clone();
    let paper_ref = paper_candidate_dedup_key(&candidate);
    let reason = suggestion_reason(evidence, config, focus);
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

/// Build a provisional row while the remaining retrieval work continues.
fn preview_suggestion(vault_id: &str, run_id: &str, candidate: PaperCandidate) -> VaultSuggestion {
    let paper_ref = paper_candidate_dedup_key(&candidate);
    VaultSuggestion {
        id: suggestion_id(vault_id, &paper_ref),
        vault_id: vault_id.to_string(),
        run_id: run_id.to_string(),
        paper_ref,
        candidate,
        reason: "Awaiting final vault ranking".to_string(),
        score: 0.0,
        state: "pending".to_string(),
        created_at: String::new(),
        updated_at: String::new(),
    }
}

/// Apply the strict vault and optional Focus eligibility thresholds.
fn candidate_clears_relevance_floor(
    evidence: &CandidateEvidence,
    config: &SuggestionRankingConfig,
    focus_required: bool,
) -> bool {
    let vault_connection = !evidence.graph_origins.is_empty()
        || evidence
            .nearest_paper
            .as_ref()
            .is_some_and(|similarity| similarity.score >= config.nearest_paper_similarity_floor)
        || evidence.path_intents.len() >= 2;
    let focus_matches = !focus_required
        || evidence
            .focus_similarity
            .is_some_and(|score| score >= config.focus_similarity_floor);
    vault_connection && focus_matches
}

/// Merge provider/path rankings into one deterministic retrieval signal.
fn merged_retrieval_rank(evidence: &[CandidateEvidence]) -> Vec<(String, f64)> {
    let mut by_search: BTreeMap<&str, Vec<(String, f64)>> = BTreeMap::new();
    for item in evidence {
        let paper_ref = paper_candidate_dedup_key(&item.candidate);
        for hit in &item.retrieval_hits {
            by_search
                .entry(&hit.search_key)
                .or_default()
                .push((paper_ref.clone(), -(hit.rank as f64)));
        }
    }
    for ranking in by_search.values_mut() {
        ranking.sort_by(|left, right| {
            right
                .1
                .partial_cmp(&left.1)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| left.0.cmp(&right.0))
        });
    }
    reciprocal_rank_fusion_many(&by_search.into_values().collect::<Vec<_>>())
}

/// Sort candidates by one optional native signal with stable identity ties.
fn sorted_rank(
    evidence: &[CandidateEvidence],
    score: impl Fn(&CandidateEvidence) -> Option<f64>,
) -> Vec<(String, f64)> {
    let mut ranking: Vec<(String, f64)> = evidence
        .iter()
        .filter_map(|item| Some((paper_candidate_dedup_key(&item.candidate), score(item)?)))
        .collect();
    ranking.sort_by(|left, right| {
        right
            .1
            .partial_cmp(&left.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| left.0.cmp(&right.0))
    });
    ranking
}

/// Find the vault paper with the highest cosine similarity to a candidate.
fn nearest_paper_similarity(
    candidate_embedding: &[f32],
    paper_centroids: &[(String, Vec<f32>)],
    titles: &HashMap<&str, &str>,
) -> Option<PaperSimilarity> {
    paper_centroids
        .iter()
        .filter_map(|(paper_id, centroid)| {
            let paper_title = titles.get(paper_id.as_str())?;
            Some(PaperSimilarity {
                paper_id: paper_id.clone(),
                paper_title: (*paper_title).to_string(),
                score: cosine_similarity(candidate_embedding, centroid).clamp(0.0, 1.0),
            })
        })
        .max_by(|left, right| {
            left.score
                .partial_cmp(&right.score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| right.paper_id.cmp(&left.paper_id))
        })
}

/// Explain the strongest concrete relationship that admitted a suggestion.
fn suggestion_reason(
    evidence: &CandidateEvidence,
    config: &SuggestionRankingConfig,
    focus: Option<&str>,
) -> String {
    if evidence.graph_origins.len() > 1 {
        return format!(
            "Connected to {} papers in this vault",
            evidence.graph_origins.len()
        );
    }
    let nearest = evidence
        .nearest_paper
        .as_ref()
        .filter(|nearest| nearest.score >= config.nearest_paper_similarity_floor);
    if let (Some(focus), Some(nearest)) = (focus, nearest) {
        return format!(
            "Matches focus \"{}\" and \"{}\"",
            focus.trim(),
            nearest.paper_title
        );
    }
    if evidence.graph_origins.len() == 1 {
        return match focus {
            Some(focus) => format!(
                "Matches focus \"{}\" and a vault citation path",
                focus.trim()
            ),
            None => "Citation neighbour of a paper in this vault".to_string(),
        };
    }
    if let Some(nearest) = nearest {
        return format!(
            "Most similar to \"{}\" ({:.0}%)",
            nearest.paper_title,
            nearest.score * 100.0
        );
    }
    let intents: Vec<&str> = evidence.path_intents.values().map(String::as_str).collect();
    format!(
        "Found through {} paths: {}",
        intents.len(),
        intents.join(" + ")
    )
}

/// Identify explicit review terminology using normalized whole phrases.
fn is_review_title(title: &str) -> bool {
    let normalized = title
        .chars()
        .map(|character| {
            if character.is_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let padded = format!(" {normalized} ");
    [
        " survey ",
        " review ",
        " meta analysis ",
        " bibliometric analysis ",
    ]
    .iter()
    .any(|phrase| padded.contains(phrase))
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
    use serde::Deserialize;

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct RankingCorpus {
        vaults: Vec<RankingVaultFixture>,
    }

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct RankingVaultFixture {
        name: String,
        include_reviews: bool,
        focus: Option<String>,
        candidates: Vec<RankingCandidateFixture>,
    }

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct RankingCandidateFixture {
        id: String,
        title: String,
        label: String,
        citations: i32,
        legacy_rank: usize,
        centroid: f64,
        nearest: f64,
        focus_score: Option<f64>,
        path_count: usize,
        graph_origins: usize,
    }

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
    fn suggestion_options_reject_inverted_years() {
        let options = VaultSuggestionOptions {
            year_from: Some(2026),
            year_to: Some(2020),
            ..VaultSuggestionOptions::default()
        };

        assert!(validate_options(options).is_err());
    }

    #[test]
    fn final_year_filter_rejects_provider_drift() {
        let mut old = candidate("old", "Old Paper", 0);
        old.year = Some(2010);
        let options = VaultSuggestionOptions {
            year_from: Some(2020),
            ..VaultSuggestionOptions::default()
        };

        assert!(!candidate_in_year_range(&old, &options));
    }

    #[test]
    fn candidate_deduplication_preserves_distinct_path_evidence() {
        let hits = vec![
            retrieval_hit(
                candidate("first", "Shared", 1),
                "path-a",
                "methods",
                "oa",
                0,
            ),
            retrieval_hit(
                candidate("duplicate", "Shared", 4),
                "path-b",
                "applications",
                "arxiv",
                1,
            ),
            retrieval_hit(
                candidate("second", "Distinct", 2),
                "path-a",
                "methods",
                "oa",
                2,
            ),
        ];

        let deduplicated = merge_retrieval_hits(hits);

        assert_eq!(deduplicated.len(), 2);
        let shared = deduplicated
            .iter()
            .find(|evidence| evidence.candidate.title == "Shared")
            .expect("shared candidate");
        assert_eq!(shared.path_intents.len(), 2);
        assert_eq!(shared.retrieval_hits.len(), 2);
        assert_eq!(shared.candidate.citation_count, Some(4));
    }

    #[test]
    fn ranking_is_independent_of_provider_arrival_order() {
        let hits = vec![
            retrieval_hit(
                candidate("a", "Specific Method", 1),
                "path-a",
                "methods",
                "oa",
                0,
            ),
            retrieval_hit(
                candidate("b", "General Survey", 500),
                "path-b",
                "overview",
                "arxiv",
                0,
            ),
            retrieval_hit(
                candidate("a2", "Specific Method", 1),
                "path-b",
                "overview",
                "arxiv",
                1,
            ),
        ];
        let mut reversed = hits.clone();
        reversed.reverse();

        let score = |evidence: &mut [CandidateEvidence]| {
            for item in evidence {
                item.nearest_paper = Some(PaperSimilarity {
                    paper_id: "seed".to_string(),
                    paper_title: "Seed Paper".to_string(),
                    score: if item.candidate.title == "Specific Method" {
                        0.82
                    } else {
                        0.61
                    },
                });
            }
        };
        let mut first = merge_retrieval_hits(hits);
        let mut second = merge_retrieval_hits(reversed);
        score(&mut first);
        score(&mut second);
        let config = SuggestionRankingConfig::default();

        let first_ids: Vec<String> =
            rank_suggestions("vault", "run", first, &config, None, false, 5)
                .into_iter()
                .map(|suggestion| suggestion.candidate.id)
                .collect();
        let second_ids: Vec<String> =
            rank_suggestions("vault", "run", second, &config, None, false, 5)
                .into_iter()
                .map(|suggestion| suggestion.candidate.id)
                .collect();

        assert_eq!(first_ids, second_ids);
    }

    #[test]
    fn strict_relevance_floor_returns_fewer_than_the_limit() {
        let mut strong = evidence(candidate("strong", "Strong", 0));
        strong.nearest_paper = Some(PaperSimilarity {
            paper_id: "seed".to_string(),
            paper_title: "Seed".to_string(),
            score: 0.8,
        });
        let mut weak = evidence(candidate("weak", "Weak", 10_000));
        weak.nearest_paper = Some(PaperSimilarity {
            paper_id: "seed".to_string(),
            paper_title: "Seed".to_string(),
            score: 0.1,
        });

        let suggestions = rank_suggestions(
            "vault",
            "run",
            vec![strong, weak],
            &SuggestionRankingConfig::default(),
            None,
            false,
            5,
        );

        assert_eq!(suggestions.len(), 1);
        assert_eq!(suggestions[0].candidate.id, "strong");
    }

    #[test]
    fn review_filter_is_explicitly_reversible() {
        let mut review = evidence(candidate("review", "A Systematic Review of Attention", 0));
        review.graph_origins.insert("seed".to_string());
        let config = SuggestionRankingConfig::default();

        assert!(rank_suggestions(
            "vault",
            "run",
            vec![review.clone()],
            &config,
            None,
            false,
            5
        )
        .is_empty());
        assert_eq!(
            rank_suggestions("vault", "run", vec![review], &config, None, true, 5).len(),
            1
        );
    }

    #[test]
    fn review_classifier_matches_words_not_substrings() {
        assert!(is_review_title("A Survey of Sparse Attention"));
        assert!(is_review_title("Transformer Meta-Analysis"));
        assert!(!is_review_title("Preview-Aware Video Encoding"));
    }

    #[test]
    fn focus_is_an_independent_relevance_gate() {
        let mut item = evidence(candidate("candidate", "Candidate", 0));
        item.graph_origins.insert("seed".to_string());
        item.focus_similarity = Some(0.1);
        let config = SuggestionRankingConfig::default();

        assert!(rank_suggestions(
            "vault",
            "run",
            vec![item.clone()],
            &config,
            Some("empirical methods"),
            false,
            5,
        )
        .is_empty());

        item.focus_similarity = Some(0.8);
        assert_eq!(
            rank_suggestions(
                "vault",
                "run",
                vec![item],
                &config,
                Some("empirical methods"),
                false,
                5,
            )
            .len(),
            1
        );
    }

    #[test]
    fn nearest_paper_similarity_keeps_the_specific_origin() {
        let titles = HashMap::from([("paper-a", "Paper A"), ("paper-b", "Paper B")]);
        let centroids = vec![
            ("paper-a".to_string(), vec![1.0, 0.0]),
            ("paper-b".to_string(), vec![0.0, 1.0]),
        ];

        let nearest =
            nearest_paper_similarity(&[0.1, 0.9], &centroids, &titles).expect("nearest paper");

        assert_eq!(nearest.paper_id, "paper-b");
        assert_eq!(nearest.paper_title, "Paper B");
        assert!(nearest.score > 0.9);
    }

    #[test]
    fn ranking_thresholds_load_from_the_vault_suggestions_section() {
        let mut config = SuggestionRankingConfig::default();
        config.apply_toml_like_overrides(
            "[vault_suggestions]\nnearest_paper_similarity_floor = 0.42\nfocus_similarity_floor = 0.51\n",
        );

        assert_eq!(config.nearest_paper_similarity_floor, 0.42);
        assert_eq!(config.focus_similarity_floor, 0.51);
    }

    #[test]
    fn fixed_corpus_improves_precision_without_harming_review_vaults() {
        let corpus: RankingCorpus = serde_json::from_str(include_str!(
            "../../tests/fixtures/vault_suggestion_ranking.json"
        ))
        .expect("ranking corpus");
        let config = SuggestionRankingConfig::default();

        for vault in corpus.vaults {
            let labels: HashMap<String, String> = vault
                .candidates
                .iter()
                .map(|item| (item.id.clone(), item.label.clone()))
                .collect();
            let baseline = legacy_fixture_rank(&vault.candidates, 5);
            let evidence = vault.candidates.iter().map(fixture_evidence).collect();
            let proposed: Vec<String> = rank_suggestions(
                "vault",
                "run",
                evidence,
                &config,
                vault.focus.as_deref(),
                vault.include_reviews,
                5,
            )
            .into_iter()
            .map(|suggestion| suggestion.candidate.id)
            .collect();
            let baseline_precision = precision(&baseline, &labels);
            let proposed_precision = precision(&proposed, &labels);

            if vault.name == "review-useful-biomedicine" {
                assert!(proposed_precision >= baseline_precision, "{}", vault.name);
                assert!(proposed.contains(&"useful-review".to_string()));
            } else {
                assert!(proposed_precision > baseline_precision, "{}", vault.name);
            }
        }
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

    fn retrieval_hit(
        candidate: PaperCandidate,
        path_id: &str,
        path_intent: &str,
        provider: &str,
        rank: usize,
    ) -> RetrievalHit {
        RetrievalHit {
            candidate,
            path_id: path_id.to_string(),
            path_intent: path_intent.to_string(),
            search_key: format!("{path_id}:{provider}"),
            rank,
        }
    }

    fn evidence(candidate: PaperCandidate) -> CandidateEvidence {
        CandidateEvidence {
            candidate,
            retrieval_hits: Vec::new(),
            path_intents: Default::default(),
            graph_origins: Default::default(),
            nearest_paper: None,
            focus_similarity: None,
        }
    }

    fn fixture_evidence(fixture: &RankingCandidateFixture) -> CandidateEvidence {
        let mut item = evidence(candidate(&fixture.id, &fixture.title, fixture.citations));
        for index in 0..fixture.path_count {
            let path_id = format!("path-{index}");
            item.path_intents
                .insert(path_id.clone(), format!("angle {index}"));
            item.retrieval_hits.push(RetrievalEvidence {
                search_key: format!("{path_id}:fixture"),
                rank: fixture.legacy_rank,
            });
        }
        for index in 0..fixture.graph_origins {
            item.graph_origins.insert(format!("seed-{index}"));
        }
        item.nearest_paper = Some(PaperSimilarity {
            paper_id: "seed".to_string(),
            paper_title: "Closest vault paper".to_string(),
            score: fixture.nearest,
        });
        item.focus_similarity = fixture.focus_score;
        item
    }

    fn legacy_fixture_rank(candidates: &[RankingCandidateFixture], limit: usize) -> Vec<String> {
        let retrieval = fixture_rank(candidates, |item| -(item.legacy_rank as f64));
        let semantic = fixture_rank(candidates, |item| item.centroid);
        let graph = fixture_rank(candidates, |item| item.graph_origins as f64);
        let citations = fixture_rank(candidates, |item| item.citations as f64);
        reciprocal_rank_fusion_many(&[
            retrieval,
            semantic,
            graph.clone(),
            graph.clone(),
            graph,
            citations,
        ])
        .into_iter()
        .take(limit)
        .map(|(id, _)| id)
        .collect()
    }

    fn fixture_rank(
        candidates: &[RankingCandidateFixture],
        score: impl Fn(&RankingCandidateFixture) -> f64,
    ) -> Vec<(String, f64)> {
        let mut ranking: Vec<(String, f64)> = candidates
            .iter()
            .map(|item| (item.id.clone(), score(item)))
            .collect();
        ranking.sort_by(|left, right| {
            right
                .1
                .partial_cmp(&left.1)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| left.0.cmp(&right.0))
        });
        ranking
    }

    fn precision(ids: &[String], labels: &HashMap<String, String>) -> f64 {
        let relevant = ids
            .iter()
            .filter(|id| labels.get(*id).is_some_and(|label| label != "irrelevant"))
            .count();
        relevant as f64 / ids.len().max(1) as f64
    }
}
