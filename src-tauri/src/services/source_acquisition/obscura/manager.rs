use std::fs;
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::Deserialize;
use tauri::{AppHandle, Emitter, Manager};
use tokio::io::AsyncReadExt;
use tokio::process::{Child, Command};
use tokio::sync::Mutex;

use crate::services::source_acquisition::types::{
    BrowserEndpoint, BrowserRuntimeState, BrowserRuntimeStatus, ObscuraConfig,
    SourceAcquisitionError,
};

pub const BROWSER_STATUS_EVENT: &str = "browser_runtime_status";
const STDERR_TAIL_LIMIT: usize = 8 * 1024;

#[derive(Clone)]
pub struct ObscuraManager {
    config: ObscuraConfig,
    state: Arc<Mutex<Option<ObscuraProcess>>>,
    startup: Arc<Mutex<()>>,
    status: Arc<std::sync::RwLock<BrowserRuntimeStatus>>,
    app: Option<AppHandle>,
    client: reqwest::Client,
}

struct ObscuraProcess {
    endpoint: BrowserEndpoint,
    child: Child,
    stderr_tail: Arc<std::sync::Mutex<Vec<u8>>>,
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
        Self::with_optional_app(config, None)
    }

    pub fn with_app(config: ObscuraConfig, app: AppHandle) -> Self {
        Self::with_optional_app(config, Some(app))
    }

    fn with_optional_app(config: ObscuraConfig, app: Option<AppHandle>) -> Self {
        Self {
            config,
            state: Arc::new(Mutex::new(None)),
            startup: Arc::new(Mutex::new(())),
            status: Arc::new(std::sync::RwLock::new(BrowserRuntimeStatus {
                state: BrowserRuntimeState::Stopped,
                message: None,
            })),
            app,
            client: reqwest::Client::new(),
        }
    }

    pub fn config(&self) -> &ObscuraConfig {
        &self.config
    }

    pub fn status(&self) -> BrowserRuntimeStatus {
        self.status.read().expect("browser status lock").clone()
    }

    pub async fn ensure_ready(&self) -> Result<BrowserEndpoint, SourceAcquisitionError> {
        if !self.config.enabled {
            let error = SourceAcquisitionError::BrowserProcessUnavailable(
                "Obscura is disabled".to_string(),
            );
            self.publish_status(BrowserRuntimeStatus::failed(
                "Browser-backed features are disabled.",
            ));
            return Err(error);
        }

        if let Some(endpoint) = self.ready_endpoint().await? {
            return Ok(endpoint);
        }

        // Only one caller may cross the stopped -> starting boundary. Chat,
        // Quick Search, and concurrent Deep Research lanes all wait for this
        // same attempt, then recheck the process before doing any work.
        let _startup = self.startup.lock().await;
        if let Some(endpoint) = self.ready_endpoint().await? {
            return Ok(endpoint);
        }

        self.publish_status(BrowserRuntimeStatus::starting());
        let port = if self.config.port == 0 {
            match free_localhost_port() {
                Ok(port) => port,
                Err(error) => {
                    self.report_startup_failure(&error);
                    return Err(error);
                }
            }
        } else {
            self.config.port
        };
        let program = self.program();
        let args = self.server_args(port);
        crate::shared::log::info(
            "obscura",
            format!(
                "starting executable={} port={} timeout_ms={}",
                program.display(),
                port,
                self.config.startup_timeout_ms
            ),
        );
        let mut child = match Command::new(&program)
            .args(&args)
            .kill_on_drop(true)
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| {
                SourceAcquisitionError::BrowserProcessUnavailable(format!(
                    "failed to start {}: {error}",
                    program.display()
                ))
            }) {
            Ok(child) => child,
            Err(error) => {
                self.report_startup_failure(&error);
                return Err(error);
            }
        };
        let stderr_tail = capture_stderr(child.stderr.take());

        let endpoint_url = format!("http://127.0.0.1:{port}");
        let started = Instant::now();
        let result = match self.wait_for_endpoint(port, &mut child, &stderr_tail).await {
            Ok(endpoint) => {
                let mut state = self.state.lock().await;
                *state = Some(ObscuraProcess {
                    endpoint: endpoint.clone(),
                    child,
                    stderr_tail,
                });
                crate::shared::log::info(
                    "obscura",
                    format!(
                        "ready port={} elapsed_ms={}",
                        endpoint.port,
                        started.elapsed().as_millis()
                    ),
                );
                self.publish_status(BrowserRuntimeStatus::ready());
                Ok(endpoint)
            }
            Err(error) => {
                let _ = child.start_kill();
                self.report_startup_failure(&error);
                Err(error)
            }
        };
        result.map_err(|error| match error {
            SourceAcquisitionError::BrowserProcessUnavailable(message) => {
                SourceAcquisitionError::BrowserProcessUnavailable(format!(
                    "{message}; executable={}; endpoint={endpoint_url}",
                    program.display()
                ))
            }
            other => other,
        })
    }

    async fn ready_endpoint(&self) -> Result<Option<BrowserEndpoint>, SourceAcquisitionError> {
        let mut state = self.state.lock().await;
        let Some(process) = state.as_mut() else {
            return Ok(None);
        };
        match process.child.try_wait().map_err(to_process_error)? {
            None => {
                self.publish_status(BrowserRuntimeStatus::ready());
                Ok(Some(process.endpoint.clone()))
            }
            Some(exit) => {
                let stderr = stderr_tail(&process.stderr_tail);
                crate::shared::log::warn(
                    "obscura",
                    format!("managed process exited status={exit} stderr={stderr}"),
                );
                *state = None;
                Ok(None)
            }
        }
    }

    fn publish_status(&self, status: BrowserRuntimeStatus) {
        *self.status.write().expect("browser status lock") = status.clone();
        if let Some(app) = &self.app {
            let _ = app.emit(BROWSER_STATUS_EVENT, status);
        }
    }

    fn report_startup_failure(&self, error: &SourceAcquisitionError) {
        crate::shared::log::error("obscura", error.to_string());
        self.publish_status(BrowserRuntimeStatus::failed(
            "Browser could not start. Retry when you are ready.",
        ));
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
        child: &mut Child,
        stderr: &Arc<std::sync::Mutex<Vec<u8>>>,
    ) -> Result<BrowserEndpoint, SourceAcquisitionError> {
        let deadline = Instant::now() + Duration::from_millis(self.config.startup_timeout_ms);
        let url = format!("http://127.0.0.1:{port}");
        let version_url = format!("{url}/json/version");

        loop {
            if let Some(exit) = child.try_wait().map_err(to_process_error)? {
                // Let the independent stderr drain observe EOF before taking
                // its bounded snapshot for the diagnostic.
                tokio::time::sleep(Duration::from_millis(10)).await;
                return Err(SourceAcquisitionError::BrowserProcessUnavailable(format!(
                    "Obscura exited before becoming healthy: status={exit}; stderr={}",
                    stderr_tail(stderr)
                )));
            }
            match self.client.get(&version_url).send().await {
                Ok(response) if response.status().is_success() => {
                    let version = response.json::<VersionResponse>().await.ok();
                    let websocket_url = version.and_then(|version| version.web_socket_debugger_url);
                    if websocket_url.is_none() {
                        return Err(SourceAcquisitionError::BrowserProcessUnavailable(
                            "Obscura health response lacked a CDP WebSocket URL".to_string(),
                        ));
                    }
                    return Ok(BrowserEndpoint {
                        port,
                        url,
                        websocket_url,
                    });
                }
                _ if Instant::now() >= deadline => {
                    return Err(SourceAcquisitionError::BrowserProcessUnavailable(format!(
                        "Obscura did not become healthy within {} ms; stderr={}",
                        self.config.startup_timeout_ms,
                        stderr_tail(stderr)
                    )));
                }
                _ => tokio::time::sleep(Duration::from_millis(100)).await,
            }
        }
    }
}

fn capture_stderr(stderr: Option<tokio::process::ChildStderr>) -> Arc<std::sync::Mutex<Vec<u8>>> {
    let tail = Arc::new(std::sync::Mutex::new(Vec::new()));
    let Some(mut stderr) = stderr else {
        return tail;
    };
    let output = tail.clone();
    tauri::async_runtime::spawn(async move {
        let mut chunk = [0_u8; 1024];
        loop {
            let Ok(read) = stderr.read(&mut chunk).await else {
                break;
            };
            if read == 0 {
                break;
            }
            let mut bytes = output.lock().expect("browser stderr lock");
            bytes.extend_from_slice(&chunk[..read]);
            if bytes.len() > STDERR_TAIL_LIMIT {
                let excess = bytes.len() - STDERR_TAIL_LIMIT;
                bytes.drain(..excess);
            }
        }
    });
    tail
}

fn stderr_tail(tail: &Arc<std::sync::Mutex<Vec<u8>>>) -> String {
    let bytes = tail.lock().expect("browser stderr lock");
    let value = String::from_utf8_lossy(&bytes).trim().to_string();
    if value.is_empty() {
        "<empty>".to_string()
    } else {
        value
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
    use std::collections::HashSet;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    use futures_util::future::join_all;

    use super::*;

    static PROCESS_TEST_LOCK: Mutex<()> = Mutex::const_new(());

    #[cfg(unix)]
    fn test_script(body: &str) -> PathBuf {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("i0i-obscura-test-{unique}.sh"));
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).expect("write script");
        let mut permissions = std::fs::metadata(&path)
            .expect("script metadata")
            .permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&path, permissions).expect("make script executable");
        path
    }

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

    #[tokio::test]
    async fn concurrent_first_callers_share_one_startup_attempt() {
        let _process_test = PROCESS_TEST_LOCK.lock().await;
        let binary = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("resources")
            .join("obscura")
            .join("obscura");
        if !binary.exists() {
            return;
        }
        let manager = ObscuraManager::new(ObscuraConfig {
            path: Some(binary),
            startup_timeout_ms: 20_000,
            ..ObscuraConfig::default()
        });

        let starts = (0..10).map(|_| {
            let manager = manager.clone();
            async move { manager.ensure_ready().await.expect("shared startup") }
        });
        let endpoints = join_all(starts).await;
        let ports = endpoints
            .iter()
            .map(|endpoint| endpoint.port)
            .collect::<HashSet<_>>();

        assert_eq!(ports.len(), 1, "every caller must reuse one process");
        assert_eq!(manager.status(), BrowserRuntimeStatus::ready());
    }

    #[tokio::test]
    async fn a_dead_managed_process_is_restarted_on_the_next_request() {
        let _process_test = PROCESS_TEST_LOCK.lock().await;
        let binary = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("resources")
            .join("obscura")
            .join("obscura");
        if !binary.exists() {
            return;
        }
        let manager = ObscuraManager::new(ObscuraConfig {
            path: Some(binary),
            startup_timeout_ms: 20_000,
            ..ObscuraConfig::default()
        });

        manager.ensure_ready().await.expect("initial startup");
        let first_process_id = {
            let mut state = manager.state.lock().await;
            let process = state.as_mut().expect("managed process");
            let process_id = process.child.id();
            process.child.kill().await.expect("stop managed process");
            process_id
        };

        manager.ensure_ready().await.expect("restart after exit");
        let second_process_id = manager
            .state
            .lock()
            .await
            .as_ref()
            .and_then(|process| process.child.id());

        assert_ne!(first_process_id, second_process_id);
        assert_eq!(manager.status(), BrowserRuntimeStatus::ready());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn early_process_exit_reports_status_and_stderr_without_waiting_for_timeout() {
        let _process_test = PROCESS_TEST_LOCK.lock().await;
        let script = test_script("echo startup exploded >&2\nexit 23");
        let manager = ObscuraManager::new(ObscuraConfig {
            path: Some(script.clone()),
            startup_timeout_ms: 5_000,
            ..ObscuraConfig::default()
        });
        let started = Instant::now();

        let error = manager.ensure_ready().await.expect_err("startup must fail");
        let message = error.to_string();

        assert!(started.elapsed() < Duration::from_secs(2));
        assert!(message.contains("status=exit status: 23"), "{message}");
        assert!(message.contains("startup exploded"), "{message}");
        assert_eq!(manager.status().state, BrowserRuntimeState::Failed);
        let _ = std::fs::remove_file(script);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn timeout_reports_duration_executable_and_endpoint() {
        let _process_test = PROCESS_TEST_LOCK.lock().await;
        let script = test_script("exec sleep 5");
        let manager = ObscuraManager::new(ObscuraConfig {
            path: Some(script.clone()),
            startup_timeout_ms: 50,
            ..ObscuraConfig::default()
        });

        let message = manager
            .ensure_ready()
            .await
            .expect_err("startup must time out")
            .to_string();

        assert!(message.contains("within 50 ms"), "{message}");
        assert!(message.contains(&format!("executable={}", script.display())));
        assert!(message.contains("endpoint=http://127.0.0.1:"));
        assert_eq!(manager.status().state, BrowserRuntimeState::Failed);
        let _ = std::fs::remove_file(script);
    }
}
