//! Europe PMC discovery configuration (RFC 0053).
//!
//! Europe PMC requires no API key for normal search use. This module only
//! loads the base URL and the shared result limit from app.conf.json.

use std::{fs, path::Path};

use serde::Deserialize;

use crate::commands::discovery::error::DiscoveryError;

#[derive(Debug, Clone)]
pub(super) struct EuropePmcConfig {
    pub(super) url: String,
    max_result_limit: i32,
}

impl EuropePmcConfig {
    pub(super) fn load() -> Result<Self, DiscoveryError> {
        let app_config = AppConfig::load()?;
        Ok(Self {
            url: app_config.discovery.providers.europe_pmc.url,
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
    europe_pmc: EuropePmcProviderConfig,
}

#[derive(Deserialize)]
struct SearchConfig {
    max_result_limit: i32,
}

#[derive(Deserialize)]
struct EuropePmcProviderConfig {
    url: String,
}
