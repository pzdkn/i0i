//! Settings commands (RFC 0055).
//!
//! Reads/writes the user settings store. Secret **values** never leave the
//! backend — `get_settings` returns only per-secret status (configured + where
//! it resolved from); the renderer writes new values one-way via `save_setting`.

use serde::Serialize;
use tauri::State;

use crate::commands::discovery::provider::DiscoveryProvider;
use crate::commands::discovery::DiscoveryProviders;
use crate::domain::discovery::{DiscoveryProviderChoice, DiscoverySearchRequest, DiscoverySort};
use crate::services::runtime_readiness::{RuntimeCapability, RuntimeReadinessService};
use crate::services::settings::{resolve_secret_with_source, SettingSource, SettingsStore};

/// One configurable credential/contact: its logical name, the setting key it's
/// stored under, the environment variable it falls back to, and whether its
/// value is sensitive (API keys are; the contact email is not).
struct SecretSpec {
    name: &'static str,
    setting_key: &'static str,
    env_key: &'static str,
    sensitive: bool,
}

const SECRETS: [SecretSpec; 4] = [
    SecretSpec {
        name: "openrouter",
        setting_key: "secret.openrouter",
        env_key: "OPENROUTER_API_KEY",
        sensitive: true,
    },
    SecretSpec {
        name: "openalex",
        setting_key: "secret.openalex",
        env_key: "OPENALEX_API_KEY",
        sensitive: true,
    },
    SecretSpec {
        name: "core",
        setting_key: "secret.core",
        env_key: "CORE_API_KEY",
        sensitive: true,
    },
    SecretSpec {
        name: "email",
        setting_key: "secret.email",
        env_key: "IOI_EMAIL",
        sensitive: false,
    },
];

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SecretStatus {
    /// Logical name (`openrouter`, `openalex`, `core`, `email`).
    name: String,
    /// The full setting key to write to (`secret.openrouter`).
    setting_key: String,
    configured: bool,
    /// Where the value resolved from, when configured.
    source: Option<SettingSource>,
    /// The resolved value — present only for **non-sensitive** entries (the
    /// contact email). Sensitive API-key values are never returned to the
    /// renderer (RFC 0055).
    value: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsView {
    secrets: Vec<SecretStatus>,
    /// Non-secret preferences (`model.*`, `search.*`) with their values.
    prefs: std::collections::HashMap<String, String>,
}

/// Snapshot of settings: secret **status only** plus non-secret preferences.
#[tauri::command]
pub fn get_settings(store: State<'_, SettingsStore>) -> Result<SettingsView, String> {
    let secrets = SECRETS
        .iter()
        .map(|spec| {
            let (value, source) = resolve_secret_with_source(spec.setting_key, spec.env_key);
            SecretStatus {
                name: spec.name.to_string(),
                setting_key: spec.setting_key.to_string(),
                configured: value.is_some(),
                source,
                // Sensitive values (API keys) never leave the backend; the
                // non-sensitive contact email is returned so the user can see it.
                value: if spec.sensitive { None } else { value },
            }
        })
        .collect();
    let prefs = store.prefs()?.into_iter().collect();
    Ok(SettingsView { secrets, prefs })
}

/// Upsert one setting. An empty value clears it (falls back to env/`.env` for
/// secrets, or the built-in default for prefs). Setting keys are namespaced;
/// anything outside the known prefixes is rejected.
#[tauri::command]
pub fn save_setting(
    store: State<'_, SettingsStore>,
    key: String,
    value: String,
) -> Result<(), String> {
    if !is_allowed_key(&key) {
        return Err(format!("Unknown setting key: {key}"));
    }
    store.set(&key, &value)
}

/// Remove a user override for `key`, falling back to env/`.env`/defaults.
#[tauri::command]
pub fn clear_setting(store: State<'_, SettingsStore>, key: String) -> Result<(), String> {
    if !is_allowed_key(&key) {
        return Err(format!("Unknown setting key: {key}"));
    }
    store.clear(&key)
}

/// Live-verify a provider's currently-resolved key (save-then-test: the UI
/// saves first, then calls this). Returns `Ok(())` on success, a message on
/// failure. `email` has nothing to ping, so it just reports configured/not.
#[tauri::command]
pub async fn test_provider_key(
    providers: State<'_, DiscoveryProviders>,
    name: String,
) -> Result<(), String> {
    match name.as_str() {
        "openalex" => providers
            .openalex
            .clone()
            .search(&probe_request(DiscoveryProviderChoice::OpenAlex))
            .await
            .map(|_| ())
            .map_err(|error| error.to_string()),
        "core" => {
            if !providers.core.is_configured() {
                return Err("No CORE API key configured.".to_string());
            }
            providers
                .core
                .clone()
                .search(&probe_request(DiscoveryProviderChoice::Core))
                .await
                .map(|_| ())
                .map_err(|error| error.to_string())
        }
        "openrouter" => test_openrouter().await,
        "email" => {
            let (value, _) = resolve_secret_with_source("secret.email", "IOI_EMAIL");
            if value.is_some() {
                Ok(())
            } else {
                Err("No contact email configured.".to_string())
            }
        }
        other => Err(format!("Unknown provider: {other}")),
    }
}

/// A minimal 1-result search used only to check a provider key works.
fn probe_request(provider: DiscoveryProviderChoice) -> DiscoverySearchRequest {
    DiscoverySearchRequest {
        query: "test".to_string(),
        year_from: None,
        year_to: None,
        result_limit: 1,
        sort_by: DiscoverySort::Relevance,
        provider,
        providers: Vec::new(),
        open_access: false,
        only_viewable: false,
        venues: Vec::new(),
        authors: Vec::new(),
        fields_of_study: Vec::new(),
    }
}

/// Smallest possible OpenRouter completion to verify the key.
async fn test_openrouter() -> Result<(), String> {
    use crate::services::chat::config::ChatConfig;
    use crate::services::llm::{self, CompletionRequest, WireMessage};

    let config = ChatConfig::load()?;
    let api_key = config.resolve_api_key()?;
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|error| error.to_string())?;
    let request = CompletionRequest {
        model: config.title_model.clone().unwrap_or(config.model),
        messages: vec![WireMessage::text("user", "ping".to_string())],
        stream: false,
        max_tokens: Some(1),
        response_format: None,
        tools: None,
        tool_choice: None,
    };
    llm::complete(&client, &config.url, &api_key, &request)
        .await
        .map(|_| ())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RerankerStatus {
    /// The `embeddings` Cargo feature is compiled in.
    feature_built: bool,
    /// The model is loaded and ready (implies the feature is built).
    ready: bool,
}

/// Embedding-reranker status for the Search-defaults tab (RFC 0054/0055).
#[tauri::command]
pub fn get_reranker_status(
    reranker: State<'_, crate::services::embedding::EmbeddingReranker>,
) -> RerankerStatus {
    RerankerStatus {
        feature_built: cfg!(feature = "embeddings"),
        ready: reranker.is_ready(),
    }
}

/// Return current packaged, configured, and external runtime capabilities.
#[tauri::command]
pub async fn get_runtime_capabilities(
    readiness: State<'_, RuntimeReadinessService>,
) -> Result<Vec<RuntimeCapability>, String> {
    Ok(readiness.snapshot().await)
}

/// Retry one bounded runtime readiness check and return the refreshed snapshot.
#[tauri::command]
pub async fn retry_runtime_capability(
    readiness: State<'_, RuntimeReadinessService>,
    capability_id: String,
) -> Result<Vec<RuntimeCapability>, String> {
    readiness.retry(&capability_id).await
}

/// Only allow writes to the known setting namespaces.
fn is_allowed_key(key: &str) -> bool {
    [
        "secret.",
        "model.",
        "search.",
        "acquisition.",
        "suggestions.",
        "research.",
    ]
    .iter()
    .any(|prefix| key.starts_with(prefix))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allowed_key_prefixes() {
        assert!(is_allowed_key("secret.openrouter"));
        assert!(is_allowed_key("model.chat"));
        assert!(is_allowed_key("search.default_expand"));
        assert!(is_allowed_key("suggestions.weekly_enabled"));
        assert!(is_allowed_key("research.continuation_outcomes"));
        assert!(!is_allowed_key("arbitrary.key"));
        assert!(!is_allowed_key("../../etc/passwd"));
    }
}
