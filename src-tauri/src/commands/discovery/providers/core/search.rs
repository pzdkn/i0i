//! CORE search provider (RFC 0053).
//!
//! CORE is a domain-general open-access aggregator. It requires a free API key;
//! when the key is absent the provider reports itself unconfigured and the
//! orchestrator skips it (no user-facing error). Not live-verified in this
//! environment — the request/response handling is deliberately tolerant.

use reqwest::{Client, StatusCode};

use super::{config::CoreConfig, normalize::normalize_work, remote::CoreSearchResponse};
use crate::{
    commands::discovery::{
        error::DiscoveryError,
        provider::{DiscoveryProvider, DiscoveryProviderId, ProviderSearchResult},
        providers::shared::{build_query_url, clamp_result_limit},
    },
    domain::discovery::DiscoverySearchRequest,
};

/// CORE provider adapter.
#[derive(Clone)]
pub struct CoreProvider {
    client: Client,
    config: CoreConfig,
}

impl CoreProvider {
    pub fn from_app_config() -> Result<Self, DiscoveryError> {
        Ok(Self {
            client: Client::new(),
            config: CoreConfig::load()?,
        })
    }

    /// Whether a usable API key is present. The orchestrator uses this to skip
    /// CORE silently instead of surfacing a key-missing provider error.
    pub fn is_configured(&self) -> bool {
        self.config.resolve_api_key().is_ok()
    }
}

impl DiscoveryProvider for CoreProvider {
    fn id(&self) -> DiscoveryProviderId {
        DiscoveryProviderId::Core
    }

    fn api_key(&self) -> Result<String, DiscoveryError> {
        self.config.resolve_api_key()
    }

    fn build_url(&self, query_params: &[(&str, String)]) -> Result<String, DiscoveryError> {
        build_query_url(&self.config.url, query_params)
    }

    async fn search(
        &self,
        request: &DiscoverySearchRequest,
    ) -> Result<ProviderSearchResult, DiscoveryError> {
        let api_key = self.api_key()?;
        let limit = clamp_result_limit(request.result_limit, self.config.max_result_limit());
        let query_params = vec![("q", core_query(request)), ("limit", limit.to_string())];
        let url = self.build_url(&query_params)?;

        let response = self
            .client
            .get(url)
            .bearer_auth(api_key)
            .header("User-Agent", "ioi/0.1 local Tauri Discovery")
            .header("Accept", "application/json")
            .send()
            .await
            .map_err(|error| DiscoveryError::new(format!("CORE request failed: {error}")))?;

        let status = response.status();
        if status != StatusCode::OK {
            let body = response.text().await.unwrap_or_default();
            return Err(DiscoveryError::new(format!(
                "CORE request failed with {status}: {body}"
            )));
        }

        let payload = response
            .json::<CoreSearchResponse>()
            .await
            .map_err(|error| DiscoveryError::new(format!("Invalid CORE response: {error}")))?;

        let filters = core_active_filters(request);
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

/// Build the CORE `q` string, embedding the year range in CORE's query syntax.
///
/// Like arXiv, CORE has no venue/field mapping from our free-text inputs, and
/// its author query syntax is not verified here, so only the year range is
/// embedded; open access is implicit (CORE is OA-only). See RFC 0053.
fn core_query(request: &DiscoverySearchRequest) -> String {
    let mut query = request.query.trim().to_string();
    if let Some(year_from) = request.year_from {
        query.push_str(&format!(" AND yearPublished>={year_from}"));
    }
    if let Some(year_to) = request.year_to {
        query.push_str(&format!(" AND yearPublished<={year_to}"));
    }
    query
}

fn core_active_filters(request: &DiscoverySearchRequest) -> Vec<String> {
    let mut filters = Vec::new();
    if let Some(year_from) = request.year_from {
        filters.push(format!("from_year:{year_from}"));
    }
    if let Some(year_to) = request.year_to {
        filters.push(format!("to_year:{year_to}"));
    }
    // CORE only indexes open-access works.
    filters.push("is_oa:always".to_string());
    filters
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::discovery::{DiscoveryProviderChoice, DiscoverySort};

    fn base_request() -> DiscoverySearchRequest {
        DiscoverySearchRequest {
            query: "deep learning".to_string(),
            year_from: None,
            year_to: None,
            result_limit: 25,
            sort_by: DiscoverySort::Relevance,
            provider: DiscoveryProviderChoice::Core,
            providers: Vec::new(),
            open_access: true,
            only_viewable: false,
            venues: Vec::new(),
            authors: Vec::new(),
            fields_of_study: Vec::new(),
        }
    }

    #[test]
    fn plain_query_passed_through() {
        assert_eq!(core_query(&base_request()), "deep learning");
    }

    #[test]
    fn year_range_embedded() {
        let mut request = base_request();
        request.year_from = Some(2020);
        request.year_to = Some(2024);
        assert_eq!(
            core_query(&request),
            "deep learning AND yearPublished>=2020 AND yearPublished<=2024"
        );
    }

    #[tokio::test]
    #[ignore = "live network + CORE_API_KEY"]
    async fn live_search_returns_candidates() {
        let provider = CoreProvider::from_app_config().unwrap();
        assert!(provider.is_configured(), "CORE_API_KEY must be set");
        let result = provider.search(&base_request()).await.unwrap();
        eprintln!("core live candidates: {}", result.candidates.len());
        assert!(!result.candidates.is_empty());
        let with_pdf = result
            .candidates
            .iter()
            .filter(|candidate| candidate.pdf_url.is_some())
            .count();
        let sample = &result.candidates[0];
        eprintln!(
            "with_pdf={with_pdf} sample: title={:?} year={:?} doi={:?} pdf={:?}",
            sample.title, sample.year, sample.doi, sample.pdf_url
        );
    }
}
