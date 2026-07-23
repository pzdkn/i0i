//! Europe PMC search provider (RFC 0053).
//!
//! Builds Europe PMC REST search requests, parses the JSON response, and
//! normalizes each result into the shared PaperCandidate type. Europe PMC
//! unifies PubMed + PMC + preprints and exposes full-text PDF/HTML links, so
//! its results arrive with an obtainable view where one exists.

use reqwest::{Client, StatusCode};

use super::{config::EuropePmcConfig, normalize::normalize_result, remote::EuropePmcResponse};
use crate::{
    commands::discovery::{
        error::DiscoveryError,
        provider::{DiscoveryProvider, DiscoveryProviderId, ProviderSearchResult},
        providers::shared::{build_query_url, clamp_result_limit},
    },
    domain::discovery::{DiscoverySearchRequest, DiscoverySort},
};

/// Europe PMC provider adapter.
#[derive(Clone)]
pub struct EuropePmcProvider {
    client: Client,
    config: EuropePmcConfig,
}

impl EuropePmcProvider {
    pub fn from_app_config() -> Result<Self, DiscoveryError> {
        Ok(Self {
            client: Client::new(),
            config: EuropePmcConfig::load()?,
        })
    }
}

impl DiscoveryProvider for EuropePmcProvider {
    fn id(&self) -> DiscoveryProviderId {
        DiscoveryProviderId::EuropePmc
    }

    /// Europe PMC requires no API key. This always returns an empty string.
    fn api_key(&self) -> Result<String, DiscoveryError> {
        Ok(String::new())
    }

    fn build_url(&self, query_params: &[(&str, String)]) -> Result<String, DiscoveryError> {
        build_query_url(&self.config.url, query_params)
    }

    async fn search(
        &self,
        request: &DiscoverySearchRequest,
    ) -> Result<ProviderSearchResult, DiscoveryError> {
        let limit = clamp_result_limit(request.result_limit, self.config.max_result_limit());
        let query_params = europe_pmc_query_params(request, limit);
        let url = self.build_url(&query_params)?;

        let response = self
            .client
            .get(url)
            .header("User-Agent", "ioi/0.1 local Tauri Discovery (mailto)")
            .header("Accept", "application/json")
            .send()
            .await
            .map_err(|error| DiscoveryError::new(format!("Europe PMC request failed: {error}")))?;

        let status = response.status();
        if status != StatusCode::OK {
            let body = response.text().await.unwrap_or_default();
            return Err(DiscoveryError::new(format!(
                "Europe PMC request failed with {status}: {body}"
            )));
        }

        let payload = response
            .json::<EuropePmcResponse>()
            .await
            .map_err(|error| {
                DiscoveryError::new(format!("Invalid Europe PMC response: {error}"))
            })?;

        let filters = europe_pmc_active_filters(request);
        let candidates = payload
            .result_list
            .result
            .into_iter()
            .map(|result| normalize_result(result, request.query.trim()))
            .collect();

        Ok(ProviderSearchResult {
            provider: self.id(),
            filters,
            candidates,
        })
    }
}

/// Build Europe PMC query parameters from an app-level search request.
fn europe_pmc_query_params(
    request: &DiscoverySearchRequest,
    limit: i32,
) -> Vec<(&'static str, String)> {
    let query = europe_pmc_query(request);
    let mut params = vec![
        ("query", query),
        ("format", "json".to_string()),
        ("resultType", "core".to_string()),
        ("pageSize", limit.to_string()),
    ];
    if let Some(sort) = europe_pmc_sort(request.sort_by.clone()) {
        params.push(("sort", sort.to_string()));
    }
    params
}

/// Assemble the Europe PMC `query` string, embedding year / open-access /
/// author constraints in Europe PMC's field syntax.
///
/// Europe PMC has no separate venue or field-of-study filter that maps cleanly
/// from our free-text inputs, so (like arXiv) only year, open access, and
/// authors are applied here; venues and fields_of_study are honored on
/// OpenAlex instead (RFC 0053).
fn europe_pmc_query(request: &DiscoverySearchRequest) -> String {
    let mut query = request.query.trim().to_string();

    match (request.year_from, request.year_to) {
        (None, None) => {}
        (from, to) => {
            let from = from
                .map(|y| y.to_string())
                .unwrap_or_else(|| "1800".to_string());
            let to = to
                .map(|y| y.to_string())
                .unwrap_or_else(|| "3000".to_string());
            query.push_str(&format!(" AND (PUB_YEAR:[{from} TO {to}])"));
        }
    }

    if request.open_access {
        query.push_str(" AND (OPEN_ACCESS:y)");
    }

    if !request.authors.is_empty() {
        let clause = request
            .authors
            .iter()
            .map(|author| format!("AUTH:\"{author}\""))
            .collect::<Vec<_>>()
            .join(" OR ");
        query.push_str(&format!(" AND ({clause})"));
    }

    query
}

/// Map a shared sort to Europe PMC's `sort` parameter. Relevance is the default
/// (no sort parameter).
fn europe_pmc_sort(sort: DiscoverySort) -> Option<&'static str> {
    match sort {
        DiscoverySort::Relevance => None,
        DiscoverySort::Newest => Some("P_PDATE_D desc"),
        DiscoverySort::MostCited => Some("CITED desc"),
    }
}

/// Build a human-readable list of active filters for the run summary.
fn europe_pmc_active_filters(request: &DiscoverySearchRequest) -> Vec<String> {
    let mut filters = Vec::new();
    if let Some(year_from) = request.year_from {
        filters.push(format!("from_year:{year_from}"));
    }
    if let Some(year_to) = request.year_to {
        filters.push(format!("to_year:{year_to}"));
    }
    if request.open_access {
        filters.push("is_oa:true".to_string());
    }
    filters
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::discovery::DiscoveryProviderChoice;

    fn base_request() -> DiscoverySearchRequest {
        DiscoverySearchRequest {
            query: "crispr".to_string(),
            year_from: None,
            year_to: None,
            result_limit: 25,
            sort_by: DiscoverySort::Relevance,
            provider: DiscoveryProviderChoice::EuropePmc,
            providers: Vec::new(),
            open_access: false,
            only_viewable: false,
            venues: Vec::new(),
            authors: Vec::new(),
            fields_of_study: Vec::new(),
        }
    }

    #[test]
    fn plain_query_is_passed_through() {
        assert_eq!(europe_pmc_query(&base_request()), "crispr");
    }

    #[test]
    fn open_access_appends_oa_clause() {
        let mut request = base_request();
        request.open_access = true;
        assert_eq!(europe_pmc_query(&request), "crispr AND (OPEN_ACCESS:y)");
    }

    #[test]
    fn year_range_embedded_in_query() {
        let mut request = base_request();
        request.year_from = Some(2023);
        request.year_to = Some(2026);
        assert_eq!(
            europe_pmc_query(&request),
            "crispr AND (PUB_YEAR:[2023 TO 2026])"
        );
    }

    #[test]
    fn authors_embedded_as_auth_clause() {
        let mut request = base_request();
        request.authors = vec!["Jennifer Doudna".to_string()];
        assert_eq!(
            europe_pmc_query(&request),
            "crispr AND (AUTH:\"Jennifer Doudna\")"
        );
    }

    #[test]
    fn sort_newest_maps_to_date_desc() {
        assert_eq!(
            europe_pmc_sort(DiscoverySort::Newest),
            Some("P_PDATE_D desc")
        );
    }

    #[test]
    fn sort_relevance_has_no_sort_param() {
        assert_eq!(europe_pmc_sort(DiscoverySort::Relevance), None);
    }

    #[test]
    fn query_params_include_core_result_type() {
        let params = europe_pmc_query_params(&base_request(), 25);
        assert!(params
            .iter()
            .any(|(key, value)| *key == "resultType" && value == "core"));
        assert!(params
            .iter()
            .any(|(key, value)| *key == "pageSize" && value == "25"));
    }
}
