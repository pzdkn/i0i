//! Quick-search query expansion (RFC 0054, Part B).
//!
//! Turns the user's literal query into a few alternative phrasings (synonyms,
//! acronym expansions, method/dataset names) using a fast, cheap LLM. The
//! variants only widen recall — ranking is still measured against the original
//! query. Expansion is best-effort: no API key, a timeout, or a malformed reply
//! all degrade to "no variants", leaving the literal results untouched.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use reqwest::Client;

use crate::services::chat::config::ChatConfig;
use crate::services::llm::{self, CompletionRequest, ResponseFormat, WireMessage};

/// Upper bound on variants returned to the caller.
const MAX_VARIANTS: usize = 3;
/// Bound the expansion call so it never delays the (already-shown) results.
const EXPANSION_TIMEOUT: Duration = Duration::from_secs(6);

const EXPAND_SYSTEM: &str = "You expand a scholarly search query into alternative phrasings that \
capture the same intent: synonyms, acronym expansions or contractions, and key method or dataset \
names. Reply with ONLY a JSON array of 2-3 short query strings. Do not repeat the original query. \
No prose, no keys, just the array.";

/// Expands queries via a cheap OpenRouter model, with a per-session cache.
#[derive(Clone)]
pub struct QueryExpander {
    inner: Option<Arc<Inner>>,
    cache: Arc<Mutex<HashMap<String, Vec<String>>>>,
}

struct Inner {
    client: Client,
    url: String,
    api_key: String,
    model: String,
}

impl QueryExpander {
    /// Build from the shared chat/OpenRouter config. Uses the cheap `title_model`
    /// when present, else the main chat model. A missing API key yields a
    /// disabled expander (expansion becomes a no-op).
    pub fn from_app_config() -> Self {
        match Self::try_from_app_config() {
            Ok(expander) => expander,
            Err(error) => {
                eprintln!("[query_expansion] disabled: {error}");
                Self::disabled()
            }
        }
    }

    fn try_from_app_config() -> Result<Self, String> {
        let config = ChatConfig::load()?;
        let api_key = config.resolve_api_key()?;
        // A user override (Settings) wins; else the cheap title model, else chat.
        let model = crate::services::settings::preference("model.expansion")
            .or_else(|| config.title_model.clone())
            .unwrap_or(config.model);
        let client = Client::builder()
            .timeout(EXPANSION_TIMEOUT)
            .build()
            .map_err(|error| error.to_string())?;
        Ok(Self {
            inner: Some(Arc::new(Inner {
                client,
                url: config.url,
                api_key,
                model,
            })),
            cache: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    pub fn disabled() -> Self {
        Self {
            inner: None,
            cache: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn is_ready(&self) -> bool {
        self.inner.is_some()
    }

    /// Return up to `MAX_VARIANTS` alternative queries for `query`. Empty when
    /// disabled, cached-empty, timed out, or the reply can't be parsed.
    pub async fn expand(&self, query: &str) -> Vec<String> {
        let key = normalize(query);
        if key.is_empty() {
            return Vec::new();
        }
        let Some(inner) = self.inner.clone() else {
            return Vec::new();
        };
        if let Some(cached) = self.cache.lock().unwrap().get(&key).cloned() {
            return cached;
        }

        let variants = match inner.request_variants(query).await {
            Ok(variants) => dedupe_variants(query, variants),
            Err(error) => {
                eprintln!("[query_expansion] expand failed: {error}");
                Vec::new()
            }
        };
        // Cache even an empty result: a query that yields nothing shouldn't be
        // retried for the rest of the session.
        self.cache.lock().unwrap().insert(key, variants.clone());
        variants
    }
}

impl Inner {
    async fn request_variants(&self, query: &str) -> Result<Vec<String>, String> {
        let request = CompletionRequest {
            model: self.model.clone(),
            messages: vec![
                WireMessage::text("system", EXPAND_SYSTEM.to_string()),
                WireMessage::text("user", query.trim().to_string()),
            ],
            stream: false,
            max_tokens: Some(160),
            response_format: Some(ResponseFormat::json_object()),
            tools: None,
            tool_choice: None,
        };
        let text = llm::complete(&self.client, &self.url, &self.api_key, &request).await?;
        Ok(parse_variants(&text))
    }
}

/// Parse a JSON array of strings from the model reply, tolerating a wrapping
/// object (e.g. `{"queries": [...]}`) or surrounding prose.
fn parse_variants(text: &str) -> Vec<String> {
    // Direct array first.
    if let Ok(list) = serde_json::from_str::<Vec<String>>(text.trim()) {
        return list;
    }
    // An object with any array-of-strings value.
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(text.trim()) {
        if let Some(array) = first_string_array(&value) {
            return array;
        }
    }
    // Last resort: locate a bracketed array substring and parse it.
    if let (Some(start), Some(end)) = (text.find('['), text.rfind(']')) {
        if end > start {
            if let Ok(list) = serde_json::from_str::<Vec<String>>(&text[start..=end]) {
                return list;
            }
        }
    }
    Vec::new()
}

fn first_string_array(value: &serde_json::Value) -> Option<Vec<String>> {
    match value {
        serde_json::Value::Array(items) => Some(collect_strings(items)),
        serde_json::Value::Object(map) => map.values().find_map(|inner| match inner {
            serde_json::Value::Array(items) => Some(collect_strings(items)),
            _ => None,
        }),
        _ => None,
    }
}

fn collect_strings(items: &[serde_json::Value]) -> Vec<String> {
    items
        .iter()
        .filter_map(|item| item.as_str().map(str::to_string))
        .collect()
}

/// Trim, drop empties, drop anything equal to the original query, dedupe
/// case-insensitively, and cap to `MAX_VARIANTS`.
fn dedupe_variants(query: &str, variants: Vec<String>) -> Vec<String> {
    let original = normalize(query);
    let mut seen = vec![original];
    let mut out = Vec::new();
    for variant in variants {
        let trimmed = variant.trim().to_string();
        let key = normalize(&trimmed);
        if trimmed.is_empty() || seen.contains(&key) {
            continue;
        }
        seen.push(key);
        out.push(trimmed);
        if out.len() >= MAX_VARIANTS {
            break;
        }
    }
    out
}

fn normalize(query: &str) -> String {
    query.trim().to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_plain_json_array() {
        let variants = parse_variants(r#"["neural machine translation", "seq2seq models"]"#);
        assert_eq!(
            variants,
            vec!["neural machine translation", "seq2seq models"]
        );
    }

    #[test]
    fn parses_object_wrapped_array() {
        let variants = parse_variants(r#"{"queries": ["a", "b"]}"#);
        assert_eq!(variants, vec!["a", "b"]);
    }

    #[test]
    fn parses_array_embedded_in_prose() {
        let variants = parse_variants("Here you go:\n[\"x\", \"y\"]\nHope that helps!");
        assert_eq!(variants, vec!["x", "y"]);
    }

    #[test]
    fn malformed_reply_yields_no_variants() {
        assert!(parse_variants("sorry, I cannot do that").is_empty());
    }

    #[test]
    fn dedupe_drops_original_and_caps_to_three() {
        let variants = dedupe_variants(
            "LLM search",
            vec![
                "llm search".to_string(), // equals original (case-insensitive)
                "language model retrieval".to_string(),
                "  neural search  ".to_string(),
                "language model retrieval".to_string(), // duplicate
                "document ranking".to_string(),
                "a fourth one".to_string(), // beyond cap
            ],
        );
        assert_eq!(
            variants,
            vec![
                "language model retrieval",
                "neural search",
                "document ranking"
            ]
        );
    }

    #[tokio::test]
    #[ignore = "live network + LLM"]
    async fn live_expand_returns_variants() {
        let expander = QueryExpander::from_app_config();
        assert!(expander.is_ready(), "expander needs OPENROUTER_API_KEY");
        let variants = expander.expand("LLM document search").await;
        eprintln!("live variants: {variants:?}");
        assert!(!variants.is_empty(), "expected at least one variant");
        assert!(variants.len() <= MAX_VARIANTS);
        // Cache hit returns the same thing without another call.
        assert_eq!(expander.expand("LLM document search").await, variants);
    }

    #[test]
    fn disabled_expander_returns_empty() {
        let expander = QueryExpander::disabled();
        assert!(!expander.is_ready());
        let variants = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(expander.expand("anything"));
        assert!(variants.is_empty());
    }
}
