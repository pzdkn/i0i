use std::fmt;
use std::path::PathBuf;

use async_trait::async_trait;
use serde::Serialize;

pub type AcquisitionResult<T> = Result<T, SourceAcquisitionError>;

#[derive(Debug, Clone)]
pub struct SourceAcquisitionConfig {
    pub browser_fallback: String,
    pub prefer_browser_for_blocked_sources: bool,
}

impl Default for SourceAcquisitionConfig {
    fn default() -> Self {
        Self {
            browser_fallback: "obscura".to_string(),
            prefer_browser_for_blocked_sources: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AcquisitionMethod {
    DirectHttp,
    ObscuraBrowser,
    ObscuraBrowserStealth,
}

impl AcquisitionMethod {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::DirectHttp => "direct_http",
            Self::ObscuraBrowser => "obscura_browser",
            Self::ObscuraBrowserStealth => "obscura_browser_stealth",
        }
    }
}

#[derive(Debug, Clone)]
pub struct FetchResponse {
    pub bytes: Vec<u8>,
    pub final_url: String,
    pub content_type: Option<String>,
}

#[derive(Debug, Clone)]
pub struct AcquiredSource {
    pub bytes: Vec<u8>,
    pub final_url: String,
    pub content_type: Option<String>,
    pub method: AcquisitionMethod,
}

/// A fetched page ingested into clean, annotatable article HTML (RFC 0056).
#[derive(Debug, Clone)]
pub struct AcquiredHtml {
    pub final_url: String,
    pub title: Option<String>,
    /// Sanitized, self-contained article HTML (safe to render).
    pub clean_html: String,
    /// Readable plain text (chat context / search).
    pub source_text: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct BrowserPageSnapshot {
    pub url: String,
    pub final_url: String,
    pub title: Option<String>,
    pub content_type: Option<String>,
    pub html: Option<String>,
    pub text: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct PageInspection {
    pub snapshot: BrowserPageSnapshot,
    pub links: Vec<String>,
    pub assets: Vec<String>,
    pub network_urls: Vec<String>,
}

#[derive(Debug, Clone)]
pub enum SourceAcquisitionError {
    DirectHttpForbidden(String),
    BrowserReturnedHtml(String),
    BrowserTimeout(String),
    BrowserProcessUnavailable(String),
    NoPdfLinkFoundOnLandingPage(String),
    DownloadedBytesNotPdf(String),
    TooLarge { bytes: u64, max_bytes: u64 },
    Http(String),
    Browser(String),
}

impl SourceAcquisitionError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::DirectHttpForbidden(_) => "direct_http_forbidden",
            Self::BrowserReturnedHtml(_) => "browser_returned_html",
            Self::BrowserTimeout(_) => "browser_timeout",
            Self::BrowserProcessUnavailable(_) => "browser_process_unavailable",
            Self::NoPdfLinkFoundOnLandingPage(_) => "no_pdf_link_found_on_landing_page",
            Self::DownloadedBytesNotPdf(_) => "downloaded_bytes_not_pdf",
            Self::TooLarge { .. } => "downloaded_bytes_too_large",
            Self::Http(_) => "http_error",
            Self::Browser(_) => "browser_error",
        }
    }
}

impl fmt::Display for SourceAcquisitionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DirectHttpForbidden(message)
            | Self::BrowserReturnedHtml(message)
            | Self::BrowserTimeout(message)
            | Self::BrowserProcessUnavailable(message)
            | Self::NoPdfLinkFoundOnLandingPage(message)
            | Self::DownloadedBytesNotPdf(message)
            | Self::Http(message)
            | Self::Browser(message) => write!(formatter, "{}: {message}", self.code()),
            Self::TooLarge { bytes, max_bytes } => write!(
                formatter,
                "{}: {bytes} bytes exceeds {max_bytes} bytes",
                self.code()
            ),
        }
    }
}

impl std::error::Error for SourceAcquisitionError {}

#[async_trait]
pub trait HttpFetcher: Send + Sync {
    async fn fetch(&self, url: &str) -> AcquisitionResult<FetchResponse>;

    /// Cheaply check whether a URL serves a PDF without downloading it fully.
    ///
    /// Returns `Ok(true)` when the first bytes sniff as `%PDF-`. The default
    /// implementation falls back to a full fetch, which keeps test fakes
    /// simple; the real fetcher overrides this with a Range request.
    async fn probe(&self, url: &str) -> AcquisitionResult<bool> {
        let response = self.fetch(url).await?;
        Ok(response.bytes.starts_with(b"%PDF-"))
    }
}

#[async_trait]
pub trait BrowserRuntime: Send + Sync {
    async fn ensure_ready(&self) -> AcquisitionResult<BrowserEndpoint>;
    async fn fetch_original(&self, url: &str) -> AcquisitionResult<FetchResponse>;
    async fn inspect_page(&self, url: &str) -> AcquisitionResult<PageInspection>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserEndpoint {
    pub port: u16,
    pub url: String,
    pub websocket_url: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ObscuraConfig {
    pub enabled: bool,
    pub path: Option<PathBuf>,
    pub stealth: bool,
    pub startup_timeout_ms: u64,
    pub request_timeout_ms: u64,
    pub port: u16,
}

impl Default for ObscuraConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            path: None,
            stealth: true,
            startup_timeout_ms: 10_000,
            request_timeout_ms: 45_000,
            port: 0,
        }
    }
}
