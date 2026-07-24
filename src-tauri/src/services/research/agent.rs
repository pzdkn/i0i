//! The orchestrated agent loop (RFC 0037).
//!
//! Rust drives `plan → search → filter → dedup → assess → refine → rank`,
//! bounded by the strategy budget, with the LLM able to break early via
//! `assess`. The loop is free of I/O side effects: it depends on the `Planner`
//! and `CandidateSource` seams, takes a cancellation flag and a progress
//! callback, and returns an outcome. Persistence and event emission live in the
//! manager, so the loop is unit-testable with fakes (no DB, no network).

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::commands::discovery::orchestrator::rank_candidates;
use crate::domain::discovery::{DiscoveryProviderChoice, PaperCandidate};
use crate::domain::research::{
    candidate_dedup_key, RankedCandidate, SearchConstraints, SearchStrategy,
};
use crate::services::research::budget::{evaluate_stop, BudgetUsage, StopReason};
use crate::services::research::dedup::{dedup, diff};
use crate::services::research::error::ResearchError;
use crate::services::research::filter::apply_constraints;
use crate::services::research::planner::Planner;
use crate::services::research::source::CandidateSource;

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
    Assessing,
    Ranking { count: usize },
}

/// Run the bounded agent loop. `on` receives progress signals; `cancelled` is
/// checked between iterations and before each LLM/provider call.
pub async fn run<P, S>(
    planner: &P,
    source: &S,
    inputs: RunInputs<'_>,
    cancelled: &AtomicBool,
    mut on: impl FnMut(Progress),
) -> Result<RunOutcome, ResearchError>
where
    P: Planner,
    S: CandidateSource,
{
    let strategy = inputs.strategy;
    let constraints = inputs.constraints;
    let target = constraints.target_count.max(0) as u32;

    let mut pool: Vec<PaperCandidate> = Vec::new();
    let mut usage = BudgetUsage::default();
    let mut gaps: Vec<String> = Vec::new();
    let mut stop_reason = StopReason::MaxIterations;

    'run: for iteration in 0..strategy.max_iterations {
        if cancelled.load(Ordering::Relaxed) {
            stop_reason = StopReason::Cancelled;
            break;
        }
        if let Some(reason) = evaluate_stop(&usage, strategy, target) {
            stop_reason = reason;
            break;
        }

        on(Progress::Planning { iteration });
        let queries = if iteration == 0 {
            planner.plan_queries(inputs.goal, constraints).await?
        } else {
            let titles = pool_titles(&pool);
            planner.refine_queries(inputs.goal, &gaps, &titles).await?
        };
        usage.llm_calls += 1;

        for query in queries {
            if cancelled.load(Ordering::Relaxed) {
                stop_reason = StopReason::Cancelled;
                break 'run;
            }
            let provider_call_cost = provider_call_cost(&query.provider, constraints);
            if usage.provider_queries + provider_call_cost > strategy.max_provider_queries {
                break;
            }
            let provider = provider_label(&query.provider, constraints);
            on(Progress::Searching {
                provider: provider.clone(),
                text: query.text.clone(),
            });
            match source.search(&query, constraints).await {
                Ok(mut found) => {
                    on(Progress::SearchResult {
                        provider,
                        count: found.len(),
                    });
                    pool.append(&mut found);
                }
                Err(error) => {
                    on(Progress::SearchFailed {
                        provider,
                        error: error.to_string(),
                    });
                }
            }
            usage.provider_queries += provider_call_cost;
        }

        pool = apply_constraints(pool, constraints);
        pool = dedup(pool);
        on(Progress::Deduped { unique: pool.len() });
        if !pool.is_empty() {
            on(Progress::CandidatePreview {
                candidates: pool.clone(),
            });
        }
        usage.candidate_count = new_count(&pool, &inputs.existing_keys);

        if cancelled.load(Ordering::Relaxed) {
            stop_reason = StopReason::Cancelled;
            break;
        }

        on(Progress::Assessing);
        let assessment = planner.assess(inputs.goal, &pool_titles(&pool)).await?;
        usage.llm_calls += 1;
        if !assessment.refine {
            stop_reason = StopReason::CoverageSufficient;
            break;
        }
        gaps = assessment.gaps;
        usage.iterations += 1;
    }

    // Stacking: rank only the candidates not already in the saved pool.
    let new_candidates = diff(pool, &inputs.existing_keys);
    on(Progress::Ranking {
        count: new_candidates.len(),
    });
    // Deep research keeps legacy ranking for now; wiring the embedding reranker
    // through the SearchManager loop is a follow-up (RFC 0054 notes).
    let ranked = rank_candidates(new_candidates, inputs.goal, constraints.target_count, &[])
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
    })
}

fn pool_titles(pool: &[PaperCandidate]) -> Vec<String> {
    pool.iter().map(|c| c.title.clone()).collect()
}

fn new_count(pool: &[PaperCandidate], existing: &HashSet<String>) -> u32 {
    pool.iter()
        .filter(|c| !existing.contains(&candidate_dedup_key(c)))
        .count() as u32
}

fn provider_call_cost(
    _query_provider: &DiscoveryProviderChoice,
    constraints: &SearchConstraints,
) -> u32 {
    constraints.providers.len().max(1) as u32
}

fn provider_label(
    query_provider: &DiscoveryProviderChoice,
    constraints: &SearchConstraints,
) -> String {
    if constraints.providers.is_empty() {
        return provider_name(query_provider).to_string();
    }

    constraints
        .providers
        .iter()
        .map(provider_name)
        .collect::<Vec<_>>()
        .join("+")
}

fn provider_name(provider: &DiscoveryProviderChoice) -> &'static str {
    match provider {
        DiscoveryProviderChoice::OpenAlex => "open_alex",
        DiscoveryProviderChoice::Arxiv => "arxiv",
        DiscoveryProviderChoice::EuropePmc => "europe_pmc",
        DiscoveryProviderChoice::Core => "core",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicU32;

    use async_trait::async_trait;

    use crate::domain::discovery::{CandidateMatch, DiscoveryProviderChoice, PaperCandidate};
    use crate::domain::research::Depth;
    use crate::services::research::planner::{Assessment, Query};

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
    /// times `assess` says "refine" before declaring coverage sufficient.
    struct FakePlanner {
        refine_budget: AtomicU32,
        plan_calls: AtomicU32,
    }

    impl FakePlanner {
        fn new(refine_budget: u32) -> Self {
            Self {
                refine_budget: AtomicU32::new(refine_budget),
                plan_calls: AtomicU32::new(0),
            }
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
            Ok(vec![Query {
                provider: DiscoveryProviderChoice::OpenAlex,
                text: "q".to_string(),
            }])
        }

        async fn refine_queries(
            &self,
            _goal: &str,
            _gaps: &[String],
            _titles: &[String],
        ) -> Result<Vec<Query>, ResearchError> {
            Ok(vec![Query {
                provider: DiscoveryProviderChoice::OpenAlex,
                text: "refined".to_string(),
            }])
        }

        async fn assess(
            &self,
            _goal: &str,
            _titles: &[String],
        ) -> Result<Assessment, ResearchError> {
            let remaining = self.refine_budget.load(Ordering::Relaxed);
            if remaining > 0 {
                self.refine_budget.fetch_sub(1, Ordering::Relaxed);
                Ok(Assessment {
                    refine: true,
                    gaps: vec!["a gap".to_string()],
                })
            } else {
                Ok(Assessment {
                    refine: false,
                    gaps: Vec::new(),
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
    }

    #[async_trait]
    impl CandidateSource for FakeSource {
        async fn search(
            &self,
            _query: &Query,
            _constraints: &SearchConstraints,
        ) -> Result<Vec<PaperCandidate>, ResearchError> {
            Ok(self.batch.clone())
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
        run(planner, source, inputs, cancelled, |_| {})
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
        let planner = FakePlanner::new(0); // assess: stop immediately
        let source = FakeSource { batch };
        let outcome = run_loop(
            &planner,
            &source,
            &constraints(20),
            &Depth::Standard.budget(),
            HashSet::new(),
            &AtomicBool::new(false),
        )
        .await;
        assert_eq!(outcome.stop_reason, StopReason::CoverageSufficient);
        assert_eq!(outcome.ranked.len(), 2);
        assert_eq!(outcome.ranked[0].rank, 1);
    }

    #[tokio::test]
    async fn emits_preview_after_deduping_candidate_pool() {
        let batch = vec![
            candidate("Alpha", "10/a"),
            candidate("Alpha copy", "10/a"),
            candidate("Beta", "10/b"),
        ];
        let planner = FakePlanner::new(0);
        let source = FakeSource { batch };
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
        let source = FakeSource { batch };
        let outcome = run_loop(
            &planner,
            &source,
            &constraints(20),
            &Depth::Standard.budget(),
            HashSet::new(),
            &AtomicBool::new(false),
        )
        .await;
        assert_eq!(outcome.stop_reason, StopReason::CoverageSufficient);
        assert_eq!(planner.plan_calls.load(Ordering::Relaxed), 1); // plan once; round 2 used refine
        assert_eq!(outcome.iterations, 1); // one refine bump
    }

    #[tokio::test]
    async fn cancellation_stops_the_run() {
        let batch = vec![candidate("Alpha", "10/a")];
        let planner = FakePlanner::new(5);
        let source = FakeSource { batch };
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
        let source = FakeSource { batch };
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
    }

    #[tokio::test]
    async fn provider_budget_counts_selected_provider_fanout() {
        let batch = vec![candidate("Alpha", "10/a")];
        let planner = FakePlanner::new(0);
        let source = FakeSource { batch };
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
        let source = FakeSource { batch };
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
}
