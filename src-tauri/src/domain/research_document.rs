//! Durable Project-document generation from pinned Research State.

use serde::{Deserialize, Serialize};

/// App-owned epistemic and citation policy recorded on every generation.
pub const DOCUMENT_GENERATION_POLICY_VERSION: &str = "research-document-v1";

/// A transparent initial structure for an ordinary Markdown Project document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResearchDocumentShape {
    Survey,
    RelatedWork,
    ResearchGapAnalysis,
    HypothesisReport,
    ExperimentPlan,
    Custom,
}

impl ResearchDocumentShape {
    /// Returns the stable storage and bridge value.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Survey => "survey",
            Self::RelatedWork => "related_work",
            Self::ResearchGapAnalysis => "research_gap_analysis",
            Self::HypothesisReport => "hypothesis_report",
            Self::ExperimentPlan => "experiment_plan",
            Self::Custom => "custom",
        }
    }

    /// Parses a stable storage and bridge value.
    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "survey" => Ok(Self::Survey),
            "related_work" => Ok(Self::RelatedWork),
            "research_gap_analysis" => Ok(Self::ResearchGapAnalysis),
            "hypothesis_report" => Ok(Self::HypothesisReport),
            "experiment_plan" => Ok(Self::ExperimentPlan),
            "custom" => Ok(Self::Custom),
            _ => Err(format!("Unknown research document shape: {value}")),
        }
    }
}

/// Immutable researcher-selected inputs for one generation attempt.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateFromResearchRequest {
    pub project_id: String,
    pub state_revision: i64,
    pub selected_entry_ids: Vec<String>,
    pub shape: ResearchDocumentShape,
    pub title: String,
    pub custom_instruction: Option<String>,
    pub originating_run_id: Option<String>,
    #[serde(default)]
    pub include_non_active: bool,
}

/// Durable lifecycle and provenance for one generation attempt.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResearchDocumentGeneration {
    pub id: String,
    pub project_id: String,
    pub status: String,
    pub shape: ResearchDocumentShape,
    pub title: String,
    pub custom_instruction: Option<String>,
    pub state_revision: i64,
    pub selected_entry_ids: Vec<String>,
    pub include_non_active: bool,
    pub originating_run_id: Option<String>,
    pub policy_version: String,
    pub model_identifier: String,
    pub resulting_document_id: Option<String>,
    pub retry_of_id: Option<String>,
    pub error: Option<String>,
    pub cancellation_requested: bool,
    pub input_entry_count: i64,
    pub citation_count: i64,
    pub created_at: String,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
}

/// A document-local citation mapped to canonical evidence and metadata snapshots.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectDocumentCitation {
    pub document_id: String,
    pub citation_key: String,
    pub paper_id: String,
    pub evidence_link_ids: Vec<String>,
    pub title_snapshot: String,
    pub authors_snapshot: Vec<String>,
    pub year_snapshot: i32,
    pub created_at: String,
}
