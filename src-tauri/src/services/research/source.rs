//! The `CandidateSource` seam used by the Deep Research loop.
//!
//! RFC 0098 routes production research through `BrowserCandidateSource`.
//! `RealCandidateSource` remains for citation-graph suggestions and historical
//! provider tests, while the loop's trait keeps network behavior replaceable.
//!
//! Lineage (seed `referenced_works`/`cited_by`) is a planned follow-up; the
//! query-shape helper already exists in the OpenAlex provider.

use std::time::Duration;

use async_trait::async_trait;

use crate::commands::discovery::browser::{BrowserDiscoveryProgress, BrowserDiscoverySource};
use crate::commands::discovery::orchestrator::DiscoveryOrchestrator;
use crate::commands::discovery::providers::{arxiv::ArxivProvider, openalex::OpenAlexProvider};
use crate::domain::discovery::{
    DiscoveryProviderChoice, DiscoverySearchRequest, DiscoverySort, Lineage, PaperCandidate,
};
use crate::domain::research::SearchConstraints;
use crate::services::research::error::ResearchError;
use crate::services::research::planner::Query;
use crate::services::source_acquisition::SourceAcquisitionService;

/// Per-query result ceiling (the loop's budget caps the number of queries).
const PER_QUERY_LIMIT: i32 = 25;

#[async_trait]
pub trait CandidateSource: Send + Sync {
    /// User-facing transport name when candidate generation is not tied to the
    /// planner's legacy provider field.
    fn transport_name(&self) -> Option<&'static str> {
        None
    }

    async fn search(
        &self,
        query: &Query,
        constraints: &SearchConstraints,
    ) -> Result<Vec<PaperCandidate>, ResearchError>;

    /// Search while exposing transport-neutral intermediate states. Test fakes
    /// and non-streaming sources inherit the direct implementation.
    async fn search_with_progress(
        &self,
        query: &Query,
        constraints: &SearchConstraints,
        on_progress: &(dyn Fn(SourceProgress) + Send + Sync),
    ) -> Result<Vec<PaperCandidate>, ResearchError> {
        let candidates = self.search(query, constraints).await?;
        on_progress(SourceProgress::Resolved(candidates.len()));
        Ok(candidates)
    }
}

#[derive(Debug, Clone)]
pub enum SourceProgress {
    SearchingWeb,
    Provisional(Vec<PaperCandidate>),
    ResolvingMetadata(usize),
    Resolved(usize),
}

/// Browser-first source shared by Quick Search and Deep Research.
pub struct BrowserCandidateSource {
    source: BrowserDiscoverySource,
}

impl BrowserCandidateSource {
    pub fn from_app(
        app: &tauri::AppHandle,
        browser: SourceAcquisitionService,
        openalex: OpenAlexProvider,
        arxiv: ArxivProvider,
    ) -> Self {
        Self {
            source: BrowserDiscoverySource::new(
                browser,
                openalex,
                arxiv,
                crate::commands::discovery::browser::BrowserDiscoveryConfig::load(app),
            ),
        }
    }

    async fn discover(
        &self,
        query: &Query,
        constraints: &SearchConstraints,
        on_progress: &(dyn Fn(SourceProgress) + Send + Sync),
    ) -> Result<Vec<PaperCandidate>, ResearchError> {
        let limit = constraints.target_count.clamp(1, PER_QUERY_LIMIT) as usize;
        let candidates = self
            .source
            .discover_with_resolvers(&query.text, limit, &constraints.providers, &|progress| {
                let progress = match progress {
                    BrowserDiscoveryProgress::SearchingWeb => SourceProgress::SearchingWeb,
                    BrowserDiscoveryProgress::Provisional(candidates) => {
                        SourceProgress::Provisional(candidates)
                    }
                    BrowserDiscoveryProgress::ResolvingMetadata { count } => {
                        SourceProgress::ResolvingMetadata(count)
                    }
                    BrowserDiscoveryProgress::Resolved { count } => SourceProgress::Resolved(count),
                };
                on_progress(progress);
            })
            .await
            .map_err(|error| ResearchError::new(error.to_string()))?;
        Ok(crate::services::research::filter::apply_resolved_constraints(candidates, constraints))
    }
}

#[async_trait]
impl CandidateSource for BrowserCandidateSource {
    fn transport_name(&self) -> Option<&'static str> {
        Some("web")
    }

    async fn search(
        &self,
        query: &Query,
        constraints: &SearchConstraints,
    ) -> Result<Vec<PaperCandidate>, ResearchError> {
        self.discover(query, constraints, &|_| {}).await
    }

    async fn search_with_progress(
        &self,
        query: &Query,
        constraints: &SearchConstraints,
        on_progress: &(dyn Fn(SourceProgress) + Send + Sync),
    ) -> Result<Vec<PaperCandidate>, ResearchError> {
        self.discover(query, constraints, on_progress).await
    }
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

    /// Fetch a bounded OpenAlex citation neighbourhood for vault suggestions.
    pub async fn lineage(
        &self,
        work_id: &str,
        lineage: Lineage,
        limit: i32,
    ) -> Result<Vec<PaperCandidate>, ResearchError> {
        self.openalex
            .lineage(work_id, lineage, limit)
            .await
            .map_err(|error| ResearchError::new(error.to_string()))
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
            only_viewable: false,
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
