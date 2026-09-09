//! Browser-first scholarly candidate discovery (RFC 0098).
//!
//! Obscura finds result links. Provider APIs are consulted only after a result
//! has a DOI, arXiv identifier, or an exact title that can be verified. The
//! output is the existing `PaperCandidate`, so every caller shares the current
//! deduplication and ranking pipeline.

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use futures_util::{stream, StreamExt};
use regex::Regex;
#[cfg(test)]
use scraper::{Html, Selector};
use sha2::{Digest, Sha256};
use tauri::Manager;
use tokio::sync::{Mutex, MutexGuard};
use url::Url;

use super::browser_engine::{BrowserResult, BrowserSearchEngine};
use super::error::DiscoveryError;
use super::provider::{DiscoveryProvider, DiscoveryProviderId, ProviderSearchResult};
use super::providers::{arxiv::ArxivProvider, openalex::OpenAlexProvider};
use crate::domain::discovery::{
    paper_candidate_dedup_key, CandidateMatch, DiscoveryProviderChoice, DiscoverySearchRequest,
    DiscoverySort, PaperCandidate,
};
use crate::services::source_acquisition::types::PageInspection;
use crate::services::source_acquisition::SourceAcquisitionService;

const RESOLUTION_CONCURRENCY: usize = 4;
// Diagnostic payloads stay short enough for routine development logs.
const LOG_TITLE_LIMIT: usize = 160;
const LOG_TEXT_PREVIEW_LIMIT: usize = 500;
const LOG_LINK_HOST_LIMIT: usize = 5;

#[derive(Debug, Clone)]
pub struct BrowserDiscoveryConfig {
    /// Browser engines attempted for each broad query.
    pub engines: Vec<BrowserSearchEngine>,
    /// Optional legacy URL template, represented as the `Custom` engine.
    pub custom_search_url_template: Option<String>,
    /// Number of result pages read for one query.
    pub pages_per_query: usize,
    /// Enough merged candidates to stop trying additional engines.
    pub candidate_target: usize,
    /// Minimum delay between starts against one engine.
    pub minimum_interval: Duration,
    /// Time an engine rests after a challenge or explicit rate limit.
    pub challenge_cooldown: Duration,
}

impl Default for BrowserDiscoveryConfig {
    fn default() -> Self {
        Self {
            engines: vec![
                BrowserSearchEngine::DuckDuckGo,
                BrowserSearchEngine::Ecosia,
                BrowserSearchEngine::Brave,
            ],
            custom_search_url_template: None,
            pages_per_query: 1,
            candidate_target: 10,
            minimum_interval: Duration::from_millis(1_000),
            challenge_cooldown: Duration::from_secs(300),
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

    fn search_url(
        &self,
        engine: BrowserSearchEngine,
        query: &str,
        page: usize,
    ) -> Result<String, DiscoveryError> {
        engine
            .search_url(query, page, self.custom_search_url_template.as_deref())
            .map_err(DiscoveryError::new)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrowserAttemptStatus {
    Succeeded,
    Empty,
    Challenged,
    RateLimited,
    Unavailable,
    ParseFailed,
}

impl BrowserAttemptStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Succeeded => "succeeded",
            Self::Empty => "empty",
            Self::Challenged => "challenged",
            Self::RateLimited => "rate_limited",
            Self::Unavailable => "unavailable",
            Self::ParseFailed => "parse_failed",
        }
    }
}

#[derive(Debug, Clone)]
pub struct BrowserProviderAttempt {
    pub provider: String,
    pub status: BrowserAttemptStatus,
    pub candidate_count: usize,
    pub elapsed_ms: u128,
    pub reason: Option<String>,
}

#[derive(Debug, Clone)]
pub enum BrowserDiscoveryProgress {
    SearchingWeb,
    SearchingProvider { provider: String },
    ProviderAttempt(BrowserProviderAttempt),
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
    health: BrowserSearchHealth,
}

#[derive(Clone, Default)]
struct BrowserSearchHealth {
    rotation: Arc<AtomicUsize>,
    duckduckgo: Arc<Mutex<EngineHealth>>,
    ecosia: Arc<Mutex<EngineHealth>>,
    brave: Arc<Mutex<EngineHealth>>,
    custom: Arc<Mutex<EngineHealth>>,
}

#[derive(Default)]
struct EngineHealth {
    last_started: Option<Instant>,
    cooldown_until: Option<Instant>,
}

impl BrowserSearchHealth {
    fn ordered_engines(&self, configured: &[BrowserSearchEngine]) -> Vec<BrowserSearchEngine> {
        if configured.is_empty() {
            return Vec::new();
        }
        let start = self.rotation.fetch_add(1, Ordering::Relaxed) % configured.len();
        configured
            .iter()
            .cycle()
            .skip(start)
            .take(configured.len())
            .copied()
            .collect()
    }

    async fn lock(&self, engine: BrowserSearchEngine) -> MutexGuard<'_, EngineHealth> {
        match engine {
            BrowserSearchEngine::DuckDuckGo => self.duckduckgo.lock().await,
            BrowserSearchEngine::Ecosia => self.ecosia.lock().await,
            BrowserSearchEngine::Brave => self.brave.lock().await,
            BrowserSearchEngine::Custom => self.custom.lock().await,
        }
    }
}

fn shared_browser_health() -> BrowserSearchHealth {
    static HEALTH: OnceLock<BrowserSearchHealth> = OnceLock::new();
    HEALTH.get_or_init(BrowserSearchHealth::default).clone()
}

impl BrowserDiscoverySource {
    #[cfg(test)]
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
            health: BrowserSearchHealth::default(),
        }
    }

    /// Construct a source that shares provider health across app search paths.
    pub fn new_shared(
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
            health: shared_browser_health(),
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
        let mut attempts = Vec::new();

        for engine in self.health.ordered_engines(&self.config.engines) {
            on_progress(BrowserDiscoveryProgress::SearchingProvider {
                provider: engine.as_str().to_string(),
            });
            let (candidates, attempt) = self.search_engine(engine, query, limit).await;
            on_progress(BrowserDiscoveryProgress::ProviderAttempt(attempt.clone()));
            attempts.push(attempt);
            merge_browser_candidates(&mut provisional, candidates);
            rank_browser_candidates(&mut provisional, query);
            if !provisional.is_empty() {
                on_progress(BrowserDiscoveryProgress::Provisional(provisional.clone()));
            }
            if provisional.len() >= limit.min(self.config.candidate_target) {
                break;
            }
        }

        provisional.truncate(limit);
        if provisional.is_empty() {
            let fallback = self.resolve_exact_query(query, limit, resolvers).await;
            if !fallback.is_empty() {
                on_progress(BrowserDiscoveryProgress::Provisional(fallback.clone()));
                on_progress(BrowserDiscoveryProgress::Resolved {
                    count: fallback.len(),
                });
                return Ok(fallback);
            }
            return Err(empty_search_error(&attempts));
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
        let mut resolved = crate::services::research::dedup::dedup(resolved);
        rank_browser_candidates(&mut resolved, query);
        resolved.truncate(limit);
        on_progress(BrowserDiscoveryProgress::Resolved {
            count: resolved.len(),
        });
        Ok(resolved)
    }

    /// Search one engine while enforcing its process-local interval and cooldown.
    async fn search_engine(
        &self,
        engine: BrowserSearchEngine,
        query: &str,
        limit: usize,
    ) -> (Vec<PaperCandidate>, BrowserProviderAttempt) {
        let started = Instant::now();
        let mut health = self.health.lock(engine).await;
        let now = Instant::now();
        if health.cooldown_until.is_some_and(|until| until > now) {
            return (
                Vec::new(),
                BrowserProviderAttempt {
                    provider: engine.as_str().to_string(),
                    status: BrowserAttemptStatus::Unavailable,
                    candidate_count: 0,
                    elapsed_ms: started.elapsed().as_millis(),
                    reason: Some("provider cooldown is active".to_string()),
                },
            );
        }
        if let Some(last_started) = health.last_started {
            let elapsed = last_started.elapsed();
            if elapsed < self.config.minimum_interval {
                tokio::time::sleep(self.config.minimum_interval - elapsed).await;
            }
        }
        health.last_started = Some(Instant::now());

        let mut candidates = Vec::new();
        let mut errors = Vec::new();
        let mut status = BrowserAttemptStatus::Empty;
        for page in 0..self.config.pages_per_query {
            let url = match self.config.search_url(engine, query, page) {
                Ok(url) => url,
                Err(error) => {
                    errors.push(error.to_string());
                    status = BrowserAttemptStatus::ParseFailed;
                    break;
                }
            };
            let page_started = Instant::now();
            match self.browser.inspect_browser_page(&url).await {
                Ok(inspection) => {
                    let html = inspection.snapshot.html.as_deref().unwrap_or_default();
                    let text = inspection.snapshot.text.as_deref().unwrap_or_default();
                    let challenged =
                        engine.is_challenge(&inspection.snapshot.final_url, text, html);
                    let rate_limited = engine.is_rate_limited(text);
                    let parsed = engine.parse_results(html, limit.saturating_sub(candidates.len()));
                    let parsed = parsed
                        .into_iter()
                        .filter_map(|result| {
                            provisional_candidate_for_engine(
                                result,
                                query,
                                candidates.len(),
                                engine,
                            )
                        })
                        .collect::<Vec<_>>();
                    log_page_inspection(
                        engine,
                        page,
                        self.config.pages_per_query,
                        &url,
                        &inspection,
                        parsed.len(),
                        challenged,
                        page_started.elapsed(),
                    );
                    if challenged || rate_limited {
                        status = if rate_limited {
                            BrowserAttemptStatus::RateLimited
                        } else {
                            BrowserAttemptStatus::Challenged
                        };
                        health.cooldown_until =
                            Some(Instant::now() + self.config.challenge_cooldown);
                        break;
                    }
                    if parsed.is_empty() && inspection.links.len() >= 5 {
                        status = BrowserAttemptStatus::ParseFailed;
                        errors.push(format!(
                            "result page exposed {} links but no recognized result rows",
                            inspection.links.len()
                        ));
                    }
                    candidates.extend(parsed);
                    if candidates.len() >= limit {
                        break;
                    }
                }
                Err(error) => {
                    let message = error.to_string();
                    log_page_failure(
                        engine,
                        page,
                        self.config.pages_per_query,
                        &url,
                        &message,
                        page_started.elapsed(),
                    );
                    errors.push(message);
                    status = BrowserAttemptStatus::Unavailable;
                }
            }
        }
        if !candidates.is_empty() {
            status = BrowserAttemptStatus::Succeeded;
        }
        let reason = match status {
            BrowserAttemptStatus::Challenged => Some("bot challenge returned".to_string()),
            BrowserAttemptStatus::RateLimited => Some("rate limit returned".to_string()),
            BrowserAttemptStatus::Empty => Some("no usable result links".to_string()),
            BrowserAttemptStatus::Unavailable | BrowserAttemptStatus::ParseFailed => {
                Some(errors.join("; "))
            }
            BrowserAttemptStatus::Succeeded => None,
        };
        (
            candidates.clone(),
            BrowserProviderAttempt {
                provider: engine.as_str().to_string(),
                status,
                candidate_count: candidates.len(),
                elapsed_ms: started.elapsed().as_millis(),
                reason,
            },
        )
    }

    /// Resolve an explicit DOI, arXiv id, or quoted title when browser rows vanish.
    async fn resolve_exact_query(
        &self,
        query: &str,
        limit: usize,
        resolvers: &[DiscoveryProviderChoice],
    ) -> Vec<PaperCandidate> {
        let Some(hint) = exact_query_hint(query) else {
            return Vec::new();
        };
        let permits = |provider| resolvers.is_empty() || resolvers.contains(&provider);
        let mut candidates = Vec::new();

        if hint.kind == ExactQueryKind::Arxiv && permits(DiscoveryProviderChoice::Arxiv) {
            let mut request = exact_request(&hint.value, false);
            request.provider = DiscoveryProviderChoice::Arxiv;
            if let Ok(result) = self.arxiv.search(&request).await {
                candidates.extend(result.candidates);
            }
        } else if hint.kind == ExactQueryKind::Doi && permits(DiscoveryProviderChoice::OpenAlex) {
            let request = exact_request(&format!("doi:{}", hint.value), false);
            if let Ok(result) = self.openalex.search(&request).await {
                candidates.extend(result.candidates);
            }
        } else if hint.kind == ExactQueryKind::Title {
            if permits(DiscoveryProviderChoice::OpenAlex) {
                let request = exact_request(&hint.value, true);
                if let Ok(result) = self.openalex.search(&request).await {
                    candidates.extend(result.candidates);
                }
            }
            if permits(DiscoveryProviderChoice::Arxiv) {
                let mut request = exact_request(&hint.value, true);
                request.provider = DiscoveryProviderChoice::Arxiv;
                if let Ok(result) = self.arxiv.search(&request).await {
                    candidates.extend(result.candidates);
                }
            }
        }

        let expected = normalize_title(&hint.value);
        let mut candidates = crate::services::research::dedup::dedup(candidates)
            .into_iter()
            .filter(|candidate| match hint.kind {
                ExactQueryKind::Arxiv => candidate
                    .arxiv_id
                    .as_deref()
                    .is_some_and(|id| normalized_identifier(id) == hint.value.to_lowercase()),
                ExactQueryKind::Doi => candidate
                    .doi
                    .as_deref()
                    .is_some_and(|doi| normalized_identifier(doi) == hint.value.to_lowercase()),
                ExactQueryKind::Title => normalize_title(&candidate.title) == expected,
            })
            .collect::<Vec<_>>();
        for candidate in &mut candidates {
            candidate
                .match_summary
                .reasons
                .push("fallback:exact_provider".to_string());
        }
        candidates.truncate(limit);
        candidates
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
#[cfg(test)]
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
#[cfg(test)]
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
#[cfg(test)]
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

/// Summarize provider exhaustion without presenting infrastructure failure as
/// evidence that no relevant scholarship exists.
fn empty_search_error(attempts: &[BrowserProviderAttempt]) -> DiscoveryError {
    if attempts.is_empty() {
        return DiscoveryError::new("Browser search has no configured providers");
    }
    let summary = attempts
        .iter()
        .map(|attempt| {
            let reason = attempt.reason.as_deref().unwrap_or("no detail");
            format!(
                "{}={} ({reason})",
                attempt.provider,
                attempt.status.as_str()
            )
        })
        .collect::<Vec<_>>()
        .join("; ");
    DiscoveryError::new(format!(
        "Browser search exhausted its providers without usable candidates: {summary}"
    ))
}

/// Logs bounded summary and debug evidence for one successful inspection.
fn log_page_inspection(
    engine: BrowserSearchEngine,
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
            "provider={} page={}/{} requested_host={} final_host={} final_path={} classification={} title={:?} html_bytes={} text_bytes={} raw_links={} assets={} network_urls={} candidates={} elapsed_ms={}",
            engine.as_str(),
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
    engine: BrowserSearchEngine,
    page: usize,
    total_pages: usize,
    requested_url: &str,
    error: &str,
    elapsed: Duration,
) {
    crate::shared::log::warn(
        "browser-discovery",
        format!(
            "provider={} page={}/{} requested_host={} classification={} error={:?} elapsed_ms={}",
            engine.as_str(),
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

#[cfg(test)]
fn provisional_candidate(
    title: String,
    url: String,
    snippet: Option<String>,
    query: &str,
    position: usize,
) -> Option<PaperCandidate> {
    provisional_candidate_for_engine(
        BrowserResult {
            title,
            url,
            snippet,
        },
        query,
        position,
        BrowserSearchEngine::Custom,
    )
}

fn provisional_candidate_for_engine(
    result: BrowserResult,
    query: &str,
    position: usize,
    engine: BrowserSearchEngine,
) -> Option<PaperCandidate> {
    let BrowserResult {
        title,
        url,
        snippet,
    } = result;
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
            reasons: vec![format!("discovered:web:{}", engine.as_str())],
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
    let discovery_reasons = provisional
        .match_summary
        .reasons
        .into_iter()
        .filter(|reason| reason.starts_with("discovered:web"))
        .collect::<Vec<_>>();
    resolved
        .match_summary
        .reasons
        .retain(|reason| !reason.starts_with("provider:"));
    for reason in discovery_reasons {
        if !resolved.match_summary.reasons.contains(&reason) {
            resolved.match_summary.reasons.push(reason);
        }
    }
    resolved
        .match_summary
        .reasons
        .push(format!("resolved:{}", resolved.source_provider));
    resolved
}

/// Merge repeated browser hits while preserving every engine provenance reason.
fn merge_browser_candidates(target: &mut Vec<PaperCandidate>, incoming: Vec<PaperCandidate>) {
    for candidate in incoming {
        let key = browser_candidate_key(&candidate);
        let Some(existing) = target
            .iter_mut()
            .find(|existing| browser_candidate_key(existing) == key)
        else {
            target.push(candidate);
            continue;
        };
        for reason in candidate.match_summary.reasons {
            if !existing.match_summary.reasons.contains(&reason) {
                existing.match_summary.reasons.push(reason);
            }
        }
        for keyword in candidate.match_summary.matched_keywords {
            if !existing.match_summary.matched_keywords.contains(&keyword) {
                existing.match_summary.matched_keywords.push(keyword);
            }
        }
        if existing.abstract_text.is_none() {
            existing.abstract_text = candidate.abstract_text;
        }
        existing.pdf_url = existing.pdf_url.clone().or(candidate.pdf_url);
    }
}

/// Prefer scholarly identity, then canonical result URL, then title and year.
fn browser_candidate_key(candidate: &PaperCandidate) -> String {
    if candidate.doi.is_some() || candidate.arxiv_id.is_some() || candidate.openalex_id.is_some() {
        return paper_candidate_dedup_key(candidate);
    }
    if let Some(external_url) = candidate.external_url.as_deref() {
        let canonical = Url::parse(external_url.trim())
            .map(|mut url| {
                url.set_fragment(None);
                url.to_string()
            })
            .unwrap_or_else(|_| external_url.trim().to_string());
        return format!("url:{}", canonical.to_ascii_lowercase());
    }
    format!(
        "{}:{}",
        paper_candidate_dedup_key(candidate),
        candidate
            .year
            .map_or_else(|| "unknown".to_string(), |year| year.to_string())
    )
}

/// Produce a deterministic browser shortlist without a model or embedding call.
fn rank_browser_candidates(candidates: &mut [PaperCandidate], query: &str) {
    let query_terms = normalized_terms(query);
    for candidate in candidates.iter_mut() {
        let searchable = format!(
            "{} {}",
            candidate.title,
            candidate.abstract_text.as_deref().unwrap_or_default()
        )
        .to_ascii_lowercase();
        let matched = query_terms
            .iter()
            .filter(|term| searchable.contains(term.as_str()))
            .count();
        let lexical = if query_terms.is_empty() {
            0.0
        } else {
            matched as f64 / query_terms.len() as f64
        };
        let engine_support = candidate
            .match_summary
            .reasons
            .iter()
            .filter(|reason| reason.starts_with("discovered:web:"))
            .count()
            .min(3) as f64
            / 3.0;
        let source = f64::from(candidate.external_url.is_some() || candidate.pdf_url.is_some());
        let identity = f64::from(
            candidate.doi.is_some()
                || candidate.arxiv_id.is_some()
                || candidate.openalex_id.is_some(),
        );
        let metadata = [
            !candidate.authors.is_empty(),
            candidate.year.is_some(),
            candidate.venue.is_some(),
        ]
        .into_iter()
        .filter(|present| *present)
        .count() as f64
            / 3.0;
        let novelty = f64::from(!candidate.already_in_library);
        candidate.match_summary.score = Some(
            0.50 * lexical
                + 0.15 * engine_support
                + 0.15 * source
                + 0.10 * identity
                + 0.05 * metadata
                + 0.05 * novelty,
        );
    }
    candidates.sort_by(|left, right| {
        right
            .match_summary
            .score
            .partial_cmp(&left.match_summary.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| left.title.cmp(&right.title))
    });
}

fn normalized_terms(value: &str) -> Vec<String> {
    let mut terms = value
        .split(|character: char| !character.is_alphanumeric())
        .filter(|term| term.len() > 2)
        .map(str::to_ascii_lowercase)
        .collect::<Vec<_>>();
    terms.sort();
    terms.dedup();
    terms
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

#[cfg(test)]
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
        && host != "brave.com"
        && !host.ends_with(".brave.com")
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ExactQueryKind {
    Arxiv,
    Doi,
    Title,
}

struct ExactQueryHint {
    kind: ExactQueryKind,
    value: String,
}

/// Extract only identifiers or explicitly quoted titles safe for API fallback.
fn exact_query_hint(query: &str) -> Option<ExactQueryHint> {
    let arxiv = Regex::new(
        r"(?i)arxiv(?:\s+identifier)?\s*:?\s*([a-z][a-z.-]*/\d{7}|\d{4}\.\d{4,5})(?:v\d+)?",
    )
    .expect("valid arXiv hint regex");
    if let Some(value) = arxiv.captures(query).and_then(|captures| captures.get(1)) {
        return Some(ExactQueryHint {
            kind: ExactQueryKind::Arxiv,
            value: value.as_str().to_string(),
        });
    }
    if let Some(value) = extract_doi(query) {
        return Some(ExactQueryHint {
            kind: ExactQueryKind::Doi,
            value,
        });
    }
    let title = Regex::new(r#"(?i)(?:title|titled)\s+[\"“]([^\"”]+)[\"”]"#)
        .expect("valid exact-title regex");
    title
        .captures(query)
        .and_then(|captures| captures.get(1))
        .map(|value| ExactQueryHint {
            kind: ExactQueryKind::Title,
            value: clean_title(value.as_str()),
        })
}

/// Returns whether a host is Google itself or one of its subdomains.
#[cfg(test)]
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
            "browser_search_url" => {
                config.custom_search_url_template = Some(value.to_string());
                config.engines = vec![BrowserSearchEngine::Custom];
            }
            "browser_search_engines" => {
                let engines = value
                    .trim_matches(['[', ']'])
                    .split(',')
                    .filter_map(BrowserSearchEngine::parse)
                    .collect::<Vec<_>>();
                if !engines.is_empty() {
                    config.engines = engines;
                    config.custom_search_url_template = None;
                }
            }
            "browser_pages_per_query" => {
                if let Ok(pages) = value.parse::<usize>() {
                    config.pages_per_query = pages.clamp(1, 2);
                }
            }
            "browser_candidate_target" => {
                if let Ok(target) = value.parse::<usize>() {
                    config.candidate_target = target.clamp(1, 100);
                }
            }
            "browser_minimum_interval_ms" => {
                if let Ok(milliseconds) = value.parse::<u64>() {
                    config.minimum_interval = Duration::from_millis(milliseconds.min(60_000));
                }
            }
            "browser_challenge_cooldown_seconds" => {
                if let Ok(seconds) = value.parse::<u64>() {
                    config.challenge_cooldown = Duration::from_secs(seconds.min(86_400));
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
    use std::sync::Mutex as StdMutex;

    use crate::services::source_acquisition::obscura::{ObscuraBrowserRuntime, ObscuraManager};
    use crate::services::source_acquisition::types::{
        AcquisitionMethod, AcquisitionResult, BrowserEndpoint, BrowserPageSnapshot, BrowserRuntime,
        FetchResponse, HttpFetcher, ObscuraConfig, SourceAcquisitionConfig, SourceAcquisitionError,
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

    struct FallbackBrowser {
        calls: Arc<StdMutex<Vec<String>>>,
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

    #[async_trait]
    impl BrowserRuntime for FallbackBrowser {
        async fn ensure_ready(&self) -> AcquisitionResult<BrowserEndpoint> {
            Ok(BrowserEndpoint {
                port: 9222,
                url: "http://127.0.0.1:9222".to_string(),
                websocket_url: None,
            })
        }

        async fn fetch_original(&self, _url: &str) -> AcquisitionResult<FetchResponse> {
            Err(SourceAcquisitionError::Browser(
                "original fetch is not used by browser discovery".to_string(),
            ))
        }

        async fn inspect_page(&self, url: &str) -> AcquisitionResult<PageInspection> {
            self.calls.lock().unwrap().push(url.to_string());
            let (title, html, text) = if url.contains("search.brave.com") {
                (
                    "Brave Search",
                    "<html><script>challengeSet = {}</script></html>",
                    "Verifying you're not a bot",
                )
            } else {
                (
                    "Ecosia Search",
                    r#"<article class="result"><a class="result-title" href="https://arxiv.org/abs/1706.03762">Attention Is All You Need</a><p class="result-snippet">Transformer architecture.</p></article>"#,
                    "Attention Is All You Need",
                )
            };
            Ok(PageInspection {
                snapshot: BrowserPageSnapshot {
                    url: url.to_string(),
                    final_url: url.to_string(),
                    title: Some(title.to_string()),
                    content_type: Some("text/html".to_string()),
                    html: Some(html.to_string()),
                    text: Some(text.to_string()),
                },
                links: Vec::new(),
                assets: Vec::new(),
                network_urls: Vec::new(),
            })
        }
    }

    fn browser_source(
        runtime: Arc<dyn BrowserRuntime>,
        engines: Vec<BrowserSearchEngine>,
    ) -> BrowserDiscoverySource {
        let service = SourceAcquisitionService::new(
            SourceAcquisitionConfig::default(),
            Arc::new(UnusedHttp),
            runtime,
            AcquisitionMethod::ObscuraBrowserStealth,
        );
        BrowserDiscoverySource::new(
            service,
            OpenAlexProvider::from_app_config().expect("OpenAlex test config"),
            ArxivProvider::from_app_config().expect("arXiv test config"),
            BrowserDiscoveryConfig {
                engines,
                minimum_interval: Duration::ZERO,
                challenge_cooldown: Duration::from_secs(60),
                ..BrowserDiscoveryConfig::default()
            },
        )
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
    fn extracts_exact_arxiv_id_and_quoted_title_hints() {
        let arxiv = exact_query_hint(
            "Find the original paper, arXiv identifier: 1706.03762, with full text",
        )
        .expect("arXiv hint");
        assert!(arxiv.kind == ExactQueryKind::Arxiv);
        assert_eq!(arxiv.value, "1706.03762");

        let title = exact_query_hint("Find the paper titled “Attention Is All You Need”")
            .expect("title hint");
        assert!(title.kind == ExactQueryKind::Title);
        assert_eq!(title.value, "Attention Is All You Need");
    }

    #[test]
    fn brave_page_chrome_is_not_a_search_candidate() {
        let html = r#"
            <a href="https://account.brave.com/?intent=checkout">Brave Search Premium</a>
            <a href="https://brave.com/wallet/">Brave Wallet</a>
        "#;

        assert!(parse_search_results(html, "attention", 10).is_empty());
    }

    #[test]
    fn config_builds_default_duckduckgo_search_urls() {
        let config = BrowserDiscoveryConfig::default();
        let url = config
            .search_url(BrowserSearchEngine::DuckDuckGo, "graph neural networks", 1)
            .unwrap();
        assert_eq!(
            url,
            "https://html.duckduckgo.com/html/?q=graph+neural+networks&s=10"
        );
    }

    #[test]
    fn config_loads_engine_order_and_operational_limits() {
        let mut config = BrowserDiscoveryConfig::default();
        apply_config(
            &mut config,
            r#"
                [discovery]
                browser_search_engines = ["ecosia", "brave"]
                browser_pages_per_query = 2
                browser_candidate_target = 14
                browser_minimum_interval_ms = 2500
                browser_challenge_cooldown_seconds = 90
            "#,
        );

        assert_eq!(
            config.engines,
            vec![BrowserSearchEngine::Ecosia, BrowserSearchEngine::Brave]
        );
        assert_eq!(config.pages_per_query, 2);
        assert_eq!(config.candidate_target, 14);
        assert_eq!(config.minimum_interval, Duration::from_millis(2_500));
        assert_eq!(config.challenge_cooldown, Duration::from_secs(90));
    }

    #[test]
    fn browser_merge_preserves_engine_provenance() {
        let url = "https://example.org/papers/causal-circuits".to_string();
        let brave = provisional_candidate_for_engine(
            BrowserResult {
                title: "Causal Circuit Discovery in Transformers".to_string(),
                url: url.clone(),
                snippet: None,
            },
            "causal circuit discovery",
            0,
            BrowserSearchEngine::Brave,
        )
        .unwrap();
        let ecosia = provisional_candidate_for_engine(
            BrowserResult {
                title: "Causal Circuit Discovery in Transformers".to_string(),
                url,
                snippet: Some("An empirical circuit comparison.".to_string()),
            },
            "causal circuit discovery",
            0,
            BrowserSearchEngine::Ecosia,
        )
        .unwrap();
        let mut candidates = vec![brave];

        merge_browser_candidates(&mut candidates, vec![ecosia]);

        assert_eq!(candidates.len(), 1);
        assert!(candidates[0]
            .match_summary
            .reasons
            .contains(&"discovered:web:brave".to_string()));
        assert!(candidates[0]
            .match_summary
            .reasons
            .contains(&"discovered:web:ecosia".to_string()));
        assert_eq!(
            candidates[0].abstract_text.as_deref(),
            Some("An empirical circuit comparison.")
        );
    }

    #[test]
    fn local_browser_ranking_is_stable_and_query_sensitive() {
        let relevant = provisional_candidate_for_engine(
            BrowserResult {
                title: "Causal Circuit Discovery in Language Models".to_string(),
                url: "https://example.org/relevant".to_string(),
                snippet: Some("A causal intervention benchmark.".to_string()),
            },
            "causal circuit intervention",
            1,
            BrowserSearchEngine::DuckDuckGo,
        )
        .unwrap();
        let unrelated = provisional_candidate_for_engine(
            BrowserResult {
                title: "General Survey of Machine Learning".to_string(),
                url: "https://example.org/unrelated".to_string(),
                snippet: None,
            },
            "causal circuit intervention",
            0,
            BrowserSearchEngine::Ecosia,
        )
        .unwrap();
        let mut first = vec![unrelated.clone(), relevant.clone()];
        let mut second = vec![unrelated, relevant];

        rank_browser_candidates(&mut first, "causal circuit intervention");
        rank_browser_candidates(&mut second, "causal circuit intervention");

        assert_eq!(
            first[0].title,
            "Causal Circuit Discovery in Language Models"
        );
        assert_eq!(
            first
                .iter()
                .map(|candidate| &candidate.id)
                .collect::<Vec<_>>(),
            second
                .iter()
                .map(|candidate| &candidate.id)
                .collect::<Vec<_>>()
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
            BrowserDiscoveryConfig {
                engines: vec![BrowserSearchEngine::Custom],
                custom_search_url_template: Some(
                    "https://scholar.google.com/scholar?q={query}".to_string(),
                ),
                minimum_interval: Duration::ZERO,
                ..BrowserDiscoveryConfig::default()
            },
        );

        let error = source
            .discover("attention is all you need", 10, &|_| {})
            .await
            .expect_err("challenge page must not become an empty result");

        assert_eq!(
            error.to_string(),
            "Browser search exhausted its providers without usable candidates: custom=challenged (bot challenge returned)"
        );
    }

    #[tokio::test]
    async fn challenged_engine_falls_through_to_the_next_engine() {
        let calls = Arc::new(StdMutex::new(Vec::new()));
        let source = browser_source(
            Arc::new(FallbackBrowser {
                calls: calls.clone(),
            }),
            vec![BrowserSearchEngine::Brave, BrowserSearchEngine::Ecosia],
        );
        let progress = StdMutex::new(Vec::new());

        let candidates = source
            .discover_with_resolvers(
                "transformer architecture",
                1,
                &[DiscoveryProviderChoice::EuropePmc],
                &|event| {
                    if let BrowserDiscoveryProgress::ProviderAttempt(attempt) = event {
                        progress.lock().unwrap().push(attempt.status);
                    }
                },
            )
            .await
            .expect("Ecosia result should survive a Brave challenge");

        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].arxiv_id.as_deref(), Some("1706.03762"));
        assert_eq!(
            progress.into_inner().unwrap(),
            vec![
                BrowserAttemptStatus::Challenged,
                BrowserAttemptStatus::Succeeded
            ]
        );
        assert_eq!(calls.lock().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn challenged_engine_is_skipped_while_its_cooldown_is_active() {
        let calls = Arc::new(StdMutex::new(Vec::new()));
        let source = browser_source(
            Arc::new(FallbackBrowser {
                calls: calls.clone(),
            }),
            vec![BrowserSearchEngine::Brave],
        );

        let first = source.discover("broad research query", 1, &|_| {}).await;
        let second = source.discover("another broad query", 1, &|_| {}).await;

        assert!(first.unwrap_err().to_string().contains("brave=challenged"));
        assert!(second
            .unwrap_err()
            .to_string()
            .contains("brave=unavailable"));
        assert_eq!(calls.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    #[ignore = "live Obscura browser-search smoke test"]
    async fn live_obscura_search_returns_candidates_or_an_honest_provider_failure() {
        let binary = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources/obscura/obscura");
        assert!(
            binary.exists(),
            "install the Obscura sidecar at {}",
            binary.display()
        );
        let runtime = ObscuraBrowserRuntime::new(ObscuraManager::new(ObscuraConfig {
            path: Some(binary),
            ..ObscuraConfig::default()
        }));
        let source = browser_source(Arc::new(runtime), vec![BrowserSearchEngine::DuckDuckGo]);

        match source
            .discover_with_resolvers(
                "mechanistic interpretability transformer circuits",
                3,
                &[DiscoveryProviderChoice::EuropePmc],
                &|_| {},
            )
            .await
        {
            Ok(candidates) => assert!(
                candidates
                    .iter()
                    .all(|candidate| candidate.external_url.is_some()),
                "every browser candidate needs an actionable URL"
            ),
            Err(error) => {
                let error = error.to_string();
                assert!(
                    [
                        "empty",
                        "challenged",
                        "rate_limited",
                        "unavailable",
                        "parse_failed"
                    ]
                    .iter()
                    .any(|status| error.contains(status)),
                    "provider failure was not classified: {error}"
                );
            }
        }
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
        let attempt = |status, reason: &str| BrowserProviderAttempt {
            provider: "brave".to_string(),
            status,
            candidate_count: 0,
            elapsed_ms: 5,
            reason: Some(reason.to_string()),
        };
        assert_eq!(
            empty_search_error(&[attempt(
                BrowserAttemptStatus::Challenged,
                "bot challenge returned"
            )])
            .to_string(),
            "Browser search exhausted its providers without usable candidates: brave=challenged (bot challenge returned)"
        );
        assert_eq!(
            empty_search_error(&[attempt(
                BrowserAttemptStatus::Empty,
                "no usable result links"
            )])
            .to_string(),
            "Browser search exhausted its providers without usable candidates: brave=empty (no usable result links)"
        );
        assert_eq!(
            empty_search_error(&[attempt(
                BrowserAttemptStatus::Unavailable,
                "CDP connection closed"
            )])
            .to_string(),
            "Browser search exhausted its providers without usable candidates: brave=unavailable (CDP connection closed)"
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
