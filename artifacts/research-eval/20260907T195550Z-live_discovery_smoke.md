# Research Loop Evaluation

- Status: pass
- Scenario: live_discovery_smoke
- Revision: `d192b69211a6db6ec7c812c9106a090a9fa0e9a3`
- Dirty worktree: false
- Codex: `codex-cli 0.153.4`
- Agent model: `gpt-5.6-sol`
- Judge model: `gpt-5.5`
- Scenario process limit: 1200s
- Research Run limit: 420s
- Started: 2026-09-07T19:55:50.948381+00:00

## Checks

- **pass** `production_run_terminal`: 1 Run(s): harness_run_1788810951492154000=ready
- **pass** `new_paper_collected`: Added papers: web:a7468c685165
- **pass** `no_duplicate_membership`: 1 unique membership rows
- **pass** `state_revision_advanced`: State revision 1 -> 2
- **pass** `evidence_references_resolve`: Resolved 1 new evidence link(s) against stored chunks
- **pass** `state_persists_after_reopen`: Reopened current revision 2; historical revision 1 readable=true
- **pass** `passage_read_before_state_update`: Completed tool order: state_read -> vault_list_papers -> search_start -> search_get -> search_start -> search_get -> search_get -> search_get -> vault_add_paper -> vault_get_paper -> reader_read -> state_update -> state_read -> vault_get_paper -> search_get
- **pass** `run_within_limits`: Persisted usage stayed within each Run's captured limits
- **pass** `live_source_hashed`: Recorded 1 acquired source hash(es)
- **pass** `full_text_read`: Reader coverage observed: {"full_text"}
- **pass** `evaluation_data_cleaned_up`: Removed isolated evaluation root /var/folders/7x/_h0bp8250hdg_jcb_wsy6zlr0000gp/T/i0i-research-eval-fce68e3584ba4e5980e71b3ba1c32df5

## LLM Judge

Strong result: it found the canonical original paper, cached an auditable PDF source, read the relevant full-text architecture passage, and recorded an accurate cited finding. The only notable weakness is that the trace shows focused reading of pages 2-4 rather than an explicit full-paper inspection.

- `state_improvement`: 3 - The state improved from a single speculative question with no evidence at revision 1 to an active source_supported finding at revision 2, with one supporting evidence record and a motivated_by relation back to the question.
- `grounding`: 3 - The finding is directly supported by passage_1e1feb95064042b8a44ad3b4e391d1b9 / chunk 5, which states that the Transformer follows an encoder-decoder architecture using stacked self-attention and point-wise fully connected layers, and that the encoder has N=6 identical layers with multi-head self-attention, position-wise feed-forward networks, residual connections, and layer normalization.
- `iterative_adaptation`: None - Not applicable under the supplied oneIterationRule for live_discovery_smoke / one_iteration scenarios.
- `relevance`: 3 - The created finding directly answers the initial question and research instruction by explaining the original Attention Is All You Need Transformer architecture, using the original arXiv paper web:a7468c685165 as the source.
- `epistemic_care`: 2 - The run distinguished the canonical arXiv paper from derivative/search results, including a failed first search and a second targeted search for arXiv:1706.03762; it then verified full_text availability via vault_get_paper exec-45c2d03f-04d6-40ff-8343-36084f7303a1 and cited a passage rather than relying on metadata. Minor limitation: it read the relevant model-architecture pages, not demonstrably the entire paper end to end.
