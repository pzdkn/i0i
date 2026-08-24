use std::time::{Duration, Instant};

use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::net::TcpStream;
use tokio_tungstenite::{connect_async, tungstenite::Message, MaybeTlsStream, WebSocketStream};

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
        let mut page = self.open_page().await?;
        page.navigate(url).await?;
        let script = format!(
            r#"(async () => {{
                const response = await fetch({}, {{ credentials: "include" }});
                const bytes = new Uint8Array(await response.arrayBuffer());
                let binary = "";
                const chunkSize = 0x8000;
                for (let offset = 0; offset < bytes.length; offset += chunkSize) {{
                    binary += String.fromCharCode(...bytes.subarray(offset, offset + chunkSize));
                }}
                return {{
                    body: btoa(binary),
                    finalUrl: response.url,
                    contentType: response.headers.get("content-type")
                }};
            }})()"#,
            serde_json::to_string(url).map_err(browser_error)?
        );
        let value = page.evaluate(&script, true).await?;
        let body = value
            .get("body")
            .and_then(Value::as_str)
            .ok_or_else(|| SourceAcquisitionError::Browser("missing response body".to_string()))?;
        let bytes = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, body)
            .map_err(browser_error)?;
        let final_url = value
            .get("finalUrl")
            .and_then(Value::as_str)
            .unwrap_or(url)
            .to_string();
        let content_type = value
            .get("contentType")
            .and_then(Value::as_str)
            .map(str::to_string);
        page.close().await;
        Ok(FetchResponse {
            bytes,
            final_url,
            content_type,
        })
    }

    pub async fn inspect_page(&self, url: &str) -> AcquisitionResult<PageInspection> {
        let mut page = self.open_page().await?;
        page.navigate(url).await?;
        let value = page
            .evaluate(
                r#"(() => ({
                    url: location.href,
                    title: document.title || null,
                    html: document.documentElement?.outerHTML || "",
                    text: document.body?.innerText || "",
                    links: Array.from(document.querySelectorAll("a[href]"), a => ({
                        url: a.href,
                        text: (a.innerText || a.textContent || "").trim()
                    })),
                    assets: Array.from(document.querySelectorAll("img[src],script[src],link[href],iframe[src],embed[src],object[data]"), node =>
                        node.src || node.href || node.data
                    ).filter(Boolean)
                }))()"#,
                false,
            )
            .await?;

        let final_url = value
            .get("url")
            .and_then(Value::as_str)
            .unwrap_or(url)
            .to_string();
        let title = value
            .get("title")
            .and_then(Value::as_str)
            .map(str::to_string);
        let html = value
            .get("html")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let text = value
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let links = value
            .get("links")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|link| link.get("url").and_then(Value::as_str))
            .map(str::to_string)
            .collect();
        let assets = value
            .get("assets")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect();
        let network_urls = page.network_urls();
        page.close().await;

        Ok(PageInspection {
            snapshot: BrowserPageSnapshot {
                url: url.to_string(),
                final_url,
                title,
                content_type: Some("text/html".to_string()),
                html: Some(html),
                text: Some(text),
            },
            links,
            assets,
            network_urls,
        })
    }

    async fn open_page(&self) -> AcquisitionResult<CdpPage> {
        let endpoint = self.manager.ensure_ready().await?;
        let websocket_url = endpoint.websocket_url.ok_or_else(|| {
            SourceAcquisitionError::BrowserProcessUnavailable(
                "Obscura did not expose a CDP websocket endpoint".to_string(),
            )
        })?;
        CdpPage::open(&websocket_url, self.manager.config().request_timeout_ms).await
    }
}

type CdpSocket = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// One disposable page connected to the managed Obscura browser over CDP.
struct CdpPage {
    socket: CdpSocket,
    target_id: String,
    session_id: String,
    next_id: u64,
    deadline: Instant,
    network_urls: Vec<String>,
}

impl CdpPage {
    async fn open(websocket_url: &str, timeout_ms: u64) -> AcquisitionResult<Self> {
        let (socket, _) = connect_async(websocket_url).await.map_err(browser_error)?;
        let mut page = Self {
            socket,
            target_id: String::new(),
            session_id: String::new(),
            next_id: 1,
            deadline: Instant::now() + Duration::from_millis(timeout_ms),
            network_urls: Vec::new(),
        };
        let target = page
            .command("Target.createTarget", json!({ "url": "about:blank" }), None)
            .await?;
        page.target_id = required_string(&target, "targetId")?;
        let attached = page
            .command(
                "Target.attachToTarget",
                json!({ "targetId": page.target_id, "flatten": true }),
                None,
            )
            .await?;
        page.session_id = required_string(&attached, "sessionId")?;
        for method in ["Page.enable", "Runtime.enable", "Network.enable"] {
            page.command(method, json!({}), Some(page.session_id.clone()))
                .await?;
        }
        Ok(page)
    }

    async fn navigate(&mut self, url: &str) -> AcquisitionResult<()> {
        let navigation = self
            .command(
                "Page.navigate",
                json!({ "url": url }),
                Some(self.session_id.clone()),
            )
            .await?;
        if let Some(error) = navigation.get("errorText").and_then(Value::as_str) {
            return Err(SourceAcquisitionError::Browser(format!(
                "page navigation failed: {error}"
            )));
        }

        loop {
            // `Page.navigate` can return before the destination commits. Check
            // the URL as well as readiness so the initial `about:blank` page
            // cannot be mistaken for a completed navigation.
            let state = self
                .evaluate(
                    "({ readyState: document.readyState, url: location.href })",
                    false,
                )
                .await?;
            let ready = state
                .get("readyState")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let current_url = state.get("url").and_then(Value::as_str).unwrap_or_default();
            if current_url != "about:blank" && matches!(ready, "interactive" | "complete") {
                return Ok(());
            }
            if Instant::now() >= self.deadline {
                return Err(SourceAcquisitionError::BrowserTimeout(
                    "page navigation did not complete before the browser deadline".to_string(),
                ));
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    async fn evaluate(
        &mut self,
        expression: &str,
        await_promise: bool,
    ) -> AcquisitionResult<Value> {
        let result = self
            .command(
                "Runtime.evaluate",
                json!({
                    "expression": expression,
                    "awaitPromise": await_promise,
                    "returnByValue": true
                }),
                Some(self.session_id.clone()),
            )
            .await?;
        if let Some(details) = result.get("exceptionDetails") {
            return Err(SourceAcquisitionError::Browser(format!(
                "browser evaluation failed: {details}"
            )));
        }
        Ok(result
            .get("result")
            .and_then(|result| result.get("value"))
            .cloned()
            .unwrap_or(Value::Null))
    }

    async fn command(
        &mut self,
        method: &str,
        params: Value,
        session_id: Option<String>,
    ) -> AcquisitionResult<Value> {
        let id = self.next_id;
        self.next_id += 1;
        let mut payload = json!({ "id": id, "method": method, "params": params });
        if let Some(session_id) = session_id {
            payload["sessionId"] = Value::String(session_id);
        }
        self.socket
            .send(Message::Text(payload.to_string().into()))
            .await
            .map_err(browser_error)?;

        loop {
            let remaining = self.deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(SourceAcquisitionError::BrowserTimeout(format!(
                    "CDP command {method} timed out"
                )));
            }
            let message = tokio::time::timeout(remaining, self.socket.next())
                .await
                .map_err(|_| {
                    SourceAcquisitionError::BrowserTimeout(format!(
                        "CDP command {method} timed out"
                    ))
                })?
                .ok_or_else(|| {
                    SourceAcquisitionError::Browser("CDP connection closed".to_string())
                })?
                .map_err(browser_error)?;
            let Message::Text(text) = message else {
                continue;
            };
            let value: Value = serde_json::from_str(text.as_ref()).map_err(browser_error)?;
            self.capture_event(&value);
            if value.get("id").and_then(Value::as_u64) != Some(id) {
                continue;
            }
            if let Some(error) = value.get("error") {
                return Err(SourceAcquisitionError::Browser(format!(
                    "CDP command {method} failed: {error}"
                )));
            }
            return Ok(value.get("result").cloned().unwrap_or(Value::Null));
        }
    }

    fn capture_event(&mut self, value: &Value) {
        if value.get("method").and_then(Value::as_str) != Some("Network.responseReceived") {
            return;
        }
        let Some(url) = value
            .pointer("/params/response/url")
            .and_then(Value::as_str)
        else {
            return;
        };
        if !self.network_urls.iter().any(|existing| existing == url) {
            self.network_urls.push(url.to_string());
        }
    }

    fn network_urls(&self) -> Vec<String> {
        self.network_urls.clone()
    }

    async fn close(&mut self) {
        let _ = self
            .command(
                "Target.closeTarget",
                json!({ "targetId": self.target_id }),
                None,
            )
            .await;
    }
}

fn required_string(value: &Value, key: &str) -> AcquisitionResult<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| SourceAcquisitionError::Browser(format!("missing CDP field {key}")))
}

fn browser_error(error: impl std::fmt::Display) -> SourceAcquisitionError {
    SourceAcquisitionError::Browser(error.to_string())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::services::source_acquisition::types::ObscuraConfig;

    #[tokio::test]
    async fn inspects_a_page_through_the_managed_cdp_server() {
        let binary = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("resources")
            .join("obscura")
            .join("obscura");
        if !binary.exists() {
            return;
        }
        let manager = ObscuraManager::new(ObscuraConfig {
            path: Some(binary),
            stealth: true,
            request_timeout_ms: 10_000,
            ..ObscuraConfig::default()
        });
        let client = ObscuraClient::new(manager);

        let page = client
            .inspect_page("data:text/html,<title>CDP test</title><main>managed session</main>")
            .await
            .expect("managed Obscura page should be inspectable");

        assert_eq!(page.snapshot.title.as_deref(), Some("CDP test"));
        assert!(page
            .snapshot
            .text
            .as_deref()
            .unwrap_or_default()
            .contains("managed session"));
    }
}
