//! Durable proposal attempts and atomic completion of managed research runs.
//! Preflight executes the same transaction as commit, then rolls it back.

use super::*;
use crate::services::mcp::PassageAnchor;
use crate::services::research::synthesis;

impl LibraryStore {
    /// Export bounded artifacts for the explicit isolated acceptance report.
    #[cfg(test)]
    pub(crate) fn synthesis_attempt_reports(
        &self,
        run_id: &str,
    ) -> StoreResult<Vec<serde_json::Value>> {
        let conn = self.open_connection()?;
        let mut query = conn.prepare("select attempt,proposal,issues,committed,model_id,thread_id,turn_id from research_synthesis_attempts where run_id=?1 order by attempt").map_err(|e|e.to_string())?;
        let rows = query.query_map([run_id], |r| Ok(serde_json::json!({"runId":run_id,"attempt":r.get::<_,i64>(0)?,"proposal":r.get::<_,String>(1)?,"issues":r.get::<_,Option<String>>(2)?,"committed":r.get::<_,bool>(3)?,"model":r.get::<_,Option<String>>(4)?,"threadId":r.get::<_,Option<String>>(5)?,"turnId":r.get::<_,Option<String>>(6)?}))).map_err(|e|e.to_string())?;
        collect_rows(rows)
    }
    /// Reserve the single correction model call against the original Run budget.
    pub(crate) fn reserve_synthesis_correction(&self, run_id: &str) -> StoreResult<()> {
        let mut conn = self.open_connection()?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|e| e.to_string())?;
        let run = read_harness_run(&tx, run_id)?;
        if run.status != "reconciling" {
            return Err("Run no longer accepts synthesis".into());
        }
        let child_calls:i64 = tx.query_row("select coalesce(sum(r.llm_call_count),0) from agent_search_runs a join search_runs r on r.id=a.run_id where a.parent_run_id=?1",[run_id],|r|r.get(0)).map_err(|e|e.to_string())?;
        if run.llm_call_count as i64 + child_calls
            >= run
                .agent_limits
                .ok_or("Missing Run limits")?
                .maximum_llm_calls as i64
        {
            return Err("Research Run model-call budget exhausted".into());
        }
        tx.execute(
            "update harness_runs set llm_call_count=llm_call_count+1 where id=?1",
            [run_id],
        )
        .map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())
    }
    /// Save the bounded final artifact before validating it; never overwrite attempts.
    pub(crate) fn save_synthesis_attempt(
        &self,
        run_id: &str,
        attempt: i64,
        proposal: &str,
        evidence: &HashMap<String, PassageAnchor>,
    ) -> StoreResult<()> {
        if proposal.len() > synthesis::MAX_PROPOSAL_BYTES {
            return Err("Proposal exceeds 128 KiB".into());
        }
        let conn = self.open_connection()?;
        let run = read_harness_run(&conn, run_id)?;
        let evidence_chars: usize = evidence.values().map(|a| a.quote.chars().count()).sum();
        if evidence_chars
            > run
                .agent_limits
                .as_ref()
                .ok_or("Missing Run limits")?
                .maximum_returned_text_chars as usize
        {
            return Err("Evidence snapshot exceeds Run read budget".into());
        }
        conn.execute("insert into research_synthesis_attempts (run_id,attempt,proposal,evidence_json,vault_revision,model_id,thread_id,turn_id)
            select ?1,?2,?3,?4,coalesce((select vault_revision from research_synthesis_attempts where run_id=?1 and attempt=0),membership_revision),?6,?7,?8 from vaults where project_id=?5",
            params![run_id,attempt,proposal,serde_json::to_string(evidence).map_err(|e| e.to_string())?,run.project_id,run.runtime_model,run.runtime_thread_id,run.runtime_turn_id]).map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Save anchors inside the same transaction as successful passage delivery.
    pub(super) fn persist_delivered_anchors_on(
        tx: &Connection,
        run_id: &str,
        anchors: &HashMap<String, PassageAnchor>,
    ) -> StoreResult<()> {
        let run = read_harness_run(tx, run_id)?;
        if !matches!(
            run.status.as_str(),
            "planning" | "searching" | "assessing" | "ranking"
        ) {
            return Err("Managed Research Run is no longer active".into());
        }
        for (reference, anchor) in anchors {
            tx.execute("insert into agent_passage_anchors (run_id,passage_ref,anchor_json,source_version) values (?1,?2,?3,?4) on conflict do nothing",
                params![run_id,reference,serde_json::to_string(anchor).map_err(|e|e.to_string())?,source_version(&tx,anchor)?]).map_err(|e|e.to_string())?;
        }
        let total: i64 = tx.query_row("select coalesce(sum(length(json_extract(anchor_json,'$.quote'))),0) from agent_passage_anchors where run_id=?1",[run_id],|r|r.get(0)).map_err(|e|e.to_string())?;
        let limit = run
            .agent_limits
            .ok_or("Missing Run limits")?
            .maximum_returned_text_chars;
        if total > limit as i64 {
            return Err("Evidence snapshot exceeds Run read budget".into());
        }
        Ok(())
    }

    /// Reload the evidence required for synthesis without relying on MCP memory.
    pub(crate) fn captured_run_evidence(
        &self,
        run_id: &str,
    ) -> StoreResult<HashMap<String, PassageAnchor>> {
        let conn = self.open_connection()?;
        let mut query = conn
            .prepare("select passage_ref,anchor_json from agent_passage_anchors where run_id=?1")
            .map_err(|e| e.to_string())?;
        let rows = query
            .query_map([run_id], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            })
            .map_err(|e| e.to_string())?;
        collect_rows(rows)?
            .into_iter()
            .map(|(reference, json)| {
                Ok((
                    reference,
                    serde_json::from_str(&json).map_err(|e| e.to_string())?,
                ))
            })
            .collect()
    }

    /// Associate the corrected artifact with its own turn, without changing Run phase.
    pub(crate) fn identify_synthesis_turn(
        &self,
        run_id: &str,
        attempt: i64,
        turn: &crate::services::codex_runtime::CodexTurn,
    ) -> StoreResult<()> {
        self.open_connection()?.execute("update research_synthesis_attempts set thread_id=?3,turn_id=?4 where run_id=?1 and attempt=?2",
            params![run_id,attempt,turn.thread_id,turn.turn_id]).map_err(|e|e.to_string())?;
        Ok(())
    }

    /// Describe precisely the additions for which the model owes dispositions.
    pub(crate) fn synthesis_additions(&self, run_id: &str) -> StoreResult<Vec<serde_json::Value>> {
        let conn = self.open_connection()?;
        let mut query = conn.prepare("select a.paper_id,p.title,exists(select 1 from agent_reader_usage u where u.run_id=a.run_id and u.paper_id=a.paper_id) from agent_vault_additions a join papers p on p.id=a.paper_id where a.run_id=?1 and a.membership_added=1").map_err(|e| e.to_string())?;
        let rows = query.query_map([run_id], |r| Ok(serde_json::json!({"paperId":r.get::<_,String>(0)?,"title":r.get::<_,String>(1)?,"readAttempted":r.get::<_,bool>(2)?}))).map_err(|e| e.to_string())?;
        collect_rows(rows)
    }

    /// Validate the exact commit path, preserving issues and rolling back all writes.
    pub(crate) fn preflight_synthesis(&self, run_id: &str, attempt: i64) -> StoreResult<()> {
        let result = self.finalize_synthesis(run_id, attempt, false);
        let conn = self.open_connection()?;
        let issues = result
            .as_ref()
            .err()
            .cloned()
            .unwrap_or_else(|| "[]".into());
        conn.execute(
            "update research_synthesis_attempts set issues=?3 where run_id=?1 and attempt=?2",
            params![run_id, attempt, issues],
        )
        .map_err(|e| e.to_string())?;
        result.map(|_| ())
    }

    /// Commit exactly once, or dry-run the identical transaction during preflight.
    pub(crate) fn finalize_synthesis(
        &self,
        run_id: &str,
        attempt: i64,
        commit: bool,
    ) -> StoreResult<HarnessRun> {
        let mut conn = self.open_connection()?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|e| e.to_string())?;
        let (raw, evidence_json, vault_revision, committed): (String,String,i64,bool) = tx.query_row(
            "select proposal,evidence_json,vault_revision,committed from research_synthesis_attempts where run_id=?1 and attempt=?2",
            params![run_id,attempt], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).map_err(|e| e.to_string())?;
        let run = read_harness_run(&tx, run_id)?;
        if committed {
            return Ok(run);
        }
        if run.status != "reconciling" {
            return Err("Run no longer accepts synthesis".into());
        }
        require_current_state_revision(&tx, &run.project_id, run.starting_state_revision)?;
        let current_vault: i64 = tx
            .query_row(
                "select membership_revision from vaults where project_id=?1",
                [&run.project_id],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        if current_vault != vault_revision {
            return Err("Source/project conflict: Vault changed during synthesis".into());
        }
        let mut outcome: ResearchRunOutcome = serde_json::from_str(&raw)
            .map_err(|e| format!("Invalid Research outcome JSON: {e}"))?;
        synthesis::validate(&outcome)?;
        let proposal = outcome
            .state_synthesis
            .as_ref()
            .expect("validated synthesis");
        if proposal.resulting_revision.is_some() || !proposal.created_entry_ids.is_empty() {
            return Err("Proposal cannot supply backend commit facts".into());
        }
        let evidence: HashMap<String, PassageAnchor> =
            serde_json::from_str(&evidence_json).map_err(|e| e.to_string())?;
        for disposition in &outcome.paper_dispositions {
            if disposition.disposition.as_str() == "irrelevant"
                && proposal.changes.iter().any(|change| {
                    let links = match change {
                        ResearchSynthesisChange::Create { evidence, .. }
                        | ResearchSynthesisChange::Revise { evidence, .. } => evidence,
                        _ => return false,
                    };
                    links.iter().any(|link| {
                        evidence
                            .get(&link.passage_ref)
                            .is_some_and(|a| a.paper_id == disposition.paper_id)
                    })
                })
            {
                return Err(format!(
                    "Paper disposition removes cited evidence: {}",
                    disposition.paper_id
                ));
            }
        }
        validate_graph(&tx, &run, &outcome)?;
        let changes = prepare_changes(&tx, &run, proposal, &evidence)?;

        // Every finalization write shares this transaction, including the report.
        Self::finalize_agent_paper_retention_on(&tx, run_id, &outcome.paper_dispositions)?;
        if changes.is_empty() {
            Self::record_agent_state_unchanged_on(
                &tx,
                run_id,
                proposal
                    .no_change_reason
                    .as_deref()
                    .expect("validated reason"),
            )?;
        } else {
            let receipt = Self::apply_agent_state_update_on(
                &tx,
                &run.project_id,
                run.starting_state_revision,
                &format!("codex-agent:{run_id}"),
                Some(run_id),
                &format!("synthesis:{run_id}"),
                &short_sha256(&raw),
                changes,
            )?;
            synthesis::resolve_handles(&mut outcome, &receipt.created_entry_ids);
            let synthesis = outcome
                .state_synthesis
                .as_mut()
                .expect("validated synthesis");
            synthesis.resulting_revision = Some(receipt.revision);
            synthesis.created_entry_ids = receipt.created_entry_ids;
        }
        Self::persist_agent_run_outcome_on(&tx, run_id, &outcome)?;
        let completed = Self::finish_codex_harness_run_on(&tx, run_id, "ready", "agent_completed")?;
        tx.execute("update research_synthesis_attempts set committed=1,issues='[]' where run_id=?1 and attempt=?2", params![run_id,attempt]).map_err(|e| e.to_string())?;
        if commit {
            tx.commit().map_err(|e| e.to_string())?;
        } else {
            tx.rollback().map_err(|e| e.to_string())?;
        }
        Ok(completed)
    }

    /// Recover preflighted proposals without a new model call on startup.
    pub(crate) fn recover_prepared_synthesis(&self) -> StoreResult<()> {
        let conn = self.open_connection()?;
        let mut query = conn.prepare("select a.run_id,a.attempt from research_synthesis_attempts a join harness_runs r on r.id=a.run_id
            where r.status='reconciling' and a.committed=0 and a.issues='[]'").map_err(|e| e.to_string())?;
        let rows = query
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))
            .map_err(|e| e.to_string())?;
        let pending = collect_rows(rows)?;
        drop(query);
        drop(conn);
        for (run_id, attempt) in pending {
            if let Err(error) = self.finalize_synthesis(&run_id, attempt, true) {
                self.record_harness_activity(
                    &run_id,
                    "synthesis_recovery_failed",
                    &error.chars().take(500).collect::<String>(),
                    Some("synthesizing"),
                )?;
            }
        }
        Ok(())
    }
}

/// Check the proposed dependency graph and every outcome State reference.
fn validate_graph(
    conn: &Connection,
    run: &HarnessRun,
    outcome: &ResearchRunOutcome,
) -> StoreResult<()> {
    let synthesis = outcome
        .state_synthesis
        .as_ref()
        .expect("validated synthesis");
    let entries =
        read_research_entry_summaries(conn, &run.project_id, run.starting_state_revision)?;
    let mut kinds: HashMap<String, ResearchEntryKind> =
        entries.iter().map(|e| (e.id.clone(), e.kind)).collect();
    let mut edges: HashMap<String, Vec<String>> = HashMap::new();
    let mut statements: HashMap<String, String> = entries
        .iter()
        .filter(|e| e.lifecycle == EntryLifecycle::Active)
        .map(|e| (e.id.clone(), normalize_state_statement(&e.text)))
        .collect();
    for entry in &entries {
        let detail = read_research_entry_detail_from_conn(
            conn,
            &entry.id,
            Some(run.starting_state_revision),
        )?;
        edges.insert(
            entry.id.clone(),
            detail
                .relations
                .iter()
                .filter(|r| {
                    matches!(
                        r.kind,
                        EntryRelationKind::DerivedFrom | EntryRelationKind::MotivatedBy
                    )
                })
                .map(|r| r.target_entry_id.clone())
                .collect(),
        );
    }
    for change in &synthesis.changes {
        if let ResearchSynthesisChange::Create { handle, kind, .. } = change {
            if kinds.insert(handle.clone(), *kind).is_some() {
                return Err(format!(
                    "Create handle collides with existing entry: {handle}"
                ));
            }
        }
    }
    for change in &synthesis.changes {
        let (id, relations, statement) = match change {
            ResearchSynthesisChange::Create {
                handle,
                relations,
                statement,
                ..
            } => (handle, relations, statement),
            ResearchSynthesisChange::Revise {
                entry_id,
                relations,
                statement,
                epistemic_status,
                ..
            } => {
                if !entries.iter().any(|e| e.id == *entry_id) {
                    return Err(format!("Unknown State entry to revise: {entry_id}"));
                }
                let kind = kinds
                    .get(entry_id)
                    .ok_or_else(|| format!("Unknown State entry: {entry_id}"))?;
                if matches!(
                    kind,
                    ResearchEntryKind::Hypothesis | ResearchEntryKind::ExperimentIdea
                ) && !relations.iter().any(|relation| {
                    matches!(
                        relation.kind,
                        EntryRelationKind::DerivedFrom | EntryRelationKind::MotivatedBy
                    )
                }) {
                    return Err(format!("{entry_id}: speculative idea requires premise"));
                }
                if *epistemic_status == EpistemicStatus::SourceSupported
                    && *kind != ResearchEntryKind::Finding
                {
                    return Err(format!(
                        "{entry_id}: Only a Finding may be source-supported"
                    ));
                }
                (entry_id, relations, statement)
            }
            ResearchSynthesisChange::SetLifecycle {
                entry_id,
                lifecycle,
                ..
            } => {
                if !entries.iter().any(|e| e.id == *entry_id) {
                    return Err(format!("Unknown State entry: {entry_id}"));
                }
                if *lifecycle != EntryLifecycle::Active {
                    statements.remove(entry_id);
                } else if let Some(entry) = entries.iter().find(|e| e.id == *entry_id) {
                    statements.insert(entry_id.clone(), normalize_state_statement(&entry.text));
                }
                continue;
            }
        };
        for relation in relations {
            if !kinds.contains_key(&relation.target) {
                return Err(format!("{id}: Unknown relation target {}", relation.target));
            }
        }
        edges.insert(
            id.clone(),
            relations
                .iter()
                .filter(|r| {
                    matches!(
                        r.kind,
                        EntryRelationKind::DerivedFrom | EntryRelationKind::MotivatedBy
                    )
                })
                .map(|r| r.target.clone())
                .collect(),
        );
        statements.insert(id.clone(), normalize_state_statement(statement));
    }
    let mut unique = HashSet::new();
    for statement in statements.values() {
        if !unique.insert(statement) {
            return Err("Equivalent active Research Entry already exists".into());
        }
    }
    for id in synthesis
        .unresolved_entry_ids
        .iter()
        .chain(&synthesis.next_direction_entry_ids)
        .chain(
            outcome
                .task_outcomes
                .iter()
                .flat_map(|t| &t.motivating_entry_ids),
        )
        .chain(
            outcome
                .display_items
                .iter()
                .filter_map(|item| item.state_entry_ref.as_ref()),
        )
    {
        if !kinds.contains_key(id) {
            return Err(format!("Unknown State entry reference: {id}"));
        }
    }
    let mut done = HashSet::new();
    for id in edges.keys() {
        visit_dependencies(id, &edges, &mut HashSet::new(), &mut done)?;
    }
    Ok(())
}

/// Reject cycles only in premise edges, leaving contest/supersede semantics intact.
fn visit_dependencies(
    id: &str,
    edges: &HashMap<String, Vec<String>>,
    visiting: &mut HashSet<String>,
    done: &mut HashSet<String>,
) -> StoreResult<()> {
    if done.contains(id) {
        return Ok(());
    }
    if !visiting.insert(id.into()) {
        return Err(format!("Dependency cycle at State entry: {id}"));
    }
    if let Some(targets) = edges.get(id) {
        for target in targets {
            visit_dependencies(target, edges, visiting, done)?;
        }
    }
    visiting.remove(id);
    done.insert(id.into());
    Ok(())
}

/// Verify delivered source versions and convert their exact quotes into links.
fn prepare_evidence(
    conn: &Connection,
    run: &HarnessRun,
    items: &[crate::domain::harness::ResearchSynthesisEvidence],
    anchors: &HashMap<String, PassageAnchor>,
) -> StoreResult<Vec<EvidenceLinkDraft>> {
    let mut drafts = Vec::new();
    let mut chunks = HashSet::new();
    for item in items {
        let delivered: bool = conn.query_row("select exists(select 1 from agent_reader_passages where run_id=?1 and passage_ref=?2)",params![run.id,item.passage_ref],|r|r.get(0)).map_err(|e| e.to_string())?;
        if !delivered {
            return Err(format!(
                "State synthesis cites a passage the Run did not read: {}",
                item.passage_ref
            ));
        }
        let anchor = anchors
            .get(&item.passage_ref)
            .ok_or_else(|| format!("Missing captured passage: {}", item.passage_ref))?;
        let captured_version: String = conn.query_row("select source_version from agent_passage_anchors where run_id=?1 and passage_ref=?2",params![run.id,item.passage_ref],|r|r.get(0)).map_err(|e|format!("Missing durable passage: {e}"))?;
        if captured_version != source_version(conn, anchor)? {
            return Err(format!(
                "Source/project conflict: extraction or source changed: {}",
                item.passage_ref
            ));
        }
        let chunk_id = anchor.chunk_id.as_ref().ok_or_else(|| {
            format!(
                "Passage is context only; choose citable evidence: {}",
                item.passage_ref
            )
        })?;
        if !chunks.insert(chunk_id) {
            return Err(format!("Choose one evidence link per chunk: {chunk_id}"));
        }
        let draft = EvidenceLinkDraft {
            chunk_id: chunk_id.clone(),
            excerpt: Some(anchor.quote.clone()),
            support_note: Some(item.explanation.clone()),
        };
        let chunk = resolve_evidence_chunk(conn, &run.project_id, &draft)?;
        if chunk.source_id != anchor.source_id
            || Some(&chunk.extraction_id) != anchor.extraction_id.as_ref()
            || !chunk.text.contains(&anchor.quote)
        {
            return Err(format!(
                "Source/project conflict: captured passage changed: {}",
                item.passage_ref
            ));
        }
        drafts.push(draft);
    }
    Ok(drafts)
}

/// Translate validated proposal operations to the existing State write contract.
fn prepare_changes(
    conn: &Connection,
    run: &HarnessRun,
    synthesis: &ResearchStateSynthesis,
    anchors: &HashMap<String, PassageAnchor>,
) -> StoreResult<Vec<AgentStateChange>> {
    synthesis
        .changes
        .iter()
        .map(|change| {
            Ok(match change {
                ResearchSynthesisChange::Create {
                    handle,
                    kind,
                    epistemic_status,
                    statement,
                    evidence: links,
                    relations,
                    reason,
                } => AgentStateChange::Create {
                    operation_key: handle.clone(),
                    draft: ResearchEntryDraft {
                        kind: *kind,
                        epistemic_status: *epistemic_status,
                        text: statement.clone(),
                        evidence: prepare_evidence(conn, run, links, anchors)?,
                        relations: relations
                            .iter()
                            .map(|r| EntryRelationDraft {
                                target_entry_id: r.target.clone(),
                                kind: r.kind,
                            })
                            .collect(),
                        context: vec![],
                        reason: Some(reason.clone()),
                    },
                    evidence_relationships: links.iter().map(|e| e.relationship.clone()).collect(),
                },
                ResearchSynthesisChange::Revise {
                    entry_id,
                    epistemic_status,
                    statement,
                    evidence: links,
                    relations,
                    reason,
                } => AgentStateChange::Revise {
                    update: ResearchEntryUpdate {
                        id: entry_id.clone(),
                        epistemic_status: *epistemic_status,
                        text: statement.clone(),
                        evidence: prepare_evidence(conn, run, links, anchors)?,
                        relations: relations
                            .iter()
                            .map(|r| EntryRelationDraft {
                                target_entry_id: r.target.clone(),
                                kind: r.kind,
                            })
                            .collect(),
                        context: vec![],
                        reason: Some(reason.clone()),
                    },
                    evidence_relationships: links.iter().map(|e| e.relationship.clone()).collect(),
                },
                ResearchSynthesisChange::SetLifecycle {
                    entry_id,
                    lifecycle,
                    reason,
                } => AgentStateChange::SetLifecycle {
                    entry_id: entry_id.clone(),
                    lifecycle: *lifecycle,
                    reason: reason.clone(),
                },
            })
        })
        .collect()
}

/// Snapshot the source and extraction identities used by a delivered passage.
fn source_version(conn: &Connection, anchor: &PassageAnchor) -> StoreResult<String> {
    let mut version: String = conn.query_row("select json_array(s.id,s.status,s.updated_at,s.local_path,p.active_source_id,p.active_extraction_id,e.status,e.updated_at)
        from document_sources s join papers p on p.id=s.paper_id left join document_extractions e on e.id=?2
        where s.id=?1 and s.paper_id=?3",params![anchor.source_id,anchor.extraction_id,anchor.paper_id],|r|r.get(0))
        .map_err(|e|format!("Source/project conflict: {e}"))?;
    let source = read_document_source(conn, &anchor.source_id)?;
    if source.source_kind == "html" {
        let text = crate::services::mcp::read_html_source_text(&source)
            .map_err(|e| format!("Source/project conflict: {e}"))?;
        version.push_str(&short_sha256(&text));
    }
    Ok(version)
}
