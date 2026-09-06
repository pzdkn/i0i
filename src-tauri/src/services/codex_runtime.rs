//! Managed Codex app-server process for native research runs.
//!
//! This module owns the child process and JSONL request correlation. It knows
//! nothing about papers or Research State; later orchestration supplies scoped
//! MCP configuration and interprets the resulting agent events.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::{broadcast, oneshot, Mutex};
use tokio::time::timeout;

use crate::services::settings;

const DEFAULT_EXECUTABLE: &str = "codex";
const DEFAULT_MODEL: &str = "gpt-5.6-terra";

/// Runtime settings read when a managed app-server is started.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodexRuntimeConfig {
    pub executable: PathBuf,
    pub model: String,
    pub startup_timeout: Duration,
    pub interrupt_grace: Duration,
}

impl CodexRuntimeConfig {
    /// Load runtime preferences from the shared settings store.
    pub fn load() -> Self {
        Self {
            executable: settings::preference("research.codex_executable")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from(DEFAULT_EXECUTABLE)),
            model: settings::preference("model.research_agent")
                .unwrap_or_else(|| DEFAULT_MODEL.to_string()),
            startup_timeout: duration_preference("research.codex_startup_seconds", 15),
            interrupt_grace: duration_preference("research.codex_interrupt_seconds", 5),
        }
    }
}

/// One app-server notification emitted after initialization.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodexEvent {
    pub method: String,
    pub params: Value,
}

/// Identifiers returned when a turn starts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodexTurn {
    pub thread_id: String,
    pub turn_id: String,
}

/// A ready Codex app-server using the supported stdio JSONL transport.
#[derive(Clone)]
pub struct CodexRuntime {
    inner: Arc<RuntimeInner>,
    config: CodexRuntimeConfig,
}

struct RuntimeInner {
    child: Mutex<Child>,
    stdin: Mutex<ChildStdin>,
    pending: Arc<Mutex<HashMap<u64, oneshot::Sender<Result<Value, String>>>>>,
    events: broadcast::Sender<CodexEvent>,
    next_request_id: AtomicU64,
}

impl CodexRuntime {
    /// Start and initialize Codex, or return a readable setup/protocol error.
    pub async fn start(config: CodexRuntimeConfig) -> Result<Self, String> {
        let mut child = Command::new(&config.executable)
            .args(["app-server", "--listen", "stdio://"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|error| runtime_start_error(&config.executable, error))?;
        let stdin = child.stdin.take().ok_or("Codex app-server stdin unavailable")?;
        let stdout = child.stdout.take().ok_or("Codex app-server stdout unavailable")?;
        let stderr = child.stderr.take().ok_or("Codex app-server stderr unavailable")?;
        let pending = Arc::new(Mutex::new(HashMap::new()));
        let (events, _) = broadcast::channel(256);

        spawn_protocol_reader(stdout, pending.clone(), events.clone());
        spawn_diagnostic_reader(stderr);

        let runtime = Self {
            inner: Arc::new(RuntimeInner {
                child: Mutex::new(child),
                stdin: Mutex::new(stdin),
                pending,
                events,
                next_request_id: AtomicU64::new(1),
            }),
            config,
        };

        timeout(runtime.config.startup_timeout, runtime.initialize())
            .await
            .map_err(|_| "Codex app-server initialization timed out".to_string())??;
        Ok(runtime)
    }

    /// Subscribe to structured app-server notifications.
    pub fn subscribe(&self) -> broadcast::Receiver<CodexEvent> {
        self.inner.events.subscribe()
    }

    /// Start an ephemeral thread with caller-supplied scoped configuration.
    pub async fn start_thread(
        &self,
        cwd: &Path,
        developer_instructions: &str,
        config: Value,
    ) -> Result<String, String> {
        let result = self
            .request(
                "thread/start",
                json!({
                    "cwd": cwd,
                    "model": self.config.model,
                    "developerInstructions": developer_instructions,
                    "config": config,
                    "ephemeral": true,
                    "approvalPolicy": "never"
                }),
            )
            .await?;
        string_at(&result, &["thread", "id"], "thread/start response")
    }

    /// Start a text turn and return its stable app-server identifiers.
    pub async fn start_turn(&self, thread_id: &str, input: &str) -> Result<CodexTurn, String> {
        let result = self
            .request(
                "turn/start",
                json!({
                    "threadId": thread_id,
                    "input": [{"type": "text", "text": input}]
                }),
            )
            .await?;
        Ok(CodexTurn {
            thread_id: thread_id.to_string(),
            turn_id: string_at(&result, &["turn", "id"], "turn/start response")?,
        })
    }

    /// Ask Codex to interrupt one active turn.
    pub async fn interrupt(&self, turn: &CodexTurn) -> Result<(), String> {
        timeout(
            self.config.interrupt_grace,
            self.request(
                "turn/interrupt",
                json!({"threadId": turn.thread_id, "turnId": turn.turn_id}),
            ),
        )
        .await
        .map_err(|_| "Codex turn interruption timed out".to_string())??;
        Ok(())
    }

    /// Stop the managed child and wait for it to exit.
    pub async fn shutdown(&self) -> Result<(), String> {
        let mut child = self.inner.child.lock().await;
        if child.try_wait().map_err(|error| error.to_string())?.is_some() {
            return Ok(());
        }
        child.kill().await.map_err(|error| error.to_string())?;
        child.wait().await.map(|_| ()).map_err(|error| error.to_string())
    }

    async fn initialize(&self) -> Result<(), String> {
        self.request(
            "initialize",
            json!({
                "clientInfo": {
                    "name": "i0i",
                    "title": "i0i Research",
                    "version": env!("CARGO_PKG_VERSION")
                },
                "capabilities": {"experimentalApi": false}
            }),
        )
        .await?;
        self.notify("initialized", json!({})).await
    }

    async fn request(&self, method: &str, params: Value) -> Result<Value, String> {
        let id = self.inner.next_request_id.fetch_add(1, Ordering::Relaxed);
        let (sender, receiver) = oneshot::channel();
        self.inner.pending.lock().await.insert(id, sender);
        if let Err(error) = self.write_message(json!({"id": id, "method": method, "params": params})).await {
            self.inner.pending.lock().await.remove(&id);
            return Err(error);
        }
        receiver
            .await
            .map_err(|_| format!("Codex app-server closed while waiting for {method}"))?
    }

    async fn notify(&self, method: &str, params: Value) -> Result<(), String> {
        self.write_message(json!({"method": method, "params": params})).await
    }

    async fn write_message(&self, message: Value) -> Result<(), String> {
        let mut stdin = self.inner.stdin.lock().await;
        let mut encoded = serde_json::to_vec(&message).map_err(|error| error.to_string())?;
        encoded.push(b'\n');
        stdin.write_all(&encoded).await.map_err(|error| error.to_string())?;
        stdin.flush().await.map_err(|error| error.to_string())
    }
}

fn spawn_protocol_reader(
    stdout: tokio::process::ChildStdout,
    pending: Arc<Mutex<HashMap<u64, oneshot::Sender<Result<Value, String>>>>>,
    events: broadcast::Sender<CodexEvent>,
) {
    tokio::spawn(async move {
        let mut lines = BufReader::new(stdout).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            match parse_protocol_line(&line) {
                Ok(ProtocolMessage::Response { id, result }) => {
                    if let Some(sender) = pending.lock().await.remove(&id) {
                        let _ = sender.send(result);
                    }
                }
                Ok(ProtocolMessage::Event(event)) => {
                    let _ = events.send(event);
                }
                Err(error) => eprintln!("[codex-runtime] invalid protocol message: {error}"),
            }
        }
        for (_, sender) in pending.lock().await.drain() {
            let _ = sender.send(Err("Codex app-server closed its output".to_string()));
        }
    });
}

fn spawn_diagnostic_reader(stderr: tokio::process::ChildStderr) {
    tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            eprintln!("[codex-runtime] {line}");
        }
    });
}

enum ProtocolMessage {
    Response { id: u64, result: Result<Value, String> },
    Event(CodexEvent),
}

fn parse_protocol_line(line: &str) -> Result<ProtocolMessage, String> {
    let value: Value = serde_json::from_str(line).map_err(|error| error.to_string())?;
    if let Some(id) = value.get("id").and_then(Value::as_u64) {
        let result = match value.get("error") {
            Some(error) => Err(error
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("Codex request failed")
                .to_string()),
            None => Ok(value.get("result").cloned().unwrap_or(Value::Null)),
        };
        return Ok(ProtocolMessage::Response { id, result });
    }
    let method = value
        .get("method")
        .and_then(Value::as_str)
        .ok_or("Protocol message has neither numeric id nor method")?;
    Ok(ProtocolMessage::Event(CodexEvent {
        method: method.to_string(),
        params: value.get("params").cloned().unwrap_or(Value::Null),
    }))
}

fn string_at(value: &Value, path: &[&str], context: &str) -> Result<String, String> {
    path.iter()
        .try_fold(value, |current, key| current.get(*key).ok_or(()))
        .ok()
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| format!("Invalid {context}: missing {}", path.join(".")))
}

fn runtime_start_error(executable: &Path, error: std::io::Error) -> String {
    format!(
        "Could not start Codex at {}: {error}. Install Codex or set research.codex_executable.",
        executable.display()
    )
}

fn duration_preference(key: &str, default_seconds: u64) -> Duration {
    Duration::from_secs(
        settings::preference(key)
            .and_then(|value| value.parse::<u64>().ok())
            .filter(|value| *value > 0)
            .unwrap_or(default_seconds),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_success_response() {
        let message = parse_protocol_line(r#"{"id":7,"result":{"thread":{"id":"t1"}}}"#)
            .expect("parse response");
        let ProtocolMessage::Response { id, result } = message else {
            panic!("expected response");
        };
        assert_eq!(id, 7);
        assert_eq!(string_at(&result.unwrap(), &["thread", "id"], "thread"), Ok("t1".into()));
    }

    #[test]
    fn parses_error_without_exposing_unknown_shape() {
        let message = parse_protocol_line(
            r#"{"id":2,"error":{"code":-1,"message":"authentication required"}}"#,
        )
        .expect("parse response");
        let ProtocolMessage::Response { result, .. } = message else {
            panic!("expected response");
        };
        assert_eq!(result.unwrap_err(), "authentication required");
    }

    #[test]
    fn parses_notification() {
        let message = parse_protocol_line(
            r#"{"method":"turn/completed","params":{"turn":{"id":"turn-1"}}}"#,
        )
        .expect("parse event");
        let ProtocolMessage::Event(event) = message else {
            panic!("expected event");
        };
        assert_eq!(event.method, "turn/completed");
        assert_eq!(event.params["turn"]["id"], "turn-1");
    }

    #[test]
    fn start_error_is_actionable() {
        let error = runtime_start_error(
            Path::new("/missing/codex"),
            std::io::Error::new(std::io::ErrorKind::NotFound, "missing"),
        );
        assert!(error.contains("research.codex_executable"));
        assert!(error.contains("/missing/codex"));
    }

    #[tokio::test]
    #[ignore = "explicit live compatibility check; starts installed Codex"]
    async fn live_runtime_initializes_and_shuts_down() {
        let runtime = CodexRuntime::start(CodexRuntimeConfig::load())
            .await
            .expect("initialize installed Codex app-server");
        runtime.shutdown().await.expect("stop Codex app-server");
    }
}
