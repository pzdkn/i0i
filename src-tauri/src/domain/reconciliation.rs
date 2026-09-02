//! Validated, immutable changes proposed by one completed Research Harness Run.

use serde::{Deserialize, Serialize};

use crate::domain::harness_improvement::{
    HarnessImprovementTarget, HarnessImprovementValue, HarnessObservationKind,
};
use crate::domain::research::SearchCandidate;
use crate::domain::research_state::{EntryRelationKind, EpistemicStatus, ResearchEntryKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CandidateDecisionKind {
    Accept,
    Reject,
}

impl CandidateDecisionKind {
    /// Returns the stable persisted/activity label.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Accept => "accept",
            Self::Reject => "reject",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CandidateDecision {
    pub candidate_id: String,
    pub decision: CandidateDecisionKind,
    pub reason: String,
    pub relevance_confidence: f64,
    pub within_scope: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlannedEvidence {
    pub candidate_id: String,
    pub excerpt: String,
    pub support_note: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlannedRelation {
    /// Existing Research Entry id or another planned entry's handle.
    pub target: String,
    pub kind: EntryRelationKind,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlannedResearchEntry {
    /// Run-local stable identifier used by relationships.
    pub handle: String,
    pub kind: ResearchEntryKind,
    pub epistemic_status: EpistemicStatus,
    pub text: String,
    #[serde(default)]
    pub evidence: Vec<PlannedEvidence>,
    #[serde(default)]
    pub relations: Vec<PlannedRelation>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlannedHarnessObservation {
    pub kind: HarnessObservationKind,
    pub signature: String,
    pub severity: f64,
    pub confidence: f64,
    pub description: String,
    pub target: Option<HarnessImprovementTarget>,
    pub proposed_value: Option<HarnessImprovementValue>,
    pub proposal_eligible: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlannedHarnessReflection {
    pub summary: String,
    pub next_direction: Option<String>,
    #[serde(default)]
    pub observations: Vec<PlannedHarnessObservation>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunReconciliationPlan {
    pub candidate_decisions: Vec<CandidateDecision>,
    pub entries: Vec<PlannedResearchEntry>,
    pub next_direction: String,
    #[serde(default)]
    pub operational_reflection: Option<PlannedHarnessReflection>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReconciliationTelemetry {
    pub executed_queries: Vec<String>,
    pub provider_completions: u32,
    pub provider_failures: u32,
    pub provider_queries: u32,
    pub llm_calls: u32,
    pub iterations: u32,
    pub inspected_candidates: u32,
    pub stop_reason: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HarnessChangeSetStatus {
    Proposed,
    Applied,
    Rejected,
    Superseded,
    Failed,
}

impl HarnessChangeSetStatus {
    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "proposed" => Ok(Self::Proposed),
            "applied" => Ok(Self::Applied),
            "rejected" => Ok(Self::Rejected),
            "superseded" => Ok(Self::Superseded),
            "failed" => Ok(Self::Failed),
            _ => Err(format!("Unknown Harness Change Set status: {value}")),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HarnessChangeSet {
    pub id: String,
    pub run_id: String,
    pub project_id: String,
    pub starting_state_revision: i64,
    pub status: HarnessChangeSetStatus,
    pub plan: Option<RunReconciliationPlan>,
    pub considered_candidates: Vec<SearchCandidate>,
    pub error: Option<String>,
    pub decision_reason: Option<String>,
    pub resulting_state_revision: Option<i64>,
    pub created_at: String,
    pub decided_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HarnessChangeSetPatch {
    pub plan: RunReconciliationPlan,
}
