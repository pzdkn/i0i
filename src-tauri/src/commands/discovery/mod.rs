//! Discovery command-side modules.

pub mod browser;
pub mod error;
pub mod orchestrator;
pub mod provider;
pub mod providers;
pub mod service;

use serde::Serialize;
use tauri::Emitter;

use crate::domain::discovery::{DiscoverySearchRequest, DiscoverySearchResponse, PaperCandidate};
use crate::domain::research::SearchConstraints;
use crate::services::source_acquisition::SourceAcquisitionService;

use self::providers::{
    arxiv::ArxivProvider, core::CoreProvider, europe_pmc::EuropePmcProvider,
    openalex::OpenAlexProvider,
};

use super::discovery::error::DiscoveryError;

/// All provider adapters, initialized once at app startup and shared across
/// all search calls.
///
/// Each provider holds its own `reqwest::Client` which maintains a connection
/// pool. Storing providers here instead of constructing them per-request lets
/// that pool survive between searches. Europe PMC and CORE (RFC 0053) construct
/// unconditionally; CORE resolves its API key lazily and reports itself
/// unconfigured when the key is absent, so a missing key never blocks startup.
pub struct DiscoveryProviders {
    pub openalex: OpenAlexProvider,
    pub arxiv: ArxivProvider,
    pub europe_pmc: EuropePmcProvider,
    pub core: CoreProvider,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveryProgress {
    pub query: String,
    pub stage: String,
    pub message: String,
    pub candidates: Vec<PaperCandidate>,
}

impl DiscoveryProviders {
    pub fn from_app_config() -> Result<Self, DiscoveryError> {
        Ok(Self {
            openalex: OpenAlexProvider::from_app_config()?,
            arxiv: ArxivProvider::from_app_config()?,
            europe_pmc: EuropePmcProvider::from_app_config()?,
            core: CoreProvider::from_app_config()?,
        })
    }
}

#[tauri::command]
pub async fn search_papers(
    app: tauri::AppHandle,
    providers: tauri::State<'_, DiscoveryProviders>,
    reranker: tauri::State<'_, crate::services::embedding::EmbeddingReranker>,
    source_acquisition: tauri::State<'_, SourceAcquisitionService>,
    request: DiscoverySearchRequest,
) -> Result<DiscoverySearchResponse, String> {
    browser_search(&app, &providers, &reranker, &source_acquisition, request).await
}

/// Progressive query expansion (RFC 0054). The frontend calls this after the
/// literal `search_papers` results are already on screen: it expands the query
/// with a cheap LLM, re-runs the original plus the variants, and returns the
/// merged, reranked set. Expansion is best-effort — no key / timeout / no
/// variants just reproduces the literal result.
#[tauri::command]
pub async fn expand_search(
    app: tauri::AppHandle,
    providers: tauri::State<'_, DiscoveryProviders>,
    reranker: tauri::State<'_, crate::services::embedding::EmbeddingReranker>,
    expander: tauri::State<'_, crate::services::query_expansion::QueryExpander>,
    source_acquisition: tauri::State<'_, SourceAcquisitionService>,
    request: DiscoverySearchRequest,
) -> Result<DiscoverySearchResponse, String> {
    let variants = expander.expand(&request.query).await;
    // No variants (no key, cached-empty, timeout, unparseable reply) ⇒ the
    // expanded set would be identical to the literal one already on screen.
    // Return an empty-candidate sentinel instead of re-running the search, so a
    // no-variant expansion costs zero extra provider calls (RFC 0054).
    if variants.is_empty() {
        return Ok(empty_expansion_response(&request));
    }
    // The literal result is already visible and the frontend merges this
    // response into it. Search only the new variants, concurrently, so browser
    // latency is paid once rather than once per expansion.
    let searches = variants.into_iter().map(|variant| {
        let mut variant_request = request.clone();
        variant_request.query = variant;
        browser_search(
            &app,
            &providers,
            &reranker,
            &source_acquisition,
            variant_request,
        )
    });
    let mut candidates = Vec::new();
    for response in futures_util::future::join_all(searches).await {
        if let Ok(response) = response {
            candidates.extend(response.candidates);
        }
    }
    if candidates.is_empty() {
        return Ok(empty_expansion_response(&request));
    }
    ranked_response(&reranker, &request, candidates).await
}

/// An empty-candidate response signalling "no expansion happened". The frontend
/// merge treats zero candidates as a no-op and leaves the literal results as-is.
fn empty_expansion_response(request: &DiscoverySearchRequest) -> DiscoverySearchResponse {
    DiscoverySearchResponse {
        provider: "multi".to_string(),
        query: request.query.trim().to_string(),
        filters: Vec::new(),
        sort_by: request.sort_by.clone(),
        result_limit: request.result_limit,
        result_count: 0,
        candidates: Vec::new(),
    }
}

async fn browser_search(
    app: &tauri::AppHandle,
    providers: &DiscoveryProviders,
    reranker: &crate::services::embedding::EmbeddingReranker,
    source_acquisition: &SourceAcquisitionService,
    request: DiscoverySearchRequest,
) -> Result<DiscoverySearchResponse, String> {
    let query = request.query.trim().to_string();
    if query.is_empty() {
        return Err("Enter a search query before running discovery.".to_string());
    }
    let source = browser::BrowserDiscoverySource::new(
        source_acquisition.clone(),
        providers.openalex.clone(),
        providers.arxiv.clone(),
        browser::BrowserDiscoveryConfig::load(app),
    );
    let api_orchestrator = orchestrator::DiscoveryOrchestrator::new(
        providers.openalex.clone(),
        providers.arxiv.clone(),
    )
    .with_expansion_providers(providers.europe_pmc.clone(), providers.core.clone())
    .with_reranker(reranker.clone());
    let mut api_request = request.clone();
    api_request.query = canonical_api_query(&query);
    let progress_app = app.clone();
    let event_query = query.clone();
    let on_progress = move |progress| {
        let (stage, message, candidates) = match progress {
            browser::BrowserDiscoveryProgress::SearchingWeb => (
                "searching".to_string(),
                "Searching the web and scholarly APIs".to_string(),
                Vec::new(),
            ),
            browser::BrowserDiscoveryProgress::Provisional(candidates) => (
                "provisional".to_string(),
                format!("Found {} links", candidates.len()),
                candidates,
            ),
            browser::BrowserDiscoveryProgress::ResolvingMetadata { count } => (
                "resolving".to_string(),
                format!("Resolving metadata for {count} papers"),
                Vec::new(),
            ),
            browser::BrowserDiscoveryProgress::Resolved { count } => (
                "resolved".to_string(),
                format!("Resolved {count} papers"),
                Vec::new(),
            ),
        };
        let _ = progress_app.emit(
            "discovery_progress",
            DiscoveryProgress {
                query: event_query.clone(),
                stage,
                message,
                candidates,
            },
        );
    };
    let browser_future =
        source.discover(&query, request.result_limit.max(1) as usize, &on_progress);
    let api_future = api_orchestrator.search_sources(&api_request, Vec::new());
    let (browser_result, api_result) = tokio::join!(browser_future, api_future);
    let browser_result =
        browser_result.map(|candidates| apply_request_constraints(candidates, &request));
    let sources =
        combine_hybrid_sources(browser_result, api_result).map_err(|error| error.to_string())?;
    let candidate_count = sources
        .results
        .iter()
        .map(|result| result.candidates.len())
        .sum::<usize>();
    let _ = app.emit(
        "discovery_progress",
        DiscoveryProgress {
            query: query.clone(),
            stage: "ranking".to_string(),
            message: format!("Ranking {candidate_count} papers"),
            candidates: Vec::new(),
        },
    );
    api_orchestrator
        .response_from_sources(request, sources)
        .await
        .map_err(|error| error.to_string())
}

/// Canonicalize compact scholarly spellings for the API lane.
fn canonical_api_query(query: &str) -> String {
    query
        .split_whitespace()
        .map(|term| match term.to_ascii_lowercase().as_str() {
            "lowrank" => "low-rank".to_string(),
            "selfsupervised" => "self-supervised".to_string(),
            _ => term.to_string(),
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Combine independent browser and API outcomes without losing partial success.
fn combine_hybrid_sources(
    browser_result: Result<Vec<PaperCandidate>, DiscoveryError>,
    api_result: Result<orchestrator::DiscoverySourceResults, DiscoveryError>,
) -> Result<orchestrator::DiscoverySourceResults, DiscoveryError> {
    let mut sources = match api_result {
        Ok(sources) => sources,
        Err(error) => orchestrator::DiscoverySourceResults {
            results: Vec::new(),
            errors: vec![format!("APIs: {error}")],
        },
    };
    match browser_result {
        Ok(candidates) => {
            sources
                .results
                .push(browser::BrowserDiscoverySource::as_provider_result(
                    candidates,
                ))
        }
        Err(error) => sources.errors.push(format!("Browser: {error}")),
    }
    if sources.results.is_empty() {
        return Err(DiscoveryError::new(format!(
            "All discovery sources failed: {}",
            sources.errors.join("; ")
        )));
    }
    Ok(sources)
}

/// Apply request constraints that browser discovery cannot enforce upstream.
fn apply_request_constraints(
    candidates: Vec<PaperCandidate>,
    request: &DiscoverySearchRequest,
) -> Vec<PaperCandidate> {
    let constraints = SearchConstraints {
        year_from: request.year_from,
        year_to: request.year_to,
        providers: Vec::new(),
        open_access: request.open_access,
        target_count: request.result_limit,
        venues: request.venues.clone(),
        authors: request.authors.clone(),
        fields_of_study: request.fields_of_study.clone(),
        seed_paper_ids: Vec::new(),
    };
    crate::services::research::filter::apply_resolved_constraints(candidates, &constraints)
}

async fn ranked_response(
    reranker: &crate::services::embedding::EmbeddingReranker,
    request: &DiscoverySearchRequest,
    candidates: Vec<PaperCandidate>,
) -> Result<DiscoverySearchResponse, String> {
    let filtered = apply_request_constraints(candidates, request);
    let merged = orchestrator::merge_and_filter(
        vec![browser::BrowserDiscoverySource::as_provider_result(
            filtered,
        )],
        request.only_viewable,
    );
    let semantic = reranker.semantic_scores(&request.query, &merged).await;
    let mut candidates =
        orchestrator::rank_candidates(merged, &request.query, request.result_limit, &semantic);
    orchestrator::sort_candidates(&mut candidates, &request.sort_by);
    Ok(DiscoverySearchResponse {
        provider: "browser".to_string(),
        query: request.query.trim().to_string(),
        filters: vec![
            "browser-first".to_string(),
            "exact metadata resolution".to_string(),
        ],
        sort_by: request.sort_by.clone(),
        result_limit: request.result_limit,
        result_count: candidates.len(),
        candidates,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::sync::Arc;

    use crate::commands::discovery::provider::{DiscoveryProviderId, ProviderSearchResult};
    use crate::domain::discovery::{CandidateMatch, DiscoverySort};
    use crate::services::source_acquisition::types::{
        AcquisitionMethod, AcquisitionResult, BrowserEndpoint, BrowserPageSnapshot, BrowserRuntime,
        FetchResponse, HttpFetcher, PageInspection, SourceAcquisitionConfig,
        SourceAcquisitionError,
    };

    struct UnusedHttp;

    #[async_trait]
    impl HttpFetcher for UnusedHttp {
        async fn fetch(&self, _url: &str) -> AcquisitionResult<FetchResponse> {
            Err(SourceAcquisitionError::Http(
                "HTTP is not used by browser discovery".to_string(),
            ))
        }
    }

    struct RecordedBraveBrowser;

    #[async_trait]
    impl BrowserRuntime for RecordedBraveBrowser {
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
            let html = include_str!("../../../tests/fixtures/browser_discovery_brave.html");
            Ok(PageInspection {
                snapshot: BrowserPageSnapshot {
                    url: url.to_string(),
                    final_url: url.to_string(),
                    title: Some("lowrank adaptation - Brave Search".to_string()),
                    content_type: Some("text/html".to_string()),
                    html: Some(html.to_string()),
                    text: Some("LoRA DINO scholarly search results".to_string()),
                },
                links: Vec::new(),
                assets: Vec::new(),
                network_urls: Vec::new(),
            })
        }
    }

    fn request() -> DiscoverySearchRequest {
        DiscoverySearchRequest {
            query: "lowrank adaptation".to_string(),
            year_from: None,
            year_to: None,
            result_limit: 10,
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

    fn api_candidate() -> PaperCandidate {
        PaperCandidate {
            id: "arxiv:2106.09685".to_string(),
            source_provider: "arxiv".to_string(),
            source_id: "2106.09685".to_string(),
            title: "LoRA: Low-Rank Adaptation of Large Language Models".to_string(),
            authors: Vec::new(),
            abstract_text: None,
            year: Some(2021),
            publication_date: None,
            venue: None,
            citation_count: None,
            doi: None,
            openalex_id: None,
            arxiv_id: Some("2106.09685".to_string()),
            external_url: Some("https://arxiv.org/abs/2106.09685".to_string()),
            pdf_url: Some("https://arxiv.org/pdf/2106.09685".to_string()),
            open_access: None,
            match_summary: CandidateMatch {
                score: Some(1.0),
                reasons: Vec::new(),
                matched_keywords: Vec::new(),
                from_seed_paper_ids: Vec::new(),
            },
            already_in_library: false,
        }
    }

    #[test]
    fn browser_challenge_does_not_discard_api_candidates() {
        let api_sources = orchestrator::DiscoverySourceResults {
            results: vec![ProviderSearchResult {
                provider: DiscoveryProviderId::Arxiv,
                filters: Vec::new(),
                candidates: vec![api_candidate()],
            }],
            errors: Vec::new(),
        };

        let combined = combine_hybrid_sources(
            Err(DiscoveryError::new(
                "Browser search was challenged by Google Scholar",
            )),
            Ok(api_sources),
        )
        .expect("API success should keep Find usable");

        assert_eq!(combined.results.len(), 1);
        assert_eq!(
            combined.results[0].candidates[0].title,
            api_candidate().title
        );
        assert!(combined
            .errors
            .iter()
            .any(|error| error.contains("challenged by Google Scholar")));
    }

    #[test]
    fn api_failure_does_not_discard_browser_candidates() {
        let combined = combine_hybrid_sources(
            Ok(vec![api_candidate()]),
            Err(DiscoveryError::new("arXiv rate limited")),
        )
        .expect("browser success should keep Find usable");

        assert_eq!(combined.results.len(), 1);
        assert_eq!(combined.results[0].provider, DiscoveryProviderId::Web);
        assert!(combined
            .errors
            .iter()
            .any(|error| error.contains("arXiv rate limited")));
    }

    #[test]
    fn api_query_canonicalizes_required_compact_spellings() {
        assert_eq!(
            canonical_api_query("lowrank adaptation"),
            "low-rank adaptation"
        );
        assert_eq!(
            canonical_api_query("selfsupervised vision"),
            "self-supervised vision"
        );
        assert_eq!(canonical_api_query("diffusion models"), "diffusion models");
    }

    #[test]
    fn all_source_failures_are_reported_together() {
        let error = combine_hybrid_sources(
            Err(DiscoveryError::new("browser challenge")),
            Err(DiscoveryError::new("arXiv rate limited")),
        )
        .expect_err("Find should fail when every source fails");

        assert!(error.to_string().contains("browser challenge"));
        assert!(error.to_string().contains("arXiv rate limited"));
    }

    async fn live_find(query: &str, expected_arxiv_id: &str) {
        let mut request = request();
        request.query = query.to_string();
        request.providers = vec![crate::domain::discovery::DiscoveryProviderChoice::Arxiv];
        request.only_viewable = true;

        let openalex = OpenAlexProvider::from_app_config().expect("OpenAlex config");
        let arxiv = ArxivProvider::from_app_config().expect("arXiv config");
        let api_orchestrator =
            orchestrator::DiscoveryOrchestrator::new(openalex.clone(), arxiv.clone());
        let browser_service = SourceAcquisitionService::new(
            SourceAcquisitionConfig::default(),
            Arc::new(UnusedHttp),
            Arc::new(RecordedBraveBrowser),
            AcquisitionMethod::ObscuraBrowserStealth,
        );
        let browser_source = browser::BrowserDiscoverySource::new(
            browser_service,
            openalex,
            arxiv,
            browser::BrowserDiscoveryConfig::default(),
        );
        let mut api_request = request.clone();
        api_request.query = canonical_api_query(query);
        let browser_future = browser_source.discover(query, 10, &|_| {});
        let api_future = api_orchestrator.search_sources(&api_request, Vec::new());
        let (browser_candidates, api_sources) = tokio::join!(browser_future, api_future);
        let sources = combine_hybrid_sources(browser_candidates, api_sources)
            .expect("recorded browser and arXiv API should produce a hybrid response");
        let response = api_orchestrator
            .response_from_sources(request, sources)
            .await
            .expect("hybrid Find response");
        let candidate = response
            .candidates
            .iter()
            .find(|candidate| candidate.arxiv_id.as_deref() == Some(expected_arxiv_id))
            .unwrap_or_else(|| {
                panic!(
                    "missing arXiv:{expected_arxiv_id}; returned: {:?}",
                    response
                        .candidates
                        .iter()
                        .map(|candidate| (&candidate.title, &candidate.arxiv_id))
                        .collect::<Vec<_>>()
                )
            });
        assert!(
            candidate.external_url.is_some(),
            "landing URL is displayable"
        );
        assert!(candidate.pdf_url.is_some(), "PDF URL is displayable");
    }

    #[tokio::test]
    #[ignore = "live arXiv API metadata resolution and provider fanout"]
    async fn live_hybrid_find_returns_lora_and_dino() {
        live_find("lowrank adaptation", "2106.09685").await;
        tokio::time::sleep(std::time::Duration::from_secs(3)).await;
        live_find("selfsupervised vision", "2104.14294").await;
    }
}
