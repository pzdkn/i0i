//! Semantic Scholar search provider.
//!
//! Builds Semantic Scholar Graph API requests, deserializes JSON responses,
//! filters to open-access papers, and normalizes each into PaperCandidate.

use reqwest::Client;
use tokio::time::{sleep, Duration};

use super::{
    config::SemanticScholarConfig, normalize::normalize_paper, remote::SemanticScholarResponse,
};
use crate::{
    commands::discovery::{
        error::DiscoveryError,
        provider::{DiscoveryProvider, DiscoveryProviderId, ProviderSearchResult},
        providers::shared::{build_query_url, clamp_result_limit},
    },
    domain::discovery::DiscoverySearchRequest,
};

const RATE_LIMIT_RETRY_DELAY_SECS: u64 = 12;
const RATE_LIMIT_MAX_RETRIES: u32 = 3;

/// Semantic Scholar provider adapter.
#[derive(Clone)]
pub struct SemanticScholarProvider {
    client: Client,
    config: SemanticScholarConfig,
}

impl SemanticScholarProvider {
    pub fn from_app_config() -> Result<Self, DiscoveryError> {
        Ok(Self {
            client: Client::new(),
            config: SemanticScholarConfig::load()?,
        })
    }
}

impl DiscoveryProvider for SemanticScholarProvider {
    fn id(&self) -> DiscoveryProviderId {
        DiscoveryProviderId::SemanticScholar
    }

    /// Returns the API key, or an empty string if none is configured.
    ///
    /// Semantic Scholar allows unauthenticated access — a missing key is not an
    /// error, only a rate-limit reduction.
    fn api_key(&self) -> Result<String, DiscoveryError> {
        Ok(self.config.resolve_api_key())
    }

    fn build_url(&self, query_params: &[(&str, String)]) -> Result<String, DiscoveryError> {
        build_query_url(&self.config.url, query_params)
    }

    async fn search(
        &self,
        request: &DiscoverySearchRequest,
    ) -> Result<ProviderSearchResult, DiscoveryError> {
        let limit = clamp_result_limit(request.result_limit, self.config.max_result_limit());
        let query_params = ss_query_params(request, limit);
        let url = self.build_url(&query_params)?;

        let api_key = self.api_key()?;
        let payload = ss_fetch_with_retry(&self.client, &url, &api_key).await?;

        let filters = ss_active_filters(request);

        let candidates = payload
            .data
            .into_iter()
            .filter(|paper| paper.is_open_access.unwrap_or(false))
            .map(|paper| normalize_paper(paper, request.query.trim()))
            .collect();

        Ok(ProviderSearchResult {
            provider: self.id(),
            filters,
            candidates,
        })
    }
}

/// Fetch from Semantic Scholar, retrying on 429 with a fixed backoff.
///
/// Unauthenticated requests are limited to ~1 req/10s. On 429 we wait
/// RATE_LIMIT_RETRY_DELAY_SECS and retry up to RATE_LIMIT_MAX_RETRIES times
/// before returning a user-friendly error that suggests getting an API key.
async fn ss_fetch_with_retry(
    client: &Client,
    url: &str,
    api_key: &str,
) -> Result<SemanticScholarResponse, DiscoveryError> {
    for attempt in 0..=RATE_LIMIT_MAX_RETRIES {
        let mut builder = client
            .get(url)
            .header("User-Agent", "ioi/0.1 local Tauri Discovery");

        if !api_key.is_empty() {
            builder = builder.header("x-api-key", api_key);
        }

        let response = builder
            .send()
            .await
            .map_err(|e| DiscoveryError::new(format!("Semantic Scholar request failed: {e}")))?;

        let status = response.status();

        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            if attempt < RATE_LIMIT_MAX_RETRIES {
                sleep(Duration::from_secs(RATE_LIMIT_RETRY_DELAY_SECS)).await;
                continue;
            }
            return Err(DiscoveryError::new(
                "Semantic Scholar rate limit exceeded. Add a SEMANTIC_SCHOLAR_API_KEY to your \
                 .env for higher limits: https://www.semanticscholar.org/product/api#api-key-form"
                    .to_string(),
            ));
        }

        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(DiscoveryError::new(format!(
                "Semantic Scholar request failed with {status}: {body}"
            )));
        }

        return response
            .json::<SemanticScholarResponse>()
            .await
            .map_err(|e| DiscoveryError::new(format!("Invalid Semantic Scholar response: {e}")));
    }

    unreachable!()
}

/// Build the Semantic Scholar `year` filter string from optional year bounds.
///
/// The Semantic Scholar API documents `YYYY`, `YYYY-`, and `YYYY-YYYY`. The
/// to-only form `-YYYY` (leading hyphen) is not in the official spec, so we
/// use `1000-YYYY` as a safe open lower bound instead.
fn ss_year_filter(year_from: Option<i32>, year_to: Option<i32>) -> Option<String> {
    match (year_from, year_to) {
        (None, None) => None,
        (Some(f), None) => Some(format!("{f}-")),
        (None, Some(t)) => Some(format!("1000-{t}")),
        (Some(f), Some(t)) => Some(format!("{f}-{t}")),
    }
}

/// Fields to request from the Semantic Scholar API.
///
/// Requesting only the fields we use keeps payloads small and avoids
/// accidentally relying on fields that may change.
fn ss_fields() -> &'static str {
    "paperId,externalIds,title,abstract,year,publicationDate,\
     authors,venue,citationCount,influentialCitationCount,\
     isOpenAccess,openAccessPdf,tldr,url"
}

/// Build Semantic Scholar query parameters from an app-level search request.
fn ss_query_params(request: &DiscoverySearchRequest, limit: i32) -> Vec<(&'static str, String)> {
    let mut params = vec![
        ("query", request.query.trim().to_string()),
        ("limit", limit.to_string()),
        ("offset", "0".to_string()),
        ("fields", ss_fields().to_string()),
    ];

    if let Some(year) = ss_year_filter(request.year_from, request.year_to) {
        params.push(("year", year));
    }

    params
}

/// Build a human-readable list of active filters for the run summary.
///
/// Semantic Scholar has no sort parameter — all three DiscoverySort variants
/// produce relevance-ordered results.
fn ss_active_filters(request: &DiscoverySearchRequest) -> Vec<String> {
    let mut filters = Vec::new();

    if let Some(year_from) = request.year_from {
        filters.push(format!("from_year:{year_from}"));
    }
    if let Some(year_to) = request.year_to {
        filters.push(format!("to_year:{year_to}"));
    }

    filters.push("is_oa:true".to_string());

    filters
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::discovery::{DiscoveryProviderChoice, DiscoverySort};

    fn base_request() -> DiscoverySearchRequest {
        DiscoverySearchRequest {
            query: "sparse autoencoder".to_string(),
            year_from: None,
            year_to: None,
            result_limit: 25,
            sort_by: DiscoverySort::Relevance,
            provider: DiscoveryProviderChoice::SemanticScholar,
            venues: Vec::new(),
            authors: Vec::new(),
            fields_of_study: Vec::new(),
        }
    }

    // --- ss_year_filter ---

    #[test]
    fn year_filter_none_when_no_bounds() {
        assert_eq!(ss_year_filter(None, None), None);
    }

    #[test]
    fn year_filter_from_only() {
        assert_eq!(ss_year_filter(Some(2020), None), Some("2020-".to_string()));
    }

    #[test]
    fn year_filter_to_only() {
        assert_eq!(
            ss_year_filter(None, Some(2024)),
            Some("1000-2024".to_string())
        );
    }

    #[test]
    fn year_filter_both_bounds() {
        assert_eq!(
            ss_year_filter(Some(2020), Some(2024)),
            Some("2020-2024".to_string())
        );
    }

    // --- ss_query_params ---

    #[test]
    fn query_params_include_required_keys() {
        let params = ss_query_params(&base_request(), 25);
        let keys: Vec<&str> = params.iter().map(|(k, _)| *k).collect();
        assert!(keys.contains(&"query"));
        assert!(keys.contains(&"limit"));
        assert!(keys.contains(&"offset"));
        assert!(keys.contains(&"fields"));
    }

    #[test]
    fn query_params_no_sort_for_relevance() {
        let params = ss_query_params(&base_request(), 25);
        assert!(!params.iter().any(|(k, _)| *k == "sort"));
    }

    #[test]
    fn query_params_no_sort_for_newest() {
        let mut req = base_request();
        req.sort_by = DiscoverySort::Newest;
        let params = ss_query_params(&req, 25);
        assert!(!params.iter().any(|(k, _)| *k == "sort"));
    }

    #[test]
    fn query_params_no_sort_for_most_cited() {
        let mut req = base_request();
        req.sort_by = DiscoverySort::MostCited;
        let params = ss_query_params(&req, 25);
        assert!(!params.iter().any(|(k, _)| *k == "sort"));
    }

    #[test]
    fn query_params_include_year_when_set() {
        let mut req = base_request();
        req.year_from = Some(2020);
        req.year_to = Some(2024);
        let params = ss_query_params(&req, 25);
        let year = params
            .iter()
            .find(|(k, _)| *k == "year")
            .map(|(_, v)| v.as_str());
        assert_eq!(year, Some("2020-2024"));
    }

    #[test]
    fn query_params_no_year_when_not_set() {
        let params = ss_query_params(&base_request(), 25);
        assert!(!params.iter().any(|(k, _)| *k == "year"));
    }

    #[test]
    fn query_params_respect_limit() {
        let params = ss_query_params(&base_request(), 10);
        let limit = params
            .iter()
            .find(|(k, _)| *k == "limit")
            .map(|(_, v)| v.as_str());
        assert_eq!(limit, Some("10"));
    }

    #[test]
    fn fields_param_includes_essential_fields() {
        let params = ss_query_params(&base_request(), 25);
        let fields = params
            .iter()
            .find(|(k, _)| *k == "fields")
            .map(|(_, v)| v.as_str())
            .unwrap_or("");
        assert!(fields.contains("paperId"));
        assert!(fields.contains("abstract"));
        assert!(fields.contains("tldr"));
        assert!(fields.contains("influentialCitationCount"));
        assert!(fields.contains("isOpenAccess"));
    }
}
