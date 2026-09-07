# Research Loop Evaluation

- Status: fail
- Scenario: live_discovery_smoke
- Revision: `a45c045ec9de152823bd7e1397aa2a13f2f40c2c`
- Dirty worktree: true
- Agent model: `gpt-5.6-sol`
- Judge model: `gpt-5.5`
- Scenario process limit: 1200s
- Research Run limit: 420s
- Started: 2026-09-07T14:20:57.821754+00:00

## Checks

- **pass** `production_run_terminal`: 1 Run(s): harness_run_1788790859215471000=ready
- **fail** `new_paper_collected`: Added papers: 
- **pass** `no_duplicate_membership`: 0 unique membership rows
- **fail** `state_revision_advanced`: State revision 1 -> 1
- **fail** `evidence_references_resolve`: Resolved 0 new evidence link(s) against stored chunks
- **pass** `state_persists_after_reopen`: Reopened current revision 1; historical revision 1 readable=true
- **fail** `passage_read_before_state_update`: Completed tool order: state_read -> vault_list_papers -> search_start -> search_get -> search_get -> search_start -> search_get -> search_start -> search_get
- **pass** `run_within_limits`: Persisted usage stayed within each Run's captured limits
- **fail** `live_source_hashed`: Recorded 0 acquired source hash(es)
- **fail** `full_text_read`: Reader coverage observed: {}
- **pass** `evaluation_data_cleaned_up`: Removed isolated evaluation root /var/folders/7x/_h0bp8250hdg_jcb_wsy6zlr0000gp/T/i0i-research-eval-dcde0ced0a8249b2a0fdf9c407e7f75f

## LLM Judge

Judge did not produce a conclusive result.
