//! Bounded planning and validation between search ranking and Project mutation.

use std::collections::{HashMap, HashSet};

use async_trait::async_trait;
use serde::Serialize;

use crate::domain::harness::EffectiveInstructionStack;
use crate::domain::harness_improvement::{HarnessImprovementTarget, HarnessImprovementValue};
use crate::domain::reconciliation::{
    CandidateDecisionKind, PlannedHarnessReflection, PlannedResearchEntry, ReconciliationTelemetry,
    RunReconciliationPlan,
};
use crate::domain::research::SearchCandidate;
use crate::domain::research_state::{EntryRelationKind, EpistemicStatus, ResearchEntryKind};

const MAX_CANDIDATES: usize = 100;
const MAX_ENTRIES: usize = 40;
const MAX_ENTRY_CHARS: usize = 2_000;
const MAX_REASON_CHARS: usize = 500;
const MAX_RELATIONS_PER_ENTRY: usize = 10;

/// Manual and propose modes always stop at review; automatic mode may apply.
pub fn should_apply_automatically(stack: &EffectiveInstructionStack) -> bool {
    stack.structured_settings.autonomy == crate::domain::harness::HarnessAutonomy::Automatic
}

#[async_trait]
pub trait ReconciliationPlanner: Send + Sync {
    async fn plan_reconciliation(
        &self,
        prompt: &str,
        validation_error: Option<&str>,
    ) -> Result<RunReconciliationPlan, String>;
}

#[async_trait]
impl ReconciliationPlanner for crate::services::research::planner::OpenRouterPlanner {
    async fn plan_reconciliation(
        &self,
        prompt: &str,
        validation_error: Option<&str>,
    ) -> Result<RunReconciliationPlan, String> {
        self.reconcile(prompt, validation_error)
            .await
            .map_err(|error| error.to_string())
    }
}

/// Gives invalid model output one correction attempt and returns the last error.
pub async fn plan_reconciliation_with_retry<P: ReconciliationPlanner>(
    planner: &P,
    prompt: &str,
    stack: &EffectiveInstructionStack,
    candidates: &[SearchCandidate],
) -> Result<RunReconciliationPlan, String> {
    plan_reconciliation_with_retry_counted(planner, prompt, stack, candidates)
        .await
        .0
}

/// Plans with one correction attempt and reports the exact model-call count.
pub async fn plan_reconciliation_with_retry_counted<P: ReconciliationPlanner>(
    planner: &P,
    prompt: &str,
    stack: &EffectiveInstructionStack,
    candidates: &[SearchCandidate],
) -> (Result<RunReconciliationPlan, String>, u32) {
    let mut correction: Option<String> = None;
    for call_count in 1..=2 {
        match planner
            .plan_reconciliation(prompt, correction.as_deref())
            .await
        {
            Ok(plan) => match validate_reconciliation_plan(&plan, stack, candidates) {
                Ok(()) => return (Ok(plan), call_count),
                Err(error) => correction = Some(error),
            },
            Err(error) => correction = Some(error),
        }
    }
    (
        Err(correction.unwrap_or_else(|| "Unknown reconciliation failure".to_string())),
        2,
    )
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ReconciliationPrompt<'a> {
    goal: &'a str,
    scope: &'a str,
    exclusions: &'a str,
    autonomy: &'a str,
    may_add_papers: bool,
    paper_budget: i32,
    remaining_model_calls: u32,
    starting_state_revision: i64,
    active_entries: &'a [crate::domain::harness::RunContextEntry],
    prior_next_direction: &'a Option<String>,
    telemetry: &'a ReconciliationTelemetry,
    candidates: Vec<PromptCandidate<'a>>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PromptCandidate<'a> {
    candidate_id: &'a str,
    title: &'a str,
    authors: &'a [String],
    year: Option<i32>,
    venue: &'a Option<String>,
    abstract_text: String,
    rank: i32,
    score: Option<f64>,
}

/// Renders only immutable Run context and candidates produced by this Run.
pub fn render_reconciliation_prompt(
    stack: &EffectiveInstructionStack,
    candidates: &[SearchCandidate],
    telemetry: &ReconciliationTelemetry,
) -> Result<String, String> {
    let configuration = &stack.structured_settings;
    let autonomy = match configuration.autonomy {
        crate::domain::harness::HarnessAutonomy::Manual => "manual",
        crate::domain::harness::HarnessAutonomy::Propose => "propose",
        crate::domain::harness::HarnessAutonomy::Automatic => "automatic",
    };
    let candidates = candidates
        .iter()
        .take(MAX_CANDIDATES)
        .map(|candidate| PromptCandidate {
            candidate_id: &candidate.id,
            title: &candidate.candidate.title,
            authors: &candidate.candidate.authors,
            year: candidate.candidate.year,
            venue: &candidate.candidate.venue,
            abstract_text: candidate
                .candidate
                .abstract_text
                .as_deref()
                .unwrap_or("")
                .chars()
                .take(3_000)
                .collect(),
            rank: candidate.rank,
            score: candidate.score,
        })
        .collect();
    serde_json::to_string_pretty(&ReconciliationPrompt {
        goal: &configuration.canonical_instructions(),
        scope: &configuration.scope,
        exclusions: &configuration.exclusions,
        autonomy,
        may_add_papers: configuration.may_add_papers,
        paper_budget: configuration.paper_budget,
        remaining_model_calls: 2,
        starting_state_revision: stack.run_context.starting_state_revision,
        active_entries: &stack.run_context.active_entries,
        prior_next_direction: &stack.run_context.prior_next_direction,
        telemetry,
        candidates,
    })
    .map_err(|error| error.to_string())
}

/// Validates model output entirely against supplied candidate and State handles.
pub fn validate_reconciliation_plan(
    plan: &RunReconciliationPlan,
    stack: &EffectiveInstructionStack,
    candidates: &[SearchCandidate],
) -> Result<(), String> {
    if candidates.len() > MAX_CANDIDATES {
        return Err(format!(
            "A reconciliation may consider at most {MAX_CANDIDATES} candidates"
        ));
    }
    let candidates_by_id: HashMap<&str, &SearchCandidate> = candidates
        .iter()
        .map(|candidate| (candidate.id.as_str(), candidate))
        .collect();
    let mut decided = HashSet::new();
    let mut accepted = HashSet::new();
    for decision in &plan.candidate_decisions {
        if !candidates_by_id.contains_key(decision.candidate_id.as_str()) {
            return Err(format!(
                "Candidate decision references an unknown handle: {}",
                decision.candidate_id
            ));
        }
        if !decided.insert(decision.candidate_id.as_str()) {
            return Err(format!(
                "Candidate was decided more than once: {}",
                decision.candidate_id
            ));
        }
        require_bounded_text(
            "Candidate decision reason",
            &decision.reason,
            MAX_REASON_CHARS,
        )?;
        if !(0.0..=1.0).contains(&decision.relevance_confidence) {
            return Err("Candidate relevance confidence must be between 0 and 1".to_string());
        }
        if decision.decision == CandidateDecisionKind::Accept {
            if !decision.within_scope {
                return Err(format!(
                    "An out-of-scope candidate cannot be accepted: {}",
                    decision.candidate_id
                ));
            }
            accepted.insert(decision.candidate_id.as_str());
        }
    }
    if decided.len() != candidates.len() {
        return Err(format!(
            "Candidate decisions must be exhaustive: decided {}, expected {}",
            decided.len(),
            candidates.len()
        ));
    }
    if accepted.len() > stack.structured_settings.paper_budget.max(0) as usize {
        return Err("Accepted candidate count exceeds the Run paper budget".to_string());
    }
    if !stack.structured_settings.may_add_papers {
        let unauthorized = accepted.iter().any(|candidate_id| {
            let paper_id = &candidates_by_id[*candidate_id].candidate.id;
            !stack.run_context.vault_paper_ids.contains(paper_id)
        });
        if unauthorized {
            return Err(
                "The Run is not authorized to accept new Papers into the Project Vault".to_string(),
            );
        }
    }
    if plan.entries.is_empty() {
        return Err(
            "A reconciliation plan must propose at least one Research State entry".to_string(),
        );
    }
    if plan.entries.len() > MAX_ENTRIES {
        return Err(format!(
            "A reconciliation may propose at most {MAX_ENTRIES} entries"
        ));
    }
    require_bounded_text("Next research direction", &plan.next_direction, 1_000)?;
    if let Some(reflection) = &plan.operational_reflection {
        validate_operational_reflection(reflection)?;
    }

    let existing_ids: HashSet<&str> = stack
        .run_context
        .active_entries
        .iter()
        .map(|entry| entry.id.as_str())
        .collect();
    let mut handles = HashSet::new();
    for entry in &plan.entries {
        require_bounded_text("Planned entry handle", &entry.handle, 100)?;
        require_bounded_text("Planned entry text", &entry.text, MAX_ENTRY_CHARS)?;
        if !handles.insert(entry.handle.as_str()) {
            return Err(format!(
                "Planned entry handle is duplicated: {}",
                entry.handle
            ));
        }
    }
    for entry in &plan.entries {
        validate_entry(entry, &accepted, &candidates_by_id, &existing_ids, &handles)?;
    }
    ordered_entries(plan, &existing_ids).map(|_| ())
}

fn validate_operational_reflection(reflection: &PlannedHarnessReflection) -> Result<(), String> {
    require_bounded_text("Operational reflection summary", &reflection.summary, 2_000)?;
    if let Some(direction) = &reflection.next_direction {
        require_bounded_text("Operational reflection next direction", direction, 1_000)?;
    }
    if reflection.observations.len() > 20 {
        return Err("Operational reflection exceeds 20 observations".to_string());
    }
    for observation in &reflection.observations {
        require_bounded_text(
            "Operational observation signature",
            &observation.signature,
            120,
        )?;
        require_bounded_text(
            "Operational observation description",
            &observation.description,
            1_000,
        )?;
        if !(0.0..=1.0).contains(&observation.severity)
            || !(0.0..=1.0).contains(&observation.confidence)
        {
            return Err("Operational observation severity and confidence must be 0..1".to_string());
        }
        if observation.target.is_some() != observation.proposed_value.is_some() {
            return Err(
                "Operational observation target and proposed value must appear together"
                    .to_string(),
            );
        }
        if observation.proposal_eligible && observation.target.is_none() {
            return Err(
                "Proposal-eligible operational observation requires an exact patch".to_string(),
            );
        }
        if let (Some(target), Some(value)) = (observation.target, &observation.proposed_value) {
            validate_improvement_value(target, value)?;
        }
    }
    Ok(())
}

fn validate_improvement_value(
    target: HarnessImprovementTarget,
    value: &HarnessImprovementValue,
) -> Result<(), String> {
    match (target, value) {
        (
            HarnessImprovementTarget::PreferredConcepts
            | HarnessImprovementTarget::ExcludedConcepts,
            HarnessImprovementValue::Concepts(values),
        ) => {
            if values.len() > 30
                || values
                    .iter()
                    .any(|value| value.trim().is_empty() || value.chars().count() > 80)
            {
                return Err("Operational concept patch is outside its bounded schema".to_string());
            }
        }
        (
            HarnessImprovementTarget::MetadataResolvers,
            HarnessImprovementValue::MetadataResolvers(values),
        ) => {
            if values
                .iter()
                .any(|value| !matches!(value.trim(), "open_alex" | "arxiv"))
            {
                return Err("Only OpenAlex and arXiv metadata resolvers may be changed".to_string());
            }
        }
        _ => {
            return Err(
                "Operational improvement value does not match its allow-listed target".to_string(),
            );
        }
    }
    Ok(())
}

/// Orders proposed entries so every planned relationship target exists first.
pub fn ordered_entries<'a>(
    plan: &'a RunReconciliationPlan,
    existing_ids: &HashSet<&str>,
) -> Result<Vec<&'a PlannedResearchEntry>, String> {
    let mut pending: Vec<&PlannedResearchEntry> = plan.entries.iter().collect();
    let mut resolved: HashSet<&str> = existing_ids.clone();
    let mut ordered = Vec::with_capacity(pending.len());
    while !pending.is_empty() {
        let before = pending.len();
        let mut index = 0;
        while index < pending.len() {
            let ready = pending[index]
                .relations
                .iter()
                .all(|relation| resolved.contains(relation.target.as_str()));
            if ready {
                let entry = pending.remove(index);
                resolved.insert(entry.handle.as_str());
                ordered.push(entry);
            } else {
                index += 1;
            }
        }
        if pending.len() == before {
            return Err("Planned Research Entry relations contain a cycle".to_string());
        }
    }
    Ok(ordered)
}

fn validate_entry(
    entry: &PlannedResearchEntry,
    accepted: &HashSet<&str>,
    candidates: &HashMap<&str, &SearchCandidate>,
    existing_ids: &HashSet<&str>,
    planned_handles: &HashSet<&str>,
) -> Result<(), String> {
    if entry.epistemic_status == EpistemicStatus::ResearcherContext {
        return Err("A Harness cannot create researcher-context entries".to_string());
    }
    if entry.epistemic_status == EpistemicStatus::SourceSupported {
        if entry.kind != ResearchEntryKind::Finding || entry.evidence.is_empty() {
            return Err(
                "A source-supported planned entry must be a Finding with evidence".to_string(),
            );
        }
    } else if !entry.evidence.is_empty() {
        return Err("Only source-supported Findings may attach direct evidence".to_string());
    }
    if entry.kind == ResearchEntryKind::Gap {
        if !matches!(
            entry.epistemic_status,
            EpistemicStatus::AgentSynthesis | EpistemicStatus::Speculative
        ) {
            return Err("A Gap must be agent synthesis or speculative".to_string());
        }
        if !entry.text.to_lowercase().contains("bounded search") {
            return Err("A Gap must explicitly say it follows from a bounded search".to_string());
        }
    }
    if matches!(
        entry.kind,
        ResearchEntryKind::Hypothesis | ResearchEntryKind::ExperimentIdea
    ) && entry.epistemic_status != EpistemicStatus::Speculative
    {
        return Err("Hypotheses and Experiment Ideas must be speculative".to_string());
    }
    if entry.epistemic_status == EpistemicStatus::AgentSynthesis
        && !entry
            .relations
            .iter()
            .any(|relation| relation.kind == EntryRelationKind::DerivedFrom)
    {
        return Err("Agent synthesis requires a derived-from relation".to_string());
    }
    if entry.relations.len() > MAX_RELATIONS_PER_ENTRY {
        return Err(format!(
            "A planned entry may have at most {MAX_RELATIONS_PER_ENTRY} relations"
        ));
    }
    for evidence in &entry.evidence {
        if !accepted.contains(evidence.candidate_id.as_str()) {
            return Err(format!(
                "Evidence must reference an accepted candidate: {}",
                evidence.candidate_id
            ));
        }
        require_bounded_text("Evidence excerpt", &evidence.excerpt, 1_000)?;
        let abstract_text = candidates[&evidence.candidate_id.as_str()]
            .candidate
            .abstract_text
            .as_deref()
            .ok_or_else(|| {
                format!(
                    "Candidate has no inspectable abstract: {}",
                    evidence.candidate_id
                )
            })?;
        if !abstract_text.contains(&evidence.excerpt) {
            return Err(format!(
                "Evidence excerpt is not an exact abstract substring: {}",
                evidence.candidate_id
            ));
        }
        if let Some(note) = &evidence.support_note {
            require_bounded_text("Evidence support note", note, 500)?;
        }
    }
    for relation in &entry.relations {
        if relation.target == entry.handle {
            return Err("A planned entry cannot relate to itself".to_string());
        }
        if !existing_ids.contains(relation.target.as_str())
            && !planned_handles.contains(relation.target.as_str())
        {
            return Err(format!(
                "Planned relation references an unknown entry handle: {}",
                relation.target
            ));
        }
        if matches!(
            relation.kind,
            EntryRelationKind::Contests | EntryRelationKind::Supersedes
        ) {
            return Err(
                "A reconciliation cannot automatically contest or supersede entries".to_string(),
            );
        }
    }
    Ok(())
}

fn require_bounded_text(label: &str, value: &str, maximum: usize) -> Result<(), String> {
    let length = value.trim().chars().count();
    if length == 0 {
        return Err(format!("{label} cannot be empty"));
    }
    if length > maximum {
        return Err(format!("{label} exceeds {maximum} characters"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::discovery::{CandidateMatch, PaperCandidate};
    use crate::domain::harness::{
        EffectiveRunContext, HarnessAutonomy, HarnessConfiguration, HARNESS_POLICY_SUMMARY,
        HARNESS_POLICY_VERSION,
    };
    use crate::domain::reconciliation::{
        CandidateDecision, PlannedEvidence, PlannedHarnessObservation, PlannedHarnessReflection,
        PlannedRelation, PlannedResearchEntry,
    };
    use std::sync::Mutex;

    fn candidate() -> SearchCandidate {
        SearchCandidate {
            id: "candidate:1".to_string(),
            search_id: "search:1".to_string(),
            first_seen_run_id: "search-run:1".to_string(),
            rank: 1,
            score: Some(0.9),
            rationale: None,
            rank_signals_json: None,
            provider_hits_json: None,
            candidate: PaperCandidate {
                id: "paper:1".to_string(),
                source_provider: "openalex".to_string(),
                source_id: "W1".to_string(),
                title: "Mechanistic LoRA".to_string(),
                authors: vec!["A. Researcher".to_string()],
                abstract_text: Some(
                    "Low-rank updates constrain adaptation to a learned subspace.".to_string(),
                ),
                year: Some(2026),
                publication_date: None,
                venue: Some("ICLR".to_string()),
                citation_count: Some(2),
                doi: None,
                openalex_id: Some("W1".to_string()),
                arxiv_id: None,
                external_url: None,
                pdf_url: None,
                open_access: None,
                match_summary: CandidateMatch {
                    score: Some(0.9),
                    reasons: Vec::new(),
                    matched_keywords: Vec::new(),
                    from_seed_paper_ids: Vec::new(),
                },
                already_in_library: false,
            },
            already_in_library: false,
            saved: false,
            seen: false,
            first_seen_at: "2026-09-02".to_string(),
        }
    }

    fn stack(may_add_papers: bool) -> EffectiveInstructionStack {
        EffectiveInstructionStack {
            product_policy_version: HARNESS_POLICY_VERSION.to_string(),
            product_policy_summary: HARNESS_POLICY_SUMMARY.to_string(),
            project_research_instructions: String::new(),
            structured_settings: HarnessConfiguration {
                goal: "Improve LoRA interpretability".to_string(),
                scope: "Mechanistic studies".to_string(),
                exclusions: "Application-only studies".to_string(),
                autonomy: HarnessAutonomy::Automatic,
                may_add_papers,
                paper_budget: 2,
                ..HarnessConfiguration::default()
            },
            run_context: EffectiveRunContext {
                starting_state_revision: 0,
                active_entries: Vec::new(),
                vault_id: "attention".to_string(),
                vault_revision: "0:0".to_string(),
                vault_paper_ids: Vec::new(),
                prior_next_direction: None,
                prior_observations: Vec::new(),
                maximum_provider_queries: 3,
                maximum_llm_calls: 4,
                paper_budget: 2,
            },
        }
    }

    fn valid_plan() -> RunReconciliationPlan {
        RunReconciliationPlan {
            candidate_decisions: vec![CandidateDecision {
                candidate_id: "candidate:1".to_string(),
                decision: CandidateDecisionKind::Accept,
                reason: "Directly studies the mechanism".to_string(),
                relevance_confidence: 0.95,
                within_scope: true,
            }],
            entries: vec![
                PlannedResearchEntry {
                    handle: "finding:1".to_string(),
                    kind: ResearchEntryKind::Finding,
                    epistemic_status: EpistemicStatus::SourceSupported,
                    text: "Low-rank updates constrain adaptation.".to_string(),
                    evidence: vec![PlannedEvidence {
                        candidate_id: "candidate:1".to_string(),
                        excerpt: "constrain adaptation to a learned subspace".to_string(),
                        support_note: Some("Provider abstract evidence".to_string()),
                    }],
                    relations: Vec::new(),
                },
                PlannedResearchEntry {
                    handle: "hypothesis:1".to_string(),
                    kind: ResearchEntryKind::Hypothesis,
                    epistemic_status: EpistemicStatus::Speculative,
                    text: "Subspace geometry may predict explanation stability.".to_string(),
                    evidence: Vec::new(),
                    relations: vec![PlannedRelation {
                        target: "finding:1".to_string(),
                        kind: EntryRelationKind::MotivatedBy,
                    }],
                },
            ],
            next_direction: "Compare subspace measurements across LoRA ranks.".to_string(),
            operational_reflection: None,
        }
    }

    #[test]
    fn validates_exhaustive_decisions_and_exact_abstract_evidence() {
        validate_reconciliation_plan(&valid_plan(), &stack(true), &[candidate()])
            .expect("valid fixed-corpus plan");
    }

    #[test]
    fn rejects_invented_or_non_exact_evidence() {
        let mut plan = valid_plan();
        plan.entries[0].evidence[0].excerpt = "The paper proves causality".to_string();
        assert!(
            validate_reconciliation_plan(&plan, &stack(true), &[candidate()])
                .unwrap_err()
                .contains("exact abstract substring")
        );
    }

    #[test]
    fn rejects_out_of_scope_acceptance_and_missing_paper_authority() {
        let mut plan = valid_plan();
        plan.candidate_decisions[0].within_scope = false;
        assert!(validate_reconciliation_plan(&plan, &stack(true), &[candidate()]).is_err());
        assert!(
            validate_reconciliation_plan(&valid_plan(), &stack(false), &[candidate()])
                .unwrap_err()
                .contains("not authorized")
        );
    }

    #[test]
    fn rejects_non_exhaustive_decisions_and_relation_cycles() {
        let mut plan = valid_plan();
        plan.candidate_decisions.clear();
        assert!(
            validate_reconciliation_plan(&plan, &stack(true), &[candidate()])
                .unwrap_err()
                .contains("exhaustive")
        );
        let mut cyclic = valid_plan();
        cyclic.entries[0].relations.push(PlannedRelation {
            target: "hypothesis:1".to_string(),
            kind: EntryRelationKind::MotivatedBy,
        });
        assert!(
            validate_reconciliation_plan(&cyclic, &stack(true), &[candidate()])
                .unwrap_err()
                .contains("cycle")
        );
    }

    #[test]
    fn bounded_search_gaps_and_speculative_outputs_remain_distinct() {
        let mut plan = valid_plan();
        plan.entries.push(PlannedResearchEntry {
            handle: "gap:1".to_string(),
            kind: ResearchEntryKind::Gap,
            epistemic_status: EpistemicStatus::Speculative,
            text: "This bounded search did not find rank-controlled causal studies.".to_string(),
            evidence: Vec::new(),
            relations: vec![PlannedRelation {
                target: "finding:1".to_string(),
                kind: EntryRelationKind::MotivatedBy,
            }],
        });
        validate_reconciliation_plan(&plan, &stack(true), &[candidate()]).expect("qualified gap");
        plan.entries.last_mut().unwrap().text = "No causal studies exist.".to_string();
        assert!(
            validate_reconciliation_plan(&plan, &stack(true), &[candidate()])
                .unwrap_err()
                .contains("bounded search")
        );
    }

    #[test]
    fn operational_reflection_accepts_only_bounded_allow_list_patches() {
        let mut plan = valid_plan();
        plan.operational_reflection = Some(PlannedHarnessReflection {
            summary: "Queries overemphasized application papers.".to_string(),
            next_direction: Some("Use mechanistic terminology.".to_string()),
            observations: vec![PlannedHarnessObservation {
                kind: crate::domain::harness_improvement::HarnessObservationKind::QueryQuality,
                signature: "application-heavy-results".to_string(),
                severity: 0.6,
                confidence: 0.9,
                description: "Application-heavy results reduced precision.".to_string(),
                target: Some(HarnessImprovementTarget::PreferredConcepts),
                proposed_value: Some(HarnessImprovementValue::Concepts(vec![
                    "mechanistic".to_string()
                ])),
                proposal_eligible: true,
            }],
        });
        validate_reconciliation_plan(&plan, &stack(true), &[candidate()])
            .expect("allow-listed operational reflection");

        let observation = &mut plan
            .operational_reflection
            .as_mut()
            .expect("reflection")
            .observations[0];
        observation.target = Some(HarnessImprovementTarget::MetadataResolvers);
        assert!(
            validate_reconciliation_plan(&plan, &stack(true), &[candidate()])
                .unwrap_err()
                .contains("does not match")
        );
    }

    #[test]
    fn only_automatic_autonomy_skips_review() {
        let automatic = stack(true);
        assert!(should_apply_automatically(&automatic));
        for autonomy in [HarnessAutonomy::Manual, HarnessAutonomy::Propose] {
            let mut reviewed = stack(true);
            reviewed.structured_settings.autonomy = autonomy;
            assert!(!should_apply_automatically(&reviewed));
        }
    }

    struct FakeReconciliationPlanner {
        responses: Mutex<Vec<Result<RunReconciliationPlan, String>>>,
        corrections: Mutex<Vec<Option<String>>>,
    }

    #[async_trait]
    impl ReconciliationPlanner for FakeReconciliationPlanner {
        async fn plan_reconciliation(
            &self,
            _prompt: &str,
            validation_error: Option<&str>,
        ) -> Result<RunReconciliationPlan, String> {
            self.corrections
                .lock()
                .expect("corrections")
                .push(validation_error.map(ToString::to_string));
            self.responses.lock().expect("responses").remove(0)
        }
    }

    #[tokio::test]
    async fn invalid_plan_gets_one_bounded_correction_attempt() {
        let mut invalid = valid_plan();
        invalid.entries[0].evidence[0].excerpt = "invented quote".to_string();
        let planner = FakeReconciliationPlanner {
            responses: Mutex::new(vec![Ok(invalid), Ok(valid_plan())]),
            corrections: Mutex::new(Vec::new()),
        };
        let result = plan_reconciliation_with_retry(
            &planner,
            "bounded packet",
            &stack(true),
            &[candidate()],
        )
        .await
        .expect("second plan validates");
        assert_eq!(result, valid_plan());
        let corrections = planner.corrections.lock().expect("corrections");
        assert_eq!(corrections.len(), 2);
        assert!(corrections[0].is_none());
        assert!(corrections[1]
            .as_deref()
            .unwrap_or_default()
            .contains("exact abstract substring"));
    }

    #[tokio::test]
    async fn second_invalid_plan_returns_explicit_failure() {
        let mut invalid = valid_plan();
        invalid.candidate_decisions.clear();
        let planner = FakeReconciliationPlanner {
            responses: Mutex::new(vec![Ok(invalid.clone()), Ok(invalid)]),
            corrections: Mutex::new(Vec::new()),
        };
        let error = plan_reconciliation_with_retry(
            &planner,
            "bounded packet",
            &stack(true),
            &[candidate()],
        )
        .await
        .expect_err("two invalid plans fail");
        assert!(error.contains("exhaustive"));
        assert_eq!(planner.corrections.lock().expect("corrections").len(), 2);
    }
}
