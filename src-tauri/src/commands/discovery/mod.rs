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
    request: DiscoverySearchRequest,
) -> Result<DiscoverySearchResponse, String> {
    DiscoveryOrchestrator::new(providers.openalex.clone(), providers.arxiv.clone())
        .with_expansion_providers(providers.europe_pmc.clone(), providers.core.clone())
        .search(request)
        .await
        .map_err(|e| e.to_string())
}
