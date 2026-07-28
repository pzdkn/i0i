//! Lightweight leveled logging (Python-style levels).
//!
//! Level threshold comes from the `I0I_LOG` env var (`error` < `warn` < `info`
//! < `debug` < `trace`), defaulting to `info`. So `I0I_LOG=debug npx tauri dev`
//! turns on the debug firehose; the default keeps the terminal readable.
//!
//! Output format: `[LEVEL <millis>] scope: message`, matching the existing
//! `[chat …]`-style lines. Frontend logs route here through the `debug_log`
//! command, so client-side detail lands in the same terminal at the same level.

use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Level {
    Error = 0,
    Warn = 1,
    Info = 2,
    Debug = 3,
    Trace = 4,
}

impl Level {
    fn tag(self) -> &'static str {
        match self {
            Level::Error => "ERROR",
            Level::Warn => "WARN",
            Level::Info => "INFO",
            Level::Debug => "DEBUG",
            Level::Trace => "TRACE",
        }
    }

    /// Parse a level name (case-insensitive); unknown/empty → `Info`.
    pub fn parse(name: &str) -> Level {
        match name.trim().to_ascii_lowercase().as_str() {
            "trace" => Level::Trace,
            "debug" => Level::Debug,
            "warn" | "warning" => Level::Warn,
            "error" => Level::Error,
            _ => Level::Info,
        }
    }
}

fn threshold() -> Level {
    static THRESHOLD: OnceLock<Level> = OnceLock::new();
    *THRESHOLD.get_or_init(|| {
        std::env::var("I0I_LOG")
            .map(|value| Level::parse(&value))
            .unwrap_or(Level::Info)
    })
}

/// Whether a message at `level` would be emitted (cheap guard for hot paths
/// that would otherwise format an expensive message).
pub fn enabled(level: Level) -> bool {
    level <= threshold()
}

/// Emit a log line if `level` is within the configured threshold.
pub fn log(level: Level, scope: &str, message: &str) {
    if !enabled(level) {
        return;
    }
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    eprintln!("[{} {millis}] {scope}: {message}", level.tag());
}

pub fn error(scope: &str, message: impl AsRef<str>) {
    log(Level::Error, scope, message.as_ref());
}
pub fn warn(scope: &str, message: impl AsRef<str>) {
    log(Level::Warn, scope, message.as_ref());
}
pub fn info(scope: &str, message: impl AsRef<str>) {
    log(Level::Info, scope, message.as_ref());
}
pub fn debug(scope: &str, message: impl AsRef<str>) {
    log(Level::Debug, scope, message.as_ref());
}
#[allow(dead_code)] // reserved level; kept for completeness
pub fn trace(scope: &str, message: impl AsRef<str>) {
    log(Level::Trace, scope, message.as_ref());
}
