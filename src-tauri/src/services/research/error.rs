//! Error type for the research service.

use std::fmt;

#[derive(Debug, Clone)]
pub struct ResearchError(pub String);

impl ResearchError {
    pub fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for ResearchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for ResearchError {}

impl From<String> for ResearchError {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl From<ResearchError> for String {
    fn from(value: ResearchError) -> Self {
        value.0
    }
}
