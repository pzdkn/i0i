//! Discovery command-side modules.

pub mod error;
pub mod provider;
pub mod providers;
pub mod service;

use crate::domain::discovery::{
    DiscoveryProviderChoice, DiscoverySearchRequest, DiscoverySearchResponse,
};

use self::{
    providers::{arxiv::ArxivProvider, openalex::OpenAlexProvider},
    service::DiscoveryService,
};

use super::discovery::error::DiscoveryError;

/// Both provider adapters, initialized once at app startup and shared across
/// all search calls.
///
/// Each provider holds its own `reqwest::Client` which maintains a connection
/// pool. Storing providers here instead of constructing them per-request lets
/// that pool survive between searches.
pub struct DiscoveryProviders {
    pub openalex: OpenAlexProvider,
    pub arxiv: ArxivProvider,
}

impl DiscoveryProviders {
    pub fn from_app_config() -> Result<Self, DiscoveryError> {
        Ok(Self {
            openalex: OpenAlexProvider::from_app_config()?,
            arxiv: ArxivProvider::from_app_config()?,
        })
    }
}

#[tauri::command]
pub async fn search_papers(
    providers: tauri::State<'_, DiscoveryProviders>,
    request: DiscoverySearchRequest,
) -> Result<DiscoverySearchResponse, String> {
    match request.provider {
        DiscoveryProviderChoice::OpenAlex => {
            DiscoveryService::new(providers.openalex.clone())
                .search(request)
                .await
        }
        DiscoveryProviderChoice::Arxiv => {
            DiscoveryService::new(providers.arxiv.clone())
                .search(request)
                .await
        }
    }
    .map_err(|e| e.to_string())
}
