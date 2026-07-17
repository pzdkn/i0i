//! Shared discovery provider contract.
//!
//! This module defines the small interface between `service.rs` and concrete
//! provider adapters such as OpenAlex. It should stay provider-neutral: no
//! OpenAlex URL params, JSON fields, API-key parsing, or HTTP details belong
//! here.

use std::fmt;

use super::error::DiscoveryError;
use crate::domain::discovery::{DiscoverySearchRequest, PaperCandidate};

/// Search providers supported by the discovery layer.
///
/// The enum is intentionally small for now. Add variants here when a provider
/// has a real adapter, not when it is only an idea in an RFC.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiscoveryProviderId {
    OpenAlex,
    Arxiv,
}

impl DiscoveryProviderId {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::OpenAlex => "openalex",
            Self::Arxiv => "arxiv",
        }
    }
}

impl fmt::Display for DiscoveryProviderId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
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

    /// Resolve the provider API key from its configured secret source.
    fn api_key(&self) -> Result<String, DiscoveryError>;

    /// Build the provider request URL from query parameters.
    fn build_url(&self, query_params: &[(&str, String)]) -> Result<String, DiscoveryError>;

    async fn search(
        &self,
        request: &DiscoverySearchRequest,
    ) -> Result<ProviderSearchResult, DiscoveryError>;
}
