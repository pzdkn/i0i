/// Lightweight discovery error.
///
/// Keep this boring until we actually need typed variants. For now a clear
/// message is enough for the Tauri command to turn into `Result<_, String>`.
use std::{error::Error, fmt};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveryError {
    pub message: String,
}

impl DiscoveryError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl From<String> for DiscoveryError {
    fn from(message: String) -> Self {
        Self::new(message)
    }
}

impl From<&str> for DiscoveryError {
    fn from(message: &str) -> Self {
        Self::new(message)
    }
}

impl fmt::Display for DiscoveryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for DiscoveryError {}
