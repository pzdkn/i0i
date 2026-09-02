//! Browser-first scholarly candidate discovery (RFC 0098).
//!
//! Obscura finds result links. Provider APIs are consulted only after a result
//! has a DOI, arXiv identifier, or an exact title that can be verified. The
//! output is the existing `PaperCandidate`, so every caller shares the current
//! deduplication and ranking pipeline.

use std::fs;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use futures_util::{stream, StreamExt};
use regex::Regex;
use scraper::{Html, Selector};
use sha2::{Digest, Sha256};
use tauri::Manager;
use url::Url;

use super::error::DiscoveryError;
use super::provider::{DiscoveryProvider, DiscoveryProviderId, ProviderSearchResult};
use super::providers::{arxiv::ArxivProvider, openalex::OpenAlexProvider};
use crate::domain::discovery::{
    CandidateMatch, DiscoveryProviderChoice, DiscoverySearchRequest, DiscoverySort, PaperCandidate,
};
use crate::services::source_acquisition::types::PageInspection;
use crate::services::source_acquisition::SourceAcquisitionService;

const DEFAULT_SEARCH_URL: &str = "https://search.brave.com/search?q={query}&source=web";
const RESOLUTION_CONCURRENCY: usize = 4;
// Diagnostic payloads stay short enough for routine development logs.
const LOG_TITLE_LIMIT: usize = 160;
const LOG_TEXT_PREVIEW_LIMIT: usize = 500;
const LOG_LINK_HOST_LIMIT: usize = 5;

#[derive(Debug, Clone)]
pub struct BrowserDiscoveryConfig {
    /// URL template for the browser search entry point.
    pub search_url_template: String,
    /// Number of result pages read for one query.
    pub pages_per_query: usize,
}

impl Default for BrowserDiscoveryConfig {
    fn default() -> Self {
        Self {
            search_url_template: DEFAULT_SEARCH_URL.to_string(),
            pages_per_query: 1,
        }
    }
}

impl BrowserDiscoveryConfig {
    pub fn load(app: &tauri::AppHandle) -> Self {
        let mut config = Self::default();
        for path in candidate_config_paths(app) {
            if let Ok(contents) = fs::read_to_string(path) {
                apply_config(&mut config, &contents);
                break;
            }
        }
        config
    }

    fn search_url(&self, query: &str, page: usize) -> Result<String, DiscoveryError> {
        if !self.search_url_template.contains("{query}") {
            return Err(DiscoveryError::new(
                "[discovery].browser_search_url must contain {query}",
            ));
        }
        let encoded: String = url::form_urlencoded::byte_serialize(query.as_bytes()).collect();
        Ok(self
            .search_url_template
            .replace("{query}", &encoded)
            .replace("{page}", &page.to_string())
            .replace("{offset}", &(page * 10).to_string()))
    }
}

#[derive(Debug, Clone)]
pub enum BrowserDiscoveryProgress {
    SearchingWeb,
    Provisional(Vec<PaperCandidate>),
    ResolvingMetadata { count: usize },
    Resolved { count: usize },
}

#[derive(Clone)]
pub struct BrowserDiscoverySource {
    browser: SourceAcquisitionService,
    openalex: OpenAlexProvider,
    arxiv: ArxivProvider,
    config: BrowserDiscoveryConfig,
}

impl BrowserDiscoverySource {
    pub fn new(
        browser: SourceAcquisitionService,
        openalex: OpenAlexProvider,
        arxiv: ArxivProvider,
        config: BrowserDiscoveryConfig,
    ) -> Self {
        Self {
            browser,
            openalex,
            arxiv,
            config,
        }
    }

    /// Discover provisional links, publish them, then enrich identities within
    /// a small concurrency bound. A failed page or resolver does not discard
    /// candidates obtained from the other pages.
    pub async fn discover(
        &self,
        query: &str,
        limit: usize,
        on_progress: &(dyn Fn(BrowserDiscoveryProgress) + Send + Sync),
    ) -> Result<Vec<PaperCandidate>, DiscoveryError> {
        self.discover_with_resolvers(query, limit, &[], on_progress)
            .await
    }

    /// Browser discovery with an explicit allow-list of metadata resolvers.
    /// An empty allow-list preserves the default OpenAlex/arXiv behavior.
    pub async fn discover_with_resolvers(
        &self,
        query: &str,
        limit: usize,
        resolvers: &[DiscoveryProviderChoice],
        on_progress: &(dyn Fn(BrowserDiscoveryProgress) + Send + Sync),
    ) -> Result<Vec<PaperCandidate>, DiscoveryError> {
        on_progress(BrowserDiscoveryProgress::SearchingWeb);
        let mut provisional = Vec::new();
        let mut page_errors = Vec::new();
        let mut inspected_pages = 0;
        let mut challenged_pages = 0;

        for page in 0..self.config.pages_per_query {
            let url = self.config.search_url(query, page)?;
            let started = Instant::now();
            match self.browser.inspect_browser_page(&url).await {
                Ok(inspection) => {
                    let html = inspection.snapshot.html.as_deref().unwrap_or_default();
                    let candidates = parse_search_results(html, query, limit);
                    let challenged = is_search_challenge(
                        &inspection.snapshot.final_url,
                        inspection.snapshot.text.as_deref().unwrap_or_default(),
                    );
                    inspected_pages += 1;
                    if challenged {
                        challenged_pages += 1;
                    }
                    log_page_inspection(
                        page,
                        self.config.pages_per_query,
                        &url,
                        &inspection,
                        candidates.len(),
                        challenged,
                        started.elapsed(),
                    );
                    provisional.extend(candidates);
                }
                Err(error) => {
                    let message = error.to_string();
                    log_page_failure(
                        page,
                        self.config.pages_per_query,
                        &url,
                        &message,
                        started.elapsed(),
                    );
                    page_errors.push(message);
                }
            }
            if provisional.len() >= limit {
                break;
            }
        }

        provisional = crate::services::research::dedup::dedup(provisional);
        provisional.truncate(limit);
        if provisional.is_empty() {
            return Err(empty_search_error(
                inspected_pages,
                challenged_pages,
                &page_errors,
            ));
        }

        on_progress(BrowserDiscoveryProgress::Provisional(provisional.clone()));
        on_progress(BrowserDiscoveryProgress::ResolvingMetadata {
            count: provisional.len(),
        });

        let resolved =
            stream::iter(provisional.into_iter().map(|candidate| async move {
                self.resolve_candidate(candidate, resolvers).await
            }))
            .buffer_unordered(RESOLUTION_CONCURRENCY)
            .collect::<Vec<_>>()
            .await;
        let resolved = crate::services::research::dedup::dedup(resolved);
        on_progress(BrowserDiscoveryProgress::Resolved {
            count: resolved.len(),
        });
        Ok(resolved)
    }

    async fn resolve_candidate(
        &self,
        provisional: PaperCandidate,
        resolvers: &[DiscoveryProviderChoice],
    ) -> PaperCandidate {
        let identifier = candidate_identifier(&provisional);
        let mut request = exact_request(
            identifier.as_deref().unwrap_or(&provisional.title),
            identifier.is_none(),
        );
        let permits = |provider| resolvers.is_empty() || resolvers.contains(&provider);

        let resolved = if provisional.arxiv_id.is_some() && permits(DiscoveryProviderChoice::Arxiv)
        {
            request.provider = DiscoveryProviderChoice::Arxiv;
            self.arxiv.search(&request).await.ok()
        } else if permits(DiscoveryProviderChoice::OpenAlex) {
            self.openalex.search(&request).await.ok()
        } else if permits(DiscoveryProviderChoice::Arxiv) {
            request.provider = DiscoveryProviderChoice::Arxiv;
            self.arxiv.search(&request).await.ok()
        } else {
            None
        };

        let Some(candidate) = resolved
            .into_iter()
            .flat_map(|result| result.candidates)
            .find(|candidate| identity_matches(&provisional, candidate))
        else {
            return provisional;
        };

        merge_resolved_candidate(provisional, candidate)
    }

    pub fn as_provider_result(candidates: Vec<PaperCandidate>) -> ProviderSearchResult {
        ProviderSearchResult {
            provider: DiscoveryProviderId::Web,
            filters: vec![
                "browser-first".to_string(),
                "exact metadata resolution".to_string(),
            ],
            candidates,
        }
    }
}

fn exact_request(query: &str, quote: bool) -> DiscoverySearchRequest {
    DiscoverySearchRequest {
        query: if quote {
            format!("\"{}\"", query.trim().trim_matches('"'))
        } else {
            query.to_string()
        },
        year_from: None,
        year_to: None,
        result_limit: 5,
        sort_by: DiscoverySort::Relevance,
        provider: Default::default(),
        providers: Vec::new(),
        open_access: false,
        only_viewable: false,
        venues: Vec::new(),
        authors: Vec::new(),
        fields_of_study: Vec::new(),
    }
}

/// Parses provisional scholarly candidates from one rendered result page.
fn parse_search_results(html: &str, query: &str, limit: usize) -> Vec<PaperCandidate> {
    let document = Html::parse_document(html);
    let scholar_result = Selector::parse(".gs_ri").expect("valid selector");
    let scholar_title = Selector::parse(".gs_rt a").expect("valid selector");
    let scholar_snippet = Selector::parse(".gs_rs").expect("valid selector");
    let mut results = Vec::new();

    for row in document.select(&scholar_result) {
        let Some(link) = row.select(&scholar_title).next() else {
            continue;
        };
        let Some(url) = link.value().attr("href").and_then(normalize_result_url) else {
            continue;
        };
        let title = clean_title(&link.text().collect::<Vec<_>>().join(" "));
        let snippet = row
            .select(&scholar_snippet)
            .next()
            .map(|node| clean_text(&node.text().collect::<Vec<_>>().join(" ")));
        if let Some(candidate) = provisional_candidate(title, url, snippet, query, results.len()) {
            results.push(candidate);
        }
        if results.len() >= limit {
            return results;
        }
    }

    if !results.is_empty() {
        return results;
    }

    results = parse_brave_results(&document, query, limit);
    if !results.is_empty() {
        return results;
    }

    let anchors = Selector::parse("a[href]").expect("valid selector");
    for link in document.select(&anchors) {
        let Some(url) = link.value().attr("href").and_then(normalize_result_url) else {
            continue;
        };
        let title = clean_title(&link.text().collect::<Vec<_>>().join(" "));
        if let Some(candidate) = provisional_candidate(title, url, None, query, results.len()) {
            results.push(candidate);
        }
        if results.len() >= limit {
            break;
        }
    }
    results
}

/// Parses Brave's server-rendered web-result rows without including page chrome.
fn parse_brave_results(document: &Html, query: &str, limit: usize) -> Vec<PaperCandidate> {
    let rows = Selector::parse("div.snippet[data-type=\"web\"]").expect("valid selector");
    let anchors = Selector::parse("a[href]").expect("valid selector");
    let titles = Selector::parse(".title").expect("valid selector");
    let snippets = Selector::parse(".generic-snippet .content").expect("valid selector");
    let mut results = Vec::new();

    for row in document.select(&rows) {
        let Some((link, title_node)) = row.select(&anchors).find_map(|link| {
            let title = link.select(&titles).next()?;
            Some((link, title))
        }) else {
            continue;
        };
        let Some(url) = link.value().attr("href").and_then(normalize_result_url) else {
            continue;
        };
        let title = clean_title(&title_node.text().collect::<Vec<_>>().join(" "));
        let snippet = row
            .select(&snippets)
            .next()
            .map(|node| clean_text(&node.text().collect::<Vec<_>>().join(" ")));
        if let Some(candidate) = provisional_candidate(title, url, snippet, query, results.len()) {
            results.push(candidate);
        }
        if results.len() >= limit {
            break;
        }
    }

    results
}

/// Returns whether Google replaced the requested result page with a challenge.
fn is_search_challenge(final_url: &str, visible_text: &str) -> bool {
    let challenge_url = Url::parse(final_url).is_ok_and(|url| {
        is_google_owned_host(url.host_str().unwrap_or_default())
            && url.path().starts_with("/sorry/")
    });
    let challenge_text = visible_text
        .to_ascii_lowercase()
        .contains("our systems have detected unusual traffic");
    challenge_url || challenge_text
}

/// Selects an honest error after every inspected page produced no candidate.
fn empty_search_error(
    inspected_pages: usize,
    challenged_pages: usize,
    page_errors: &[String],
) -> DiscoveryError {
    if inspected_pages > 0 && inspected_pages == challenged_pages {
        return DiscoveryError::new(
            "Browser search was challenged by Google Scholar; retry later or use another configured search entry point.",
        );
    }
    if page_errors.is_empty() {
        DiscoveryError::new("Browser search returned no scholarly links")
    } else {
        DiscoveryError::new(format!("Browser search failed: {}", page_errors.join("; ")))
    }
}

/// Logs bounded summary and debug evidence for one successful inspection.
fn log_page_inspection(
    page: usize,
    total_pages: usize,
    requested_url: &str,
    inspection: &PageInspection,
    candidate_count: usize,
    challenged: bool,
    elapsed: Duration,
) {
    let html = inspection.snapshot.html.as_deref().unwrap_or_default();
    let text = inspection.snapshot.text.as_deref().unwrap_or_default();
    let (final_host, final_path) = sanitized_host_and_path(&inspection.snapshot.final_url);
    let classification = if challenged {
        "challenge"
    } else if candidate_count == 0 {
        "empty_result_page"
    } else {
        "result_page"
    };
    crate::shared::log::info(
        "browser-discovery",
        format!(
            "page={}/{} requested_host={} final_host={} final_path={} classification={} title={:?} html_bytes={} text_bytes={} raw_links={} assets={} network_urls={} candidates={} elapsed_ms={}",
            page + 1,
            total_pages,
            sanitized_host(requested_url),
            final_host,
            final_path,
            classification,
            sanitized_page_title(inspection.snapshot.title.as_deref().unwrap_or_default()),
            html.len(),
            text.len(),
            inspection.links.len(),
            inspection.assets.len(),
            inspection.network_urls.len(),
            candidate_count,
            elapsed.as_millis(),
        ),
    );
    if !crate::shared::log::enabled(crate::shared::log::Level::Debug) {
        return;
    }
    let link_hosts = inspection
        .links
        .iter()
        .filter_map(|link| Url::parse(link).ok())
        .filter_map(|url| url.host_str().map(str::to_string))
        .take(LOG_LINK_HOST_LIMIT)
        .collect::<Vec<_>>();
    crate::shared::log::debug(
        "browser-discovery",
        format!(
            "classification={} text_preview={:?} link_hosts={:?}",
            classification,
            sanitized_text_preview(inspection.snapshot.text.as_deref().unwrap_or_default()),
            link_hosts,
        ),
    );
}

/// Logs a sanitized warning when Obscura cannot inspect a result page.
fn log_page_failure(
    page: usize,
    total_pages: usize,
    requested_url: &str,
    error: &str,
    elapsed: Duration,
) {
    crate::shared::log::warn(
        "browser-discovery",
        format!(
            "page={}/{} requested_host={} classification={} error={:?} elapsed_ms={}",
            page + 1,
            total_pages,
            sanitized_host(requested_url),
            "inspection_failed",
            sanitized_log_text(error, LOG_TEXT_PREVIEW_LIMIT),
            elapsed.as_millis(),
        ),
    );
}

/// Returns only the host portion of a URL for safe correlation.
fn sanitized_host(url: &str) -> String {
    Url::parse(url)
        .ok()
        .and_then(|url| url.host_str().map(str::to_string))
        .unwrap_or_else(|| "-".to_string())
}

/// Returns host and path without query parameters or fragments.
fn sanitized_host_and_path(url: &str) -> (String, String) {
    let Ok(url) = Url::parse(url) else {
        return ("-".to_string(), "-".to_string());
    };
    (
        url.host_str().unwrap_or("-").to_string(),
        url.path().to_string(),
    )
}

/// Bounds a page title and removes URL query parameters when it is URL-shaped.
fn sanitized_page_title(title: &str) -> String {
    let title = clean_text(title);
    if let Ok(url) = Url::parse(&title) {
        let host = url.host_str().unwrap_or("-");
        return truncate_chars(&format!("{host}{}", url.path()), LOG_TITLE_LIMIT);
    }
    sanitized_log_text(&title, LOG_TITLE_LIMIT)
}

/// Returns a bounded page-text preview with network identifiers removed.
fn sanitized_text_preview(text: &str) -> String {
    sanitized_log_text(text, LOG_TEXT_PREVIEW_LIMIT)
}

/// Sanitizes arbitrary page-derived text before it reaches a log line.
fn sanitized_log_text(text: &str, limit: usize) -> String {
    let lower = text.to_ascii_lowercase();
    let cutoff = ["ip address:", "client ip:", "remote address:"]
        .into_iter()
        .filter_map(|marker| lower.find(marker))
        .min()
        .unwrap_or(text.len());
    let sanitized = text[..cutoff]
        .split_whitespace()
        .map(sanitize_log_token)
        .collect::<Vec<_>>()
        .join(" ");
    truncate_chars(&sanitized, limit)
}

/// Replaces an HTTP URL token with its query-free host and path.
fn sanitize_log_token(token: &str) -> String {
    let trimmed = token.trim_matches(|character: char| {
        matches!(
            character,
            '(' | ')' | '[' | ']' | '<' | '>' | ',' | ';' | '"'
        )
    });
    if let Ok(url) = Url::parse(trimmed) {
        if matches!(url.scheme(), "http" | "https") {
            return format!("{}{}", url.host_str().unwrap_or("-"), url.path());
        }
    }
    token.to_string()
}

/// Truncates Unicode text to a maximum character count including an ellipsis.
fn truncate_chars(value: &str, limit: usize) -> String {
    if value.chars().count() <= limit {
        return value.to_string();
    }
    if limit <= 3 {
        return ".".repeat(limit);
    }
    format!("{}...", value.chars().take(limit - 3).collect::<String>())
}

fn provisional_candidate(
    title: String,
    url: String,
    snippet: Option<String>,
    query: &str,
    position: usize,
) -> Option<PaperCandidate> {
    if title.len() < 12 || !looks_like_external_result(&url) {
        return None;
    }
    let source_id = short_hash(&url);
    let arxiv_id = extract_arxiv_id(&url);
    let doi = extract_doi(&url);
    let pdf_url = is_pdf_url(&url).then(|| url.clone());
    Some(PaperCandidate {
        id: format!("web:{source_id}"),
        source_provider: "web".to_string(),
        source_id,
        title,
        authors: Vec::new(),
        abstract_text: snippet,
        year: None,
        publication_date: None,
        venue: None,
        citation_count: None,
        doi,
        openalex_id: None,
        arxiv_id,
        external_url: Some(url),
        pdf_url,
        open_access: None,
        match_summary: CandidateMatch {
            score: Some((1.0 - position as f64 * 0.03).max(0.1)),
            reasons: vec!["discovered:web".to_string()],
            matched_keywords: query.split_whitespace().map(str::to_string).collect(),
            from_seed_paper_ids: Vec::new(),
        },
        already_in_library: false,
    })
}

fn merge_resolved_candidate(
    provisional: PaperCandidate,
    mut resolved: PaperCandidate,
) -> PaperCandidate {
    // The row already exists before resolution. Keep its identity so richer
    // metadata updates that row instead of creating visible churn.
    resolved.id = provisional.id.clone();
    if resolved.external_url.is_none() {
        resolved.external_url = provisional.external_url;
    }
    if resolved.pdf_url.is_none() {
        resolved.pdf_url = provisional.pdf_url;
    }
    resolved
        .match_summary
        .reasons
        .retain(|reason| !reason.starts_with("provider:"));
    resolved.match_summary.reasons.extend([
        "discovered:web".to_string(),
        format!("resolved:{}", resolved.source_provider),
    ]);
    resolved
}

fn identity_matches(provisional: &PaperCandidate, resolved: &PaperCandidate) -> bool {
    if let (Some(expected), Some(actual)) = (&provisional.doi, &resolved.doi) {
        return normalized_identifier(expected) == normalized_identifier(actual);
    }
    if let (Some(expected), Some(actual)) = (&provisional.arxiv_id, &resolved.arxiv_id) {
        return normalized_identifier(expected) == normalized_identifier(actual);
    }
    normalize_title(&provisional.title) == normalize_title(&resolved.title)
}

fn candidate_identifier(candidate: &PaperCandidate) -> Option<String> {
    candidate
        .doi
        .as_ref()
        .map(|doi| format!("doi:{doi}"))
        .or_else(|| candidate.arxiv_id.as_ref().map(|id| format!("id:{id}")))
}

fn normalize_title(title: &str) -> String {
    title
        .chars()
        .filter(|character| character.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn normalized_identifier(identifier: &str) -> String {
    identifier
        .trim()
        .trim_start_matches("https://doi.org/")
        .to_lowercase()
}

fn normalize_result_url(raw: &str) -> Option<String> {
    let absolute = Url::parse(raw).ok()?;
    if let Some(target) = absolute
        .query_pairs()
        .find(|(key, _)| key == "q" || key == "url")
        .map(|(_, value)| value.into_owned())
        .filter(|value| value.starts_with("http://") || value.starts_with("https://"))
    {
        return Some(target);
    }
    Some(absolute.into())
}

fn looks_like_external_result(url: &str) -> bool {
    let Ok(url) = Url::parse(url) else {
        return false;
    };
    let host = url.host_str().unwrap_or_default();
    !host.is_empty()
        && !host.ends_with("google.com")
        && !host.ends_with("googleusercontent.com")
        && !host.ends_with("gstatic.com")
        && host != "search.brave.com"
}

/// Returns whether a host is Google itself or one of its subdomains.
fn is_google_owned_host(host: &str) -> bool {
    host == "google.com" || host.ends_with(".google.com")
}

fn extract_doi(url: &str) -> Option<String> {
    let pattern = Regex::new(r"(?i)10\.\d{4,9}/[-._;()/:a-z0-9]+ ").expect("valid DOI regex");
    pattern
        .find(&format!("{url} "))
        .map(|matched| matched.as_str().trim().trim_end_matches('.').to_lowercase())
}

fn extract_arxiv_id(url: &str) -> Option<String> {
    let pattern = Regex::new(r"(?i)arxiv\.org/(?:abs|pdf)/([^?#]+)").expect("valid arXiv regex");
    pattern
        .captures(url)
        .and_then(|captures| captures.get(1))
        .map(|matched| matched.as_str().trim_end_matches(".pdf").to_string())
}

fn is_pdf_url(url: &str) -> bool {
    let lower = url.to_lowercase();
    lower.ends_with(".pdf") || lower.contains("/pdf/") || lower.contains("/pdf?")
}

fn clean_title(value: &str) -> String {
    clean_text(value)
        .trim_start_matches("[PDF]")
        .trim_start_matches("[HTML]")
        .trim()
        .to_string()
}

fn clean_text(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn short_hash(value: &str) -> String {
    let digest = Sha256::digest(value.as_bytes());
    digest[..6]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn candidate_config_paths(app: &tauri::AppHandle) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Ok(cwd) = std::env::current_dir() {
        paths.push(cwd.join("i0i.config.toml"));
    }
    if let Ok(config_dir) = app.path().app_config_dir() {
        paths.push(config_dir.join("i0i.config.toml"));
    }
    paths
}

fn apply_config(config: &mut BrowserDiscoveryConfig, contents: &str) {
    let mut in_discovery = false;
    for line in contents.lines().map(str::trim) {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            in_discovery = line == "[discovery]";
            continue;
        }
        if !in_discovery {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.trim().trim_matches('"');
        match key.trim() {
            "browser_search_url" => config.search_url_template = value.to_string(),
            "browser_pages_per_query" => {
                if let Ok(pages) = value.parse::<usize>() {
                    config.pages_per_query = pages.clamp(1, 2);
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use serde::Deserialize;
    use std::sync::Arc;

    use crate::services::source_acquisition::types::{
        AcquisitionMethod, AcquisitionResult, BrowserEndpoint, BrowserPageSnapshot, BrowserRuntime,
        FetchResponse, HttpFetcher, SourceAcquisitionConfig, SourceAcquisitionError,
    };

    #[derive(Deserialize)]
    struct CorpusCase {
        goal: String,
        expected_title: String,
        html: String,
    }

    struct UnusedHttp;

    #[async_trait]
    impl HttpFetcher for UnusedHttp {
        async fn fetch(&self, _url: &str) -> AcquisitionResult<FetchResponse> {
            Err(SourceAcquisitionError::Http(
                "HTTP is not used by browser discovery".to_string(),
            ))
        }
    }

    struct ChallengeBrowser {
        inspection: PageInspection,
    }

    #[async_trait]
    impl BrowserRuntime for ChallengeBrowser {
        async fn ensure_ready(&self) -> AcquisitionResult<BrowserEndpoint> {
            Ok(BrowserEndpoint {
                port: 9222,
                url: "http://127.0.0.1:9222".to_string(),
                websocket_url: Some("ws://127.0.0.1:9222/devtools/browser".to_string()),
            })
        }

        async fn fetch_original(&self, _url: &str) -> AcquisitionResult<FetchResponse> {
            Err(SourceAcquisitionError::Browser(
                "original fetch is not used by browser discovery".to_string(),
            ))
        }

        async fn inspect_page(&self, _url: &str) -> AcquisitionResult<PageInspection> {
            Ok(self.inspection.clone())
        }
    }

    #[test]
    fn parses_google_scholar_results_as_provisional_candidates() {
        let html = r#"
            <div class="gs_ri">
              <h3 class="gs_rt"><a href="https://doi.org/10.1000/example">A Useful Research Paper</a></h3>
              <div class="gs_rs">A concise abstract snippet.</div>
            </div>
        "#;
        let candidates = parse_search_results(html, "useful research", 10);
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].doi.as_deref(), Some("10.1000/example"));
        assert_eq!(candidates[0].source_provider, "web");
        assert_eq!(
            candidates[0].abstract_text.as_deref(),
            Some("A concise abstract snippet.")
        );
    }

    #[test]
    fn parses_only_primary_web_results_from_brave() {
        let html = include_str!("../../../tests/fixtures/browser_discovery_brave.html");
        let candidates = parse_search_results(html, "lowrank adaptation", 10);

        assert_eq!(candidates.len(), 2);
        assert_eq!(candidates[0].arxiv_id.as_deref(), Some("2106.09685"));
        assert_eq!(
            candidates[0].title,
            "[2106.09685] LoRA: Low-Rank Adaptation of Large Language Models"
        );
        assert_eq!(
            candidates[0].abstract_text.as_deref(),
            Some("We propose Low-Rank Adaptation, or LoRA, for efficient fine-tuning.")
        );
        assert_eq!(candidates[1].arxiv_id.as_deref(), Some("2104.14294"));
        assert!(candidates
            .iter()
            .all(|candidate| candidate.source_provider == "web"));
    }

    #[test]
    fn exact_title_verification_ignores_case_and_punctuation() {
        let provisional = provisional_candidate(
            "Attention Is All You Need".to_string(),
            "https://example.test/paper".to_string(),
            None,
            "attention",
            0,
        )
        .unwrap();
        let mut resolved = provisional.clone();
        resolved.title = "Attention is all you need!".to_string();
        assert!(identity_matches(&provisional, &resolved));
    }

    #[test]
    fn config_builds_default_brave_search_urls() {
        let config = BrowserDiscoveryConfig::default();
        let url = config.search_url("graph neural networks", 1).unwrap();
        assert_eq!(
            url,
            "https://search.brave.com/search?q=graph+neural+networks&source=web"
        );
    }

    #[test]
    fn classifies_recorded_google_challenge() {
        let html = include_str!("../../../tests/fixtures/browser_discovery_challenge.html");
        let text = Html::parse_document(html)
            .root_element()
            .text()
            .collect::<Vec<_>>()
            .join(" ");
        let candidates = parse_search_results(html, "attention is all you need", 10);

        assert!(candidates.is_empty());
        assert!(is_search_challenge(
            "https://www.google.com/sorry/index?continue=private&token=secret",
            &text,
        ));
    }

    #[tokio::test]
    async fn discovery_reports_a_challenge_returned_by_the_browser() {
        let html = include_str!("../../../tests/fixtures/browser_discovery_challenge.html");
        let text = Html::parse_document(html)
            .root_element()
            .text()
            .collect::<Vec<_>>()
            .join(" ");
        let inspection = PageInspection {
            snapshot: BrowserPageSnapshot {
                url: "https://scholar.google.com/scholar".to_string(),
                final_url: "https://www.google.com/sorry/index?continue=private&token=secret"
                    .to_string(),
                title: Some(
                    "https://scholar.google.com/scholar?q=attention+is+all+you+need&start=0"
                        .to_string(),
                ),
                content_type: Some("text/html".to_string()),
                html: Some(html.to_string()),
                text: Some(text),
            },
            links: vec![
                "https://www.google.com/sorry/index#".to_string(),
                "https://www.google.com/policies/terms/".to_string(),
                "https://support.google.com/websearch/answer/86640".to_string(),
            ],
            assets: Vec::new(),
            network_urls: Vec::new(),
        };
        let service = SourceAcquisitionService::new(
            SourceAcquisitionConfig::default(),
            Arc::new(UnusedHttp),
            Arc::new(ChallengeBrowser { inspection }),
            AcquisitionMethod::ObscuraBrowserStealth,
        );
        let source = BrowserDiscoverySource::new(
            service,
            OpenAlexProvider::from_app_config().expect("OpenAlex test config"),
            ArxivProvider::from_app_config().expect("arXiv test config"),
            BrowserDiscoveryConfig::default(),
        );

        let error = source
            .discover("attention is all you need", 10, &|_| {})
            .await
            .expect_err("challenge page must not become an empty result");

        assert_eq!(
            error.to_string(),
            "Browser search was challenged by Google Scholar; retry later or use another configured search entry point."
        );
    }

    #[test]
    fn sanitizes_bounded_challenge_evidence() {
        let html = include_str!("../../../tests/fixtures/browser_discovery_challenge.html");
        let text = Html::parse_document(html)
            .root_element()
            .text()
            .collect::<Vec<_>>()
            .join(" ");

        let preview = sanitized_text_preview(&text);
        assert!(preview.contains("unusual traffic"));
        assert!(preview.chars().count() <= 500);
        for sensitive in ["2001:db8::1", "192.0.2.1", "?q=", "continue=", "token="] {
            assert!(!preview.contains(sensitive));
        }
        assert_eq!(
            sanitized_page_title(
                "https://scholar.google.com/scholar?q=attention+is+all+you+need&start=0"
            ),
            "scholar.google.com/scholar"
        );
        assert_eq!(
            sanitized_host_and_path(
                "https://www.google.com/sorry/index?continue=private&token=secret"
            ),
            ("www.google.com".to_string(), "/sorry/index".to_string())
        );

        let long_preview = sanitized_text_preview(&"x".repeat(600));
        assert_eq!(long_preview.chars().count(), 500);
        assert!(long_preview.ends_with("..."));
    }

    #[test]
    fn selects_distinct_empty_search_errors() {
        assert_eq!(
            empty_search_error(1, 1, &[]).to_string(),
            "Browser search was challenged by Google Scholar; retry later or use another configured search entry point."
        );
        assert_eq!(
            empty_search_error(1, 0, &[]).to_string(),
            "Browser search returned no scholarly links"
        );
        assert_eq!(
            empty_search_error(0, 0, &["CDP connection closed".to_string()]).to_string(),
            "Browser search failed: CDP connection closed"
        );
    }

    #[test]
    fn recorded_corpus_retains_every_hand_marked_result() {
        let cases: Vec<CorpusCase> = serde_json::from_str(include_str!(
            "../../../tests/fixtures/browser_discovery_corpus.json"
        ))
        .expect("valid browser discovery corpus");
        assert_eq!(cases.len(), 10);

        let found = cases
            .iter()
            .filter(|case| {
                parse_search_results(&case.html, &case.goal, 10)
                    .iter()
                    .any(|candidate| {
                        normalize_title(&candidate.title) == normalize_title(&case.expected_title)
                    })
            })
            .count();

        assert_eq!(
            found,
            cases.len(),
            "recorded corpus recall must remain 100%"
        );
    }
}
