//! Simple discovery orchestration.
//!
//! The service sits between the Tauri command and provider implementations. It
//! should stay small: validate the app-level request, call the provider, and
//! assemble the app-level response returned to Svelte.

use crate::domain::discovery::{DiscoverySearchRequest, DiscoverySearchResponse};
use super::error::DiscoveryError;
use super::{
    provider::{DiscoveryProvider, DiscoveryProviderId, ProviderSearchResult},
    providers::openalex::OpenAlexProvider,
};

/// Runs discovery for the app.
///
/// For now this service owns exactly one provider: OpenAlex. That keeps the
/// control flow easy to follow while the provider boundary is still settling.
/// Multi-provider dispatch can come later once one provider path feels boring.
pub struct DiscoveryService {
    openalex: OpenAlexProvider,
}

impl DiscoveryService {
    // TODO: => This should be DiscoveryProvider instead of a particular one OpenAlex
    pub fn new(openalex: OpenAlexProvider) -> Self {
        Self { openalex }
    }

    /// Search papers using the current provider set and return the app-level
    /// discovery response.
    ///
    /// This method deliberately does not build OpenAlex URLs or parse OpenAlex
    /// JSON. Provider-specific work belongs in `providers/openalex/search.rs`.
    pub async fn search(
        &self,
        request: DiscoverySearchRequest,
    ) -> Result<DiscoverySearchResponse, DiscoveryError> {
        let query = validated_query(&request)?;
        let provider_result = self.openalex.search(&request).await?;

        Ok(build_response(request, query, provider_result))
    }
}

fn validated_query(request: &DiscoverySearchRequest) -> Result<String, DiscoveryError> {
    let query = request.query.trim().to_string();
    if query.is_empty() {
        return Err(DiscoveryError::new(
            "Enter a search query before running discovery.",
        ));
    }

    Ok(query)
}

fn build_response(
    request: DiscoverySearchRequest,
    query: String,
    provider_result: ProviderSearchResult,
) -> DiscoverySearchResponse {
    let result_count = provider_result.candidates.len();
    // TODO: ProviderSearchResult
    // And DiscoverySearchResponse all sound the same
    DiscoverySearchResponse {
        provider: provider_name(provider_result.provider),
        query,
        filters: provider_result.filters,
        sort_by: request.sort_by,
        result_limit: request.result_limit,
        result_count,
        candidates: provider_result.candidates,
    }
}

fn provider_name(provider: DiscoveryProviderId) -> String {
    match provider {
        DiscoveryProviderId::OpenAlex => "openalex".to_string(),
        DiscoveryProviderId::Arxiv => "arxiv".to_string(),
        DiscoveryProviderId::SemanticScholar => "semantic_scholar".to_string(),
    }
}
