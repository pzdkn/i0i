//! Simple discovery orchestration.
//!
//! The service sits between the Tauri command and provider implementations. It
//! should stay small: validate the app-level request, call the provider, and
//! assemble the app-level response returned to Svelte.
//!
//! RFC 0043 routes production search through `DiscoveryOrchestrator`, which
//! handles multi-provider merge/dedupe/rank. Keep this small wrapper around as
//! a readable single-provider scaffold for provider tests and future spikes.

#![allow(dead_code)]

use super::error::DiscoveryError;
use super::provider::{DiscoveryProvider, ProviderSearchResult};
use crate::domain::discovery::{DiscoverySearchRequest, DiscoverySearchResponse};

/// Runs discovery for the app.
///
/// The service depends on the provider trait so provider-specific HTTP and JSON
/// details stay behind the adapter boundary.
pub struct DiscoveryService<P>
where
    P: DiscoveryProvider,
{
    provider: P,
}

impl<P> DiscoveryService<P>
where
    P: DiscoveryProvider,
{
    pub fn new(provider: P) -> Self {
        Self { provider }
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
            DiscoverySearchResponse {
                provider: provider_result.provider.to_string(),
                query,
                filters: provider_result.filters,
                sort_by: request.sort_by,
                result_limit: request.result_limit,
                result_count,
                candidates: provider_result.candidates,
            }
        }

        let query = validated_query(&request)?;
        let provider_result = self.provider.search(&request).await?;

        Ok(build_response(request, query, provider_result))
    }
}
