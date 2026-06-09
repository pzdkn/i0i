//! OpenAlex discovery configuration.
//!
//! This module is intentionally about loading app configuration, not about
//! searching OpenAlex. It keeps the nested JSON shape and local `.env` fallback
//! out of the provider adapter.

use std::{env, fs, path::Path};

use serde::Deserialize;

use crate::commands::discovery::error::DiscoveryError;

#[derive(Debug, Clone)]
pub(super) struct OpenAlexConfig {
    pub(super) url: String,
    api_key_env: String,
    max_result_limit: i32,
}

impl OpenAlexConfig {
    pub(super) fn load() -> Result<Self, DiscoveryError> {
        let app_config = AppConfig::load()?;
        Ok(Self {
            url: app_config.discovery.providers.openalex.url,
            api_key_env: app_config.discovery.providers.openalex.api_key,
            max_result_limit: app_config.discovery.search.max_result_limit,
        })
    }

    pub(super) fn max_result_limit(&self) -> i32 {
        self.max_result_limit
    }

    pub(super) fn resolve_api_key(&self) -> Result<String, DiscoveryError> {
        let env_key = self.api_key_env.trim();
        if env_key.is_empty() {
            return Err(DiscoveryError::new(
                "OpenAlex config is missing the API key environment variable name.",
            ));
        }

        env::var(env_key)
            .ok()
            .or_else(|| read_dotenv_value(env_key))
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                DiscoveryError::new(format!(
                    "OpenAlex API key not found. Set {env_key} in the environment or .env."
                ))
            })
    }
}

#[derive(Debug, Clone, Deserialize)]
struct AppConfig {
    discovery: DiscoveryConfig,
}

impl AppConfig {
    fn load() -> Result<Self, DiscoveryError> {
        let config_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("app.conf.json");
        let config_text = fs::read_to_string(&config_path).map_err(|error| {
            DiscoveryError::new(format!(
                "Could not read discovery config at {}: {error}",
                config_path.display()
            ))
        })?;

        serde_json::from_str(&config_text)
            .map_err(|error| DiscoveryError::new(format!("Invalid discovery config: {error}")))
    }
}

#[derive(Debug, Clone, Deserialize)]
struct DiscoveryConfig {
    providers: DiscoveryProvidersConfig,
    search: DiscoverySearchConfig,
}

#[derive(Debug, Clone, Deserialize)]
struct DiscoveryProvidersConfig {
    openalex: OpenAlexProviderConfig,
}

#[derive(Debug, Clone, Deserialize)]
struct DiscoverySearchConfig {
    max_result_limit: i32,
}

#[derive(Debug, Clone, Deserialize)]
struct OpenAlexProviderConfig {
    url: String,
    api_key: String,
}

fn read_dotenv_value(key: &str) -> Option<String> {
    dotenv_paths()
        .into_iter()
        .filter_map(|path| fs::read_to_string(path).ok())
        .find_map(|contents| parse_dotenv_value(&contents, key))
}

fn dotenv_paths() -> [std::path::PathBuf; 2] {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    [manifest_dir.join(".env"), manifest_dir.join("../.env")]
}

fn parse_dotenv_value(contents: &str, key: &str) -> Option<String> {
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
