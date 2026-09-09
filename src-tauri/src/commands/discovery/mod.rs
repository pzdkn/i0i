//! Discovery command-side modules.

pub mod browser;
mod browser_engine;
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
    #[allow(dead_code)]
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
    let source = browser::BrowserDiscoverySource::new_shared(
        source_acquisition.clone(),
        providers.openalex.clone(),
        providers.arxiv.clone(),
        browser::BrowserDiscoveryConfig::load(app),
    );
    let progress_app = app.clone();
    let event_query = query.clone();
    let on_progress = move |progress| {
        let (stage, message, candidates) = match progress {
            browser::BrowserDiscoveryProgress::SearchingWeb => (
                "searching".to_string(),
                "Searching the web".to_string(),
                Vec::new(),
            ),
            browser::BrowserDiscoveryProgress::SearchingProvider { provider } => (
                "searching".to_string(),
                format!("Searching {provider}"),
                Vec::new(),
            ),
            browser::BrowserDiscoveryProgress::ProviderAttempt(attempt) => (
                "provider_attempt".to_string(),
                match attempt.reason {
                    Some(reason) => {
                        format!("{} {}: {reason}", attempt.provider, attempt.status.as_str())
                    }
                    None => format!(
                        "{} returned {} candidates in {} ms",
                        attempt.provider, attempt.candidate_count, attempt.elapsed_ms
                    ),
                },
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
    let candidates = source
        .discover_with_resolvers(
            &query,
            request.result_limit.max(1) as usize,
            &request.providers,
            &on_progress,
        )
        .await
        .map_err(|error| error.to_string())?;
    let candidate_count = candidates.len();
    let _ = app.emit(
        "discovery_progress",
        DiscoveryProgress {
            query: query.clone(),
            stage: "ranking".to_string(),
            message: format!("Ranking {candidate_count} papers"),
            candidates: Vec::new(),
        },
    );
    ranked_response(reranker, &request, candidates).await
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
