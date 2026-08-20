//! Vault-scoped paper recommendations (RFC 0091).
//!
//! This manager reuses the RFC 0088 research loop without creating a Discover
//! search. It builds a bounded profile from the vault, filters owned and
//! dismissed papers, fuses Deep Research with local vault similarity and a
//! weak citation prior, then atomically replaces the five-row inbox.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use reqwest::Client;
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter};

use crate::commands::discovery::providers::{arxiv::ArxivProvider, openalex::OpenAlexProvider};
use crate::domain::discovery::{
    paper_candidate_dedup_key, DiscoveryProviderChoice, Lineage, PaperCandidate,
};
use crate::domain::research::{Depth, SearchConstraints};
use crate::domain::vault_suggestion::{
    VaultSuggestion, VaultSuggestionPreview, VaultSuggestionUpdated,
};
use crate::services::chat::config::ChatConfig;
use crate::services::embedding::{EmbeddingReranker, MODEL_NAME, MODEL_VERSION};
use crate::services::research::agent::{self, Progress, RunInputs};
use crate::services::research::budget::StopReason;
use crate::services::research::planner::OpenRouterPlanner;
use crate::services::research::source::RealCandidateSource;
use crate::services::search::fusion::reciprocal_rank_fusion_many;
use crate::storage::library_store::LibraryStore;

const MAX_SUGGESTIONS: usize = 5;
const MAX_PROFILE_CHARS: usize = 12_000;
const MAX_PAPERS_IN_PROFILE: usize = 24;
const MAX_CHUNKS_PER_PAPER: usize = 2;
const GRAPH_SEED_LIMIT: usize = 6;
const GRAPH_PER_SEED_LIMIT: i32 = 10;
const GRAPH_HOP_TWO_SEEDS: usize = 4;
/// Structural agreement is the distinctive vault signal. Repeating its rank
/// list makes RRF treat it as three independent arrivals without blending raw
/// counts with incomparable semantic and citation scores.
const GRAPH_RRF_WEIGHT: usize = 3;

type Cancellations = Arc<Mutex<HashMap<String, Arc<AtomicBool>>>>;

/// Owns manual suggestion runs and their transient progress events.
#[derive(Clone)]
pub struct VaultSuggestionManager {
    app: AppHandle,
    store: LibraryStore,
    reranker: EmbeddingReranker,
    active_vaults: Arc<Mutex<HashSet<String>>>,
    cancellations: Cancellations,
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
        }
    }

    /// Queue at most one due weekly run during startup.
    pub fn recover_and_queue_startup_run(&self) -> Result<(), String> {
        if !crate::services::settings::preference_bool("suggestions.weekly_enabled", true) {
            return Ok(());
        }
        if let Some(vault_id) = self.store.due_vault_for_weekly_suggestions()? {
            self.run(vault_id)?;
        }
        Ok(())
    }

    /// Queue one run and return its durable id immediately.
    pub fn run(&self, vault_id: String) -> Result<String, String> {
        let snapshot = self.store.get_library()?;
        let paper_count = snapshot
            .vault_papers
            .iter()
            .filter(|membership| membership.vault_id == vault_id)
            .count();
        if paper_count == 0 {
            return Err("No papers to match yet".to_string());
        }

        if !self
            .active_vaults
            .lock()
            .expect("suggestion run lock")
            .insert(vault_id.clone())
        {
            return Err("A suggestion run is already active for this vault".to_string());
        }
        let run = match self.store.create_vault_suggestion_run(&vault_id) {
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
            let result = manager.execute(&vault_id, &run_id_for_job, cancelled).await;
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
        cancelled: Arc<AtomicBool>,
    ) -> Result<(), String> {
        self.update(vault_id, run_id, "planning", "Planning", 0, false)?;
        let snapshot = self.store.get_library()?;
        let vault_paper_ids: HashSet<&str> = snapshot
            .vault_papers
            .iter()
            .filter(|membership| membership.vault_id == vault_id)
            .map(|membership| membership.paper_id.as_str())
            .collect();
        let vault_papers: Vec<_> = snapshot
            .papers
            .iter()
            .filter(|paper| vault_paper_ids.contains(paper.id.as_str()))
            .collect();
        let profile = build_vault_profile(&self.store, &vault_papers)?;
        let blocked = blocked_candidate_refs(
            &snapshot.papers,
            self.store.decided_vault_suggestion_refs(vault_id)?,
        );

        let chat = ChatConfig::load()?;
        let planner = OpenRouterPlanner::new(
            Client::new(),
            chat.url.clone(),
            chat.resolve_api_key()?,
            crate::services::settings::preference("model.planner")
                .unwrap_or_else(|| chat.model.clone()),
        );
        let source = RealCandidateSource::new(
            OpenAlexProvider::from_app_config().map_err(|error| error.to_string())?,
            ArxivProvider::from_app_config().map_err(|error| error.to_string())?,
        );
        let constraints = SearchConstraints {
            year_from: None,
            year_to: None,
            providers: vec![
                DiscoveryProviderChoice::OpenAlex,
                DiscoveryProviderChoice::Arxiv,
            ],
            open_access: false,
            target_count: 25,
            venues: Vec::new(),
            authors: Vec::new(),
            fields_of_study: Vec::new(),
            seed_paper_ids: vault_papers.iter().map(|paper| paper.id.clone()).collect(),
        };
        let strategy = Depth::Standard.budget();

        let app = self.app.clone();
        let vault = vault_id.to_string();
        let run = run_id.to_string();
        let blocked_for_preview = blocked.clone();
        let seen_previews = Arc::new(Mutex::new(HashSet::<String>::new()));
        let seen_for_callback = seen_previews.clone();
        let on_progress = move |progress: Progress| match progress {
            Progress::CandidatePreview { candidates } => {
                let mut seen = seen_for_callback.lock().expect("preview lock");
                let preview: Vec<VaultSuggestion> = candidates
                    .into_iter()
                    .filter(|candidate| !candidate_is_blocked(candidate, &blocked_for_preview))
                    .filter(|candidate| seen.insert(paper_candidate_dedup_key(candidate)))
                    .take(MAX_SUGGESTIONS)
                    .map(|candidate| {
                        suggestion_from_candidate(&vault, &run, candidate, 0.0, None, None)
                    })
                    .collect();
                if !preview.is_empty() {
                    let _ = app.emit(
                        "vault_suggestion_preview",
                        VaultSuggestionPreview {
                            vault_id: vault.clone(),
                            run_id: run.clone(),
                            suggestions: preview,
                        },
                    );
                }
            }
            other => {
                let (status, message, found) = describe_progress(&other);
                let _ = app.emit(
                    "vault_suggestion_updated",
                    VaultSuggestionUpdated {
                        vault_id: vault.clone(),
                        run_id: run.clone(),
                        status: status.to_string(),
                        message,
                        found,
                    },
                );
            }
        };

        let outcome = agent::run(
            &planner,
            &source,
            &self.reranker,
            RunInputs {
                goal: &profile,
                constraints: &constraints,
                strategy: &strategy,
                existing_keys: HashSet::new(),
            },
            &cancelled,
            on_progress,
        )
        .await
        .map_err(|error| error.to_string())?;

        if outcome.stop_reason == StopReason::Cancelled {
            self.update(vault_id, run_id, "cancelled", "Cancelled", 0, true)?;
            return Ok(());
        }

        self.update(
            vault_id,
            run_id,
            "ranking",
            "Expanding citation graph",
            0,
            false,
        )?;
        let graph = graph_candidates(&source, &vault_papers).await;
        let mut candidates: Vec<PaperCandidate> = outcome
            .ranked
            .into_iter()
            .map(|ranked| ranked.candidate)
            .filter(|candidate| !candidate_is_blocked(candidate, &blocked))
            .collect();
        let mut candidate_refs: HashSet<String> =
            candidates.iter().map(paper_candidate_dedup_key).collect();
        for (paper_ref, graph_candidate) in &graph.candidates {
            if !candidate_is_blocked(graph_candidate, &blocked)
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
        let suggestions = rank_suggestions(
            vault_id,
            run_id,
            candidates,
            &semantic_scores,
            &graph.arrivals,
        );
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
            Some(outcome.stop_reason.as_str()),
            None,
            count,
            true,
        )?;
        self.emit(vault_id, run_id, "ready", &message, count as u32);
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
) -> Result<String, String> {
    let mut profile = String::from("Find research papers related to this vault:\n");
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
        .take(MAX_SUGGESTIONS)
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

fn describe_progress(progress: &Progress) -> (&'static str, String, u32) {
    match progress {
        Progress::Planning { .. } => ("planning", "Planning".to_string(), 0),
        Progress::Searching { .. } => ("searching", "Searching".to_string(), 0),
        Progress::SearchResult { count, .. } => (
            "searching",
            format!("Searching · {count} found"),
            *count as u32,
        ),
        Progress::SearchFailed { .. } => ("searching", "Searching other sources".to_string(), 0),
        Progress::Deduped { unique } => ("searching", format!("{unique} unique"), *unique as u32),
        Progress::Assessing => ("assessing", "Assessing coverage".to_string(), 0),
        Progress::Ranking { count } => ("ranking", "Ranking".to_string(), *count as u32),
        Progress::CandidatePreview { candidates } => (
            "searching",
            "Searching".to_string(),
            candidates.len() as u32,
        ),
    }
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
        let suggestions = rank_suggestions("vault", "run", candidates, &[], &HashMap::new());
        assert_eq!(suggestions.len(), MAX_SUGGESTIONS);
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
        let suggestions = rank_suggestions("vault", "run", candidates, &[], &arrivals);

        assert_eq!(suggestions[0].candidate.id, "shared");
        assert!(suggestions[0].reason.contains("3 papers"));
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
