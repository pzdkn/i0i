//! OpenAlex discovery configuration.
//!
//! This module is intentionally about loading app configuration, not about
//! searching OpenAlex. It keeps the nested JSON shape and local `.env` fallback
//! out of the provider adapter.

use std::{env, fs, path::Path};

use serde::Deserialize;

use crate::commands::discovery::error::DiscoveryError;
use crate::shared::env::read_dotenv_value;

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
