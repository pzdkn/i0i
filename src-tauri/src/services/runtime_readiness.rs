//! Runtime capability readiness for packaged and external dependencies.
//!
//! This module translates low-level component health into the small product
//! vocabulary shown in Settings. It does not install software or persist
//! transient health.

use std::path::Path;
use std::time::Duration;

use serde::Serialize;
use tokio::process::Command;

use crate::pdf_extraction::PdfExtractionManager;
use crate::services::codex_runtime::{CodexRuntime, CodexRuntimeConfig};
use crate::services::embedding::EmbeddingReranker;
use crate::services::settings::resolve_secret;
use crate::services::source_acquisition::types::BrowserRuntimeState;
use crate::services::source_acquisition::SourceAcquisitionService;

const MANIFEST: &str = include_str!("../../runtime-dependencies.toml");

/// Product-level state for one optional or packaged capability.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeCapabilityState {
    Ready,
    Starting,
    SetupRequired,
    Unavailable,
}

/// One concise capability row returned to the frontend.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeCapability {
    pub id: String,
    pub label: String,
    pub state: RuntimeCapabilityState,
    pub message: String,
    pub action: Option<String>,
    pub setup_url: Option<String>,
    pub version: Option<String>,
}

/// Aggregates existing component checks without owning their lifecycle.
#[derive(Clone)]
pub struct RuntimeReadinessService {
    pdf_extractions: PdfExtractionManager,
    source_acquisition: SourceAcquisitionService,
    reranker: EmbeddingReranker,
}

impl RuntimeReadinessService {
    /// Construct readiness checks over the app's shared runtime services.
    pub fn new(
        pdf_extractions: PdfExtractionManager,
        source_acquisition: SourceAcquisitionService,
        reranker: EmbeddingReranker,
    ) -> Self {
        Self {
            pdf_extractions,
            source_acquisition,
            reranker,
        }
    }

    /// Return a side-effect-free snapshot suitable for Settings.
    pub async fn snapshot(&self) -> Vec<RuntimeCapability> {
        vec![
            ready("pdf_reader", "PDF reader", "Ready"),
            self.pdf_evidence(),
            self.web_discovery(),
            chat_readiness(),
            self.autonomous_research().await,
            self.semantic_ranking(),
        ]
    }

    /// Retry the component that has a meaningful bounded readiness operation.
    pub async fn retry(&self, capability_id: &str) -> Result<Vec<RuntimeCapability>, String> {
        match capability_id {
            "pdf_evidence" => self.pdf_extractions.verify_runtime()?,
            "web_discovery" => {
                self.source_acquisition
                    .ensure_browser_ready()
                    .await
                    .map_err(|error| error.to_string())?;
            }
            "autonomous_research" => {
                let runtime = CodexRuntime::start(CodexRuntimeConfig::load()).await?;
                runtime.shutdown().await?;
            }
            "chat" | "pdf_reader" | "semantic_ranking" => {}
            other => return Err(format!("Unknown runtime capability: {other}")),
        }
        Ok(self.snapshot().await)
    }

    fn pdf_evidence(&self) -> RuntimeCapability {
        match self.pdf_extractions.verify_runtime() {
            Ok(()) => ready("pdf_evidence", "PDF evidence", "Pdfium ready"),
            Err(error) => unavailable(
                "pdf_evidence",
                "PDF evidence",
                packaging_message("Pdfium", &error),
                Some("retry"),
            ),
        }
    }

    fn web_discovery(&self) -> RuntimeCapability {
        let status = self.source_acquisition.browser_status();
        match status.state {
            BrowserRuntimeState::Ready => ready("web_discovery", "Web discovery", "Obscura ready"),
            BrowserRuntimeState::Starting | BrowserRuntimeState::Stopped => RuntimeCapability {
                id: "web_discovery".to_string(),
                label: "Web discovery".to_string(),
                state: RuntimeCapabilityState::Starting,
                message: status
                    .message
                    .unwrap_or_else(|| "Starting Obscura".to_string()),
                action: None,
                setup_url: None,
                version: None,
            },
            BrowserRuntimeState::Failed => unavailable(
                "web_discovery",
                "Web discovery",
                status
                    .message
                    .unwrap_or_else(|| "Packaged Obscura could not start".to_string()),
                Some("retry"),
            ),
        }
    }

    async fn autonomous_research(&self) -> RuntimeCapability {
        let config = CodexRuntimeConfig::load();
        match installed_codex_version(&config.executable).await {
            Ok(version) if version_supported(&version, &minimum_codex_version()) => {
                RuntimeCapability {
                    id: "autonomous_research".to_string(),
                    label: "Autonomous research".to_string(),
                    state: RuntimeCapabilityState::Ready,
                    message: "Codex installed; connection is verified when research starts"
                        .to_string(),
                    action: Some("test".to_string()),
                    setup_url: None,
                    version: Some(version),
                }
            }
            Ok(version) => RuntimeCapability {
                id: "autonomous_research".to_string(),
                label: "Autonomous research".to_string(),
                state: RuntimeCapabilityState::SetupRequired,
                message: format!(
                    "Codex {version} is older than the tested minimum {}",
                    minimum_codex_version()
                ),
                action: Some("open_codex_install".to_string()),
                setup_url: Some(codex_install_url()),
                version: Some(version),
            },
            Err(_) => RuntimeCapability {
                id: "autonomous_research".to_string(),
                label: "Autonomous research".to_string(),
                state: RuntimeCapabilityState::SetupRequired,
                message: "Install Codex or select its executable in Models".to_string(),
                action: Some("open_codex_install".to_string()),
                setup_url: Some(codex_install_url()),
                version: None,
            },
        }
    }

    fn semantic_ranking(&self) -> RuntimeCapability {
        if !cfg!(feature = "embeddings") {
            return unavailable(
                "semantic_ranking",
                "Semantic ranking",
                "Not included in this build",
                None,
            );
        }
        if self.reranker.is_ready() {
            ready(
                "semantic_ranking",
                "Semantic ranking",
                "Available; the local model loads during first use",
            )
        } else {
            unavailable(
                "semantic_ranking",
                "Semantic ranking",
                "Local model unavailable; lexical ranking remains active",
                None,
            )
        }
    }
}

fn ready(id: &str, label: &str, message: &str) -> RuntimeCapability {
    RuntimeCapability {
        id: id.to_string(),
        label: label.to_string(),
        state: RuntimeCapabilityState::Ready,
        message: message.to_string(),
        action: None,
        setup_url: None,
        version: None,
    }
}

fn unavailable(
    id: &str,
    label: &str,
    message: impl Into<String>,
    action: Option<&str>,
) -> RuntimeCapability {
    RuntimeCapability {
        id: id.to_string(),
        label: label.to_string(),
        state: RuntimeCapabilityState::Unavailable,
        message: message.into(),
        action: action.map(str::to_string),
        setup_url: None,
        version: None,
    }
}

fn packaging_message(component: &str, error: &str) -> String {
    let first_line = error.lines().next().unwrap_or(error);
    format!("Packaged {component} is unavailable: {first_line}")
}

fn chat_readiness() -> RuntimeCapability {
    if resolve_secret("secret.openrouter", "OPENROUTER_API_KEY").is_some() {
        ready("chat", "Chat", "Model provider configured")
    } else {
        RuntimeCapability {
            id: "chat".to_string(),
            label: "Chat".to_string(),
            state: RuntimeCapabilityState::SetupRequired,
            message: "Configure a model provider in API Keys".to_string(),
            action: Some("open_api_keys".to_string()),
            setup_url: None,
            version: None,
        }
    }
}

async fn installed_codex_version(executable: &Path) -> Result<String, String> {
    let output = tokio::time::timeout(
        Duration::from_secs(3),
        Command::new(executable).arg("--version").output(),
    )
    .await
    .map_err(|_| "Codex version check timed out".to_string())?
    .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    String::from_utf8_lossy(&output.stdout)
        .split_whitespace()
        .find(|part| {
            part.chars()
                .next()
                .is_some_and(|character| character.is_ascii_digit())
        })
        .map(str::to_string)
        .ok_or_else(|| "Codex version output contained no version".to_string())
}

fn minimum_codex_version() -> String {
    toml::from_str::<toml::Value>(MANIFEST)
        .ok()
        .and_then(|manifest| {
            manifest
                .get("external")?
                .get("codex")?
                .get("minimum_version")?
                .as_str()
                .map(str::to_string)
        })
        .unwrap_or_else(|| "0.0.0".to_string())
}

fn codex_install_url() -> String {
    toml::from_str::<toml::Value>(MANIFEST)
        .ok()
        .and_then(|manifest| {
            manifest
                .get("external")?
                .get("codex")?
                .get("install_url")?
                .as_str()
                .map(str::to_string)
        })
        .unwrap_or_else(|| "https://developers.openai.com/codex/cli/".to_string())
}

fn version_supported(installed: &str, minimum: &str) -> bool {
    version_parts(installed) >= version_parts(minimum)
}

fn version_parts(version: &str) -> (u64, u64, u64) {
    let mut parts = version
        .split('.')
        .map(|part| part.parse::<u64>().unwrap_or(0));
    (
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_codex_minimum_from_release_manifest() {
        assert_eq!(minimum_codex_version(), "0.153.4");
    }

    #[test]
    fn compares_numeric_versions() {
        assert!(version_supported("0.153.4", "0.153.4"));
        assert!(version_supported("0.160.0", "0.153.4"));
        assert!(!version_supported("0.99.0", "0.153.4"));
    }

    #[test]
    fn packaging_errors_are_bounded_to_the_first_line() {
        assert_eq!(
            packaging_message("Pdfium", "missing library\nlong diagnostics"),
            "Packaged Pdfium is unavailable: missing library"
        );
    }
}
