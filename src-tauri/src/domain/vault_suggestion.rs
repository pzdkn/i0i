//! Durable recommendations for papers a vault is missing (RFC 0091).
//!
//! Suggestions belong to a vault, not to a user-created Discover search. A run
//! records the background lifecycle; each suggestion carries the normalized
//! discovery candidate needed to preview, open, or add the paper.

use serde::{Deserialize, Serialize};

use crate::domain::discovery::PaperCandidate;

/// Per-vault controls used to prepare and execute suggestion searches.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultSuggestionOptions {
    pub focus: Option<String>,
    pub year_from: Option<i32>,
    pub year_to: Option<i32>,
    pub query_path_count: u8,
    pub result_count: u8,
    pub include_reviews: bool,
}

impl Default for VaultSuggestionOptions {
    fn default() -> Self {
        Self {
            focus: None,
            year_from: None,
            year_to: None,
            query_path_count: 3,
            result_count: 5,
            include_reviews: false,
        }
    }
}

/// One LLM-proposed search angle awaiting the user's approval.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultSuggestionQueryPath {
    pub id: String,
    pub intent: String,
    pub query: String,
}

/// A transient, reviewable query proposal for one vault revision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultSuggestionQueryPlan {
    pub id: String,
    pub vault_id: String,
    pub vault_revision: String,
    pub queries: Vec<VaultSuggestionQueryPath>,
}

/// One actionable recommendation in a vault's Suggestions view.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultSuggestion {
    pub id: String,
    pub vault_id: String,
    pub run_id: String,
    pub paper_ref: String,
    pub candidate: PaperCandidate,
    pub reason: String,
    pub score: f64,
    pub state: String,
    pub created_at: String,
    pub updated_at: String,
}

/// Durable summary of one manual or scheduled suggestion run.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultSuggestionRun {
    pub id: String,
    pub vault_id: String,
    pub status: String,
    pub message: String,
    pub stop_reason: Option<String>,
    pub error: Option<String>,
    pub result_count: i32,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub created_at: String,
    pub options: VaultSuggestionOptions,
    pub query_paths: Vec<VaultSuggestionQueryPath>,
}

/// Snapshot read by the vault UI when it opens or a run completes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultSuggestionSnapshot {
    pub suggestions: Vec<VaultSuggestion>,
    pub latest_run: Option<VaultSuggestionRun>,
}

/// Structured, transient progress for the active Suggestions control room.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultSuggestionUpdated {
    pub vault_id: String,
    pub run_id: String,
    pub status: String,
    pub message: String,
    pub found: u32,
    pub sequence: u64,
    pub phase: String,
    pub query_path: Option<String>,
    pub label: String,
    pub detail: Option<String>,
}

/// Up to five provisional rows emitted before a run is persisted.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultSuggestionPreview {
    pub vault_id: String,
    pub run_id: String,
    pub suggestions: Vec<VaultSuggestion>,
}
