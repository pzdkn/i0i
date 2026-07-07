use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use tauri::Manager;

use super::types::{
    AcquiredSource, AcquisitionMethod, AcquisitionResult, BrowserPageSnapshot, BrowserRuntime,
    FetchResponse, HttpFetcher, PageInspection, SourceAcquisitionConfig, SourceAcquisitionError,
};
use crate::services::source_acquisition::http::DirectHttpFetcher;
use crate::services::source_acquisition::obscura::{ObscuraBrowserRuntime, ObscuraManager};
use crate::services::source_acquisition::types::ObscuraConfig;

#[derive(Clone)]
pub struct SourceAcquisitionService {
    config: SourceAcquisitionConfig,
    http: Arc<dyn HttpFetcher>,
    browser: Arc<dyn BrowserRuntime>,
    browser_method: AcquisitionMethod,
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
        let manager = ObscuraManager::new(obscura_config);
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
        Self {
            config,
            http,
            browser,
            browser_method,
        }
    }

    pub async fn acquire_pdf(
        &self,
        source_url: &str,
        landing_page_url: Option<&str>,
        max_bytes: u64,
    ) -> AcquisitionResult<AcquiredSource> {
        match self.http.fetch(source_url).await {
            Ok(response) => {
                match self.pdf_from_response(response, AcquisitionMethod::DirectHttp, max_bytes) {
                    Ok(source) => return Ok(source),
                    Err(SourceAcquisitionError::DownloadedBytesNotPdf(_))
                    | Err(SourceAcquisitionError::BrowserReturnedHtml(_)) => {}
                    Err(error) => return Err(error),
                }
            }
            Err(error) if !self.config.prefer_browser_for_blocked_sources => return Err(error),
            Err(error) if self.config.browser_fallback != "obscura" => return Err(error),
            Err(_) => {}
        }

        let browser_response = self.browser_fetch_original(source_url).await;
        match browser_response {
            Ok(response) => {
                match self.pdf_from_response(response, self.browser_method.clone(), max_bytes) {
                    Ok(source) => return Ok(source),
                    Err(SourceAcquisitionError::DownloadedBytesNotPdf(_))
                    | Err(SourceAcquisitionError::BrowserReturnedHtml(_)) => {}
                    Err(error) => return Err(error),
                }
            }
            Err(_) => {}
        }

        if let Some(landing_page_url) = landing_page_url {
            return self
                .acquire_pdf_from_landing_page(landing_page_url, max_bytes)
                .await;
        }

        Err(SourceAcquisitionError::BrowserReturnedHtml(format!(
            "Browser fallback did not return a PDF for {source_url}"
        )))
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

    pub async fn acquire_web_page(&self, url: &str) -> AcquisitionResult<BrowserPageSnapshot> {
        self.debug_fetch_url(url).await
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
            if let Ok(response) = self.http.fetch(&candidate_url).await {
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

fn pdf_candidates(inspection: &PageInspection) -> Vec<String> {
    let mut candidates = Vec::new();
    for url in inspection
        .network_urls
        .iter()
        .chain(inspection.links.iter())
        .chain(inspection.assets.iter())
    {
        let normalized = url.to_lowercase();
        if normalized.contains(".pdf")
            || normalized.contains("/pdf")
            || normalized.contains("download")
        {
            if !candidates.contains(url) {
                candidates.push(url.clone());
            }
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
}
