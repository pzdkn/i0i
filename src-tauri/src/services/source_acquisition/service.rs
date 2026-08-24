use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use futures_util::stream::{self, StreamExt};
use tauri::Manager;

use super::locations::{build_location_plan, PdfLocation, PdfLocationHints};
use super::types::{
    AcquiredHtml, AcquiredSource, AcquisitionMethod, AcquisitionResult, BrowserPageSnapshot,
    BrowserRuntime, FetchResponse, HttpFetcher, PageInspection, SourceAcquisitionConfig,
    SourceAcquisitionError,
};
use crate::services::source_acquisition::http::DirectHttpFetcher;
use crate::services::source_acquisition::obscura::{ObscuraBrowserRuntime, ObscuraManager};
use crate::services::source_acquisition::types::ObscuraConfig;

// RFC 0051: every stage of acquisition is bounded so a bad URL can never hang
// the Reader. The overall deadline caps the full plan including the browser
// fallback; per-attempt timeouts keep the hedged loop moving.
const ACQUIRE_OVERALL_DEADLINE: Duration = Duration::from_secs(60);
const DIRECT_ATTEMPT_TIMEOUT: Duration = Duration::from_secs(10);
const HEDGED_IN_FLIGHT: usize = 2;
const LANDING_CANDIDATE_LIMIT: usize = 5;
const NEGATIVE_CACHE_TTL: Duration = Duration::from_secs(600);
const PROBE_ATTEMPT_TIMEOUT: Duration = Duration::from_secs(5);
const PROBE_LOCATION_LIMIT: usize = 4;

#[derive(Debug, Clone)]
struct NegativeEntry {
    at: Instant,
}

#[derive(Debug, Default, Clone)]
struct HostStats {
    successes: u32,
    failures: u32,
}

#[derive(Clone)]
pub struct SourceAcquisitionService {
    config: SourceAcquisitionConfig,
    http: Arc<dyn HttpFetcher>,
    browser: Arc<dyn BrowserRuntime>,
    browser_method: AcquisitionMethod,
    /// Dedicated client for location resolvers (Unpaywall); short timeout,
    /// independent from the download client.
    resolver_client: reqwest::Client,
    /// URLs that recently failed, so re-opens skip known-dead locations.
    negative_cache: Arc<Mutex<HashMap<String, NegativeEntry>>>,
    /// Per-host success/failure counts; hosts that keep failing sink in rank.
    host_stats: Arc<Mutex<HashMap<String, HostStats>>>,
}

impl SourceAcquisitionService {
    pub fn from_app_config(app: &tauri::AppHandle, client: reqwest::Client) -> Self {
        let config = load_source_acquisition_config(app);
        let obscura_config = ObscuraConfig::load(app);
        let browser_method = if obscura_config.stealth {
            AcquisitionMethod::ObscuraBrowserStealth
        } else {
            AcquisitionMethod::ObscuraBrowser
        };
        let manager = ObscuraManager::with_app(obscura_config, app.clone());
        let browser = Arc::new(ObscuraBrowserRuntime::new(manager));
        let http = Arc::new(DirectHttpFetcher::new(client));
        Self::new(config, http, browser, browser_method)
    }

    pub fn new(
        config: SourceAcquisitionConfig,
        http: Arc<dyn HttpFetcher>,
        browser: Arc<dyn BrowserRuntime>,
        browser_method: AcquisitionMethod,
    ) -> Self {
        let resolver_client = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Self {
            config,
            http,
            browser,
            browser_method,
            resolver_client,
            negative_cache: Arc::new(Mutex::new(HashMap::new())),
            host_stats: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn acquire_pdf(
        &self,
        source_url: &str,
        landing_page_url: Option<&str>,
        max_bytes: u64,
    ) -> AcquisitionResult<AcquiredSource> {
        self.acquire_pdf_with_hints(
            &PdfLocationHints::from_url(source_url, landing_page_url),
            max_bytes,
            false,
        )
        .await
    }

    /// Acquire a PDF from every location the hints can resolve (RFC 0051).
    ///
    /// Direct downloads are hedged (two in flight, bounded per attempt); the
    /// browser runs once as a last resort. The whole call is capped by an
    /// overall deadline so callers can never hang.
    pub async fn acquire_pdf_with_hints(
        &self,
        hints: &PdfLocationHints,
        max_bytes: u64,
        force: bool,
    ) -> AcquisitionResult<AcquiredSource> {
        match tokio::time::timeout(
            ACQUIRE_OVERALL_DEADLINE,
            self.acquire_pdf_inner(hints, max_bytes, force),
        )
        .await
        {
            Ok(result) => result,
            Err(_) => Err(SourceAcquisitionError::Http(format!(
                "Timed out after {}s fetching PDF for {}",
                ACQUIRE_OVERALL_DEADLINE.as_secs(),
                hints
                    .pdf_url
                    .as_deref()
                    .or(hints.doi.as_deref())
                    .or(hints.arxiv_id.as_deref())
                    .unwrap_or("unknown source")
            ))),
        }
    }

    async fn acquire_pdf_inner(
        &self,
        hints: &PdfLocationHints,
        max_bytes: u64,
        force: bool,
    ) -> AcquisitionResult<AcquiredSource> {
        let full_plan = build_location_plan(&self.resolver_client, hints).await;
        let total_locations = full_plan.len();
        let mut plan = if force {
            full_plan
        } else {
            self.without_recent_failures(full_plan)
        };
        self.order_by_host_reliability(&mut plan);

        if plan.is_empty() {
            if total_locations > 0 {
                return Err(SourceAcquisitionError::Http(
                    "All known PDF locations failed recently; use Retry to try again".to_string(),
                ));
            }
            return Err(SourceAcquisitionError::Http(
                "No PDF locations available for this paper".to_string(),
            ));
        }

        let mut last_error: Option<SourceAcquisitionError> = None;

        // Hedged direct attempts: first response whose bytes sniff as a PDF
        // wins; dropping the stream aborts the losers.
        let mut attempts = stream::iter(plan.clone().into_iter().map(|location| {
            let http = Arc::clone(&self.http);
            async move {
                let outcome =
                    tokio::time::timeout(DIRECT_ATTEMPT_TIMEOUT, http.fetch(&location.url)).await;
                (location, outcome)
            }
        }))
        .buffer_unordered(HEDGED_IN_FLIGHT);

        while let Some((location, outcome)) = attempts.next().await {
            let error = match outcome {
                Ok(Ok(response)) => {
                    match self.pdf_from_response(response, AcquisitionMethod::DirectHttp, max_bytes)
                    {
                        Ok(source) => {
                            eprintln!(
                                "[source_acquisition] pdf acquired via {} location {}",
                                location.source, location.url
                            );
                            self.record_success(&location.url);
                            return Ok(source);
                        }
                        Err(error) => error,
                    }
                }
                Ok(Err(error)) => error,
                Err(_) => SourceAcquisitionError::Http(format!(
                    "Timed out after {}s: {}",
                    DIRECT_ATTEMPT_TIMEOUT.as_secs(),
                    location.url
                )),
            };
            self.record_failure(&location.url, &error);
            last_error = Some(error);
        }
        drop(attempts);

        if self.config.browser_fallback == "obscura"
            && self.config.prefer_browser_for_blocked_sources
        {
            // Last resort, one browser fetch on the best-ranked location.
            if let Some(location) = plan.first() {
                if let Ok(response) = self.browser_fetch_original(&location.url).await {
                    match self.pdf_from_response(response, self.browser_method.clone(), max_bytes) {
                        Ok(source) => {
                            self.record_success(&location.url);
                            return Ok(source);
                        }
                        Err(error) => last_error = Some(error),
                    }
                }
            }

            if let Some(landing_page_url) = hints.landing_url.as_deref() {
                match self
                    .acquire_pdf_from_landing_page(landing_page_url, max_bytes)
                    .await
                {
                    Ok(source) => return Ok(source),
                    Err(error) => last_error = Some(error),
                }
            }
        }

        Err(last_error.unwrap_or_else(|| {
            SourceAcquisitionError::Http("No PDF locations available".to_string())
        }))
    }

    /// Cheaply classify whether a PDF is obtainable for these hints without
    /// downloading it (RFC 0051 discover-time verification).
    pub async fn probe_pdf_availability(&self, hints: &PdfLocationHints) -> &'static str {
        let plan = build_location_plan(&self.resolver_client, hints).await;
        if plan.is_empty() {
            return "unavailable";
        }

        let mut saw_blocked = false;
        for location in plan.iter().take(PROBE_LOCATION_LIMIT) {
            match tokio::time::timeout(PROBE_ATTEMPT_TIMEOUT, self.http.probe(&location.url)).await
            {
                Ok(Ok(true)) => {
                    self.record_success(&location.url);
                    return "verified";
                }
                // Served something, just not a PDF: a browser might get past it.
                Ok(Ok(false)) => saw_blocked = true,
                Ok(Err(SourceAcquisitionError::DirectHttpForbidden(_))) => saw_blocked = true,
                _ => {}
            }
        }

        if saw_blocked {
            "browser_required"
        } else {
            "unavailable"
        }
    }

    fn without_recent_failures(&self, plan: Vec<PdfLocation>) -> Vec<PdfLocation> {
        let mut cache = self.negative_cache.lock().expect("negative cache lock");
        cache.retain(|_, entry| entry.at.elapsed() < NEGATIVE_CACHE_TTL);
        plan.into_iter()
            .filter(|location| !cache.contains_key(&location.url))
            .collect()
    }

    /// Stable-sort by plan rank, demoting hosts with a bad session record.
    fn order_by_host_reliability(&self, plan: &mut [PdfLocation]) {
        let stats = self.host_stats.lock().expect("host stats lock");
        plan.sort_by_key(|location| {
            let penalty = url_host(&location.url)
                .and_then(|host| stats.get(&host))
                .map(|entry| entry.failures.saturating_sub(entry.successes))
                .unwrap_or(0);
            (location.rank, penalty)
        });
    }

    fn record_success(&self, url: &str) {
        self.negative_cache
            .lock()
            .expect("negative cache lock")
            .remove(url);
        if let Some(host) = url_host(url) {
            let mut stats = self.host_stats.lock().expect("host stats lock");
            stats.entry(host).or_default().successes += 1;
        }
    }

    fn record_failure(&self, url: &str, error: &SourceAcquisitionError) {
        let _ = error;
        self.negative_cache
            .lock()
            .expect("negative cache lock")
            .insert(url.to_string(), NegativeEntry { at: Instant::now() });
        if let Some(host) = url_host(url) {
            let mut stats = self.host_stats.lock().expect("host stats lock");
            stats.entry(host).or_default().failures += 1;
        }
    }

    pub async fn debug_fetch_url(&self, url: &str) -> AcquisitionResult<BrowserPageSnapshot> {
        self.browser.ensure_ready().await?;
        Ok(self.browser.inspect_page(url).await?.snapshot)
    }

    pub async fn ensure_browser_ready(
        &self,
    ) -> AcquisitionResult<crate::services::source_acquisition::types::BrowserEndpoint> {
        self.browser.ensure_ready().await
    }

    /// Snapshot used by Discover when it mounts after the latest status event.
    pub fn browser_status(
        &self,
    ) -> crate::services::source_acquisition::types::BrowserRuntimeStatus {
        self.browser.status()
    }

    pub async fn acquire_web_page(&self, url: &str) -> AcquisitionResult<BrowserPageSnapshot> {
        self.debug_fetch_url(url).await
    }

    /// Inspect a rendered page in the managed browser, including its links and
    /// observed network URLs. Discovery uses this richer form while the Reader
    /// usually needs only the snapshot returned by `acquire_web_page`.
    pub async fn inspect_browser_page(&self, url: &str) -> AcquisitionResult<PageInspection> {
        self.browser.ensure_ready().await?;
        self.browser.inspect_page(url).await
    }

    /// Fetch a page over direct HTTP and ingest it into clean, annotatable
    /// article HTML (RFC 0056). Most article HTML needs no browser, so this
    /// uses the direct fetcher; the reader falls back to "View original" if a
    /// page turns out to need JS. Remote images are inlined as `data:` URIs so
    /// the article renders offline under the app's strict CSP.
    pub async fn acquire_html_page(&self, url: &str) -> AcquisitionResult<AcquiredHtml> {
        let response = self.http.fetch(url).await?;
        let raw = String::from_utf8_lossy(&response.bytes);
        let ingested = crate::html_ingestion::ingest_html(&raw, Some(&response.final_url));
        let clean_html = self.inline_images(ingested.clean_html).await;
        Ok(AcquiredHtml {
            final_url: response.final_url,
            title: ingested.title,
            clean_html,
            source_text: ingested.source_text,
        })
    }

    /// Fetch remote `<img src="http…">` images and rewrite them as inlined
    /// `data:` URIs (RFC 0056), bounded by count and per-image size so the
    /// cached article stays reasonable. Fetch failures leave the tag as-is.
    async fn inline_images(&self, html: String) -> String {
        const MAX_INLINE_IMAGES: usize = 40;
        const MAX_INLINE_IMAGE_BYTES: usize = 2 * 1024 * 1024;
        use base64::Engine as _;

        let pattern = match regex::Regex::new(r#"src="(https?://[^"]+)""#) {
            Ok(pattern) => pattern,
            Err(_) => return html,
        };
        let mut urls: Vec<String> = Vec::new();
        for capture in pattern.captures_iter(&html) {
            if let Some(url) = capture.get(1) {
                let url = url.as_str().to_string();
                if !urls.contains(&url) {
                    urls.push(url);
                }
            }
            if urls.len() >= MAX_INLINE_IMAGES {
                break;
            }
        }

        let mut result = html;
        for url in urls {
            let Ok(response) = self.http.fetch(&url).await else {
                continue;
            };
            if response.bytes.is_empty() || response.bytes.len() > MAX_INLINE_IMAGE_BYTES {
                continue;
            }
            let mime = response
                .content_type
                .as_deref()
                .and_then(|value| value.split(';').next())
                .filter(|value| value.starts_with("image/"))
                .unwrap_or("image/*");
            let encoded = base64::engine::general_purpose::STANDARD.encode(&response.bytes);
            let data_uri = format!("data:{mime};base64,{encoded}");
            result = result.replace(&format!("src=\"{url}\""), &format!("src=\"{data_uri}\""));
        }
        result
    }

    async fn acquire_pdf_from_landing_page(
        &self,
        landing_page_url: &str,
        max_bytes: u64,
    ) -> AcquisitionResult<AcquiredSource> {
        self.browser.ensure_ready().await?;
        let inspection = self.browser.inspect_page(landing_page_url).await?;
        let candidates = pdf_candidates(&inspection);
        for candidate_url in candidates {
            if let Ok(Ok(response)) =
                tokio::time::timeout(DIRECT_ATTEMPT_TIMEOUT, self.http.fetch(&candidate_url)).await
            {
                if let Ok(source) =
                    self.pdf_from_response(response, AcquisitionMethod::DirectHttp, max_bytes)
                {
                    return Ok(source);
                }
            }

            if let Ok(response) = self.browser_fetch_original(&candidate_url).await {
                if let Ok(source) =
                    self.pdf_from_response(response, self.browser_method.clone(), max_bytes)
                {
                    return Ok(source);
                }
            }
        }

        Err(SourceAcquisitionError::NoPdfLinkFoundOnLandingPage(
            landing_page_url.to_string(),
        ))
    }

    async fn browser_fetch_original(&self, url: &str) -> AcquisitionResult<FetchResponse> {
        self.browser.ensure_ready().await?;
        self.browser.fetch_original(url).await
    }

    fn pdf_from_response(
        &self,
        response: FetchResponse,
        method: AcquisitionMethod,
        max_bytes: u64,
    ) -> AcquisitionResult<AcquiredSource> {
        let bytes_downloaded = response.bytes.len() as u64;
        if bytes_downloaded > max_bytes {
            return Err(SourceAcquisitionError::TooLarge {
                bytes: bytes_downloaded,
                max_bytes,
            });
        }
        if !response.bytes.starts_with(b"%PDF-") {
            let content_type = response.content_type.unwrap_or_default();
            if content_type.contains("html") {
                return Err(SourceAcquisitionError::BrowserReturnedHtml(
                    response.final_url,
                ));
            }
            return Err(SourceAcquisitionError::DownloadedBytesNotPdf(
                response.final_url,
            ));
        }

        Ok(AcquiredSource {
            bytes: response.bytes,
            final_url: response.final_url,
            content_type: response.content_type,
            method,
        })
    }
}

fn load_source_acquisition_config(app: &tauri::AppHandle) -> SourceAcquisitionConfig {
    let mut config = SourceAcquisitionConfig::default();
    for path in candidate_config_paths(app) {
        if let Ok(contents) = fs::read_to_string(path) {
            apply_source_acquisition_overrides(&mut config, &contents);
            break;
        }
    }
    config
}

fn apply_source_acquisition_overrides(config: &mut SourceAcquisitionConfig, contents: &str) {
    let mut in_source_acquisition = false;

    for line in contents.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        if line.starts_with('[') && line.ends_with(']') {
            in_source_acquisition = line == "[source_acquisition]";
            continue;
        }

        if !in_source_acquisition {
            continue;
        }

        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        let value = value.trim().trim_matches('"');

        match key {
            "browser_fallback" => config.browser_fallback = value.to_string(),
            "prefer_browser_for_blocked_sources" => {
                config.prefer_browser_for_blocked_sources = value == "true"
            }
            _ => {}
        }
    }
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

/// Extract the host from a URL for per-host reliability tracking.
fn url_host(url: &str) -> Option<String> {
    reqwest::Url::parse(url)
        .ok()
        .and_then(|parsed| parsed.host_str().map(str::to_lowercase))
}

fn pdf_candidates(inspection: &PageInspection) -> Vec<String> {
    // Network URLs first: the page actually requested them, so they are far
    // more likely to be the real PDF than an arbitrary link. Capped so a
    // link-heavy publisher page cannot turn the fallback into a crawl.
    let mut candidates = Vec::new();
    for url in inspection
        .network_urls
        .iter()
        .chain(inspection.links.iter())
        .chain(inspection.assets.iter())
    {
        let normalized = url.to_lowercase();
        if (normalized.contains(".pdf") || normalized.contains("/pdf")) && !candidates.contains(url)
        {
            candidates.push(url.clone());
        }
        if candidates.len() >= LANDING_CANDIDATE_LIMIT {
            break;
        }
    }
    candidates
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Mutex;

    use async_trait::async_trait;

    use super::*;
    use crate::services::source_acquisition::types::BrowserEndpoint;

    struct FakeHttp {
        responses: Mutex<HashMap<String, AcquisitionResult<FetchResponse>>>,
    }

    impl FakeHttp {
        fn new(entries: Vec<(&str, AcquisitionResult<FetchResponse>)>) -> Self {
            Self {
                responses: Mutex::new(
                    entries
                        .into_iter()
                        .map(|(url, result)| (url.to_string(), result))
                        .collect(),
                ),
            }
        }
    }

    #[async_trait]
    impl HttpFetcher for FakeHttp {
        async fn fetch(&self, url: &str) -> AcquisitionResult<FetchResponse> {
            self.responses
                .lock()
                .expect("fake http lock")
                .remove(url)
                .unwrap_or_else(|| Err(SourceAcquisitionError::Http(url.to_string())))
        }
    }

    struct FakeBrowser {
        fetches: Mutex<HashMap<String, AcquisitionResult<FetchResponse>>>,
        inspections: Mutex<HashMap<String, AcquisitionResult<PageInspection>>>,
        ready_calls: Mutex<usize>,
    }

    impl FakeBrowser {
        fn new(
            fetches: Vec<(&str, AcquisitionResult<FetchResponse>)>,
            inspections: Vec<(&str, AcquisitionResult<PageInspection>)>,
        ) -> Self {
            Self {
                fetches: Mutex::new(
                    fetches
                        .into_iter()
                        .map(|(url, result)| (url.to_string(), result))
                        .collect(),
                ),
                inspections: Mutex::new(
                    inspections
                        .into_iter()
                        .map(|(url, result)| (url.to_string(), result))
                        .collect(),
                ),
                ready_calls: Mutex::new(0),
            }
        }
    }

    #[async_trait]
    impl BrowserRuntime for FakeBrowser {
        async fn ensure_ready(&self) -> AcquisitionResult<BrowserEndpoint> {
            *self.ready_calls.lock().expect("ready lock") += 1;
            Ok(BrowserEndpoint {
                port: 9222,
                url: "http://127.0.0.1:9222".to_string(),
                websocket_url: Some("ws://127.0.0.1:9222/devtools/browser".to_string()),
            })
        }

        async fn fetch_original(&self, url: &str) -> AcquisitionResult<FetchResponse> {
            self.fetches
                .lock()
                .expect("fake browser lock")
                .remove(url)
                .unwrap_or_else(|| Err(SourceAcquisitionError::Browser(url.to_string())))
        }

        async fn inspect_page(&self, url: &str) -> AcquisitionResult<PageInspection> {
            self.inspections
                .lock()
                .expect("fake inspection lock")
                .remove(url)
                .unwrap_or_else(|| Err(SourceAcquisitionError::Browser(url.to_string())))
        }
    }

    fn service(
        http: FakeHttp,
        browser: FakeBrowser,
    ) -> (SourceAcquisitionService, Arc<FakeBrowser>) {
        let browser = Arc::new(browser);
        let service = SourceAcquisitionService::new(
            SourceAcquisitionConfig::default(),
            Arc::new(http),
            browser.clone(),
            AcquisitionMethod::ObscuraBrowserStealth,
        );
        (service, browser)
    }

    fn pdf_response(url: &str) -> FetchResponse {
        FetchResponse {
            bytes: b"%PDF-1.7\nbody".to_vec(),
            final_url: url.to_string(),
            content_type: Some("application/pdf".to_string()),
        }
    }

    fn html_response(url: &str) -> FetchResponse {
        FetchResponse {
            bytes: b"<html></html>".to_vec(),
            final_url: url.to_string(),
            content_type: Some("text/html".to_string()),
        }
    }

    #[tokio::test]
    async fn rfc0056_acquire_html_page_ingests_clean_article() {
        let url = "https://example.org/article";
        let response = FetchResponse {
            bytes: br#"<html><body><nav>menu</nav><article><h1>Title</h1>
                <p>A sufficiently long article paragraph about diffusion models and proteins
                so the readability extractor treats it as real body content worth keeping.</p>
                </article><script>evil()</script></body></html>"#
                .to_vec(),
            final_url: url.to_string(),
            content_type: Some("text/html".to_string()),
        };
        let (service, _) = service(
            FakeHttp::new(vec![(url, Ok(response))]),
            FakeBrowser::new(vec![], vec![]),
        );
        let acquired = service.acquire_html_page(url).await.expect("html acquired");
        assert!(acquired.clean_html.contains("article paragraph"));
        assert!(!acquired.clean_html.contains("<script"));
        assert!(acquired.source_text.contains("diffusion models"));
        assert_eq!(acquired.final_url, url);
    }

    fn page_snapshot(url: &str, title: &str) -> BrowserPageSnapshot {
        BrowserPageSnapshot {
            url: url.to_string(),
            final_url: url.to_string(),
            title: Some(title.to_string()),
            content_type: Some("text/html".to_string()),
            html: Some("<html></html>".to_string()),
            text: Some(title.to_string()),
        }
    }

    fn inspection(url: &str, links: Vec<&str>) -> PageInspection {
        PageInspection {
            snapshot: page_snapshot(url, "Landing"),
            links: links.into_iter().map(str::to_string).collect(),
            assets: vec![],
            network_urls: vec![],
        }
    }

    #[tokio::test]
    async fn milestone_2_browser_fetches_one_url() {
        let url = "https://example.com";
        let (service, browser) = service(
            FakeHttp::new(vec![]),
            FakeBrowser::new(vec![], vec![(url, Ok(inspection(url, vec![])))]),
        );

        let snapshot = service.debug_fetch_url(url).await.expect("snapshot");

        assert_eq!(snapshot.final_url, url);
        assert_eq!(snapshot.title.as_deref(), Some("Landing"));
        assert_eq!(*browser.ready_calls.lock().expect("ready lock"), 1);
    }

    #[tokio::test]
    async fn milestone_3_browser_recovers_blocked_pdf() {
        let pdf_url = "https://publisher.example/paper.pdf";
        let (service, _) = service(
            FakeHttp::new(vec![(
                pdf_url,
                Err(SourceAcquisitionError::DirectHttpForbidden(
                    "403".to_string(),
                )),
            )]),
            FakeBrowser::new(vec![(pdf_url, Ok(pdf_response(pdf_url)))], vec![]),
        );

        let acquired = service
            .acquire_pdf(pdf_url, None, 10_000)
            .await
            .expect("browser pdf");

        assert_eq!(acquired.method, AcquisitionMethod::ObscuraBrowserStealth);
        assert!(acquired.bytes.starts_with(b"%PDF-"));
    }

    #[tokio::test]
    async fn direct_html_response_falls_back_to_browser_pdf() {
        let pdf_url = "https://publisher.example/paper.pdf";
        let (service, _) = service(
            FakeHttp::new(vec![(pdf_url, Ok(html_response(pdf_url)))]),
            FakeBrowser::new(vec![(pdf_url, Ok(pdf_response(pdf_url)))], vec![]),
        );

        let acquired = service
            .acquire_pdf(pdf_url, None, 10_000)
            .await
            .expect("browser pdf after html block page");

        assert_eq!(acquired.method, AcquisitionMethod::ObscuraBrowserStealth);
        assert!(acquired.bytes.starts_with(b"%PDF-"));
    }

    #[tokio::test]
    async fn milestone_4_landing_page_discovers_pdf_candidate() {
        let pdf_url = "https://publisher.example/blocked";
        let landing_url = "https://publisher.example/article";
        let discovered_pdf = "https://publisher.example/article/download.pdf";
        let (service, _) = service(
            FakeHttp::new(vec![
                (
                    pdf_url,
                    Err(SourceAcquisitionError::Http("blocked".to_string())),
                ),
                (discovered_pdf, Ok(pdf_response(discovered_pdf))),
            ]),
            FakeBrowser::new(
                vec![(pdf_url, Ok(html_response(pdf_url)))],
                vec![(
                    landing_url,
                    Ok(inspection(landing_url, vec![discovered_pdf])),
                )],
            ),
        );

        let acquired = service
            .acquire_pdf(pdf_url, Some(landing_url), 10_000)
            .await
            .expect("landing pdf");

        assert_eq!(acquired.method, AcquisitionMethod::DirectHttp);
        assert_eq!(acquired.final_url, discovered_pdf);
    }

    #[tokio::test]
    async fn milestone_5_acquired_source_carries_provenance() {
        let pdf_url = "https://publisher.example/paper.pdf";
        let (service, _) = service(
            FakeHttp::new(vec![]),
            FakeBrowser::new(vec![(pdf_url, Ok(pdf_response(pdf_url)))], vec![]),
        );

        let acquired = service
            .acquire_pdf(pdf_url, None, 10_000)
            .await
            .expect("browser pdf");

        assert_eq!(
            acquired.method.as_str(),
            AcquisitionMethod::ObscuraBrowserStealth.as_str()
        );
        assert_eq!(acquired.final_url, pdf_url);
        assert_eq!(acquired.content_type.as_deref(), Some("application/pdf"));
    }

    #[tokio::test]
    async fn milestone_6_imports_general_web_snapshot() {
        let url = "https://example.com/article";
        let (service, _) = service(
            FakeHttp::new(vec![]),
            FakeBrowser::new(vec![], vec![(url, Ok(inspection(url, vec![])))]),
        );

        let snapshot = service.acquire_web_page(url).await.expect("web snapshot");

        assert_eq!(snapshot.url, url);
        assert_eq!(snapshot.content_type.as_deref(), Some("text/html"));
        assert!(snapshot.html.is_some());
    }

    #[tokio::test]
    async fn rfc0051_arxiv_id_hint_expands_to_constructed_url() {
        let arxiv_url = "https://arxiv.org/pdf/2309.08600";
        let (service, _) = service(
            FakeHttp::new(vec![(arxiv_url, Ok(pdf_response(arxiv_url)))]),
            FakeBrowser::new(vec![], vec![]),
        );

        let hints = PdfLocationHints {
            pdf_url: None,
            landing_url: None,
            doi: None,
            arxiv_id: Some("2309.08600".to_string()),
        };
        let acquired = service
            .acquire_pdf_with_hints(&hints, 10_000, false)
            .await
            .expect("arxiv pdf via constructed url");

        assert_eq!(acquired.final_url, arxiv_url);
        assert_eq!(acquired.method, AcquisitionMethod::DirectHttp);
    }

    #[tokio::test]
    async fn rfc0051_negative_cache_skips_recent_failures_until_forced() {
        let pdf_url = "https://publisher.example/paper.pdf";
        let (service, _) = service(
            FakeHttp::new(vec![(
                pdf_url,
                Err(SourceAcquisitionError::Http("500".to_string())),
            )]),
            FakeBrowser::new(vec![], vec![]),
        );
        let hints = PdfLocationHints::from_url(pdf_url, None);

        let first = service.acquire_pdf_with_hints(&hints, 10_000, false).await;
        assert!(first.is_err(), "first attempt should fail");

        // The URL is now negative-cached: a re-open fails fast without retrying.
        let second = service.acquire_pdf_with_hints(&hints, 10_000, false).await;
        assert!(
            second
                .expect_err("second attempt should fail")
                .to_string()
                .contains("failed recently"),
            "second attempt should be served from the negative cache"
        );

        // Force (Reader Retry) bypasses the cache and actually re-fetches;
        // the fake has consumed its scripted response, so the error is the
        // fake's default rather than the negative-cache message.
        let forced = service.acquire_pdf_with_hints(&hints, 10_000, true).await;
        assert!(
            !forced
                .expect_err("forced attempt should fail")
                .to_string()
                .contains("failed recently"),
            "force must bypass the negative cache"
        );
    }
}
