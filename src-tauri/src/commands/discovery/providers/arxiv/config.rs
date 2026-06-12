//! arXiv discovery configuration.
//!
//! arXiv requires no API key. This module only loads the base URL and shared
//! search limits from app.conf.json.

use std::{fs, path::Path};

use serde::Deserialize;

use crate::commands::discovery::error::DiscoveryError;

#[derive(Debug, Clone)]
pub(super) struct ArxivConfig {
    pub(super) url: String,
    max_result_limit: i32,
}

impl ArxivConfig {
    pub(super) fn load() -> Result<Self, DiscoveryError> {
        let app_config = AppConfig::load()?;
        Ok(Self {
            url: app_config.discovery.providers.arxiv.url,
            max_result_limit: app_config.discovery.search.max_result_limit,
        })
    }

    pub(super) fn max_result_limit(&self) -> i32 {
        self.max_result_limit
    }
}

#[derive(Deserialize)]
struct AppConfig {
    discovery: DiscoveryConfig,
}

impl AppConfig {
    fn load() -> Result<Self, DiscoveryError> {
        let config_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("app.conf.json");
        let config_text = fs::read_to_string(&config_path).map_err(|error| {
            DiscoveryError::new(format!(
                "Could not read config at {}: {error}",
                config_path.display()
            ))
        })?;
        serde_json::from_str(&config_text)
            .map_err(|error| DiscoveryError::new(format!("Invalid config: {error}")))
    }
}

#[derive(Deserialize)]
struct DiscoveryConfig {
    providers: ProvidersConfig,
    search: SearchConfig,
}

#[derive(Deserialize)]
struct ProvidersConfig {
    arxiv: ArxivProviderConfig,
}

#[derive(Deserialize)]
struct SearchConfig {
    max_result_limit: i32,
}

#[derive(Deserialize)]
struct ArxivProviderConfig {
    url: String,
}
