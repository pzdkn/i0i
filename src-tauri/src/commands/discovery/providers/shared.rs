//! Helpers shared across all provider adapters.

use std::{fs, path::Path};

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

/// Read a single key's value from `.env` files adjacent to the cargo manifest.
pub(crate) fn read_dotenv_value(key: &str) -> Option<String> {
    dotenv_paths()
        .into_iter()
        .filter_map(|path| fs::read_to_string(path).ok())
        .find_map(|contents| parse_dotenv_value(&contents, key))
}

fn dotenv_paths() -> [std::path::PathBuf; 2] {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    [manifest_dir.join(".env"), manifest_dir.join("../.env")]
}

pub(crate) fn parse_dotenv_value(contents: &str, key: &str) -> Option<String> {
    contents.lines().find_map(|line| {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            return None;
        }
        let (name, value) = line.split_once('=')?;
        if name.trim() != key {
            return None;
        }
        Some(value.trim().trim_matches(['"', '\'']).to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dotenv_parser_reads_named_key() {
        assert_eq!(
            parse_dotenv_value(
                "OTHER=value\nOPENALEX_API_KEY='secret'\n",
                "OPENALEX_API_KEY"
            ),
            Some("secret".to_string())
        );
    }
}
