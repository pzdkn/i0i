use std::fs;
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::Deserialize;
use tauri::{AppHandle, Manager};
use tokio::process::{Child, Command};
use tokio::sync::Mutex;

use crate::services::source_acquisition::types::{
    BrowserEndpoint, ObscuraConfig, SourceAcquisitionError,
};

#[derive(Clone)]
pub struct ObscuraManager {
    config: ObscuraConfig,
    state: Arc<Mutex<Option<ObscuraProcess>>>,
    client: reqwest::Client,
}

struct ObscuraProcess {
    endpoint: BrowserEndpoint,
    child: Child,
}

impl Drop for ObscuraProcess {
    fn drop(&mut self) {
        let _ = self.child.start_kill();
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct VersionResponse {
    web_socket_debugger_url: Option<String>,
}

impl ObscuraConfig {
    pub fn load(app: &AppHandle) -> Self {
        let mut config = Self::default();
        for path in candidate_config_paths(app) {
            if let Ok(contents) = fs::read_to_string(path) {
                config.apply_toml_like_overrides(&contents);
                break;
            }
        }
        if config.path.is_none() {
            config.path = candidate_obscura_paths(app)
                .into_iter()
                .find(|path| path.exists());
        }
        config
    }

    fn apply_toml_like_overrides(&mut self, contents: &str) {
        let mut in_obscura = false;

        for line in contents.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            if line.starts_with('[') && line.ends_with(']') {
                in_obscura = line == "[obscura]";
                continue;
            }

            if !in_obscura {
                continue;
            }

            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let key = key.trim();
            let value = value.trim().trim_matches('"');

            match key {
                "enabled" => self.enabled = value == "true",
                "path" => {
                    if !value.is_empty() {
                        self.path = Some(PathBuf::from(value));
                    }
                }
                "stealth" => self.stealth = value == "true",
                "startup_timeout_ms" => {
                    if let Ok(value) = value.parse::<u64>() {
                        self.startup_timeout_ms = value;
                    }
                }
                "request_timeout_ms" => {
                    if let Ok(value) = value.parse::<u64>() {
                        self.request_timeout_ms = value;
                    }
                }
                "port" => {
                    if let Ok(value) = value.parse::<u16>() {
                        self.port = value;
                    }
                }
                _ => {}
            }
        }
    }
}

impl ObscuraManager {
    pub fn new(config: ObscuraConfig) -> Self {
        Self {
            config,
            state: Arc::new(Mutex::new(None)),
            client: reqwest::Client::new(),
        }
    }

    pub fn config(&self) -> &ObscuraConfig {
        &self.config
    }

    pub async fn ensure_ready(&self) -> Result<BrowserEndpoint, SourceAcquisitionError> {
        if !self.config.enabled {
            return Err(SourceAcquisitionError::BrowserProcessUnavailable(
                "Obscura is disabled".to_string(),
            ));
        }

        {
            let mut state = self.state.lock().await;
            if let Some(process) = state.as_mut() {
                if process
                    .child
                    .try_wait()
                    .map_err(to_process_error)?
                    .is_none()
                {
                    return Ok(process.endpoint.clone());
                }
                *state = None;
            }
        }

        let port = if self.config.port == 0 {
            free_localhost_port()?
        } else {
            self.config.port
        };
        let program = self.program();
        let args = self.server_args(port);
        let mut child = Command::new(&program)
            .args(&args)
            .kill_on_drop(true)
            .spawn()
            .map_err(|error| {
                SourceAcquisitionError::BrowserProcessUnavailable(format!(
                    "failed to start {}: {error}",
                    program.display()
                ))
            })?;

        let endpoint_url = format!("http://127.0.0.1:{port}");
        match self.wait_for_endpoint(port).await {
            Ok(endpoint) => {
                let mut state = self.state.lock().await;
                *state = Some(ObscuraProcess {
                    endpoint: endpoint.clone(),
                    child,
                });
                Ok(endpoint)
            }
            Err(error) => {
                let _ = child.start_kill();
                Err(error)
            }
        }
        .map_err(|error| match error {
            SourceAcquisitionError::BrowserProcessUnavailable(message) => {
                SourceAcquisitionError::BrowserProcessUnavailable(format!(
                    "{message}; endpoint={endpoint_url}"
                ))
            }
            other => other,
        })
    }

    pub(crate) fn program(&self) -> PathBuf {
        self.config
            .path
            .clone()
            .unwrap_or_else(|| PathBuf::from("obscura"))
    }

    pub(crate) fn server_args(&self, port: u16) -> Vec<String> {
        let mut args = vec![
            "serve".to_string(),
            "--host".to_string(),
            "127.0.0.1".to_string(),
            "--port".to_string(),
            port.to_string(),
        ];
        if self.config.stealth {
            args.push("--stealth".to_string());
        }
        args
    }

    async fn wait_for_endpoint(
        &self,
        port: u16,
    ) -> Result<BrowserEndpoint, SourceAcquisitionError> {
        let deadline = Instant::now() + Duration::from_millis(self.config.startup_timeout_ms);
        let url = format!("http://127.0.0.1:{port}");
        let version_url = format!("{url}/json/version");

        loop {
            match self.client.get(&version_url).send().await {
                Ok(response) if response.status().is_success() => {
                    let version = response.json::<VersionResponse>().await.ok();
                    let websocket_url = version.and_then(|version| version.web_socket_debugger_url);
                    return Ok(BrowserEndpoint {
                        port,
                        url,
                        websocket_url,
                    });
                }
                _ if Instant::now() >= deadline => {
                    return Err(SourceAcquisitionError::BrowserProcessUnavailable(format!(
                        "Obscura did not become healthy within {} ms",
                        self.config.startup_timeout_ms
                    )));
                }
                _ => tokio::time::sleep(Duration::from_millis(100)).await,
            }
        }
    }
}

fn candidate_config_paths(app: &AppHandle) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Ok(cwd) = std::env::current_dir() {
        paths.push(cwd.join("i0i.config.toml"));
    }
    if let Ok(config_dir) = app.path().app_config_dir() {
        paths.push(config_dir.join("i0i.config.toml"));
    }
    paths
}

fn candidate_obscura_paths(app: &AppHandle) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Ok(resource_dir) = app.path().resource_dir() {
        paths.push(resource_dir.join("obscura").join("obscura"));
        paths.push(resource_dir.join("resources/obscura/obscura"));
    }
    if let Ok(cwd) = std::env::current_dir() {
        paths.push(cwd.join("resources/obscura/obscura"));
        if let Some(repo_root) = cwd.parent() {
            paths.push(repo_root.join("src-tauri/resources/obscura/obscura"));
        }
    }
    paths
}

fn free_localhost_port() -> Result<u16, SourceAcquisitionError> {
    let listener = TcpListener::bind(("127.0.0.1", 0)).map_err(|error| {
        SourceAcquisitionError::BrowserProcessUnavailable(format!(
            "failed to reserve localhost port: {error}"
        ))
    })?;
    listener
        .local_addr()
        .map(|addr| addr.port())
        .map_err(|error| {
            SourceAcquisitionError::BrowserProcessUnavailable(format!(
                "failed to inspect reserved localhost port: {error}"
            ))
        })
}

fn to_process_error(error: std::io::Error) -> SourceAcquisitionError {
    SourceAcquisitionError::BrowserProcessUnavailable(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn milestone_1_builds_obscura_server_command_with_stealth() {
        let manager = ObscuraManager::new(ObscuraConfig {
            path: Some(PathBuf::from("/tmp/obscura")),
            stealth: true,
            ..ObscuraConfig::default()
        });

        assert_eq!(manager.program(), PathBuf::from("/tmp/obscura"));
        assert_eq!(
            manager.server_args(49152),
            vec![
                "serve",
                "--host",
                "127.0.0.1",
                "--port",
                "49152",
                "--stealth"
            ]
        );
    }

    #[test]
    fn milestone_1_can_allocate_localhost_port() {
        match free_localhost_port() {
            Ok(port) => assert!(port > 0),
            Err(SourceAcquisitionError::BrowserProcessUnavailable(message))
                if message.contains("Operation not permitted") => {}
            Err(error) => panic!("unexpected port allocation error: {error}"),
        }
    }
}
