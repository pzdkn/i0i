use std::time::Duration;

use tokio::process::Command;

use super::manager::ObscuraManager;
use crate::services::source_acquisition::types::{
    AcquisitionResult, BrowserPageSnapshot, FetchResponse, PageInspection, SourceAcquisitionError,
};

#[derive(Clone)]
pub struct ObscuraClient {
    manager: ObscuraManager,
}

impl ObscuraClient {
    pub fn new(manager: ObscuraManager) -> Self {
        Self { manager }
    }

    pub async fn fetch_original(&self, url: &str) -> AcquisitionResult<FetchResponse> {
        let bytes = self.run_fetch(url, "original").await?;
        Ok(FetchResponse {
            bytes,
            final_url: url.to_string(),
            content_type: None,
        })
    }

    pub async fn inspect_page(&self, url: &str) -> AcquisitionResult<PageInspection> {
        let html = String::from_utf8_lossy(&self.run_fetch(url, "html").await?).to_string();
        let text = String::from_utf8_lossy(&self.run_fetch(url, "text").await?).to_string();
        let links = lines(&String::from_utf8_lossy(
            &self.run_fetch(url, "links").await?,
        ));
        let assets = asset_urls(&String::from_utf8_lossy(
            &self.run_fetch(url, "assets").await?,
        ));

        Ok(PageInspection {
            snapshot: BrowserPageSnapshot {
                url: url.to_string(),
                final_url: url.to_string(),
                title: None,
                content_type: Some("text/html".to_string()),
                html: Some(html),
                text: Some(text),
            },
            links,
            assets,
            network_urls: vec![],
        })
    }

    async fn run_fetch(&self, url: &str, dump: &str) -> AcquisitionResult<Vec<u8>> {
        let program = self.manager.program();
        let timeout = Duration::from_millis(self.manager.config().request_timeout_ms);
        let mut command = Command::new(&program);
        command.arg("fetch").arg(url).arg("--dump").arg(dump);
        if self.manager.config().stealth {
            command.arg("--stealth");
        }

        let output = tokio::time::timeout(timeout, command.output())
            .await
            .map_err(|_| {
                SourceAcquisitionError::BrowserTimeout(format!(
                    "obscura fetch timed out after {} ms for {url}",
                    self.manager.config().request_timeout_ms
                ))
            })?
            .map_err(|error| {
                SourceAcquisitionError::BrowserProcessUnavailable(format!(
                    "failed to run {}: {error}",
                    program.display()
                ))
            })?;

        if !output.status.success() {
            return Err(SourceAcquisitionError::Browser(
                String::from_utf8_lossy(&output.stderr).to_string(),
            ));
        }

        Ok(output.stdout)
    }
}

fn lines(output: &str) -> Vec<String> {
    output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect()
}

fn asset_urls(output: &str) -> Vec<String> {
    let mut urls = Vec::new();
    for line in lines(output) {
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(&line) {
            collect_http_strings(&value, &mut urls);
        } else {
            push_unique_url(&mut urls, line);
        }
    }
    urls
}

fn collect_http_strings(value: &serde_json::Value, urls: &mut Vec<String>) {
    match value {
        serde_json::Value::String(text) => push_unique_url(urls, text.clone()),
        serde_json::Value::Array(values) => {
            for value in values {
                collect_http_strings(value, urls);
            }
        }
        serde_json::Value::Object(fields) => {
            for value in fields.values() {
                collect_http_strings(value, urls);
            }
        }
        _ => {}
    }
}

fn push_unique_url(urls: &mut Vec<String>, value: String) {
    let trimmed = value.trim();
    let is_http_url = trimmed.starts_with("http://") || trimmed.starts_with("https://");
    if is_http_url && !urls.iter().any(|url| url == trimmed) {
        urls.push(trimmed.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_line_based_obscura_output() {
        assert_eq!(
            lines("https://a.test\n\n https://b.test \n"),
            vec!["https://a.test", "https://b.test"]
        );
    }

    #[test]
    fn parses_json_line_asset_urls() {
        assert_eq!(
            asset_urls(
                r#"{"url":"https://publisher.example/a.pdf","kind":"document"}
{"src":"https://publisher.example/image.png"}"#
            ),
            vec![
                "https://publisher.example/a.pdf",
                "https://publisher.example/image.png"
            ]
        );
    }
}
