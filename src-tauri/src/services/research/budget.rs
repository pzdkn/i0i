//! Hard budget rails and stop criteria for the agent loop.
//!
//! `evaluate_stop` is pure: the loop enforces these ceilings independent of what
//! the planner asks for, so a runaway planner cannot exceed them.

use crate::domain::research::SearchStrategy;

/// Exactly one reason a run ended, recorded on the run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopReason {
    TargetReached,
    CoverageSufficient,
    /// RFC 0088 R1.3: reflection said the pool answers the goal. Distinct from
    /// `MaxIterations`, which cannot tell "done" from "out of budget".
    Converged,
    MaxIterations,
    MaxProviderQueries,
    MaxLlmCalls,
    ProviderFailuresExhausted,
    Cancelled,
}

impl StopReason {
    /// Stable storage and event representation for this reason.
    pub fn as_str(self) -> &'static str {
        match self {
            StopReason::TargetReached => "target_reached",
            StopReason::CoverageSufficient => "coverage_sufficient",
            StopReason::Converged => "converged",
            StopReason::MaxIterations => "max_iterations",
            StopReason::MaxProviderQueries => "max_provider_queries",
            StopReason::MaxLlmCalls => "max_llm_calls",
            StopReason::ProviderFailuresExhausted => "provider_failures_exhausted",
            StopReason::Cancelled => "cancelled",
        }
    }

    /// Did the run finish, or did it run out?
    ///
    /// RFC 0088 R6.2, stolen from jina's Beast Mode: a spent budget still
    /// returns the ranked pool, but the caller must be able to say "here is what
    /// I found, and I stopped early" rather than presenting an exhausted run as
    /// a complete one. Cancellation is not completion either.
    pub fn is_complete(self) -> bool {
        matches!(
            self,
            StopReason::TargetReached | StopReason::CoverageSufficient | StopReason::Converged
        )
    }
}

/// What a run has left, as `reflect` sees it (RFC 0088 R1.1).
///
/// The planner is told how much room remains so "should we continue" can be
/// answered against the budget rather than in the abstract — one more round is
/// a different proposition with five provider queries left than with fifty.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BudgetRemaining {
    pub rounds: u32,
    pub provider_queries: u32,
}

impl BudgetRemaining {
    /// Compute the unspent round and provider-query budget.
    pub fn from_usage(usage: &BudgetUsage, strategy: &SearchStrategy) -> Self {
        Self {
            rounds: strategy.max_iterations.saturating_sub(usage.iterations),
            provider_queries: strategy
                .max_provider_queries
                .saturating_sub(usage.provider_queries),
        }
    }
}

/// Per-provider tally for one run (RFC 0088 R1.4).
///
/// `dry_rounds` is the one that does work: a provider returning nothing new for
/// two consecutive rounds is not going to start, and every further query to it
/// is spend with no expected return.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ProviderStats {
    pub queries: u32,
    pub candidates: u32,
    pub new_candidates: u32,
    pub dry_rounds: u32,
}

impl ProviderStats {
    /// Fold one round's result for this provider into the tally.
    pub fn record_round(&mut self, queries: u32, candidates: u32, new_candidates: u32) {
        self.queries += queries;
        self.candidates += candidates;
        self.new_candidates += new_candidates;
        if new_candidates == 0 {
            self.dry_rounds += 1;
        } else {
            self.dry_rounds = 0;
        }
    }
}

/// Consecutive empty rounds before a provider is retired for the rest of the run.
const DRY_ROUNDS_BEFORE_RETIRE: u32 = 2;

/// Whether a provider still earns its queries (RFC 0088 R1.4).
///
/// Stolen from jina-ai/node-DeepResearch, which disables actions that stop
/// yielding new information. A provider that has never been queried is paying
/// by default — you cannot retire what you have not tried.
pub fn provider_is_paying(stats: &ProviderStats) -> bool {
    stats.dry_rounds < DRY_ROUNDS_BEFORE_RETIRE
}

/// Queries to issue in `round` (0-based) — RFC 0088 R1.5.
///
/// Copied from dzhng/deep-research, which halves breadth at each level of its
/// recursion (`Math.ceil(breadth / 2)`). Round 0 casts the wide net; later
/// rounds are refinements aimed at named gaps, and paying round-zero prices for
/// them is how a bounded loop still gets expensive. Never returns 0 — a round
/// that may issue no queries should not have been started.
pub fn query_budget_for_round(round: u32, base: u32) -> u32 {
    let base = base.max(1);
    let mut allowance = base;
    for _ in 0..round {
        allowance = allowance.div_ceil(2);
        if allowance <= 1 {
            return 1;
        }
    }
    allowance
}

/// Queries the first round may issue before the decay in `query_budget_for_round`
/// applies. The planner is asked for 3–5; this is the ceiling on how many of
/// them are executed.
pub const BASE_QUERIES_PER_ROUND: u32 = 5;

/// Running tally of what a run has consumed so far.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BudgetUsage {
    pub iterations: u32,
    pub provider_queries: u32,
    pub llm_calls: u32,
    pub candidate_count: u32,
}

/// Decide whether the loop must stop *before* doing more work. Checked at the
/// top of each iteration. Target-reached wins over the budget ceilings.
pub fn evaluate_stop(
    usage: &BudgetUsage,
    strategy: &SearchStrategy,
    target_count: u32,
) -> Option<StopReason> {
    if target_count > 0 && usage.candidate_count >= target_count {
        return Some(StopReason::TargetReached);
    }
    if usage.iterations >= strategy.max_iterations {
        return Some(StopReason::MaxIterations);
    }
    if usage.provider_queries >= strategy.max_provider_queries {
        return Some(StopReason::MaxProviderQueries);
    }
    if usage.llm_calls >= strategy.max_llm_calls {
        return Some(StopReason::MaxLlmCalls);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::research::Depth;

    fn standard() -> SearchStrategy {
        Depth::Standard.budget() // 3 iterations, 8 queries, 12 llm calls
    }

    #[test]
    fn no_stop_when_under_all_ceilings() {
        let usage = BudgetUsage {
            iterations: 1,
            provider_queries: 2,
            llm_calls: 3,
            candidate_count: 5,
        };
        assert_eq!(evaluate_stop(&usage, &standard(), 20), None);
    }

    #[test]
    fn target_reached_takes_precedence() {
        let usage = BudgetUsage {
            iterations: 99,
            provider_queries: 99,
            llm_calls: 99,
            candidate_count: 20,
        };
        assert_eq!(
            evaluate_stop(&usage, &standard(), 20),
            Some(StopReason::TargetReached)
        );
    }

    #[test]
    fn max_iterations_fires() {
        let usage = BudgetUsage {
            iterations: 3,
            ..Default::default()
        };
        assert_eq!(
            evaluate_stop(&usage, &standard(), 20),
            Some(StopReason::MaxIterations)
        );
    }

    #[test]
    fn max_provider_queries_fires() {
        let usage = BudgetUsage {
            iterations: 0,
            provider_queries: 8,
            ..Default::default()
        };
        assert_eq!(
            evaluate_stop(&usage, &standard(), 20),
            Some(StopReason::MaxProviderQueries)
        );
    }

    #[test]
    fn max_llm_calls_fires() {
        let usage = BudgetUsage {
            iterations: 0,
            provider_queries: 0,
            llm_calls: 12,
            ..Default::default()
        };
        assert_eq!(
            evaluate_stop(&usage, &standard(), 20),
            Some(StopReason::MaxLlmCalls)
        );
    }

    #[test]
    fn zero_target_does_not_short_circuit() {
        let usage = BudgetUsage {
            iterations: 3,
            candidate_count: 100,
            ..Default::default()
        };
        // target_count 0 means "unbounded": a huge candidate count must NOT stop
        // via TargetReached; only the ceilings (here MaxIterations) do.
        assert_eq!(
            evaluate_stop(&usage, &standard(), 0),
            Some(StopReason::MaxIterations)
        );
    }

    // --- RFC 0088 R1.4/R1.5: budget shaping ---

    #[test]
    fn a_provider_that_keeps_paying_is_never_retired() {
        let mut stats = ProviderStats::default();
        for _ in 0..10 {
            stats.record_round(1, 25, 3);
            assert!(provider_is_paying(&stats));
        }
        assert_eq!(stats.dry_rounds, 0);
    }

    #[test]
    fn two_dry_rounds_retire_a_provider() {
        let mut stats = ProviderStats::default();
        stats.record_round(1, 25, 0);
        assert!(provider_is_paying(&stats), "one dry round is not a pattern");
        stats.record_round(1, 25, 0);
        assert!(!provider_is_paying(&stats));
    }

    #[test]
    fn one_new_candidate_resets_the_dry_streak() {
        let mut stats = ProviderStats::default();
        stats.record_round(1, 25, 0);
        stats.record_round(1, 25, 1);
        stats.record_round(1, 25, 0);
        assert!(
            provider_is_paying(&stats),
            "the streak must be consecutive, not cumulative"
        );
    }

    #[test]
    fn an_untried_provider_is_paying() {
        // You cannot retire what you have not queried.
        assert!(provider_is_paying(&ProviderStats::default()));
    }

    #[test]
    fn query_budget_halves_each_round_and_floors_at_one() {
        assert_eq!(query_budget_for_round(0, 5), 5);
        assert_eq!(query_budget_for_round(1, 5), 3);
        assert_eq!(query_budget_for_round(2, 5), 2);
        assert_eq!(query_budget_for_round(3, 5), 1);
        assert_eq!(query_budget_for_round(9, 5), 1);
    }

    #[test]
    fn query_budget_never_returns_zero() {
        for base in [0, 1, 2, 100] {
            for round in 0..12 {
                assert!(
                    query_budget_for_round(round, base) >= 1,
                    "base {base} round {round}"
                );
            }
        }
    }

    #[test]
    fn only_finishing_counts_as_complete() {
        assert!(StopReason::TargetReached.is_complete());
        assert!(StopReason::Converged.is_complete());
        assert!(StopReason::CoverageSufficient.is_complete());
        // Running out is not finishing, and neither is being stopped.
        assert!(!StopReason::MaxIterations.is_complete());
        assert!(!StopReason::MaxProviderQueries.is_complete());
        assert!(!StopReason::MaxLlmCalls.is_complete());
        assert!(!StopReason::ProviderFailuresExhausted.is_complete());
        assert!(!StopReason::Cancelled.is_complete());
    }
}
