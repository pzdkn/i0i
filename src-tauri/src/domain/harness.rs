//! Project-scoped autonomous research configuration, Runs, and Activity.

use chrono::{DateTime, Datelike, Duration, LocalResult, NaiveDate, NaiveTime, TimeZone, Utc};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};

use crate::domain::research::Depth;

pub const HARNESS_POLICY_VERSION: &str = "project-research-v1";
pub const HARNESS_POLICY_SUMMARY: &str = "Rust owns bounded orchestration, Project boundaries, evidence provenance, epistemic validation, cancellation, persistence, and budget enforcement.";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HarnessAutonomy {
    #[default]
    Manual,
    Propose,
    Automatic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScheduleCadence {
    Daily,
    Weekly,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HarnessSchedule {
    pub enabled: bool,
    pub cadence: ScheduleCadence,
    pub local_time: String,
    pub weekday: Option<u32>,
    pub timezone: String,
}

impl Default for HarnessSchedule {
    fn default() -> Self {
        Self {
            enabled: false,
            cadence: ScheduleCadence::Daily,
            local_time: "09:00".to_string(),
            weekday: Some(1),
            timezone: "UTC".to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HarnessStopConditions {
    pub maximum_cycles: Option<i64>,
    pub end_at: Option<String>,
    pub maximum_unproductive_runs: Option<i64>,
    pub maximum_run_seconds: Option<i64>,
    pub maximum_provider_queries: Option<u32>,
    pub maximum_llm_calls: Option<u32>,
    pub stop_on_convergence: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HarnessRunTrigger {
    Manual,
    Scheduled,
    StartupCatchUp,
}

impl HarnessRunTrigger {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::Scheduled => "scheduled",
            Self::StartupCatchUp => "startup_catch_up",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "manual" => Ok(Self::Manual),
            "scheduled" => Ok(Self::Scheduled),
            "startup_catch_up" => Ok(Self::StartupCatchUp),
            _ => Err(format!("Unknown Harness Run trigger: {value}")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HarnessConfiguration {
    pub goal: String,
    pub research_instructions: String,
    #[serde(default)]
    pub scope: String,
    #[serde(default)]
    pub exclusions: String,
    pub preferred_concepts: Vec<String>,
    pub excluded_concepts: Vec<String>,
    pub sources: Vec<String>,
    pub depth: Depth,
    pub paper_budget: i32,
    #[serde(default)]
    pub autonomy: HarnessAutonomy,
    #[serde(default)]
    pub may_add_papers: bool,
    #[serde(default)]
    pub writable_document_ids: Vec<String>,
    #[serde(default)]
    pub schedule: HarnessSchedule,
    #[serde(default)]
    pub stop_conditions: HarnessStopConditions,
}

impl Default for HarnessConfiguration {
    fn default() -> Self {
        Self {
            goal: String::new(),
            research_instructions: String::new(),
            scope: String::new(),
            exclusions: String::new(),
            preferred_concepts: Vec::new(),
            excluded_concepts: Vec::new(),
            sources: vec!["browser".into(), "open_alex".into(), "arxiv".into()],
            depth: Depth::Standard,
            paper_budget: 10,
            autonomy: HarnessAutonomy::Automatic,
            may_add_papers: true,
            writable_document_ids: Vec::new(),
            schedule: HarnessSchedule::default(),
            stop_conditions: HarnessStopConditions::default(),
        }
    }
}

impl HarnessConfiguration {
    /// Returns the single instruction presented by the simplified Research UI.
    ///
    /// Legacy fields are included so configurations created before RFC 0124 do
    /// not lose intent when they are first opened or migrated.
    pub fn canonical_instructions(&self) -> String {
        let has_legacy_fields = !self.goal.trim().is_empty()
            || !self.scope.trim().is_empty()
            || !self.exclusions.trim().is_empty()
            || !self.preferred_concepts.is_empty()
            || !self.excluded_concepts.is_empty();
        if !has_legacy_fields {
            return self.research_instructions.trim().to_string();
        }
        let mut sections: Vec<String> = Vec::new();
        for (heading, value) in [
            ("Goal", self.goal.trim()),
            ("Instructions", self.research_instructions.trim()),
            ("Scope", self.scope.trim()),
            ("Avoid", self.exclusions.trim()),
        ] {
            if !value.is_empty() {
                sections.push(format!("{heading}:\n{value}"));
            }
        }
        if !self.preferred_concepts.is_empty() {
            sections.push(format!(
                "Prioritize:\n{}",
                self.preferred_concepts.join(", ")
            ));
        }
        if !self.excluded_concepts.is_empty() {
            sections.push(format!(
                "Avoid concepts:\n{}",
                self.excluded_concepts.join(", ")
            ));
        }
        sections.join("\n\n")
    }

    /// Converts the current configuration to RFC 0124's simple persisted form.
    pub fn into_simple_research(mut self) -> Self {
        self.research_instructions = self.canonical_instructions();
        self.goal.clear();
        self.scope.clear();
        self.exclusions.clear();
        self.preferred_concepts.clear();
        self.excluded_concepts.clear();
        self.sources = vec!["browser".into(), "open_alex".into(), "arxiv".into()];
        self.depth = Depth::Standard;
        self.autonomy = HarnessAutonomy::Automatic;
        self.may_add_papers = true;
        self.writable_document_ids.clear();
        self.stop_conditions = HarnessStopConditions::default();
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResearchHarness {
    pub project_id: String,
    pub status: String,
    pub configuration: HarnessConfiguration,
    pub configuration_version: i64,
    pub next_run_at: Option<String>,
    pub last_scheduled_for: Option<String>,
    pub requested_post_run_status: String,
    pub completed_cycle_count: i64,
    pub consecutive_unproductive_runs: i64,
    pub terminal_stop_reason: Option<String>,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HarnessRun {
    pub id: String,
    pub project_id: String,
    pub status: String,
    pub configuration_snapshot: HarnessConfiguration,
    pub configuration_version: i64,
    pub policy_version: String,
    pub effective_instructions: EffectiveInstructionStack,
    pub search_id: String,
    pub search_run_id: Option<String>,
    pub stop_reason: Option<String>,
    pub summary: Option<String>,
    pub starting_state_revision: i64,
    pub resulting_state_revision: Option<i64>,
    pub starting_vault_revision: i64,
    pub resulting_vault_revision: Option<i64>,
    pub provider_query_count: u32,
    pub llm_call_count: u32,
    pub iteration_count: u32,
    pub inspected_candidate_count: u32,
    pub trigger: HarnessRunTrigger,
    pub scheduled_for: Option<String>,
    pub started_at: String,
    pub finished_at: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HarnessUsage {
    pub provider_queries: u32,
    pub llm_calls: u32,
    pub iterations: u32,
    pub inspected_candidates: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentRevisionFact {
    pub document_id: String,
    pub content_revision: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResearchCheckpoint {
    pub run_id: String,
    pub project_id: String,
    pub status: String,
    pub starting_state_revision: i64,
    pub resulting_state_revision: Option<i64>,
    pub starting_vault_revision: i64,
    pub resulting_vault_revision: Option<i64>,
    pub applied_change_set_id: Option<String>,
    pub accepted_candidate_count: i64,
    pub rejected_candidate_count: i64,
    pub added_paper_ids: Vec<String>,
    pub affected_documents: Vec<DocumentRevisionFact>,
    pub usage: HarnessUsage,
    pub stop_reason: Option<String>,
    pub complete: bool,
    pub converged: bool,
    pub reflection_id: Option<String>,
    pub next_direction: Option<String>,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub restore_available: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HarnessConfigurationVersion {
    pub project_id: String,
    pub version: i64,
    pub configuration: HarnessConfiguration,
    pub actor: String,
    pub source_improvement_id: Option<String>,
    pub reason: String,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunContextEntry {
    pub id: String,
    pub kind: String,
    pub epistemic_status: String,
    pub text: String,
    pub lifecycle: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunContextObservation {
    pub kind: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EffectiveRunContext {
    pub starting_state_revision: i64,
    pub active_entries: Vec<RunContextEntry>,
    #[serde(default)]
    pub vault_id: String,
    /// Membership snapshot marker encoded as `paper_count:max_membership_rowid`.
    #[serde(default)]
    pub vault_revision: String,
    pub vault_paper_ids: Vec<String>,
    pub prior_next_direction: Option<String>,
    #[serde(default)]
    pub prior_observations: Vec<RunContextObservation>,
    pub maximum_provider_queries: u32,
    pub maximum_llm_calls: u32,
    pub paper_budget: i32,
}

/// Renders the immutable Run snapshot consumed by query planning.
pub fn render_harness_search_goal(stack: &EffectiveInstructionStack) -> Result<String, String> {
    const MAX_CONTEXT_TEXT_CHARS: usize = 500;
    const MAX_PACKET_CHARS: usize = 80_000;

    let configuration = &stack.structured_settings;
    let context = &stack.run_context;
    let mut packet = format!(
        "Research instructions:\n{}\n\nEpistemic boundary: Research State is query-planning context, not source evidence. researcher_context entries, notes, chats, Project documents, prior assistant output, hypotheses, and experiment ideas may guide discovery but may not support factual claims or become citations.\n",
        configuration.canonical_instructions(),
    );
    if let Some(direction) = context.prior_next_direction.as_deref() {
        packet.push_str("\nPrevious Run next direction:\n");
        packet.push_str(&direction.chars().take(1_000).collect::<String>());
        packet.push('\n');
    }
    packet.push_str(&format!(
        "\nActive Research State at revision {}:\n",
        context.starting_state_revision
    ));
    for entry in &context.active_entries {
        packet.push_str(&format!(
            "- [{} · {}] {}\n",
            entry.kind,
            entry.epistemic_status,
            entry
                .text
                .chars()
                .take(MAX_CONTEXT_TEXT_CHARS)
                .collect::<String>()
        ));
    }
    if !context.prior_observations.is_empty() {
        packet.push_str("\nRecent operational observations (query guidance only):\n");
        for observation in &context.prior_observations {
            packet.push_str(&format!(
                "- [{}] {}\n",
                observation.kind,
                observation
                    .description
                    .chars()
                    .take(MAX_CONTEXT_TEXT_CHARS)
                    .collect::<String>()
            ));
        }
    }
    if packet.chars().count() > MAX_PACKET_CHARS {
        return Err(format!(
            "Harness orientation packet exceeds {MAX_PACKET_CHARS} characters"
        ));
    }
    Ok(packet)
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EffectiveInstructionStack {
    pub product_policy_version: String,
    pub product_policy_summary: String,
    pub project_research_instructions: String,
    pub structured_settings: HarnessConfiguration,
    pub run_context: EffectiveRunContext,
}

/// Computes the first scheduled UTC occurrence strictly after `after`.
pub fn next_schedule_occurrence(
    schedule: &HarnessSchedule,
    after: DateTime<Utc>,
) -> Result<DateTime<Utc>, String> {
    let timezone: Tz = schedule
        .timezone
        .parse()
        .map_err(|_| format!("Unknown IANA timezone: {}", schedule.timezone))?;
    let local_time = NaiveTime::parse_from_str(&schedule.local_time, "%H:%M")
        .map_err(|_| "Schedule time must use HH:MM".to_string())?;
    if schedule.cadence == ScheduleCadence::Weekly && !matches!(schedule.weekday, Some(1..=7)) {
        return Err("Weekly schedules require a weekday from 1 to 7".to_string());
    }
    let local_after = after.with_timezone(&timezone);
    for offset in 0..=8 {
        let date: NaiveDate = local_after.date_naive() + Duration::days(offset);
        if schedule.cadence == ScheduleCadence::Weekly
            && date.weekday().number_from_monday() != schedule.weekday.unwrap_or(1)
        {
            continue;
        }
        let mut candidate = date.and_time(local_time);
        for _ in 0..=180 {
            let resolved = match timezone.from_local_datetime(&candidate) {
                LocalResult::Single(value) => Some(value),
                LocalResult::Ambiguous(first, second) => Some(first.min(second)),
                LocalResult::None => None,
            };
            if let Some(value) = resolved {
                let utc = value.with_timezone(&Utc);
                if utc > after {
                    return Ok(utc);
                }
                break;
            }
            candidate += Duration::minutes(1);
        }
    }
    Err("Could not compute the next scheduled occurrence".to_string())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HarnessEvent {
    pub id: String,
    pub run_id: String,
    pub sequence: i64,
    pub kind: String,
    pub summary: String,
    pub detail: Option<serde_json::Value>,
    pub phase: Option<String>,
    pub progress_current: Option<i64>,
    pub progress_total: Option<i64>,
    pub actor: String,
    pub occurred_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HarnessSnapshot {
    pub harness: ResearchHarness,
    pub runs: Vec<HarnessRun>,
    pub events: Vec<HarnessEvent>,
}

#[derive(Debug, Clone)]
pub struct DueHarnessClaim {
    pub project_id: String,
    pub scheduled_for: String,
    pub trigger: HarnessRunTrigger,
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Timelike, Utc};

    use super::{
        next_schedule_occurrence, HarnessAutonomy, HarnessConfiguration, HarnessSchedule,
        ScheduleCadence,
    };

    #[test]
    fn legacy_configuration_composes_into_one_idempotent_instruction() {
        let legacy = HarnessConfiguration {
            goal: "Map LoRA mechanisms".to_string(),
            research_instructions: "Prefer causal evidence".to_string(),
            scope: "Transformer adapters".to_string(),
            exclusions: "Benchmark-only studies".to_string(),
            preferred_concepts: vec!["ablation".to_string()],
            excluded_concepts: vec!["survey".to_string()],
            ..HarnessConfiguration::default()
        };

        let simple = legacy.into_simple_research();

        assert!(simple.research_instructions.contains("Map LoRA mechanisms"));
        assert!(simple
            .research_instructions
            .contains("Prefer causal evidence"));
        assert!(simple
            .research_instructions
            .contains("Transformer adapters"));
        assert!(simple
            .research_instructions
            .contains("Benchmark-only studies"));
        assert!(simple.research_instructions.contains("ablation"));
        assert!(simple.research_instructions.contains("survey"));
        assert_eq!(simple.autonomy, HarnessAutonomy::Automatic);
        assert!(simple.may_add_papers);
        assert_eq!(
            simple.clone().into_simple_research().research_instructions,
            simple.research_instructions
        );
    }

    #[test]
    fn daily_schedule_preserves_local_time_across_dst() {
        let schedule = HarnessSchedule {
            enabled: true,
            cadence: ScheduleCadence::Daily,
            local_time: "09:00".to_string(),
            weekday: None,
            timezone: "Europe/Berlin".to_string(),
        };
        let before_dst = next_schedule_occurrence(
            &schedule,
            Utc.with_ymd_and_hms(2026, 3, 27, 12, 0, 0).unwrap(),
        )
        .unwrap();
        let after_dst = next_schedule_occurrence(
            &schedule,
            Utc.with_ymd_and_hms(2026, 3, 28, 12, 0, 0).unwrap(),
        )
        .unwrap();
        assert_eq!(before_dst.hour(), 8);
        assert_eq!(after_dst.hour(), 7);
    }

    #[test]
    fn weekly_schedule_uses_configured_local_weekday() {
        let schedule = HarnessSchedule {
            enabled: true,
            cadence: ScheduleCadence::Weekly,
            local_time: "09:00".to_string(),
            weekday: Some(1),
            timezone: "Europe/Berlin".to_string(),
        };
        let next = next_schedule_occurrence(
            &schedule,
            Utc.with_ymd_and_hms(2026, 9, 2, 10, 0, 0).unwrap(),
        )
        .unwrap();
        assert_eq!(next.to_rfc3339(), "2026-09-07T07:00:00+00:00");
    }
}
