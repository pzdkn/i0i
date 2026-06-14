//! Semantic Scholar discovery configuration.
//!
//! The API key is optional — Semantic Scholar allows unauthenticated requests
//! at a reduced rate limit. A missing or unset key produces an empty string,
//! not an error.

use std::{env, fs, path::Path};

use serde::Deserialize;

use crate::commands::discovery::error::DiscoveryError;
use crate::shared::env::read_dotenv_value;

#[derive(Debug, Clone)]
pub(super) struct SemanticScholarConfig {
    pub(super) url: String,
    api_key_env: String,
    max_result_limit: i32,
}

impl SemanticScholarConfig {
    pub(super) fn load() -> Result<Self, DiscoveryError> {
        let app_config = AppConfig::load()?;
        Ok(Self {
            url: app_config.discovery.providers.semantic_scholar.url,
            api_key_env: app_config
                .discovery
                .providers
                .semantic_scholar
                .api_key
                .unwrap_or_default(),
            max_result_limit: app_config.discovery.search.max_result_limit,
        })
    }

    pub(super) fn max_result_limit(&self) -> i32 {
        self.max_result_limit
    }

    /// Resolve the API key from the configured env-var name.
    ///
    /// Returns an empty string if the env-var name is unset in config, or if
    /// the variable itself is not found — callers treat an empty string as
    /// "run unauthenticated".
    pub(super) fn resolve_api_key(&self) -> String {
        let env_key = self.api_key_env.trim();
        if env_key.is_empty() {
            return String::new();
        }

        env::var(env_key)
            .ok()
            .or_else(|| read_dotenv_value(env_key))
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
            .unwrap_or_default()
    }
}

#[derive(Deserialize)]
struct AppConfig {
    discovery: DiscoveryConfig,
}

impl AppConfig {
    fn load() -> Result<Self, DiscoveryError> {
        let config_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("app.conf.json");
        let config_text = fs::read_to_string(&config_path).map_err(|e| {
            DiscoveryError::new(format!(
                "Could not read config at {}: {e}",
                config_path.display()
            ))
        })?;
        serde_json::from_str(&config_text)
            .map_err(|e| DiscoveryError::new(format!("Invalid config: {e}")))
    }
}

#[derive(Deserialize)]
struct DiscoveryConfig {
    providers: ProvidersConfig,
    search: SearchConfig,
}

#[derive(Deserialize)]
struct ProvidersConfig {
    semantic_scholar: SemanticScholarProviderConfig,
}

#[derive(Deserialize)]
struct SearchConfig {
    max_result_limit: i32,
}

#[derive(Deserialize)]
struct SemanticScholarProviderConfig {
    url: String,
    api_key: Option<String>,
}
