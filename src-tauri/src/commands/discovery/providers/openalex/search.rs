//! OpenAlex search provider.
//!
//! This file should stay focused on the OpenAlex-specific part of discovery:
//! build an OpenAlex request from our shared discovery query, call the OpenAlex
//! API, deserialize the response into `remote.rs` structs, and hand each work to
//! `normalize.rs`.
//!
//! Configuration is intentionally not parsed here. The final version should
//! receive its OpenAlex settings from the app's global YAML config, likely as a
//! provider-specific subset.

use super::normalize::normalize_work;
use crate::commands::providers::query::{
    DiscoveryProvider, DiscoveryProviderId, ProviderCapabilities,
};
use reqwest;

/// OpenAlex provider adapter.
///
/// `provider_config` is expected to come from the global app config YAML. This
/// struct should not know how to parse that YAML; it should only use the already
/// loaded OpenAlex subset.
///
/// TODO: Replace `HashMap<String, String>` with the typed provider-config shape
/// once the global config module exists.
pub struct OpenAlexProvider {
    client: reqwest::Client,
    api_key: Option<String>,
    provider_config: HashMap<String, String>, // This is the subset of the global config.
}

#[async_trait]
impl DiscoveryProvider for OpenAlexProvider {
    /// Stable id used by the discovery service to identify this provider.
    fn id(&self) -> DiscoveryProviderId {
        DiscoveryProviderId::OpenAlex
    }

    /// Declare which shared discovery features OpenAlex can support directly.
    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            supports_open_access_filter: true,
            supports_citation_sort: true,
            supports_seed_similarity: false,
            requires_api_key: true,
        }
    }

    /// Resolve the OpenAlex API key from the provider config.
    ///
    /// TODO: This should read from the typed global YAML config instead of
    /// process env directly once config loading is wired.
    fn api_key(&self) -> Result<String> {
        if let Ok(value) = env::var(self.provider_config.api_key) {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                return Ok(trimmed.to_string());
            }
        }
    }

    /// Build an OpenAlex URL by appending encoded query parameters.
    ///
    /// TODO: Change `query_params` from `String` to a list of key/value pairs,
    /// for example `&[(&str, String)]`, because OpenAlex params are structured.
    fn build_url(&self, query_params: String) -> Result<String> {
        let mut url = Url::parse(self.provider_config.url).map_err(|error| error.to_string())?;
        {
            let mut pairs = url.query_pairs_mut();
            for (key, value) in query_params {
                pairs.append_pair(key, value)
            }
        }
        Ok(url)
    }

    /// Search OpenAlex and return normalized provider candidates.
    ///
    /// Intended flow:
    /// 1. validate shared query
    /// 2. translate query fields into OpenAlex query params
    /// 3. call OpenAlex
    /// 4. deserialize into `OpenAlexWorksResponse`
    /// 5. normalize each work with `normalize_work`
    async fn search(&self, query: &DiscoveryQuery) -> Result<ProviderSearchResult, DiscoveryError> {
        /// build OpenAlex params
        /// call OpenAlex
        /// normalize PaperCandiadte
        // Query Processing
        let query_str = query.trim().to_string();
        if query.is_empty() {
            return DiscoveryError("Search query is empty".to_string());
        }

        let api_key = self.api_key();
        let result_limit = self
            .provider_config
            .discovery
            .search_config
            .max_result_limit;

        // Lets skip filters and sorting for now

        let mut query_params = vec![
            ("api_key", api_key),
            ("search", query.clone()),
            ("per-page", result_limit.to_string()),
            (
                "select",
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
                .join(","),
            ),
        ];

        let url = self.build_url(&query_params);
        // get response
        let response = reqwest::Client::new()
            .get(url)
            .header("User-Agent", "ioi/0.1 local Tauri Discovery")
            .send()
            .await
            .map_err(|error| format!("OpenAlex request failed: {error}"))?;

        let status = response.status();

        match status {
            StatusCode::Err => {
                let body = response.text().await.unwrap_or_default();
                return Err(body);
            }
            StatusCode::OK => {
                let payload = response.json::<OpenAlexResponse>().await;
                let candidates = payload
                    .results
                    .into_iter()
                    .map(|work| normalize_work(work, &query))
                    .collect::<Vec<_>>();

                OK(DiscoverySearchResponse {
                    provider: self.id,
                    query,
                    filters,
                    sorty_by: request.sort_by,
                    result_limit,
                    result_count: candidates.len(),
                    candidates,
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn abstract_reconstruction_orders_words_by_position() {
        let mut index = HashMap::new();
        index.insert("world".to_string(), vec![1]);
        index.insert("hello".to_string(), vec![0]);

        assert_eq!(reconstruct_abstract(index), "hello world");
    }

    #[test]
    fn filters_include_year_range_and_open_access() {
        let request = DiscoverySearchRequest {
            query: "sparse autoencoder".to_string(),
            year_from: Some(2023),
            year_to: Some(2026),
            result_limit: 25,
            sort_by: DiscoverySort::Relevance,
            open_access_only: true,
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
    fn openalex_id_uses_work_suffix() {
        assert_eq!(
            openalex_work_suffix(Some("https://openalex.org/W123456")),
            Some("W123456".to_string())
        );
    }

    #[test]
    fn pdf_url_prefers_best_open_access_location() {
        let work = OpenAlexWork {
            id: Some("https://openalex.org/W123456".to_string()),
            doi: None,
            display_name: None,
            publication_year: None,
            publication_date: None,
            cited_by_count: None,
            authorships: None,
            primary_location: Some(OpenAlexLocation {
                landing_page_url: None,
                pdf_url: Some("https://publisher.test/paywalled.pdf".to_string()),
                source: None,
            }),
            best_oa_location: Some(OpenAlexLocation {
                landing_page_url: None,
                pdf_url: Some("https://repository.test/open.pdf".to_string()),
                source: None,
            }),
            open_access: None,
            abstract_inverted_index: None,
            relevance_score: None,
            ids: None,
        };

        assert_eq!(
            choose_openalex_pdf_url(&work).as_deref(),
            Some("https://repository.test/open.pdf")
        );
    }
}
