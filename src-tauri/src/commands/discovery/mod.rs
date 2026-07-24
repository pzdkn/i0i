//! Discovery command-side modules.

pub mod error;
pub mod orchestrator;
pub mod provider;
pub mod providers;
pub mod service;

use crate::domain::discovery::{DiscoverySearchRequest, DiscoverySearchResponse};

use self::{
    orchestrator::DiscoveryOrchestrator,
    providers::{
        arxiv::ArxivProvider, core::CoreProvider, europe_pmc::EuropePmcProvider,
        openalex::OpenAlexProvider,
    },
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
    providers: tauri::State<'_, DiscoveryProviders>,
    reranker: tauri::State<'_, crate::services::embedding::EmbeddingReranker>,
    request: DiscoverySearchRequest,
) -> Result<DiscoverySearchResponse, String> {
    orchestrator(&providers, &reranker)
        .search(request)
        .await
        .map_err(|e| e.to_string())
}

/// Progressive query expansion (RFC 0054). The frontend calls this after the
/// literal `search_papers` results are already on screen: it expands the query
/// with a cheap LLM, re-runs the original plus the variants, and returns the
/// merged, reranked set. Expansion is best-effort — no key / timeout / no
/// variants just reproduces the literal result.
#[tauri::command]
pub async fn expand_search(
    providers: tauri::State<'_, DiscoveryProviders>,
    reranker: tauri::State<'_, crate::services::embedding::EmbeddingReranker>,
    expander: tauri::State<'_, crate::services::query_expansion::QueryExpander>,
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
    orchestrator(&providers, &reranker)
        .search_expanded(request, variants)
        .await
        .map_err(|e| e.to_string())
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

/// Build a quick-search orchestrator with the expansion providers (RFC 0053)
/// and the embedding reranker (RFC 0054) attached.
fn orchestrator(
    providers: &DiscoveryProviders,
    reranker: &crate::services::embedding::EmbeddingReranker,
) -> DiscoveryOrchestrator {
    DiscoveryOrchestrator::new(providers.openalex.clone(), providers.arxiv.clone())
        .with_expansion_providers(providers.europe_pmc.clone(), providers.core.clone())
        .with_reranker(reranker.clone())
}
