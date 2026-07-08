//! OpenAlex search provider.
//!
//! This adapter owns the OpenAlex-specific work: build an OpenAlex request,
//! deserialize OpenAlex JSON, and normalize each work into the app's discovery
//! domain.

use reqwest::{Client, StatusCode};

use super::{config::OpenAlexConfig, normalize::normalize_work, remote::OpenAlexWorksResponse};
use crate::{
    commands::discovery::{
        error::DiscoveryError,
        provider::{DiscoveryProvider, DiscoveryProviderId, ProviderSearchResult},
        providers::shared::{build_query_url, clamp_result_limit},
    },
    domain::discovery::{DiscoverySearchRequest, DiscoverySort, Lineage},
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

    fn build_url(&self, query_params: &[(&str, String)]) -> Result<String, DiscoveryError> {
        build_query_url(&self.config.url, query_params)
    }

    /// Search OpenAlex and return normalized provider candidates.
    async fn search(
        &self,
        request: &DiscoverySearchRequest,
    ) -> Result<ProviderSearchResult, DiscoveryError> {
        let filters = openalex_filters(request);
        let limit = clamp_result_limit(request.result_limit, self.config.max_result_limit());
        let query_params = openalex_query_params(self.api_key()?, request, limit, &filters);
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

    if request.open_access {
        filters.push("is_oa:true".to_string());
    }

    // Structured filters applied at query time. Multiple values for one key are
    // OR-joined with `|` (OpenAlex's within-key OR); distinct keys are AND-joined
    // by the caller via the comma-separated `filter` param.
    if !request.venues.is_empty() {
        filters.push(format!(
            "primary_location.source.display_name.search:{}",
            request.venues.join("|")
        ));
    }
    if !request.authors.is_empty() {
        filters.push(format!(
            "authorships.author.display_name.search:{}",
            request.authors.join("|")
        ));
    }
    if !request.fields_of_study.is_empty() {
        filters.push(format!(
            "concepts.display_name.search:{}",
            request.fields_of_study.join("|")
        ));
    }

    filters
}

/// Build the OpenAlex `filter` value that traverses the citation graph from a
/// seed work. The OpenAlex filter spelling is the inverse of the intent name:
/// `cited_by:<id>` returns the works a paper *references*, and `cites:<id>`
/// returns the works that *cite* it.
// Wired into RealCandidateSource in the RFC 0037 seams layer.
#[allow(dead_code)]
fn openalex_lineage_filter(work_id: &str, lineage: Lineage) -> String {
    match lineage {
        Lineage::References => format!("cited_by:{work_id}"),
        Lineage::Citations => format!("cites:{work_id}"),
    }
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
            providers: Vec::new(),
            open_access: true,
            venues: Vec::new(),
            authors: Vec::new(),
            fields_of_study: Vec::new(),
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

    #[test]
    fn lineage_references_uses_cited_by_filter() {
        // References = works this paper cites -> OpenAlex `cited_by:<id>`.
        assert_eq!(
            openalex_lineage_filter("W123", Lineage::References),
            "cited_by:W123"
        );
    }

    #[test]
    fn lineage_citations_uses_cites_filter() {
        // Citations = works that cite this paper -> OpenAlex `cites:<id>`.
        assert_eq!(
            openalex_lineage_filter("W123", Lineage::Citations),
            "cites:W123"
        );
    }

    #[test]
    fn filters_include_structured_venue_author_and_field() {
        let request = DiscoverySearchRequest {
            query: "interpretability".to_string(),
            year_from: None,
            year_to: None,
            result_limit: 25,
            sort_by: DiscoverySort::Relevance,
            provider: Default::default(),
            providers: Vec::new(),
            open_access: true,
            venues: vec!["NeurIPS".to_string(), "ICML".to_string()],
            authors: vec!["Yoshua Bengio".to_string()],
            fields_of_study: vec!["computer science".to_string()],
        };

        assert_eq!(
            openalex_filters(&request),
            vec![
                "is_oa:true",
                "primary_location.source.display_name.search:NeurIPS|ICML",
                "authorships.author.display_name.search:Yoshua Bengio",
                "concepts.display_name.search:computer science",
            ]
        );
    }
}
