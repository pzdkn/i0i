//! arXiv search provider.
//!
//! Builds arXiv Atom API requests, parses the XML response, and normalizes
//! each entry into the app's shared PaperCandidate type.

use super::{config::ArxivConfig, normalize::normalize_entry, remote::parse_feed};
use crate::{
    commands::discovery::{
        error::DiscoveryError,
        provider::{DiscoveryProvider, DiscoveryProviderId, ProviderSearchResult},
        providers::shared::{build_query_url, clamp_result_limit},
    },
    domain::discovery::{DiscoverySearchRequest, DiscoverySort},
};
use reqwest::Client;

/// arXiv provider adapter.
#[derive(Clone)]
pub struct ArxivProvider {
    client: Client,
    config: ArxivConfig,
}

impl ArxivProvider {
    pub fn from_app_config() -> Result<Self, DiscoveryError> {
        Ok(Self {
            client: Client::new(),
            config: ArxivConfig::load()?,
        })
    }
}

impl DiscoveryProvider for ArxivProvider {
    fn id(&self) -> DiscoveryProviderId {
        DiscoveryProviderId::Arxiv
    }

    /// arXiv requires no API key. This always returns an empty string.
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
        let query_params = arxiv_query_params(request, limit);
        let url = self.build_url(&query_params)?;

        let response = self
            .client
            .get(url)
            .header("User-Agent", "ioi/0.1 local Tauri Discovery")
            .send()
            .await
            .map_err(|error| DiscoveryError::new(format!("arXiv request failed: {error}")))?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(DiscoveryError::new(format!(
                "arXiv request failed with {status}: {body}"
            )));
        }

        let xml = response.text().await.map_err(|error| {
            DiscoveryError::new(format!("Failed to read arXiv response: {error}"))
        })?;

        let filters = arxiv_active_filters(request);
        let candidates = parse_feed(&xml)
            .map_err(|e| DiscoveryError::new(format!("Failed to parse arXiv response: {e}")))?
            .into_iter()
            .map(|entry| normalize_entry(entry, request.query.trim()))
            .collect();

        Ok(ProviderSearchResult {
            provider: self.id(),
            filters,
            candidates,
        })
    }
}

/// Build arXiv query parameters from an app-level search request.
fn arxiv_query_params(request: &DiscoverySearchRequest, limit: i32) -> Vec<(&'static str, String)> {
    let query = format_search_query(request.query.trim());
    let search_query = embed_year_filter(&query, request.year_from, request.year_to);
    // Structured-filter asymmetry (RFC 0037): arXiv supports author search but has
    // no venue concept, and its field filter is a category *code* (`cat:cs.LG`) that
    // our free-text `fields_of_study` cannot reliably express — so only authors are
    // applied here; venues and fields_of_study are honored on OpenAlex instead.
    let search_query = embed_authors(&search_query, &request.authors);
    let (sort_by, sort_order) = arxiv_sort(request.sort_by.clone());

    vec![
        ("search_query", search_query),
        ("start", "0".to_string()),
        ("max_results", limit.to_string()),
        ("sortBy", sort_by.to_string()),
        ("sortOrder", sort_order.to_string()),
    ]
}

/// Convert natural search text to arXiv's documented all-fields grammar.
fn format_search_query(query: &str) -> String {
    if has_arxiv_field(query) {
        return query.to_string();
    }
    if looks_like_arxiv_id(query) {
        return format!("id:{query}");
    }

    let terms = split_query_terms(query);
    let all_fields = terms
        .iter()
        .map(|term| format!("all:\"{}\"", term.replace('"', "")))
        .collect::<Vec<_>>()
        .join(" AND ");
    if terms.len() < 2 {
        return all_fields;
    }

    let phrase = terms.join(" ").replace('"', "");
    format!("(ti:\"{phrase}\" OR ({all_fields}))")
}

/// Recognize modern and legacy bare arXiv identifiers.
fn looks_like_arxiv_id(query: &str) -> bool {
    let id = query
        .trim()
        .trim_start_matches("arXiv:")
        .split_once('v')
        .map(|(base, _)| base)
        .unwrap_or(query.trim());
    if let Some((year_month, number)) = id.split_once('.') {
        return year_month.len() == 4
            && number.len() >= 4
            && year_month
                .chars()
                .all(|character| character.is_ascii_digit())
            && number.chars().all(|character| character.is_ascii_digit());
    }
    id.split_once('/').is_some_and(|(archive, number)| {
        !archive.is_empty()
            && archive
                .chars()
                .all(|character| character.is_ascii_alphabetic() || "-.".contains(character))
            && number.len() == 7
            && number.chars().all(|character| character.is_ascii_digit())
    })
}

/// Return whether the caller already supplied an arXiv fielded query.
fn has_arxiv_field(query: &str) -> bool {
    const PREFIXES: [&str; 9] = [
        "ti:", "au:", "abs:", "co:", "jr:", "cat:", "rn:", "id:", "all:",
    ];
    query.split_whitespace().any(|term| {
        let term = term.trim_start_matches('(').to_ascii_lowercase();
        PREFIXES.iter().any(|prefix| term.starts_with(prefix))
    })
}

/// Split natural text on spaces while preserving quoted phrases as one term.
fn split_query_terms(query: &str) -> Vec<String> {
    let mut terms = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    for character in query.chars() {
        match character {
            '"' => quoted = !quoted,
            character if character.is_whitespace() && !quoted => {
                if !current.is_empty() {
                    terms.push(std::mem::take(&mut current));
                }
            }
            _ => current.push(character),
        }
    }
    if !current.is_empty() {
        terms.push(current);
    }
    terms
}

/// Embed the year range as a date filter inside the arXiv query string.
///
/// arXiv has no separate year-filter parameter. The date constraint must be
/// appended to `search_query` using Lucene range syntax. When neither bound is
/// set the query is returned unchanged.
fn embed_year_filter(query: &str, year_from: Option<i32>, year_to: Option<i32>) -> String {
    match (year_from, year_to) {
        (None, None) => query.to_string(),
        _ => {
            let from = year_from
                .map(|y| format!("{y}0101"))
                .unwrap_or_else(|| "*".to_string());
            let to = year_to
                .map(|y| format!("{y}1231"))
                .unwrap_or_else(|| "*".to_string());
            format!("{query} AND submittedDate:[{from} TO {to}]")
        }
    }
}

/// Embed author constraints into the arXiv `search_query` as an OR-group of
/// `au:` clauses ANDed onto the base query. Empty author list leaves it unchanged.
fn embed_authors(query: &str, authors: &[String]) -> String {
    if authors.is_empty() {
        return query.to_string();
    }
    let clause = authors
        .iter()
        .map(|author| format!("au:\"{author}\""))
        .collect::<Vec<_>>()
        .join(" OR ");
    format!("{query} AND ({clause})")
}

/// Map a shared sort value to arXiv's (sortBy, sortOrder) parameter pair.
///
/// arXiv has no citation-count sort, so MostCited falls back to relevance to
/// avoid silent behaviour differences between providers.
fn arxiv_sort(sort: DiscoverySort) -> (&'static str, &'static str) {
    match sort {
        DiscoverySort::Relevance | DiscoverySort::MostCited => ("relevance", "descending"),
        DiscoverySort::Newest => ("submittedDate", "descending"),
    }
}

/// Build a human-readable list of active filters for the run summary.
fn arxiv_active_filters(request: &DiscoverySearchRequest) -> Vec<String> {
    let mut filters = Vec::new();

    if let Some(year_from) = request.year_from {
        filters.push(format!("from_year:{year_from}"));
    }
    if let Some(year_to) = request.year_to {
        filters.push(format!("to_year:{year_to}"));
    }

    filters.push("is_oa:always".to_string());

    filters
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::discovery::DiscoveryProviderChoice;

    fn base_request() -> DiscoverySearchRequest {
        DiscoverySearchRequest {
            query: "sparse autoencoder".to_string(),
            year_from: None,
            year_to: None,
            result_limit: 25,
            sort_by: DiscoverySort::Relevance,
            provider: DiscoveryProviderChoice::Arxiv,
            providers: Vec::new(),
            open_access: true,
            only_viewable: false,
            venues: Vec::new(),
            authors: Vec::new(),
            fields_of_study: Vec::new(),
        }
    }

    // --- arxiv_sort ---

    #[test]
    fn sort_relevance_maps_to_arxiv_relevance() {
        assert_eq!(
            arxiv_sort(DiscoverySort::Relevance),
            ("relevance", "descending")
        );
    }

    #[test]
    fn sort_newest_maps_to_submitted_date_descending() {
        assert_eq!(
            arxiv_sort(DiscoverySort::Newest),
            ("submittedDate", "descending")
        );
    }

    #[test]
    fn sort_most_cited_falls_back_to_relevance() {
        // arXiv has no citation-count sort; must not silently change behaviour.
        assert_eq!(
            arxiv_sort(DiscoverySort::MostCited),
            ("relevance", "descending")
        );
    }

    // --- embed_year_filter ---

    #[test]
    fn no_year_filter_leaves_query_unchanged() {
        assert_eq!(
            embed_year_filter("sparse autoencoder", None, None),
            "sparse autoencoder"
        );
    }

    #[test]
    fn natural_query_uses_all_fields_boolean_syntax() {
        let mut request = base_request();
        request.query = "low-rank adaptation".to_string();

        let params = arxiv_query_params(&request, 25);
        assert_eq!(
            params[0],
            (
                "search_query",
                "(ti:\"low-rank adaptation\" OR (all:\"low-rank\" AND all:\"adaptation\"))"
                    .to_string()
            )
        );
    }

    #[test]
    fn year_range_embedded_in_query() {
        assert_eq!(
            embed_year_filter("sparse autoencoder", Some(2023), Some(2026)),
            "sparse autoencoder AND submittedDate:[20230101 TO 20261231]"
        );
    }

    #[test]
    fn year_from_only_uses_open_upper_bound() {
        assert_eq!(
            embed_year_filter("sparse autoencoder", Some(2023), None),
            "sparse autoencoder AND submittedDate:[20230101 TO *]"
        );
    }

    #[test]
    fn year_to_only_uses_open_lower_bound() {
        assert_eq!(
            embed_year_filter("sparse autoencoder", None, Some(2022)),
            "sparse autoencoder AND submittedDate:[* TO 20221231]"
        );
    }

    // --- embed_authors ---

    #[test]
    fn no_authors_leaves_query_unchanged() {
        assert_eq!(
            embed_authors("sparse autoencoder", &[]),
            "sparse autoencoder"
        );
    }

    #[test]
    fn authors_embedded_as_arxiv_au_clause() {
        assert_eq!(
            embed_authors(
                "sparse autoencoder",
                &["Yoshua Bengio".to_string(), "Geoffrey Hinton".to_string()]
            ),
            "sparse autoencoder AND (au:\"Yoshua Bengio\" OR au:\"Geoffrey Hinton\")"
        );
    }

    // --- arxiv_query_params ---

    #[test]
    fn query_params_include_all_required_keys() {
        let params = arxiv_query_params(&base_request(), 25);
        let keys: Vec<&str> = params.iter().map(|(k, _)| *k).collect();
        assert!(keys.contains(&"search_query"));
        assert!(keys.contains(&"start"));
        assert!(keys.contains(&"max_results"));
        assert!(keys.contains(&"sortBy"));
        assert!(keys.contains(&"sortOrder"));
    }

    #[test]
    fn query_params_respect_result_limit() {
        let params = arxiv_query_params(&base_request(), 10);
        let max = params.iter().find(|(k, _)| *k == "max_results").unwrap();
        assert_eq!(max.1, "10");
    }
}
