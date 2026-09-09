//! Validation of final research proposals before any project mutation.
//! Static model constraints live here; storage verifies ownership and evidence.

use crate::domain::harness::{ResearchRunOutcome, ResearchStateSynthesis, ResearchSynthesisChange};
use crate::domain::research_state::{EpistemicStatus as Status, ResearchEntryKind as Kind};
use std::collections::{HashMap, HashSet};

/// Maximum UTF-8 size of a model proposal, excluding backend commit metadata.
pub const MAX_PROPOSAL_BYTES: usize = 128 * 1024;

/// An actionable validation error referring to the submitted artifact.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Issue {
    pub path: String,
    pub code: String,
    pub message: String,
}

/// Check independent structural errors together so one correction can fix them.
pub fn validate(outcome: &ResearchRunOutcome) -> Result<(), String> {
    let mut issues: Vec<Issue> = Vec::new();
    let mut check = |valid: bool, path: String, code: &str, message: &str| {
        if !valid && issues.len() < 40 {
            issues.push(Issue {
                path,
                code: code.into(),
                message: message.into(),
            });
        }
    };
    let text = |value: &str, max: usize| !value.trim().is_empty() && value.chars().count() <= max;
    check(
        text(&outcome.summary, 2000),
        "summary".into(),
        "text",
        "Use 1-2000 characters",
    );
    check(
        outcome.display_items.len() <= 20
            && outcome.paper_dispositions.len() <= 100
            && outcome.task_outcomes.len() <= 20
            && outcome.unanswered_questions.len() <= 20,
        "$".into(),
        "item_limit",
        "Too many report items",
    );
    for (i, item) in outcome.display_items.iter().enumerate() {
        check(
            text(&item.text, 1000),
            format!("displayItems[{i}].text"),
            "text",
            "Use 1-1000 characters",
        );
        check(
            item.cited_passage_refs.len() <= 20
                && item.cited_passage_refs.iter().all(|value| text(value, 500))
                && item
                    .state_entry_ref
                    .as_deref()
                    .is_none_or(|value| text(value, 500)),
            format!("displayItems[{i}].references"),
            "reference",
            "Use at most 20 passage references and one optional State entry reference",
        );
    }
    for (i, item) in outcome.paper_dispositions.iter().enumerate() {
        check(
            text(&item.paper_id, 500) && text(&item.reason, 500),
            format!("paperDispositions[{i}]"),
            "text",
            "Paper id and reason require 1-500 characters",
        );
    }
    for (i, question) in outcome.unanswered_questions.iter().enumerate() {
        check(
            text(question, 1000),
            format!("unansweredQuestions[{i}]"),
            "text",
            "Use 1-1000 characters",
        );
    }
    if let Some(direction) = &outcome.next_direction {
        check(
            text(direction, 1000),
            "nextDirection".into(),
            "text",
            "Use 1-1000 characters or null",
        );
    }
    for (path, prose) in std::iter::once(("summary".to_string(), outcome.summary.as_str()))
        .chain(
            outcome
                .display_items
                .iter()
                .enumerate()
                .map(|(index, item)| (format!("displayItems[{index}].text"), item.text.as_str())),
        )
        .chain(
            outcome
                .unanswered_questions
                .iter()
                .enumerate()
                .map(|(index, value)| (format!("unansweredQuestions[{index}]"), value.as_str())),
        )
        .chain(
            outcome
                .next_direction
                .as_deref()
                .map(|value| ("nextDirection".to_string(), value)),
        )
    {
        check(
            !contains_internal_reference(prose),
            path,
            "internal_reference",
            "Put internal references only in structured reference fields",
        );
    }
    for (i, task) in outcome.task_outcomes.iter().enumerate() {
        check(
            !task.learned_points.is_empty()
                && task.learned_points.len() <= 20
                && task.search_run_ids.len() <= 12
                && task.motivating_entry_ids.len() <= 20
                && task.cited_passage_refs.len() <= 40,
            format!("taskOutcomes[{i}]"),
            "item_limit",
            "Task item limits exceeded",
        );
        check(
            task.learned_points.iter().all(|v| text(v, 1000))
                && task
                    .search_run_ids
                    .iter()
                    .chain(&task.motivating_entry_ids)
                    .chain(&task.cited_passage_refs)
                    .all(|v| text(v, 500)),
            format!("taskOutcomes[{i}]"),
            "text",
            "Nonempty points (1000 characters) and references (500 characters) required",
        );
    }
    if let Some(synthesis) = &outcome.state_synthesis {
        check(
            synthesis.changes.len() <= 20
                && synthesis.unresolved_entry_ids.len() <= 20
                && synthesis.next_direction_entry_ids.len() <= 20,
            "stateSynthesis".into(),
            "item_limit",
            "At most 20 items per list",
        );
        check(
            synthesis.changes.is_empty() == synthesis.no_change_reason.is_some(),
            "stateSynthesis.noChangeReason".into(),
            "no_change",
            "Supply a reason exactly when changes is empty",
        );
        if let Some(reason) = &synthesis.no_change_reason {
            check(
                text(reason, 1000),
                "stateSynthesis.noChangeReason".into(),
                "text",
                "Use 1-1000 characters",
            );
            check(
                !contains_internal_reference(reason),
                "stateSynthesis.noChangeReason".into(),
                "internal_reference",
                "Put internal references only in structured reference fields",
            );
        }
        check(
            outcome.next_direction.is_some() || synthesis.next_direction_entry_ids.is_empty(),
            "stateSynthesis.nextDirectionEntryIds".into(),
            "direction",
            "Next direction entry IDs require a next direction",
        );
        check(
            synthesis
                .unresolved_entry_ids
                .iter()
                .chain(&synthesis.next_direction_entry_ids)
                .all(|v| text(v, 500)),
            "stateSynthesis".into(),
            "reference",
            "References require 1-500 characters",
        );
        let mut handles: HashSet<&str> = HashSet::new();
        let mut targets: HashSet<&str> = HashSet::new();
        for (i, change) in synthesis.changes.iter().enumerate() {
            let path = format!("stateSynthesis.changes[{i}]");
            let (kind, status, statement, evidence, relations, reason) = match change {
                ResearchSynthesisChange::Create {
                    handle,
                    kind,
                    epistemic_status,
                    statement,
                    evidence,
                    relations,
                    reason,
                } => {
                    check(
                        text(handle, 100) && handles.insert(handle),
                        format!("{path}.handle"),
                        "handle",
                        "Create handles must be unique and contain 1-100 characters",
                    );
                    (
                        Some(*kind),
                        *epistemic_status,
                        statement,
                        evidence,
                        relations,
                        reason,
                    )
                }
                ResearchSynthesisChange::Revise {
                    entry_id,
                    epistemic_status,
                    statement,
                    evidence,
                    relations,
                    reason,
                } => {
                    check(
                        text(entry_id, 500) && targets.insert(entry_id),
                        format!("{path}.entryId"),
                        "operation_conflict",
                        "One operation per existing entry",
                    );
                    (
                        None,
                        *epistemic_status,
                        statement,
                        evidence,
                        relations,
                        reason,
                    )
                }
                ResearchSynthesisChange::SetLifecycle {
                    entry_id, reason, ..
                } => {
                    check(
                        text(entry_id, 500) && targets.insert(entry_id),
                        format!("{path}.entryId"),
                        "operation_conflict",
                        "One operation per existing entry",
                    );
                    check(
                        text(reason, 1000),
                        format!("{path}.reason"),
                        "text",
                        "Use 1-1000 characters",
                    );
                    continue;
                }
            };
            check(
                text(statement, 4000) && text(reason, 1000),
                path.clone(),
                "text",
                "Statement/reason must be nonempty and within 4000/1000 characters",
            );
            check(
                evidence.len() <= 20 && relations.len() <= 20,
                path.clone(),
                "item_limit",
                "At most 20 evidence links and relations",
            );
            check(
                status != Status::ResearcherContext,
                format!("{path}.epistemicStatus"),
                "authorship",
                "An agent cannot invent researcher-authored context",
            );
            check(if status == Status::SourceSupported { kind.is_none_or(|k| k == Kind::Finding) && !evidence.is_empty() } else { evidence.is_empty() },
                format!("{path}.evidence"), "evidence_kind", "Only source-supported Findings carry direct evidence; derived ideas use relations to Findings");
            check(kind.is_none_or(|k| match k {
                Kind::Gap => matches!(status, Status::AgentSynthesis | Status::Speculative),
                Kind::Hypothesis | Kind::ExperimentIdea => status == Status::Speculative,
                _ => true,
            }), format!("{path}.epistemicStatus"), "kind_status", "Gap must be synthesis/speculative; Hypothesis and Experiment Idea must be speculative");
            check(
                status != Status::AgentSynthesis
                    || relations.iter().any(|r| r.kind.as_str() == "derived_from"),
                format!("{path}.relations"),
                "premise",
                "Agent synthesis requires a derived_from premise",
            );
            check(
                !matches!(kind, Some(Kind::Hypothesis | Kind::ExperimentIdea))
                    || relations
                        .iter()
                        .any(|r| matches!(r.kind.as_str(), "derived_from" | "motivated_by")),
                format!("{path}.relations"),
                "premise",
                "Managed hypotheses and experiment ideas require a premise",
            );
            for (j, e) in evidence.iter().enumerate() {
                check(text(&e.passage_ref, 500) && text(&e.explanation, 1000)
                    && matches!(e.relationship.as_str(), "supports" | "contradicts" | "context"),
                    format!("{path}.evidence[{j}]"), "evidence", "Use a delivered passage reference, explanation, and supports/contradicts/context relationship");
            }
            for (j, r) in relations.iter().enumerate() {
                check(
                    text(&r.target, 500),
                    format!("{path}.relations[{j}]"),
                    "reference",
                    "Relation target requires 1-500 characters",
                );
            }
        }
    } else {
        check(
            false,
            "stateSynthesis".into(),
            "missing",
            "State synthesis is required",
        );
    }
    if issues.is_empty() {
        Ok(())
    } else {
        Err(serde_json::to_string(&issues).expect("validation issues serialize"))
    }
}

/// Resolve local handles in every model-facing reference list after allocation.
pub fn resolve_handles(outcome: &mut ResearchRunOutcome, ids: &HashMap<String, String>) {
    let resolve = |id: &mut String| {
        if let Some(value) = ids.get(id) {
            *id = value.clone();
        }
    };
    if let Some(synthesis) = outcome.state_synthesis.as_mut() {
        for id in synthesis
            .unresolved_entry_ids
            .iter_mut()
            .chain(&mut synthesis.next_direction_entry_ids)
        {
            resolve(id);
        }
        for change in &mut synthesis.changes {
            match change {
                ResearchSynthesisChange::Create { relations, .. }
                | ResearchSynthesisChange::Revise { relations, .. } => {
                    for relation in relations {
                        resolve(&mut relation.target);
                    }
                }
                _ => {}
            }
        }
    }
    for item in &mut outcome.display_items {
        if let Some(reference) = &mut item.state_entry_ref {
            resolve(reference);
        }
    }
    for task in &mut outcome.task_outcomes {
        for id in &mut task.motivating_entry_ids {
            resolve(id);
        }
    }
}

/// Preserve a bounded report for subsequent runs without resending the graph.
pub fn continuation(mut outcome: ResearchRunOutcome) -> ResearchRunOutcome {
    outcome.state_synthesis = None;
    outcome.paper_dispositions.clear();
    outcome.summary = display_safe_prose(&outcome.summary)
        .chars()
        .take(1000)
        .collect();
    outcome.display_items.truncate(2);
    for item in &mut outcome.display_items {
        item.text = display_safe_prose(&item.text).chars().take(500).collect();
        item.cited_passage_refs.truncate(2);
    }
    outcome.task_outcomes.truncate(1);
    for task in &mut outcome.task_outcomes {
        task.learned_points.truncate(2);
        for point in &mut task.learned_points {
            *point = display_safe_prose(point).chars().take(500).collect();
        }
        task.cited_passage_refs.truncate(2);
        task.search_run_ids.truncate(2);
        task.motivating_entry_ids.truncate(2);
    }
    outcome.unanswered_questions.truncate(2);
    for question in &mut outcome.unanswered_questions {
        *question = display_safe_prose(question).chars().take(500).collect();
    }
    if let Some(direction) = &mut outcome.next_direction {
        *direction = display_safe_prose(direction).chars().take(500).collect();
    }
    outcome
}

/// Remove i0i storage identifiers from historical human-facing prose.
pub(crate) fn display_safe_prose(text: &str) -> String {
    let parenthesized = regex::Regex::new(
        r"\(\s*(?:passage_[0-9a-fA-F]{32}|research_entry_[0-9a-fA-F]{32}|(?:search|run)_[0-9]{10,})\s*\)",
    )
    .expect("parenthesized reference pattern is valid");
    let internal = internal_reference_pattern();
    let without_parentheses = parenthesized.replace_all(text, "");
    let without_ids = internal.replace_all(&without_parentheses, "");
    let whitespace = regex::Regex::new(r"[ \t]{2,}").expect("whitespace pattern is valid");
    let punctuation = regex::Regex::new(r"\s+([.,;:!?])").expect("punctuation pattern is valid");
    let cleaned = whitespace.replace_all(without_ids.trim(), " ");
    punctuation.replace_all(&cleaned, "$1").trim().to_string()
}

/// Return whether human-facing prose contains one of i0i's storage identifiers.
fn contains_internal_reference(text: &str) -> bool {
    internal_reference_pattern().is_match(text)
}

/// Compile the exact storage-key forms that must never become prose.
fn internal_reference_pattern() -> regex::Regex {
    regex::Regex::new(
        r"\b(?:passage_[0-9a-fA-F]{32}|research_entry_[0-9a-fA-F]{32}|(?:search|run)_[0-9]{10,})\b",
    )
    .expect("internal reference pattern is valid")
}

/// Validate synthesis alone for callers that do not own a complete report.
pub fn validate_shape(
    synthesis: &ResearchStateSynthesis,
    direction: Option<&str>,
) -> Result<(), String> {
    validate(&ResearchRunOutcome {
        summary: "Synthesis".into(),
        display_items: vec![],
        paper_dispositions: vec![],
        state_synthesis: Some(synthesis.clone()),
        task_outcomes: vec![],
        unanswered_questions: vec![],
        next_direction: direction.map(str::to_owned),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn outcome_with_item(text: &str) -> ResearchRunOutcome {
        serde_json::from_value(serde_json::json!({
            "summary": "Readable summary",
            "displayItems": [{
                "kind": "finding",
                "text": text,
                "citedPassageRefs": [],
                "stateEntryRef": null
            }],
            "paperDispositions": [],
            "stateSynthesis": {
                "changes": [],
                "unresolvedEntryIds": [],
                "nextDirectionEntryIds": [],
                "noChangeReason": "No change"
            },
            "taskOutcomes": [],
            "unansweredQuestions": [],
            "nextDirection": null
        }))
        .expect("valid outcome fixture")
    }

    #[test]
    fn new_outcome_rejects_internal_ids_in_prose() {
        let outcome =
            outcome_with_item("Bounded finding (passage_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa).");
        let error = validate(&outcome).expect_err("opaque prose reference must fail");
        assert!(error.contains("internal_reference"));
    }

    #[test]
    fn continuation_cleans_historical_internal_ids() {
        let outcome =
            outcome_with_item("Bounded finding (passage_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa).");
        let continuation = continuation(outcome);
        assert_eq!(continuation.display_items[0].text, "Bounded finding.");
    }
}
