//! Bounded operational reflections and reviewable Harness improvements.

use serde::{Deserialize, Serialize};

pub const REFLECTION_POLICY_VERSION: &str = "harness-reflection-v1";
pub const PROPOSAL_POLICY_VERSION: &str = "harness-proposal-v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HarnessObservationKind {
    QueryQuality,
    IrrelevantResultClass,
    SourceFailure,
    Terminology,
    CoverageBias,
    RelevanceError,
    ScopeDrift,
    WastedWork,
}

impl HarnessObservationKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::QueryQuality => "query_quality",
            Self::IrrelevantResultClass => "irrelevant_result_class",
            Self::SourceFailure => "source_failure",
            Self::Terminology => "terminology",
            Self::CoverageBias => "coverage_bias",
            Self::RelevanceError => "relevance_error",
            Self::ScopeDrift => "scope_drift",
            Self::WastedWork => "wasted_work",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "query_quality" => Ok(Self::QueryQuality),
            "irrelevant_result_class" => Ok(Self::IrrelevantResultClass),
            "source_failure" => Ok(Self::SourceFailure),
            "terminology" => Ok(Self::Terminology),
            "coverage_bias" => Ok(Self::CoverageBias),
            "relevance_error" => Ok(Self::RelevanceError),
            "scope_drift" => Ok(Self::ScopeDrift),
            "wasted_work" => Ok(Self::WastedWork),
            _ => Err(format!("Unknown Harness observation kind: {value}")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HarnessImprovementTarget {
    PreferredConcepts,
    ExcludedConcepts,
    MetadataResolvers,
}

impl HarnessImprovementTarget {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::PreferredConcepts => "preferred_concepts",
            Self::ExcludedConcepts => "excluded_concepts",
            Self::MetadataResolvers => "metadata_resolvers",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "preferred_concepts" => Ok(Self::PreferredConcepts),
            "excluded_concepts" => Ok(Self::ExcludedConcepts),
            "metadata_resolvers" => Ok(Self::MetadataResolvers),
            _ => Err(format!(
                "Harness improvement target is not allowed: {value}"
            )),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "items", rename_all = "snake_case")]
pub enum HarnessImprovementValue {
    Concepts(Vec<String>),
    MetadataResolvers(Vec<String>),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HarnessObservationDraft {
    pub kind: HarnessObservationKind,
    pub signature: String,
    pub severity: f64,
    pub confidence: f64,
    pub description: String,
    pub metrics_json: String,
    pub target: Option<HarnessImprovementTarget>,
    pub proposed_value: Option<HarnessImprovementValue>,
    pub proposal_eligible: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HarnessReflectionDraft {
    pub summary: String,
    pub next_direction: Option<String>,
    pub metrics_json: String,
    pub observations: Vec<HarnessObservationDraft>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HarnessReflection {
    pub id: String,
    pub run_id: String,
    pub project_id: String,
    pub policy_version: String,
    pub configuration_version: i64,
    pub summary: String,
    pub next_direction: Option<String>,
    pub metrics_json: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HarnessObservation {
    pub id: String,
    pub reflection_id: String,
    pub run_id: String,
    pub project_id: String,
    pub kind: HarnessObservationKind,
    pub signature: String,
    pub severity: f64,
    pub confidence: f64,
    pub description: String,
    pub metrics_json: String,
    pub target: Option<HarnessImprovementTarget>,
    pub proposed_value: Option<HarnessImprovementValue>,
    pub proposal_eligible: bool,
    pub created_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HarnessImprovementStatus {
    Proposed,
    Accepted,
    Rejected,
    Superseded,
}

impl HarnessImprovementStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Proposed => "proposed",
            Self::Accepted => "accepted",
            Self::Rejected => "rejected",
            Self::Superseded => "superseded",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "proposed" => Ok(Self::Proposed),
            "accepted" => Ok(Self::Accepted),
            "rejected" => Ok(Self::Rejected),
            "superseded" => Ok(Self::Superseded),
            _ => Err(format!("Unknown Harness improvement status: {value}")),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HarnessImprovement {
    pub id: String,
    pub project_id: String,
    pub status: HarnessImprovementStatus,
    pub target: HarnessImprovementTarget,
    pub base_configuration_version: i64,
    pub before_value: HarnessImprovementValue,
    pub proposed_value: HarnessImprovementValue,
    pub rationale: String,
    pub expected_effect: String,
    pub policy_version: String,
    pub decision_actor: Option<String>,
    pub decision_reason: Option<String>,
    pub resulting_configuration_version: Option<i64>,
    pub run_ids: Vec<String>,
    pub observation_ids: Vec<String>,
    pub observations: Vec<HarnessObservation>,
    pub created_at: String,
    pub decided_at: Option<String>,
}
