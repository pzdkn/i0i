//! Shared discovery provider contract.
//!
//! This module defines the small interface between `service.rs` and concrete
//! provider adapters such as OpenAlex. It should stay provider-neutral: no
//! OpenAlex URL params, JSON fields, API-key parsing, or HTTP details belong
//! here.

use std::{error::Error, fmt};

use crate::domain::discovery::{DiscoverySearchRequest, PaperCandidate};
use super::error::DiscoveryError;

/// Search providers supported by the discovery layer.
///
/// The enum is intentionally small for now. Add variants here when a provider
/// has a real adapter, not when it is only an idea in an RFC.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiscoveryProviderId {
    OpenAlex,
    Arxiv,
    SemanticScholar,
}

impl DiscoveryProviderId {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::OpenAlex => "openalex",
            Self::Arxiv => "arxiv",
            Self::SemanticScholar => "semantic_scholar",
        }
    }
}

impl fmt::Display for DiscoveryProviderId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Provider capability summary used by the service/UI to understand support.
///
/// Capabilities describe what the provider can do directly. They do not promise
/// that the service currently exposes every capability in the UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProviderCapabilities {
    pub supports_open_access_filter: bool,
    pub supports_citation_sort: bool,
    pub supports_seed_similarity: bool,
    pub requires_api_key: bool,
}

/// Normalized result returned by one provider adapter.
///
/// This is intentionally smaller than `DiscoverySearchResponse`. The service
/// owns the final app-level response shape because later it may call more than
/// one provider and merge their results.
#[derive(Debug, Clone)]
pub struct ProviderSearchResult {
    pub provider: DiscoveryProviderId,
    pub filters: Vec<String>,
    pub candidates: Vec<PaperCandidate>,
}


/// Common behavior every discovery provider must expose.
pub trait DiscoveryProvider {
    fn id(&self) -> DiscoveryProviderId;

    fn capabilities(&self) -> ProviderCapabilities;

    /// Resolve the provider API key.
    ///
    /// TODO: This is provider-specific and should move into `OpenAlexProvider`
    /// once `search.rs` is refactored enough to make it a private helper. =>
    /// No this is an undefined interace!
    /// All providers should implement this. Dont move it into OpenAlex
    fn api_key(&self) -> String;

    /// Build the provider request URL from query parameters.
    ///
    /// TODO: This is provider-specific and should move into `OpenAlexProvider`.
    /// The parameter type is also temporary; structured key/value params will
    /// be clearer than a raw `String`. => No this is an undefined interace!
    /// All providers should implement this. Dont move it into OpenAlex
    fn build_url(&self, query_params: String) -> String;

    async fn search(
        &self,
        request: &DiscoverySearchRequest,
    ) -> Result<ProviderSearchResult, DiscoveryError>;
}
