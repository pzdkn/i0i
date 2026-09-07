# Research Loop Evaluation

- Status: pass
- Scenario: live_discovery_smoke
- Revision: `a45c045ec9de152823bd7e1397aa2a13f2f40c2c`
- Dirty worktree: true
- Agent model: `gpt-5.6-sol`
- Judge model: `gpt-5.5`
- Scenario process limit: 1200s
- Research Run limit: 420s
- Started: 2026-09-07T14:39:32.173477+00:00

## Checks

- **pass** `production_run_terminal`: 1 Run(s): harness_run_1788791973426145000=ready
- **pass** `new_paper_collected`: Added papers: web:a7468c685165
- **pass** `no_duplicate_membership`: 1 unique membership rows
- **pass** `state_revision_advanced`: State revision 1 -> 2
- **pass** `evidence_references_resolve`: Resolved 2 new evidence link(s) against stored chunks
- **pass** `state_persists_after_reopen`: Reopened current revision 2; historical revision 1 readable=true
- **pass** `passage_read_before_state_update`: Completed tool order: state_read -> vault_list -> vault_list_papers -> search_start -> search_get -> vault_add_paper -> vault_get_paper -> reader_read -> state_update -> vault_get_paper -> state_read
- **pass** `run_within_limits`: Persisted usage stayed within each Run's captured limits
- **pass** `live_source_hashed`: Recorded 1 acquired source hash(es)
- **pass** `full_text_read`: Reader coverage observed: {"full_text"}
- **pass** `evaluation_data_cleaned_up`: Removed isolated evaluation root /var/folders/7x/_h0bp8250hdg_jcb_wsy6zlr0000gp/T/i0i-research-eval-c7fdf602d2ad494b914d952baadff479

## LLM Judge

Strong result overall: it found the canonical arXiv paper, cached an auditable PDF/full-text source, read the relevant architecture passages, and stored an accurate cited finding. Small care issue: the original question was left active and the run did not demonstrate reading the entire paper, only the relevant full-text section.

- `iterative_adaptation`: None - Not applicable for the live_discovery_smoke one-iteration scenario per the supplied oneIterationRule.
- `grounding`: 3 - The finding is well grounded in the cited architecture passages. Chunk 5 states the encoder-decoder setup, autoregressive decoder generation, stacked self-attention and point-wise feed-forward layers, six encoder layers, residual connections, and layer normalization. Chunk 6 states the six-layer decoder, encoder-output attention, residual/layer normalization, and masking so predictions depend only on prior outputs. The source is the cached arXiv PDF for paper web:a7468c685165, sourceId pdf:web:a7468c685165:c3ab01e47e61.
- `epistemic_care`: 2 - The run used source_supported status and attached two direct supporting excerpts rather than relying on the abstract. It also verified the paper metadata and cached PDF/full_text availability via vault_get_paper exec-c1d77372-cd27-4967-939d-a591d3df0490 and exec-52663c7a-212e-45fe-8905-66f615f7ff8e. Minor limitation: it did not retire or update the original speculative question, and the paper status remained UNREAD despite relevant passages being read.
- `relevance`: 3 - The run directly answered the active question and research instruction by finding the original 2017 Attention Is All You Need record, reading the full-text extraction around Section 3 Model Architecture, and recording a cited architecture finding motivated by the initial question.
- `state_improvement`: 3 - The final state improves revision 1 by adding one active source-supported finding with two evidence records and a motivated_by relation to the original question. This substantially answers the initial speculative question, though the original question remains active rather than being explicitly resolved.
