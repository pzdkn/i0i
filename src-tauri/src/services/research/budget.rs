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
    MaxIterations,
    MaxProviderQueries,
    MaxLlmCalls,
    ProviderFailuresExhausted,
    Cancelled,
}

impl StopReason {
    pub fn as_str(self) -> &'static str {
        match self {
            StopReason::TargetReached => "target_reached",
            StopReason::CoverageSufficient => "coverage_sufficient",
            StopReason::MaxIterations => "max_iterations",
            StopReason::MaxProviderQueries => "max_provider_queries",
            StopReason::MaxLlmCalls => "max_llm_calls",
            StopReason::ProviderFailuresExhausted => "provider_failures_exhausted",
            StopReason::Cancelled => "cancelled",
        }
    }
}

/// Running tally of what a run has consumed so far.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
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
}
