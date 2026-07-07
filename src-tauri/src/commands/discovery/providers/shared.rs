//! Helpers shared across all provider adapters.

use url::Url;

use crate::commands::discovery::error::DiscoveryError;

/// Build a URL by appending query parameters to a base URL string.
pub(crate) fn build_query_url(
    base_url: &str,
    query_params: &[(&str, String)],
) -> Result<String, DiscoveryError> {
    let mut url =
        Url::parse(base_url).map_err(|e| DiscoveryError::new(format!("Invalid URL: {e}")))?;
    {
        let mut pairs = url.query_pairs_mut();
        for (key, value) in query_params {
            pairs.append_pair(key, value);
        }
    }
    Ok(url.to_string())
}

/// Clamp a request's result limit to the provider's configured maximum.
pub(crate) fn clamp_result_limit(request_limit: i32, max_config_limit: i32) -> i32 {
    request_limit.clamp(1, max_config_limit.max(1))
}

/// Tokenize a query string into lowercase keywords for the match summary.
pub(crate) fn extract_matched_keywords(query: &str) -> Vec<String> {
    query
        .split_whitespace()
        .map(|part| part.trim_matches(|ch: char| !ch.is_alphanumeric()))
        .filter(|part| !part.is_empty())
        .map(str::to_lowercase)
        .collect()
}
