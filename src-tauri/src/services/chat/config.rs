//! Chat provider configuration.
//!
//! Loads the `chat` block from `app.conf.json` and resolves the API key from
//! the environment, falling back to a repo-local `.env`. A missing key is a
//! request-time error, never a startup panic.

use std::{fs, path::Path};

use serde::Deserialize;

/// Upper bound on paper-context characters sent to the model per ask, regardless
/// of the configured budget. ~32k chars (~8k tokens) covers a paper's key
/// sections while keeping ask latency and cost bounded (RFC 0059 follow-up).
const CONTEXT_CHARS_CAP: usize = 32_000;

/// Default model for agent annotation/marking — a cheap, fast open model, since
/// picking verbatim passages to highlight is a much lighter task than answering
/// (RFC 0059 follow-up). Overridable via the `model.annotation` setting.
const DEFAULT_ANNOTATION_MODEL: &str = "meta-llama/llama-3.3-70b-instruct";

#[derive(Debug, Clone)]
pub struct ChatConfig {
    pub url: String,
    api_key_env: String,
    pub model: String,
    pub annotation_model: String,
    pub max_context_chars: usize,
    pub title_model: Option<String>,
    pub title_max_tokens: u32,
    pub title_timeout_ms: u64,
}

impl ChatConfig {
    pub fn load() -> Result<Self, String> {
        let config_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("app.conf.json");
        let config_text = fs::read_to_string(&config_path).map_err(|error| {
            format!(
                "Could not read chat config at {}: {error}",
                config_path.display()
            )
        })?;
        Self::from_json(&config_text)
    }

    fn from_json(config_text: &str) -> Result<Self, String> {
        let app_config: AppConfig = serde_json::from_str(config_text)
            .map_err(|error| format!("Invalid chat config: {error}"))?;
        Ok(Self {
            url: app_config.chat.provider.url,
            api_key_env: app_config.chat.provider.api_key,
            // A user model override (Settings) wins over app.conf.json (RFC 0055).
            model: crate::services::settings::preference("model.chat")
                .unwrap_or(app_config.chat.provider.model),
            // Fast/cheap model for agent annotation, separate from the answer
            // model (RFC 0059 follow-up). Falls back to a cheap open default.
            annotation_model: crate::services::settings::preference("model.annotation")
                .unwrap_or_else(|| DEFAULT_ANNOTATION_MODEL.to_string()),
            // Cap the paper context fed to the model. A large budget (e.g. the
            // 60k some configs set) makes every ask slow and costly for little
            // added answer quality — the model rarely needs the whole paper to
            // answer or to pick passages to highlight. Bounded here so latency
            // is predictable; raise CONTEXT_CHARS_CAP if you need more.
            max_context_chars: app_config
                .chat
                .provider
                .max_context_chars
                .min(CONTEXT_CHARS_CAP),
            title_model: app_config
                .chat
                .provider
                .title_model
                .map(|model| model.trim().to_string())
                .filter(|model| !model.is_empty()),
            title_max_tokens: app_config
                .chat
                .provider
                .title_max_tokens
                .unwrap_or(24)
                .max(1),
            title_timeout_ms: app_config
                .chat
                .provider
                .title_timeout_ms
                .unwrap_or(5_000)
                .max(1),
        })
    }

    /// Resolve the API key, or a friendly error if it is not configured.
    ///
    /// Process environment first, then the repo-local `.env`. A missing key is
    /// returned as an error so chat can surface it in the UI rather than the app
    /// failing to start.
    pub fn resolve_api_key(&self) -> Result<String, String> {
        let env_key = self.api_key_env.trim();
        if env_key.is_empty() {
            return Err(
                "Chat config is missing the API key environment variable name.".to_string(),
            );
        }

        crate::services::settings::resolve_secret("secret.openrouter", env_key).ok_or_else(|| {
            format!(
                "OpenRouter API key not found. Set it in Settings, or {env_key} in the environment or .env."
            )
        })
    }
}

#[derive(Debug, Clone, Deserialize)]
struct AppConfig {
    chat: ChatBlock,
}

#[derive(Debug, Clone, Deserialize)]
struct ChatBlock {
    provider: ChatProviderConfig,
}

#[derive(Debug, Clone, Deserialize)]
struct ChatProviderConfig {
    url: String,
    api_key: String,
    model: String,
    max_context_chars: usize,
    #[serde(default)]
    title_model: Option<String>,
    #[serde(default)]
    title_max_tokens: Option<u32>,
    #[serde(default)]
    title_timeout_ms: Option<u64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = r#"{
        "discovery": { "ignored": true },
        "chat": {
            "provider": {
                "url": "https://openrouter.ai/api/v1/chat/completions",
                "api_key": "OPENROUTER_API_KEY",
                "model": "anthropic/claude-sonnet-4.5",
                "max_context_chars": 60000
            }
        }
    }"#;

    #[test]
    fn parses_chat_block_and_ignores_siblings() {
        let config = ChatConfig::from_json(FIXTURE).expect("chat block parses");
        assert_eq!(config.url, "https://openrouter.ai/api/v1/chat/completions");
        assert_eq!(config.model, "anthropic/claude-sonnet-4.5");
        // The configured 60000 is clamped to the latency cap (CONTEXT_CHARS_CAP).
        assert_eq!(config.max_context_chars, CONTEXT_CHARS_CAP);
        assert!(config.title_model.is_none());
        assert_eq!(config.title_max_tokens, 24);
        assert_eq!(config.title_timeout_ms, 5_000);
    }

    #[test]
    fn parses_optional_title_generation_config() {
        let fixture = r#"{
            "chat": {
                "provider": {
                    "url": "https://openrouter.ai/api/v1/chat/completions",
                    "api_key": "OPENROUTER_API_KEY",
                    "model": "anthropic/claude-sonnet-4.5",
                    "max_context_chars": 60000,
                    "title_model": "anthropic/claude-sonnet-4.5",
                    "title_max_tokens": 18,
                    "title_timeout_ms": 2500
                }
            }
        }"#;
        let config = ChatConfig::from_json(fixture).expect("chat block parses");

        assert_eq!(
            config.title_model.as_deref(),
            Some("anthropic/claude-sonnet-4.5")
        );
        assert_eq!(config.title_max_tokens, 18);
        assert_eq!(config.title_timeout_ms, 2500);
    }

    #[test]
    fn missing_api_key_env_name_is_an_error_not_a_panic() {
        let config = ChatConfig {
            url: "u".to_string(),
            api_key_env: "   ".to_string(),
            model: "m".to_string(),
            annotation_model: "a".to_string(),
            max_context_chars: 10,
            title_model: None,
            title_max_tokens: 24,
            title_timeout_ms: 5_000,
        };
        let error = config
            .resolve_api_key()
            .expect_err("empty env name should error");
        assert!(error.contains("missing the API key environment variable name"));
    }

    #[test]
    fn unresolved_api_key_returns_friendly_request_time_error() {
        let config = ChatConfig {
            url: "u".to_string(),
            api_key_env: "I0I_CHAT_KEY_DEFINITELY_MISSING_XYZ".to_string(),
            model: "m".to_string(),
            annotation_model: "a".to_string(),
            max_context_chars: 10,
            title_model: None,
            title_max_tokens: 24,
            title_timeout_ms: 5_000,
        };
        let error = config
            .resolve_api_key()
            .expect_err("missing key should error");
        assert!(error.contains("OpenRouter API key not found"));
        assert!(error.contains("I0I_CHAT_KEY_DEFINITELY_MISSING_XYZ"));
    }
}
