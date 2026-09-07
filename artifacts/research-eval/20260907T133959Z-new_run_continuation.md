# Research Loop Evaluation

- Status: fail
- Scenario: new_run_continuation
- Revision: `a45c045ec9de152823bd7e1397aa2a13f2f40c2c`
- Dirty worktree: true
- Agent model: `gpt-5.6-sol`
- Judge model: `gpt-5.5`
- Scenario process limit: 1200s
- Research Run limit: 300s
- Started: 2026-09-07T13:40:00.209103+00:00

## Checks

- **fail** `production_run_terminal`: 1 Run(s): harness_run_1788788400964230000=cancelled
- **pass** `new_paper_collected`: Added papers: fixture:adaptive-clipping-support, fixture:adaptive-clipping-conflict, fixture:federated-clipping-abstract, fixture:long-horizon-unavailable
- **pass** `no_duplicate_membership`: 4 unique membership rows
- **pass** `state_revision_advanced`: State revision 1 -> 3
- **pass** `evidence_references_resolve`: Resolved 3 new evidence link(s) against stored chunks
- **pass** `state_persists_after_reopen`: Reopened current revision 3; historical revision 1 readable=true
- **pass** `passage_read_before_state_update`: Completed tool order: state_read -> vault_list_papers -> search_start -> search_start -> search_get -> search_get -> vault_add_paper -> vault_add_paper -> vault_add_paper -> vault_add_paper -> vault_get_paper -> vault_get_paper -> vault_get_paper -> vault_get_paper -> reader_read -> reader_read -> reader_read -> reader_read -> search_start -> search_get -> state_update -> state_read -> state_update
- **pass** `run_within_limits`: Persisted usage stayed within each Run's captured limits
- **pass** `full_text_read`: Reader coverage observed: {"full_text", "none", "abstract_only"}
- **pass** `limited_coverage_reported_honestly`: Reader coverage observed: {"full_text", "none", "abstract_only"}
- **fail** `new_thread_continues_persisted_state`: Second Run missing
- **pass** `evaluation_data_cleaned_up`: Removed isolated evaluation root /var/folders/7x/_h0bp8250hdg_jcb_wsy6zlr0000gp/T/i0i-research-eval-9fa191dd5f33476bb34c0db72f60295a

## LLM Judge

Judge did not produce a conclusive result.

## Errors

- Run harness_run_1788788400964230000 ended as cancelled: time_limit
