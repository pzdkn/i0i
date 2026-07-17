//! The `CandidateSource` seam: the provider dispatch the loop depends on.
//!
//! `RealCandidateSource` maps a planned `Query` + the run's constraints into a
//! `DiscoverySearchRequest` (carrying the query-time venue/author/field params)
//! and dispatches to the concrete provider, with a per-provider politeness
//! delay. The loop depends on the trait so tests can substitute a fake.
//!
//! Lineage (seed `referenced_works`/`cited_by`) is a planned follow-up; the
//! query-shape helper already exists in the OpenAlex provider.

use std::time::Duration;

use async_trait::async_trait;

use crate::commands::discovery::orchestrator::DiscoveryOrchestrator;
use crate::commands::discovery::providers::{arxiv::ArxivProvider, openalex::OpenAlexProvider};
use crate::domain::discovery::{
    DiscoveryProviderChoice, DiscoverySearchRequest, DiscoverySort, PaperCandidate,
};
use crate::domain::research::SearchConstraints;
use crate::services::research::error::ResearchError;
use crate::services::research::planner::Query;

/// Per-query result ceiling (the loop's budget caps the number of queries).
const PER_QUERY_LIMIT: i32 = 25;

#[async_trait]
pub trait CandidateSource: Send + Sync {
    async fn search(
        &self,
        query: &Query,
        constraints: &SearchConstraints,
    ) -> Result<Vec<PaperCandidate>, ResearchError>;
}

/// Real provider-backed source. Holds cloned providers (each owns a pooled
/// `reqwest::Client`) so the connection pool survives across queries.
pub struct RealCandidateSource {
    openalex: OpenAlexProvider,
    arxiv: ArxivProvider,
}

impl RealCandidateSource {
    pub fn new(openalex: OpenAlexProvider, arxiv: ArxivProvider) -> Self {
        Self { openalex, arxiv }
    }

    fn request_for(query: &Query, constraints: &SearchConstraints) -> DiscoverySearchRequest {
        DiscoverySearchRequest {
            query: query.text.clone(),
            year_from: constraints.year_from,
            year_to: constraints.year_to,
            result_limit: PER_QUERY_LIMIT,
            sort_by: DiscoverySort::Relevance,
            provider: query.provider.clone(),
            providers: constraints.providers.clone(),
            open_access: constraints.open_access,
            venues: constraints.venues.clone(),
            authors: constraints.authors.clone(),
            fields_of_study: constraints.fields_of_study.clone(),
        }
    }

    /// Politeness delay before a provider call (arXiv asks for ~3s).
    fn throttle_delay(provider: &DiscoveryProviderChoice) -> Duration {
        match provider {
            DiscoveryProviderChoice::Arxiv => Duration::from_secs(3),
            _ => Duration::from_millis(200),
        }
    }
}

#[async_trait]
impl CandidateSource for RealCandidateSource {
    async fn search(
        &self,
        query: &Query,
        constraints: &SearchConstraints,
    ) -> Result<Vec<PaperCandidate>, ResearchError> {
        let request = Self::request_for(query, constraints);
        let providers = if constraints.providers.is_empty() {
            vec![query.provider.clone()]
        } else {
            constraints.providers.clone()
        };
        // The orchestrator now fans the providers out concurrently (RFC 0044),
        // so a single politeness gate of the slowest provider's delay preserves
        // arXiv's ~3s spacing without serializing one delay per provider.
        if let Some(delay) = providers.iter().map(Self::throttle_delay).max() {
            tokio::time::sleep(delay).await;
        }
        let response = DiscoveryOrchestrator::new(self.openalex.clone(), self.arxiv.clone())
            .search(request)
            .await
            .map_err(|e| ResearchError::new(e.to_string()))?;
        Ok(response.candidates)
    }
}
