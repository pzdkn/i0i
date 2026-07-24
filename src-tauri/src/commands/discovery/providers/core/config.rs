//! CORE discovery configuration (RFC 0053).
//!
//! CORE requires a free API key. Following the OpenAlex pattern, the key is
//! resolved lazily at search time (not at construction) so a missing key never
//! blocks app startup — the orchestrator skips CORE when it is not configured.

use std::{fs, path::Path};

use serde::Deserialize;

use crate::commands::discovery::error::DiscoveryError;

#[derive(Debug, Clone)]
pub(super) struct CoreConfig {
    pub(super) url: String,
    api_key_env: String,
    max_result_limit: i32,
}

impl CoreConfig {
    pub(super) fn load() -> Result<Self, DiscoveryError> {
        let app_config = AppConfig::load()?;
        Ok(Self {
            url: app_config.discovery.providers.core.url,
            api_key_env: app_config.discovery.providers.core.api_key,
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
                "CORE config is missing the API key environment variable name.",
            ));
        }

        crate::services::settings::resolve_secret("secret.core", env_key).ok_or_else(|| {
            DiscoveryError::new(format!(
                "CORE API key not found. Set it in Settings, or {env_key} in the environment or .env."
            ))
        })
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
    core: CoreProviderConfig,
}

#[derive(Deserialize)]
struct SearchConfig {
    max_result_limit: i32,
}

#[derive(Deserialize)]
struct CoreProviderConfig {
    url: String,
    api_key: String,
}
