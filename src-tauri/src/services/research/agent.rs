//! The orchestrated agent loop (RFC 0037, reshaped by RFC 0088).
//!
//! Rust drives `plan | reflect → search → filter → dedup → rank`, bounded by the
//! strategy budget, with the LLM able to break early via `reflect`.
//!
//! Three things RFC 0088 took from the harnesses surveyed there:
//!
//! - **One reflection call, not two** (R1.1). `assess` and `refine_queries` did
//!   overlapping work — one decided whether to stop, the other wrote the next
//!   queries from gaps computed separately. `reflect` returns both.
//! - **Retire a provider that stops paying** (R1.4, from jina-ai). `new_count`
//!   was already computed and discarded; it now decides.
//! - **Narrow as the run deepens** (R1.5, from dzhng). The query allowance
//!   halves each round instead of casting the same wide net every time.
//!
//! The loop is free of I/O side effects: it depends on the `Planner` and
//! `CandidateSource` seams, takes a cancellation flag and a progress callback,
//! and returns an outcome. Persistence and event emission live in the manager,
//! so the loop is unit-testable with fakes (no DB, no network).

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use futures_util::future::join_all;

use crate::commands::discovery::orchestrator::{
    apply_semantic_floor, legacy_top_n, rank_candidates, SEMANTIC_FLOOR, SEMANTIC_RERANK_WINDOW,
};
use crate::domain::discovery::{DiscoveryProviderChoice, PaperCandidate};
use crate::domain::research::{
    candidate_dedup_key, RankedCandidate, SearchConstraints, SearchStrategy,
};
use crate::services::embedding::EmbeddingReranker;
use crate::services::research::budget::{
    evaluate_stop, provider_is_paying, query_budget_for_round, BudgetRemaining, BudgetUsage,
    ProviderStats, StopReason, BASE_QUERIES_PER_ROUND,
};
use crate::services::research::dedup::{dedup, diff};
use crate::services::research::error::ResearchError;
use crate::services::research::filter::apply_constraints;
use crate::services::research::planner::{
    CandidateDetail, CoverageStats, Planner, PoolEntry, PoolSummary, Query,
};
use crate::services::research::source::CandidateSource;
use crate::services::research::source::SourceProgress;

/// Maximum abstracts included in one reflection prompt (RFC 0088 R6.3).
const REFLECTION_DETAIL_LIMIT: usize = 8;

/// Inputs for one run.
pub struct RunInputs<'a> {
    pub goal: &'a str,
    pub constraints: &'a SearchConstraints,
    pub strategy: &'a SearchStrategy,
    /// Dedup keys already in the saved search's pool (for stacking).
    pub existing_keys: HashSet<String>,
}

/// What a run produced.
pub struct RunOutcome {
    /// Ranked *new* candidates (not already in the pool) to stack in.
    pub ranked: Vec<RankedCandidate>,
    pub stop_reason: StopReason,
    pub iterations: u32,
    /// RFC 0088 R6.2: did the run finish, or did it run out? A spent budget
    /// still returns its pool, but the caller must be able to say so.
    pub complete: bool,
    /// RFC 0088 R6.4: per-round record, for the evaluation fixtures.
    pub trace: Vec<RoundTrace>,
    /// Actual bounded-loop consumption, persisted by the manager.
    pub usage: BudgetUsage,
}

/// One round, as the evaluation harness sees it (RFC 0088 R6.4).
#[derive(Debug, Clone, PartialEq)]
pub struct RoundTrace {
    pub round: u32,
    pub queries_issued: u32,
    pub queries_skipped_retired: u32,
    pub candidates_after_dedup: usize,
    pub new_candidates: u32,
    pub gaps: Vec<String>,
    pub should_continue: bool,
}

/// Progress signals emitted as the loop runs (mapped to events by the manager).
#[derive(Debug, Clone)]
pub enum Progress {
    Planning { iteration: u32 },
    Searching { provider: String, text: String },
    SearchResult { provider: String, count: usize },
    SearchFailed { provider: String, error: String },
    Deduped { unique: usize },
    CandidatePreview { candidates: Vec<PaperCandidate> },
    Resolving { count: usize },
    Assessing,
    Ranking { count: usize },
}

/// Run the bounded agent loop. `on` receives progress signals; `cancelled` is
/// checked between iterations and before each LLM/provider call.
pub async fn run<P, S>(
    planner: &P,
    source: &S,
    reranker: &EmbeddingReranker,
    inputs: RunInputs<'_>,
    cancelled: &AtomicBool,
    on: impl FnMut(Progress) + Send,
) -> Result<RunOutcome, ResearchError>
where
    P: Planner,
    S: CandidateSource,
{
    let on = Mutex::new(on);
    let strategy = inputs.strategy;
    let constraints = inputs.constraints;
    let target = constraints.target_count.max(0) as u32;

    let mut pool: Vec<PaperCandidate> = Vec::new();
    let mut usage = BudgetUsage::default();
    let mut stop_reason = StopReason::MaxIterations;
    let mut trace: Vec<RoundTrace> = Vec::new();
    // R1.4: one tally per provider, keyed by the label the loop already builds.
    let mut provider_stats: HashMap<String, ProviderStats> = HashMap::new();
    // Queries `reflect` asked for last round; empty on round 0, where `plan` runs.
    let mut planned: Vec<Query> = Vec::new();

    'run: for iteration in 0..strategy.max_iterations {
        if cancelled.load(Ordering::Relaxed) {
            stop_reason = StopReason::Cancelled;
            break;
        }
        if let Some(reason) = evaluate_stop(&usage, strategy, target) {
            stop_reason = reason;
            break;
        }

        emit_progress(&on, Progress::Planning { iteration });
        let queries = if iteration == 0 {
            let planned = planner.plan_queries(inputs.goal, constraints).await?;
            usage.llm_calls += 1;
            planned
        } else {
            // R1.1: `reflect` already wrote these at the end of the previous
            // round, in the same call that decided to continue.
            std::mem::take(&mut planned)
        };

        // R1.5: the allowance narrows as the run deepens.
        let allowance = query_budget_for_round(iteration, BASE_QUERIES_PER_ROUND) as usize;
        let mut queries_issued = 0u32;
        let mut queries_skipped_retired = 0u32;
        let before_round = pool.len();
        let known_before_round = pool.iter().map(candidate_dedup_key).collect::<HashSet<_>>();
        // Per-provider deltas for this round, folded into the tallies after it.
        let mut round_found: HashMap<String, (u32, u32, HashSet<String>)> = HashMap::new();

        let browser_transport = source.transport_name();
        let mut remaining_queries = queries.into_iter().take(allowance).collect::<Vec<_>>();

        if let Some(transport) = browser_transport {
            let budget_left = strategy
                .max_provider_queries
                .saturating_sub(usage.provider_queries) as usize;
            remaining_queries.truncate(budget_left);
            queries_issued = remaining_queries.len() as u32;
            usage.provider_queries += queries_issued;
            round_found
                .entry(transport.to_string())
                .or_insert_with(|| (0, 0, HashSet::new()))
                .0 += queries_issued;

            // Browser navigation dominates elapsed time. Planned lanes are
            // independent, so pay that latency once per round while retaining
            // the existing round and provider-query budgets.
            let progress_sink = &on;
            let searches = remaining_queries.drain(..).map(|query| {
                // Browser is the discovery transport. Provider choices remain
                // available to its exact-metadata resolution stage.
                let query_constraints = constraints.clone();
                let provider = transport.to_string();
                emit_progress(
                    progress_sink,
                    Progress::Searching {
                        provider: provider.clone(),
                        text: query.text.clone(),
                    },
                );
                async move {
                    let result = source
                        .search_with_progress(&query, &query_constraints, &|source_progress| {
                            match source_progress {
                                SourceProgress::SearchingWeb => {}
                                SourceProgress::Provisional(candidates) => emit_progress(
                                    progress_sink,
                                    Progress::CandidatePreview { candidates },
                                ),
                                SourceProgress::ResolvingMetadata(count) => {
                                    emit_progress(progress_sink, Progress::Resolving { count })
                                }
                                SourceProgress::Resolved(count) => emit_progress(
                                    progress_sink,
                                    Progress::SearchResult {
                                        provider: provider.clone(),
                                        count,
                                    },
                                ),
                            }
                        })
                        .await;
                    (provider, result)
                }
            });

            for (provider, result) in join_all(searches).await {
                match result {
                    Ok(mut found) => {
                        emit_progress(
                            &on,
                            Progress::SearchResult {
                                provider: provider.clone(),
                                count: found.len(),
                            },
                        );
                        let tally = round_found
                            .entry(provider)
                            .or_insert_with(|| (0, 0, HashSet::new()));
                        tally.1 += found.len() as u32;
                        tally.2.extend(found.iter().map(candidate_dedup_key));
                        pool.append(&mut found);
                    }
                    Err(error) => emit_progress(
                        &on,
                        Progress::SearchFailed {
                            provider,
                            error: error.to_string(),
                        },
                    ),
                }
            }
        }

        for query in remaining_queries {
            if cancelled.load(Ordering::Relaxed) {
                stop_reason = StopReason::Cancelled;
                break 'run;
            }
            let (active_providers, skipped) =
                active_providers(&query.provider, constraints, &provider_stats);
            queries_skipped_retired += skipped;
            if active_providers.is_empty() {
                continue;
            }

            let provider_call_cost = active_providers.len() as u32;
            if usage.provider_queries + provider_call_cost > strategy.max_provider_queries {
                break;
            }
            let provider = active_providers
                .iter()
                .map(provider_name)
                .collect::<Vec<_>>()
                .join("+");
            let mut query_constraints = constraints.clone();
            if !constraints.providers.is_empty() {
                query_constraints.providers = active_providers.clone();
            }
            for choice in &active_providers {
                round_found
                    .entry(provider_name(choice).to_string())
                    .or_insert_with(|| (0, 0, HashSet::new()))
                    .0 += 1;
            }
            emit_progress(
                &on,
                Progress::Searching {
                    provider: provider.clone(),
                    text: query.text.clone(),
                },
            );
            match source
                .search_with_progress(&query, &query_constraints, &|progress| match progress {
                    SourceProgress::SearchingWeb => {}
                    SourceProgress::Provisional(candidates) => {
                        emit_progress(&on, Progress::CandidatePreview { candidates })
                    }
                    SourceProgress::ResolvingMetadata(count) => {
                        emit_progress(&on, Progress::Resolving { count })
                    }
                    SourceProgress::Resolved(count) => emit_progress(
                        &on,
                        Progress::SearchResult {
                            provider: "web".to_string(),
                            count,
                        },
                    ),
                })
                .await
            {
                Ok(mut found) => {
                    emit_progress(
                        &on,
                        Progress::SearchResult {
                            provider: provider.clone(),
                            count: found.len(),
                        },
                    );
                    for candidate in &found {
                        let tally = candidate_provider(candidate);
                        if let Some(entry) = round_found.get_mut(tally) {
                            entry.1 += 1;
                            entry.2.insert(candidate_dedup_key(candidate));
                        }
                    }
                    pool.append(&mut found);
                }
                Err(error) => {
                    emit_progress(
                        &on,
                        Progress::SearchFailed {
                            provider: provider.clone(),
                            error: error.to_string(),
                        },
                    );
                }
            }
            queries_issued += 1;
            usage.provider_queries += provider_call_cost;
        }

        pool = apply_constraints(pool, constraints);
        pool = dedup(pool);
        emit_progress(&on, Progress::Deduped { unique: pool.len() });
        if !pool.is_empty() {
            emit_progress(
                &on,
                Progress::CandidatePreview {
                    candidates: pool.clone(),
                },
            );
        }
        usage.candidate_count = new_count(&pool, &inputs.existing_keys);

        // What the round actually added, after dedup — the number R1.4 needs.
        let round_gain = pool.len().saturating_sub(before_round) as u32;
        for (provider, (queries, candidates, returned_keys)) in round_found {
            let provider_gain = returned_keys.difference(&known_before_round).count() as u32;
            provider_stats.entry(provider).or_default().record_round(
                queries,
                candidates,
                provider_gain,
            );
        }

        if cancelled.load(Ordering::Relaxed) {
            stop_reason = StopReason::Cancelled;
            break;
        }

        emit_progress(&on, Progress::Assessing);
        let summary = build_pool_summary(&pool, reranker, inputs.goal).await;
        usage.iterations += 1;
        let remaining = BudgetRemaining::from_usage(&usage, strategy);
        let reflection = planner.reflect(inputs.goal, &summary, &remaining).await?;
        usage.llm_calls += 1;

        trace.push(RoundTrace {
            round: iteration,
            queries_issued,
            queries_skipped_retired,
            candidates_after_dedup: pool.len(),
            new_candidates: round_gain,
            gaps: reflection.gaps.clone(),
            should_continue: reflection.should_continue,
        });

        // The target is a hard success condition even when it is reached on
        // the final allowed round and reflection would otherwise continue.
        if target > 0 && usage.candidate_count >= target {
            stop_reason = StopReason::TargetReached;
            break;
        }
        if !reflection.should_continue {
            // R1.3: converged — the pool answers the goal — which is a different
            // outcome from running out of rounds.
            stop_reason = StopReason::Converged;
            break;
        }
        planned = reflection.next_queries;
    }

    // Stacking: rank only the candidates not already in the saved pool.
    let new_candidates = diff(pool, &inputs.existing_keys);
    emit_progress(
        &on,
        Progress::Ranking {
            count: new_candidates.len(),
        },
    );
    // Rank by meaning (RFC 0057). Bound the embedding set to the legacy-top-N,
    // score each candidate's semantic similarity to the goal, drop off-topic
    // results below the floor, then rank with the semantic-aware weights. When
    // the reranker is off/unavailable, `semantic_scores` is empty and every step
    // degrades to today's legacy ranking.
    let windowed = if reranker.is_ready() && new_candidates.len() > SEMANTIC_RERANK_WINDOW {
        legacy_top_n(new_candidates, inputs.goal, SEMANTIC_RERANK_WINDOW)
    } else {
        new_candidates
    };
    let semantic = reranker.semantic_scores(inputs.goal, &windowed).await;
    let (windowed, semantic) = apply_semantic_floor(windowed, semantic);
    let ranked = rank_candidates(windowed, inputs.goal, constraints.target_count, &semantic)
        .into_iter()
        .enumerate()
        .map(|(index, candidate)| RankedCandidate {
            score: candidate.match_summary.score,
            rationale: Some("ranked by deterministic search signals".to_string()),
            rank_signals_json: None,
            provider_hits_json: None,
            candidate,
            rank: (index + 1) as i32,
        })
        .collect();

    Ok(RunOutcome {
        ranked,
        stop_reason,
        iterations: usage.iterations,
        complete: stop_reason.is_complete(),
        trace,
        usage,
    })
}

fn emit_progress<F>(on: &Mutex<F>, progress: Progress)
where
    F: FnMut(Progress),
{
    let mut callback = on.lock().expect("research progress callback lock");
    callback(progress);
}

/// Build the bounded, abstract-aware summary used by the reflection call.
async fn build_pool_summary(
    pool: &[PaperCandidate],
    reranker: &EmbeddingReranker,
    goal: &str,
) -> PoolSummary {
    let semantic_scores = reranker.semantic_scores(goal, pool).await;
    let has_semantic_scores = !pool.is_empty() && semantic_scores.len() == pool.len();

    let entries = pool
        .iter()
        .enumerate()
        .map(|(index, candidate)| PoolEntry {
            title: candidate.title.clone(),
            year: candidate.year,
            venue: candidate.venue.clone(),
            semantic_score: has_semantic_scores.then(|| semantic_scores[index]),
        })
        .collect::<Vec<_>>();

    let mut borderline_indices = if has_semantic_scores {
        (0..pool.len()).collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    borderline_indices.sort_by(|left, right| {
        let left_distance = (semantic_scores[*left] - SEMANTIC_FLOOR).abs();
        let right_distance = (semantic_scores[*right] - SEMANTIC_FLOOR).abs();
        left_distance.total_cmp(&right_distance)
    });
    let detailed = borderline_indices
        .into_iter()
        .filter_map(|index| {
            let candidate = &pool[index];
            let abstract_text = candidate.abstract_text.as_deref()?.trim();
            if abstract_text.is_empty() {
                return None;
            }
            Some(CandidateDetail {
                title: candidate.title.clone(),
                year: candidate.year,
                abstract_text: abstract_text.to_string(),
            })
        })
        .take(REFLECTION_DETAIL_LIMIT)
        .collect();

    let years = pool.iter().filter_map(|candidate| candidate.year);
    let year_min = years.clone().min();
    let year_max = years.max();
    let coverage = CoverageStats {
        total: pool.len(),
        below_floor: if has_semantic_scores {
            semantic_scores
                .iter()
                .filter(|score| **score < SEMANTIC_FLOOR)
                .count()
        } else {
            0
        },
        mean_score: has_semantic_scores
            .then(|| semantic_scores.iter().sum::<f64>() / semantic_scores.len().max(1) as f64),
        year_min,
        year_max,
    };

    PoolSummary {
        entries,
        detailed,
        coverage,
    }
}

fn new_count(pool: &[PaperCandidate], existing: &HashSet<String>) -> u32 {
    pool.iter()
        .filter(|c| !existing.contains(&candidate_dedup_key(c)))
        .count() as u32
}

/// Return only providers that have not exhausted their marginal value.
fn active_providers(
    query_provider: &DiscoveryProviderChoice,
    constraints: &SearchConstraints,
    stats: &HashMap<String, ProviderStats>,
) -> (Vec<DiscoveryProviderChoice>, u32) {
    let requested = if constraints.providers.is_empty() {
        vec![*query_provider]
    } else {
        constraints.providers.clone()
    };
    let mut skipped = 0;
    let active = requested
        .into_iter()
        .filter(|provider| {
            let paying = match stats.get(provider_name(provider)) {
                Some(provider_stats) => provider_is_paying(provider_stats),
                None => true,
            };
            if !paying {
                skipped += 1;
            }
            paying
        })
        .collect();
    (active, skipped)
}

fn provider_name(provider: &DiscoveryProviderChoice) -> &'static str {
    match provider {
        DiscoveryProviderChoice::OpenAlex => "open_alex",
        DiscoveryProviderChoice::Arxiv => "arxiv",
        DiscoveryProviderChoice::EuropePmc => "europe_pmc",
        DiscoveryProviderChoice::Core => "core",
    }
}

/// Normalize provider labels carried by candidates to the run's tally keys.
fn candidate_provider(candidate: &PaperCandidate) -> &str {
    match candidate.source_provider.as_str() {
        "openalex" => "open_alex",
        "europepmc" => "europe_pmc",
        provider => provider,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicU32;
    use std::sync::Arc;

    use async_trait::async_trait;

    use crate::domain::discovery::{CandidateMatch, DiscoveryProviderChoice, PaperCandidate};
    use crate::domain::research::Depth;
    use crate::services::embedding::TextEmbedder;
    use crate::services::research::planner::{Query, Reflection};

    fn candidate(title: &str, doi: &str) -> PaperCandidate {
        PaperCandidate {
            id: title.to_string(),
            source_provider: "openalex".to_string(),
            source_id: title.to_string(),
            title: title.to_string(),
            authors: Vec::new(),
            abstract_text: Some("abstract".to_string()),
            year: Some(2024),
            publication_date: None,
            venue: None,
            citation_count: None,
            doi: Some(doi.to_string()),
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

    /// Returns a fixed batch each search; `refine_budget` controls how many
    /// reflections request another round before declaring convergence.
    struct FakePlanner {
        refine_budget: AtomicU32,
        plan_calls: AtomicU32,
        query_count: usize,
    }

    impl FakePlanner {
        fn new(refine_budget: u32) -> Self {
            Self {
                refine_budget: AtomicU32::new(refine_budget),
                plan_calls: AtomicU32::new(0),
                query_count: 1,
            }
        }

        fn with_query_count(mut self, query_count: usize) -> Self {
            self.query_count = query_count;
            self
        }

        fn queries(&self, prefix: &str) -> Vec<Query> {
            (0..self.query_count)
                .map(|index| Query {
                    provider: DiscoveryProviderChoice::OpenAlex,
                    text: format!("{prefix}-{index}"),
                })
                .collect()
        }
    }

    #[async_trait]
    impl Planner for FakePlanner {
        async fn plan_queries(
            &self,
            _goal: &str,
            _constraints: &SearchConstraints,
        ) -> Result<Vec<Query>, ResearchError> {
            self.plan_calls.fetch_add(1, Ordering::Relaxed);
            Ok(self.queries("q"))
        }

        async fn reflect(
            &self,
            _goal: &str,
            _pool: &PoolSummary,
            _remaining: &BudgetRemaining,
        ) -> Result<Reflection, ResearchError> {
            let remaining = self.refine_budget.load(Ordering::Relaxed);
            if remaining > 0 {
                self.refine_budget.fetch_sub(1, Ordering::Relaxed);
                Ok(Reflection {
                    should_continue: true,
                    gaps: vec!["a gap".to_string()],
                    next_queries: self.queries("refined"),
                })
            } else {
                Ok(Reflection {
                    should_continue: false,
                    gaps: Vec::new(),
                    next_queries: Vec::new(),
                })
            }
        }

        async fn rank(
            &self,
            _goal: &str,
            candidates: &[PaperCandidate],
        ) -> Result<Vec<RankedCandidate>, ResearchError> {
            Ok(candidates
                .iter()
                .enumerate()
                .map(|(i, c)| RankedCandidate {
                    candidate: c.clone(),
                    rank: (i + 1) as i32,
                    score: Some(1.0),
                    rationale: Some("ok".to_string()),
                    rank_signals_json: None,
                    provider_hits_json: None,
                })
                .collect())
        }
    }

    struct FakeSource {
        batch: Vec<PaperCandidate>,
        calls: AtomicU32,
    }

    impl FakeSource {
        fn new(batch: Vec<PaperCandidate>) -> Self {
            Self {
                batch,
                calls: AtomicU32::new(0),
            }
        }
    }

    #[async_trait]
    impl CandidateSource for FakeSource {
        async fn search(
            &self,
            _query: &Query,
            _constraints: &SearchConstraints,
        ) -> Result<Vec<PaperCandidate>, ResearchError> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            Ok(self.batch.clone())
        }
    }

    /// Returns one never-before-seen paper per query.
    struct GrowingSource {
        next: AtomicU32,
    }

    #[async_trait]
    impl CandidateSource for GrowingSource {
        async fn search(
            &self,
            _query: &Query,
            _constraints: &SearchConstraints,
        ) -> Result<Vec<PaperCandidate>, ResearchError> {
            let index = self.next.fetch_add(1, Ordering::Relaxed);
            Ok(vec![candidate(
                &format!("Paper {index}"),
                &format!("10/growing-{index}"),
            )])
        }
    }

    struct ConcurrentBrowserSource {
        active: AtomicU32,
        max_active: AtomicU32,
    }

    #[async_trait]
    impl CandidateSource for ConcurrentBrowserSource {
        fn transport_name(&self) -> Option<&'static str> {
            Some("web")
        }

        async fn search(
            &self,
            query: &Query,
            _constraints: &SearchConstraints,
        ) -> Result<Vec<PaperCandidate>, ResearchError> {
            let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
            self.max_active.fetch_max(active, Ordering::SeqCst);
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            self.active.fetch_sub(1, Ordering::SeqCst);
            Ok(vec![candidate(
                &query.text,
                &format!("10/concurrent-{}", query.text),
            )])
        }
    }

    fn constraints(target: i32) -> SearchConstraints {
        SearchConstraints {
            year_from: None,
            year_to: None,
            providers: Vec::new(),
            open_access: false,
            target_count: target,
            venues: Vec::new(),
            authors: Vec::new(),
            fields_of_study: Vec::new(),
            seed_paper_ids: Vec::new(),
        }
    }

    async fn run_loop(
        planner: &FakePlanner,
        source: &FakeSource,
        constraints: &SearchConstraints,
        strategy: &SearchStrategy,
        existing: HashSet<String>,
        cancelled: &AtomicBool,
    ) -> RunOutcome {
        let inputs = RunInputs {
            goal: "goal",
            constraints,
            strategy,
            existing_keys: existing,
        };
        run(
            planner,
            source,
            &EmbeddingReranker::disabled(),
            inputs,
            cancelled,
            |_| {},
        )
        .await
        .expect("loop ok")
    }

    #[tokio::test]
    async fn happy_path_dedups_and_ranks() {
        // batch has a duplicate DOI -> dedup to 2 unique.
        let batch = vec![
            candidate("Alpha", "10/a"),
            candidate("Alpha copy", "10/a"),
            candidate("Beta", "10/b"),
        ];
        let planner = FakePlanner::new(0); // reflect: converge immediately
        let source = FakeSource::new(batch);
        let outcome = run_loop(
            &planner,
            &source,
            &constraints(20),
            &Depth::Standard.budget(),
            HashSet::new(),
            &AtomicBool::new(false),
        )
        .await;
        assert_eq!(outcome.stop_reason, StopReason::Converged);
        assert!(outcome.complete);
        assert_eq!(outcome.ranked.len(), 2);
        assert_eq!(outcome.ranked[0].rank, 1);
    }

    #[tokio::test]
    async fn browser_queries_in_one_round_run_concurrently() {
        let planner = FakePlanner::new(0).with_query_count(3);
        let source = ConcurrentBrowserSource {
            active: AtomicU32::new(0),
            max_active: AtomicU32::new(0),
        };
        let search_constraints = constraints(20);
        let strategy = Depth::Standard.budget();
        let inputs = RunInputs {
            goal: "goal",
            constraints: &search_constraints,
            strategy: &strategy,
            existing_keys: HashSet::new(),
        };

        let outcome = run(
            &planner,
            &source,
            &EmbeddingReranker::disabled(),
            inputs,
            &AtomicBool::new(false),
            |_| {},
        )
        .await
        .expect("browser run");

        assert_eq!(outcome.ranked.len(), 3);
        assert!(source.max_active.load(Ordering::SeqCst) >= 2);
    }

    #[tokio::test]
    async fn emits_preview_after_deduping_candidate_pool() {
        let batch = vec![
            candidate("Alpha", "10/a"),
            candidate("Alpha copy", "10/a"),
            candidate("Beta", "10/b"),
        ];
        let planner = FakePlanner::new(0);
        let source = FakeSource::new(batch);
        let constraints = constraints(20);
        let strategy = Depth::Standard.budget();
        let inputs = RunInputs {
            goal: "goal",
            constraints: &constraints,
            strategy: &strategy,
            existing_keys: HashSet::new(),
        };
        let mut events = Vec::new();

        run(
            &planner,
            &source,
            &EmbeddingReranker::disabled(),
            inputs,
            &AtomicBool::new(false),
            |progress| events.push(progress),
        )
        .await
        .expect("loop ok");

        let preview_sizes = events
            .iter()
            .filter_map(|event| match event {
                Progress::CandidatePreview { candidates } => Some(candidates.len()),
                _ => None,
            })
            .collect::<Vec<_>>();

        assert_eq!(preview_sizes, vec![2]);
    }

    #[tokio::test]
    async fn refine_drives_a_second_round_then_stops() {
        let batch = vec![candidate("Alpha", "10/a")];
        let planner = FakePlanner::new(1); // refine once, then stop
        let source = FakeSource::new(batch);
        let outcome = run_loop(
            &planner,
            &source,
            &constraints(20),
            &Depth::Standard.budget(),
            HashSet::new(),
            &AtomicBool::new(false),
        )
        .await;
        assert_eq!(outcome.stop_reason, StopReason::Converged);
        assert_eq!(planner.plan_calls.load(Ordering::Relaxed), 1); // round 2 used reflection's queries
        assert_eq!(outcome.iterations, 2);
        assert_eq!(outcome.trace.len(), 2);
        assert_eq!(outcome.trace[0].gaps, vec!["a gap"]);
        assert!(!outcome.trace[1].should_continue);
    }

    #[tokio::test]
    async fn cancellation_stops_the_run() {
        let batch = vec![candidate("Alpha", "10/a")];
        let planner = FakePlanner::new(5);
        let source = FakeSource::new(batch);
        let outcome = run_loop(
            &planner,
            &source,
            &constraints(20),
            &Depth::Standard.budget(),
            HashSet::new(),
            &AtomicBool::new(true), // pre-cancelled
        )
        .await;
        assert_eq!(outcome.stop_reason, StopReason::Cancelled);
    }

    #[tokio::test]
    async fn budget_exhaustion_reports_max_iterations() {
        let batch = vec![candidate("Alpha", "10/a")];
        let planner = FakePlanner::new(99); // never satisfied
        let source = FakeSource::new(batch);
        // target 0 = unbounded, so only the iteration ceiling stops it.
        let outcome = run_loop(
            &planner,
            &source,
            &constraints(0),
            &Depth::Quick.budget(), // max_iterations = 1
            HashSet::new(),
            &AtomicBool::new(false),
        )
        .await;
        assert_eq!(outcome.stop_reason, StopReason::MaxIterations);
        assert!(!outcome.complete);
    }

    #[tokio::test]
    async fn target_reached_on_final_round_is_complete() {
        let planner = FakePlanner::new(99);
        let source = FakeSource::new(vec![candidate("Alpha", "10/a")]);
        let outcome = run_loop(
            &planner,
            &source,
            &constraints(1),
            &Depth::Quick.budget(),
            HashSet::new(),
            &AtomicBool::new(false),
        )
        .await;

        assert_eq!(outcome.stop_reason, StopReason::TargetReached);
        assert!(outcome.complete);
    }

    #[tokio::test]
    async fn later_rounds_issue_fewer_queries() {
        let planner = FakePlanner::new(99).with_query_count(5);
        let source = GrowingSource {
            next: AtomicU32::new(0),
        };
        let strategy = Depth::Thorough.budget();
        let search_constraints = constraints(0);
        let inputs = RunInputs {
            goal: "goal",
            constraints: &search_constraints,
            strategy: &strategy,
            existing_keys: HashSet::new(),
        };

        let outcome = run(
            &planner,
            &source,
            &EmbeddingReranker::disabled(),
            inputs,
            &AtomicBool::new(false),
            |_| {},
        )
        .await
        .expect("loop ok");

        let issued = outcome
            .trace
            .iter()
            .map(|round| round.queries_issued)
            .collect::<Vec<_>>();
        assert_eq!(issued, vec![5, 3, 2, 1]);
    }

    #[tokio::test]
    async fn provider_is_retired_after_two_dry_rounds() {
        let planner = FakePlanner::new(99);
        let source = FakeSource::new(vec![candidate("Alpha", "10/a")]);
        let outcome = run_loop(
            &planner,
            &source,
            &constraints(0),
            &Depth::Thorough.budget(),
            HashSet::new(),
            &AtomicBool::new(false),
        )
        .await;

        assert_eq!(source.calls.load(Ordering::Relaxed), 3);
        assert_eq!(outcome.trace.len(), 4);
        assert_eq!(outcome.trace[3].queries_issued, 0);
        assert_eq!(outcome.trace[3].queries_skipped_retired, 1);
    }

    #[test]
    fn provider_retirement_does_not_remove_productive_siblings() {
        let mut search_constraints = constraints(0);
        search_constraints.providers = vec![
            DiscoveryProviderChoice::OpenAlex,
            DiscoveryProviderChoice::Arxiv,
        ];
        let mut stats = HashMap::new();
        stats.insert(
            "open_alex".to_string(),
            ProviderStats {
                dry_rounds: 2,
                ..ProviderStats::default()
            },
        );
        stats.insert(
            "arxiv".to_string(),
            ProviderStats {
                new_candidates: 3,
                ..ProviderStats::default()
            },
        );

        let (active, skipped) = active_providers(
            &DiscoveryProviderChoice::OpenAlex,
            &search_constraints,
            &stats,
        );

        assert_eq!(active, vec![DiscoveryProviderChoice::Arxiv]);
        assert_eq!(skipped, 1);
    }

    #[tokio::test]
    async fn provider_budget_counts_selected_provider_fanout() {
        let batch = vec![candidate("Alpha", "10/a")];
        let planner = FakePlanner::new(0);
        let source = FakeSource::new(batch);
        let mut constraints = constraints(20);
        constraints.providers = vec![
            DiscoveryProviderChoice::OpenAlex,
            DiscoveryProviderChoice::Arxiv,
        ];
        let strategy = SearchStrategy {
            depth: Depth::Quick,
            max_iterations: 1,
            max_provider_queries: 1,
            max_llm_calls: 4,
        };
        let inputs = RunInputs {
            goal: "goal",
            constraints: &constraints,
            strategy: &strategy,
            existing_keys: HashSet::new(),
        };
        let mut searched = false;

        let outcome = run(
            &planner,
            &source,
            &EmbeddingReranker::disabled(),
            inputs,
            &AtomicBool::new(false),
            |progress| {
                if matches!(progress, Progress::Searching { .. }) {
                    searched = true;
                }
            },
        )
        .await
        .expect("loop ok");

        assert!(!searched, "two selected providers exceed the budget of one");
        assert!(outcome.ranked.is_empty());
    }

    #[tokio::test]
    async fn stacking_excludes_existing_candidates() {
        let batch = vec![candidate("Alpha", "10/a"), candidate("Beta", "10/b")];
        let planner = FakePlanner::new(0);
        let source = FakeSource::new(batch);
        let mut existing = HashSet::new();
        existing.insert("doi:10/a".to_string()); // Alpha already in the pool
        let outcome = run_loop(
            &planner,
            &source,
            &constraints(20),
            &Depth::Standard.budget(),
            existing,
            &AtomicBool::new(false),
        )
        .await;
        assert_eq!(outcome.ranked.len(), 1);
        assert_eq!(outcome.ranked[0].candidate.title, "Beta");
    }

    /// Deterministic stand-in for the ONNX biencoder: a tiny presence-of-token
    /// embedding, enough for cosine to tell "distributed LLM training" apart from
    /// an unrelated image-recognition paper.
    struct KeywordEmbedder;

    impl TextEmbedder for KeywordEmbedder {
        fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, String> {
            Ok(texts
                .iter()
                .map(|text| {
                    let lower = text.to_lowercase();
                    vec![
                        f32::from(lower.contains("distributed")),
                        f32::from(lower.contains("language") || lower.contains("llm")),
                        f32::from(lower.contains("image")),
                        0.01, // non-zero norm even when nothing matches
                    ]
                })
                .collect())
        }
    }

    #[tokio::test]
    async fn reflection_summary_uses_scores_and_bounds_abstracts() {
        let pool = (0..10)
            .map(|index| {
                if index == 0 {
                    candidate("Distributed language model training", "10/relevant")
                } else {
                    candidate(
                        &format!("Image recognition paper {index}"),
                        &format!("10/off-{index}"),
                    )
                }
            })
            .collect::<Vec<_>>();
        let reranker = EmbeddingReranker::with_embedder(Arc::new(KeywordEmbedder));

        let summary = build_pool_summary(&pool, &reranker, "distributed llm training").await;

        assert_eq!(summary.entries.len(), 10);
        assert!(summary
            .entries
            .iter()
            .all(|entry| entry.semantic_score.is_some()));
        assert_eq!(summary.detailed.len(), REFLECTION_DETAIL_LIMIT);
        assert_eq!(summary.coverage.total, 10);
        assert!(summary.coverage.below_floor > 0);
        assert!(summary.coverage.mean_score.is_some());
        assert_eq!(summary.coverage.year_min, Some(2024));
        assert_eq!(summary.coverage.year_max, Some(2024));
    }

    #[tokio::test]
    async fn reflection_summary_degrades_to_titles_without_reranker() {
        let pool = vec![candidate("Alpha", "10/a")];

        let summary = build_pool_summary(&pool, &EmbeddingReranker::disabled(), "goal").await;

        assert_eq!(summary.entries.len(), 1);
        assert_eq!(summary.entries[0].semantic_score, None);
        assert!(summary.detailed.is_empty());
        assert_eq!(summary.coverage.mean_score, None);
    }

    #[tokio::test]
    async fn deep_research_ranks_by_semantic_similarity_to_goal() {
        // A relevant paper with zero citations vs. a famous, off-topic one.
        // Legacy ranking would reward the citation count; the biencoder must
        // lift the on-topic paper to the top instead (RFC 0057).
        let mut relevant = candidate("Distributed training of large language models", "10/rel");
        relevant.citation_count = Some(0);
        let mut famous_offtopic =
            candidate("Deep residual learning for image recognition", "10/off");
        famous_offtopic.citation_count = Some(200_000);

        let planner = FakePlanner::new(0);
        let source = FakeSource::new(vec![famous_offtopic, relevant]);
        let reranker = EmbeddingReranker::with_embedder(Arc::new(KeywordEmbedder));
        let strategy = Depth::Standard.budget();
        let constraints = constraints(20);
        let inputs = RunInputs {
            goal: "distributed training for llms",
            constraints: &constraints,
            strategy: &strategy,
            existing_keys: HashSet::new(),
        };

        let outcome = run(
            &planner,
            &source,
            &reranker,
            inputs,
            &AtomicBool::new(false),
            |_| {},
        )
        .await
        .expect("loop ok");

        assert_eq!(
            outcome.ranked[0].candidate.title,
            "Distributed training of large language models"
        );
        // Every ranked candidate now carries an inspectable semantic score —
        // the tell that the biencoder ran on the deep-research path.
        assert!(outcome.ranked.iter().all(|ranked| ranked
            .candidate
            .match_summary
            .reasons
            .iter()
            .any(|reason| reason.starts_with("semantic:"))));
    }

    #[tokio::test]
    async fn deep_research_without_reranker_attaches_no_semantic_reason() {
        // Disabled reranker ⇒ empty scores ⇒ legacy ranking, no semantic reason.
        let batch = vec![candidate("Alpha", "10/a")];
        let planner = FakePlanner::new(0);
        let source = FakeSource::new(batch);
        let outcome = run_loop(
            &planner,
            &source,
            &constraints(20),
            &Depth::Standard.budget(),
            HashSet::new(),
            &AtomicBool::new(false),
        )
        .await;
        assert!(!outcome.ranked[0]
            .candidate
            .match_summary
            .reasons
            .iter()
            .any(|reason| reason.starts_with("semantic:")));
    }
}
