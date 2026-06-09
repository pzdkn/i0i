//! Discovery command-side modules.
//!
//! Rust only compiles sibling files when the parent module declares them here.
//! This makes `service.rs`, `provider.rs`, and provider adapters discoverable
//! from `crate::commands::discovery`.

pub mod error;
pub mod provider;
pub mod providers;
pub mod service;

use crate::domain::discovery::{DiscoverySearchRequest, DiscoverySearchResponse};

use self::{providers::openalex::OpenAlexProvider, service::DiscoveryService};

pub type AppDiscoveryService = DiscoveryService<OpenAlexProvider>;

#[tauri::command]
pub async fn search_papers(
    service: tauri::State<'_, AppDiscoveryService>,
    request: DiscoverySearchRequest,
) -> Result<DiscoverySearchResponse, String> {
    service
        .search(request)
        .await
        .map_err(|error| error.to_string())
}
