//! OpenAlex search provider.
//!
//! This adapter owns the OpenAlex-specific work: build an OpenAlex request,
//! deserialize OpenAlex JSON, and normalize each work into the app's discovery
//! domain.

use reqwest::{Client, StatusCode};
use url::Url;

use super::{config::OpenAlexConfig, normalize::normalize_work, remote::OpenAlexWorksResponse};
use crate::{
    commands::discovery::{
        error::DiscoveryError,
        provider::{DiscoveryProvider, DiscoveryProviderId, ProviderSearchResult},
    },
    domain::discovery::{DiscoverySearchRequest, DiscoverySort},
};

/// OpenAlex provider adapter.
#[derive(Clone)]
pub struct OpenAlexProvider {
    client: Client,
    config: OpenAlexConfig,
}

impl OpenAlexProvider {
    pub fn from_app_config() -> Result<Self, DiscoveryError> {
        Ok(Self {
            client: Client::new(),
            config: OpenAlexConfig::load()?,
        })
    }

    fn result_limit(&self, request: &DiscoverySearchRequest) -> i32 {
        let max_limit = self.config.max_result_limit().max(1);
        request.result_limit.clamp(1, max_limit)
    }
}

impl DiscoveryProvider for OpenAlexProvider {
    /// Stable id used by the discovery service to identify this provider.
    fn id(&self) -> DiscoveryProviderId {
        DiscoveryProviderId::OpenAlex
    }

    /// Resolve the OpenAlex API key from the configured env-var name.
    fn api_key(&self) -> Result<String, DiscoveryError> {
        self.config.resolve_api_key()
    }

    /// Build an OpenAlex URL by appending encoded query parameters.
    fn build_url(&self, query_params: &[(&str, String)]) -> Result<String, DiscoveryError> {
        let mut url = Url::parse(&self.config.url)
            .map_err(|error| DiscoveryError::new(format!("Invalid OpenAlex URL: {error}")))?;

        {
            let mut pairs = url.query_pairs_mut();
            for (key, value) in query_params {
                pairs.append_pair(key, value);
            }
        }

        Ok(url.to_string())
    }

    /// Search OpenAlex and return normalized provider candidates.
    async fn search(
        &self,
        request: &DiscoverySearchRequest,
    ) -> Result<ProviderSearchResult, DiscoveryError> {
        let filters = openalex_filters(request);
        let query_params = openalex_query_params(
            self.api_key()?,
            request,
            self.result_limit(request),
            &filters,
        );
        let url = self.build_url(&query_params)?;
        let response = self
            .client
            .get(url)
            .header("User-Agent", "ioi/0.1 local Tauri Discovery")
            .send()
            .await
            .map_err(|error| DiscoveryError::new(format!("OpenAlex request failed: {error}")))?;

        let status = response.status();
        if status != StatusCode::OK {
            let body = response.text().await.unwrap_or_default();
            return Err(DiscoveryError::new(format!(
                "OpenAlex request failed with {status}: {body}"
            )));
        }

        let payload = response
            .json::<OpenAlexWorksResponse>()
            .await
            .map_err(|error| DiscoveryError::new(format!("Invalid OpenAlex response: {error}")))?;
        let candidates = payload
            .results
            .into_iter()
            .map(|work| normalize_work(work, request.query.trim()))
            .collect();

        Ok(ProviderSearchResult {
            provider: self.id(),
            filters,
            candidates,
        })
    }
}

// OpenAlex request construction.
//
// These helpers translate the app-level `DiscoverySearchRequest` into OpenAlex
// query parameters. Keeping them together makes the provider's HTTP call easier
// to scan: search() orchestrates the request, these functions define its shape.
fn openalex_query_params(
    api_key: String,
    request: &DiscoverySearchRequest,
    result_limit: i32,
    filters: &[String],
) -> Vec<(&'static str, String)> {
    let mut query_params = vec![
        ("api_key", api_key),
        ("search", request.query.trim().to_string()),
        ("per-page", result_limit.to_string()),
        ("select", openalex_select_fields().join(",")),
    ];

    if !filters.is_empty() {
        query_params.push(("filter", filters.join(",")));
    }

    if let Some(sort) = openalex_sort(request.sort_by.clone()) {
        query_params.push(("sort", sort.to_string()));
    }

    query_params
}

/// Limit the OpenAlex payload to the fields our normalizer actually reads.
fn openalex_select_fields() -> [&'static str; 13] {
    [
        "id",
        "doi",
        "display_name",
        "publication_year",
        "publication_date",
        "cited_by_count",
        "authorships",
        "primary_location",
        "best_oa_location",
        "open_access",
        "abstract_inverted_index",
        "relevance_score",
        "ids",
    ]
}

/// Convert shared discovery filters into OpenAlex's comma-separated filter API.
fn openalex_filters(request: &DiscoverySearchRequest) -> Vec<String> {
    let mut filters = Vec::new();

    if let Some(year_from) = request.year_from {
        filters.push(format!("from_publication_date:{year_from}-01-01"));
    }

    if let Some(year_to) = request.year_to {
        filters.push(format!("to_publication_date:{year_to}-12-31"));
    }

    filters.push("is_oa:true".to_string());

    filters
}

/// OpenAlex defaults to relevance for search queries, so relevance needs no
/// explicit `sort` parameter.
fn openalex_sort(sort: DiscoverySort) -> Option<&'static str> {
    match sort {
        DiscoverySort::Relevance => None,
        DiscoverySort::Newest => Some("publication_date:desc"),
        DiscoverySort::MostCited => Some("cited_by_count:desc"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filters_include_year_range_and_open_access() {
        let request = DiscoverySearchRequest {
            query: "sparse autoencoder".to_string(),
            year_from: Some(2023),
            year_to: Some(2026),
            result_limit: 25,
            sort_by: DiscoverySort::Relevance,
            provider: Default::default(),
        };

        assert_eq!(
            openalex_filters(&request),
            vec![
                "from_publication_date:2023-01-01",
                "to_publication_date:2026-12-31",
                "is_oa:true"
            ]
        );
    }
}
