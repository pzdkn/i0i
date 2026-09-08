//! Revisioned Project Research State with explicit epistemic provenance.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResearchEntryKind {
    Finding,
    Question,
    Gap,
    Hypothesis,
    ExperimentIdea,
}

impl ResearchEntryKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Finding => "finding",
            Self::Question => "question",
            Self::Gap => "gap",
            Self::Hypothesis => "hypothesis",
            Self::ExperimentIdea => "experiment_idea",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "finding" => Ok(Self::Finding),
            "question" => Ok(Self::Question),
            "gap" => Ok(Self::Gap),
            "hypothesis" => Ok(Self::Hypothesis),
            "experiment_idea" => Ok(Self::ExperimentIdea),
            _ => Err(format!("Unknown Research Entry kind: {value}")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EpistemicStatus {
    SourceSupported,
    AgentSynthesis,
    ResearcherContext,
    Speculative,
}

impl EpistemicStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SourceSupported => "source_supported",
            Self::AgentSynthesis => "agent_synthesis",
            Self::ResearcherContext => "researcher_context",
            Self::Speculative => "speculative",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "source_supported" => Ok(Self::SourceSupported),
            "agent_synthesis" => Ok(Self::AgentSynthesis),
            "researcher_context" => Ok(Self::ResearcherContext),
            "speculative" => Ok(Self::Speculative),
            _ => Err(format!("Unknown epistemic status: {value}")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntryLifecycle {
    Active,
    Contested,
    Superseded,
}

impl EntryLifecycle {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Contested => "contested",
            Self::Superseded => "superseded",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "active" => Ok(Self::Active),
            "contested" => Ok(Self::Contested),
            "superseded" => Ok(Self::Superseded),
            _ => Err(format!("Unknown Research Entry lifecycle: {value}")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntryRelationKind {
    DerivedFrom,
    MotivatedBy,
    Contests,
    Supersedes,
}

impl EntryRelationKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::DerivedFrom => "derived_from",
            Self::MotivatedBy => "motivated_by",
            Self::Contests => "contests",
            Self::Supersedes => "supersedes",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "derived_from" => Ok(Self::DerivedFrom),
            "motivated_by" => Ok(Self::MotivatedBy),
            "contests" => Ok(Self::Contests),
            "supersedes" => Ok(Self::Supersedes),
            _ => Err(format!("Unknown Research Entry relation: {value}")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResearchContextKind {
    ReaderNote,
    ChatTurn,
    ProjectDocument,
    ProjectInstruction,
}

impl ResearchContextKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ReaderNote => "reader_note",
            Self::ChatTurn => "chat_turn",
            Self::ProjectDocument => "project_document",
            Self::ProjectInstruction => "project_instruction",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "reader_note" => Ok(Self::ReaderNote),
            "chat_turn" => Ok(Self::ChatTurn),
            "project_document" => Ok(Self::ProjectDocument),
            "project_instruction" => Ok(Self::ProjectInstruction),
            _ => Err(format!("Unknown Research Context kind: {value}")),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResearchStateRevision {
    pub project_id: String,
    pub revision: i64,
    pub run_id: Option<String>,
    pub reason: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResearchEntrySummary {
    pub id: String,
    pub project_id: String,
    pub kind: ResearchEntryKind,
    pub epistemic_status: EpistemicStatus,
    pub text: String,
    pub lifecycle: EntryLifecycle,
    pub first_revision: i64,
    pub last_revision: i64,
    pub origin_run_id: Option<String>,
    pub evidence_count: i64,
    pub relation_count: i64,
    pub context_count: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResearchStateSnapshot {
    pub project_id: String,
    pub revision: i64,
    pub current_revision: i64,
    pub revisions: Vec<ResearchStateRevision>,
    pub entries: Vec<ResearchEntrySummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResearchEvidenceLink {
    pub id: String,
    pub entry_id: String,
    pub state_revision: i64,
    pub paper_id: String,
    pub paper_title: String,
    pub source_id: String,
    pub extraction_id: String,
    pub chunk_id: String,
    pub excerpt: String,
    pub source_start: i64,
    pub source_end: i64,
    pub page_start: i32,
    pub page_end: i32,
    pub support_note: Option<String>,
    /// How the evidence bears on the statement; legacy rows are `unspecified`.
    pub relationship: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryRelation {
    pub id: String,
    pub entry_id: String,
    pub state_revision: i64,
    pub target_entry_id: String,
    pub kind: EntryRelationKind,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResearchContextLink {
    pub id: String,
    pub entry_id: String,
    pub state_revision: i64,
    pub kind: ResearchContextKind,
    pub context_id: String,
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResearchEntryVersion {
    pub entry_id: String,
    pub state_revision: i64,
    pub kind: ResearchEntryKind,
    pub epistemic_status: EpistemicStatus,
    pub text: String,
    pub lifecycle: EntryLifecycle,
    pub origin_run_id: Option<String>,
    pub reason: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResearchEntryDetail {
    pub entry: ResearchEntrySummary,
    pub evidence: Vec<ResearchEvidenceLink>,
    pub relations: Vec<EntryRelation>,
    pub context: Vec<ResearchContextLink>,
    pub history: Vec<ResearchEntryVersion>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EvidenceLinkDraft {
    pub chunk_id: String,
    /// Optional exact inspected span inside the chunk; manual links default to the chunk preview.
    #[serde(default)]
    pub excerpt: Option<String>,
    pub support_note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryRelationDraft {
    pub target_entry_id: String,
    pub kind: EntryRelationKind,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResearchContextLinkDraft {
    pub kind: ResearchContextKind,
    pub context_id: String,
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResearchEntryDraft {
    pub kind: ResearchEntryKind,
    pub epistemic_status: EpistemicStatus,
    pub text: String,
    #[serde(default)]
    pub evidence: Vec<EvidenceLinkDraft>,
    #[serde(default)]
    pub relations: Vec<EntryRelationDraft>,
    #[serde(default)]
    pub context: Vec<ResearchContextLinkDraft>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResearchEntryUpdate {
    pub id: String,
    pub epistemic_status: EpistemicStatus,
    pub text: String,
    #[serde(default)]
    pub evidence: Vec<EvidenceLinkDraft>,
    #[serde(default)]
    pub relations: Vec<EntryRelationDraft>,
    #[serde(default)]
    pub context: Vec<ResearchContextLinkDraft>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResearchStateMutation {
    pub state: ResearchStateSnapshot,
    pub entry: ResearchEntryDetail,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResearchEvidenceCandidate {
    pub chunk_id: String,
    pub paper_id: String,
    pub paper_title: String,
    pub page_start: i32,
    pub page_end: i32,
    pub heading_path: Option<String>,
    pub text: String,
}
