use async_trait::async_trait;
use reqwest::StatusCode;

use super::types::{AcquisitionResult, FetchResponse, HttpFetcher, SourceAcquisitionError};

#[derive(Clone)]
pub struct DirectHttpFetcher {
    client: reqwest::Client,
}

impl DirectHttpFetcher {
    pub fn new(client: reqwest::Client) -> Self {
        Self { client }
    }
}

#[async_trait]
impl HttpFetcher for DirectHttpFetcher {
    async fn fetch(&self, url: &str) -> AcquisitionResult<FetchResponse> {
        let response = self
            .client
            .get(url)
            .header("Accept", "application/pdf,text/html,*/*;q=0.8")
            .send()
            .await
            .map_err(|error| SourceAcquisitionError::Http(error.to_string()))?;

        let status = response.status();
        let final_url = response.url().to_string();
        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok())
            .map(str::to_string);

        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            if status == StatusCode::FORBIDDEN || status == StatusCode::TOO_MANY_REQUESTS {
                return Err(SourceAcquisitionError::DirectHttpForbidden(format!(
                    "{status} from {final_url}: {}",
                    body_snippet(&body)
                )));
            }
            return Err(SourceAcquisitionError::Http(format!(
                "{status} from {final_url}: {}",
                body_snippet(&body)
            )));
        }

        let bytes = response
            .bytes()
            .await
            .map_err(|error| SourceAcquisitionError::Http(error.to_string()))?;

        Ok(FetchResponse {
            bytes: bytes.to_vec(),
            final_url,
            content_type,
        })
    }
}

fn body_snippet(body: &str) -> String {
    body.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(220)
        .collect()
}
