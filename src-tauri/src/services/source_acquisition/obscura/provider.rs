use async_trait::async_trait;

use super::client::ObscuraClient;
use super::manager::ObscuraManager;
use crate::services::source_acquisition::types::{
    AcquisitionResult, BrowserEndpoint, BrowserRuntime, FetchResponse, PageInspection,
};

#[derive(Clone)]
pub struct ObscuraBrowserRuntime {
    manager: ObscuraManager,
    client: ObscuraClient,
}

impl ObscuraBrowserRuntime {
    pub fn new(manager: ObscuraManager) -> Self {
        let client = ObscuraClient::new(manager.clone());
        Self { manager, client }
    }
}

#[async_trait]
impl BrowserRuntime for ObscuraBrowserRuntime {
    async fn ensure_ready(&self) -> AcquisitionResult<BrowserEndpoint> {
        self.manager.ensure_ready().await
    }

    async fn fetch_original(&self, url: &str) -> AcquisitionResult<FetchResponse> {
        self.manager.ensure_ready().await?;
        self.client.fetch_original(url).await
    }

    async fn inspect_page(&self, url: &str) -> AcquisitionResult<PageInspection> {
        self.manager.ensure_ready().await?;
        self.client.inspect_page(url).await
    }
}
