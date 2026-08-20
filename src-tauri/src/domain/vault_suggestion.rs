//! Durable recommendations for papers a vault is missing (RFC 0091).
//!
//! Suggestions belong to a vault, not to a user-created Discover search. A run
//! records the background lifecycle; each suggestion carries the normalized
//! discovery candidate needed to preview, open, or add the paper.

use serde::{Deserialize, Serialize};

use crate::domain::discovery::PaperCandidate;

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
}

/// Snapshot read by the vault UI when it opens or a run completes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultSuggestionSnapshot {
    pub suggestions: Vec<VaultSuggestion>,
    pub latest_run: Option<VaultSuggestionRun>,
}

/// Coarse progress event. Detailed research rounds remain transient.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultSuggestionUpdated {
    pub vault_id: String,
    pub run_id: String,
    pub status: String,
    pub message: String,
    pub found: u32,
}

/// Up to five provisional rows emitted before a run is persisted.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultSuggestionPreview {
    pub vault_id: String,
    pub run_id: String,
    pub suggestions: Vec<VaultSuggestion>,
}
