//! Domain types for deep-research agentic search (RFC 0037).
//!
//! A `Search` is the durable unit: a saved goal + constraints whose candidate
//! pool *stacks* across runs. A `SearchRun` is one execution (a thin ledger);
//! `SearchCandidate` rows are the stacked pool, each remembering the run that
//! first surfaced it.

// These types are consumed across the RFC 0037 layers (seams, loop, commands,
// frontend serialization); some are not yet wired as the build lands layer by
// layer. Remove once the commands + frontend layers are in.
#![allow(dead_code)]

use serde::{Deserialize, Serialize};

use crate::domain::discovery::{
    paper_candidate_dedup_key, DiscoveryProviderChoice, PaperCandidate,
};

/// How often a saved search re-runs on its schedule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Frequency {
    Daily,
    Weekly,
    Monthly,
}

/// Schedule configuration for a saved search (Phase 2 automation).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchSchedule {
    pub enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub frequency: Option<Frequency>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub next_run_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub last_run_at: Option<String>,
}

/// Coarse user-facing depth knob; expands to a `SearchStrategy` budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Depth {
    Quick,
    Standard,
    Thorough,
}

impl Depth {
    /// Expand the preset into the hard budget rails the loop enforces.
    pub fn budget(self) -> SearchStrategy {
        match self {
            Depth::Quick => SearchStrategy {
                depth: self,
                max_iterations: 1,
                max_provider_queries: 3,
                max_llm_calls: 4,
            },
            Depth::Standard => SearchStrategy {
                depth: self,
                max_iterations: 3,
                max_provider_queries: 8,
                max_llm_calls: 12,
            },
            Depth::Thorough => SearchStrategy {
                depth: self,
                max_iterations: 4,
                max_provider_queries: 16,
                max_llm_calls: 24,
            },
        }
    }
}

/// Hard budget rails for a run, derived from `Depth`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchStrategy {
    pub depth: Depth,
    pub max_iterations: u32,
    pub max_provider_queries: u32,
    pub max_llm_calls: u32,
}

/// Structured constraints for a search. Soft intent stays in `goal`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchConstraints {
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub year_from: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub year_to: Option<i32>,
    #[serde(
        default,
        deserialize_with = "crate::domain::discovery::deserialize_providers_lenient"
    )]
    pub providers: Vec<DiscoveryProviderChoice>,
    #[serde(default)]
    pub open_access: bool,
    pub target_count: i32,
    #[serde(default)]
    pub venues: Vec<String>,
    #[serde(default)]
    pub authors: Vec<String>,
    #[serde(default)]
    pub fields_of_study: Vec<String>,
    #[serde(default)]
    pub seed_paper_ids: Vec<String>,
}

/// Input to create a new search.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchDraft {
    pub title: String,
    pub goal: String,
    pub constraints: SearchConstraints,
    pub strategy: SearchStrategy,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub schedule: Option<SearchSchedule>,
}

/// A saved search (the durable unit).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Search {
    pub id: String,
    pub title: String,
    pub goal: String,
    pub constraints: SearchConstraints,
    pub strategy: SearchStrategy,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub schedule: Option<SearchSchedule>,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub stop_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub summary: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// Lifecycle status of a run (also reused as a search's latest-run status).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchRunStatus {
    Queued,
    Planning,
    Searching,
    Assessing,
    Ranking,
    Ready,
    Failed,
    Cancelled,
}

impl SearchRunStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            SearchRunStatus::Queued => "queued",
            SearchRunStatus::Planning => "planning",
            SearchRunStatus::Searching => "searching",
            SearchRunStatus::Assessing => "assessing",
            SearchRunStatus::Ranking => "ranking",
            SearchRunStatus::Ready => "ready",
            SearchRunStatus::Failed => "failed",
            SearchRunStatus::Cancelled => "cancelled",
        }
    }
}

/// One execution of a search — a thin ledger, not candidate storage.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchRun {
    pub id: String,
    pub search_id: String,
    pub mode: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub provider_set: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub query_expansions: Option<String>,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub stop_reason: Option<String>,
    pub iteration: i32,
    pub added_count: i32,
    pub total_count: i32,
    pub provider_query_count: u32,
    pub llm_call_count: u32,
    pub inspected_candidate_count: u32,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub started_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub finished_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub error: Option<String>,
    pub created_at: String,
}

/// A ranked candidate produced by the loop, ready to stack into the pool.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RankedCandidate {
    pub candidate: PaperCandidate,
    pub rank: i32,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub score: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub rationale: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub rank_signals_json: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub provider_hits_json: Option<String>,
}

/// A persisted candidate in a search's stacked pool.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchCandidate {
    pub id: String,
    pub search_id: String,
    pub first_seen_run_id: String,
    pub rank: i32,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub score: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub rationale: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub rank_signals_json: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub provider_hits_json: Option<String>,
    pub candidate: PaperCandidate,
    pub already_in_library: bool,
    pub saved: bool,
    pub seen: bool,
    pub first_seen_at: String,
}

/// Normalized dedup key for a candidate: prefer DOI, then arXiv id, then a
/// lowercased/trimmed title. Used by stacking (`diff`) and within-run dedup.
pub fn candidate_dedup_key(candidate: &PaperCandidate) -> String {
    paper_candidate_dedup_key(candidate)
}
