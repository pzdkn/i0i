# Today's RFC List

Updated: 2026-09-09

This is the working index for active RFCs and older documents reviewed on
2026-08-24.

- **Open** means meaningful implementation or verification work remains.
- **Superseded** means later RFCs delivered or replaced the document's active
  scope; any remaining ideas require new focused RFCs.
- **Stale** means the document has been superseded by later RFCs or the current
  implementation. It remains useful as design history, but is not a backlog
  item without a fresh review.

## Open

1. [RFC 0107: Relevant research history for paper chats](rfcs/chat/0107-relevant-research-history-for-paper-chats.md)
   — search same-paper notes and earlier conversations into each chat's bounded,
   inspectable context.
2. [RFC 0087: Dead paths in vault surfaces](rfcs/frontend-and-mocking/0087-dead-paths-in-the-vault-surfaces.md)
   — implement **Ask this vault**; the other sections have landed.
3. [RFC 0088: Deep Research harness](rfcs/discovery/0088-deep-research-harness.md)
   — finish evaluation fixtures and decide which remaining harness tasks still
   justify implementation; browser candidate generation moves to RFC 0098.

## Deferred

1. [RFC 0089: Paper quiz](rfcs/study/0089-paper-quiz.md)
   — add paper-grounded quiz generation, grading, and quiz sessions.
2. [RFC 0090: Paper references in notes](rfcs/reader/0090-paper-references-in-notes.md)
   — finish the `[@` paper autocomplete interaction.
3. [RFC 0092: Weekly digest](rfcs/study/0092-weekly-digest.md)
   — summarize the user's actual reading, notes, questions, and quiz activity.
4. [RFC 0093: Study mode](rfcs/study/0093-study-mode.md)
   — define and implement the home for quizzes, digests, and learning history.

## Finished

Implementation is landed for these RFCs. Any remaining manual or live
verification is noted below and remains reflected in each RFC's formal status.

1. [RFC 0153: Agent-neutral resilient scholarly search](rfcs/projects/0153-agent-neutral-resilient-scholarly-search.md)
   — Quick Find and Project Research rotate DuckDuckGo, Ecosia, and Brave,
   retain partial results, expose provider outcomes, and fail honestly when all
   free browser providers are unavailable.
2. [RFC 0124: Simple incremental Project Research](rfcs/projects/0124-simple-incremental-project-research.md)
   — one instruction and one bounded Run now enrich the Project automatically;
   older Runs, settings, and technical Activity are progressively disclosed.
2. [RFC 0123: Reliable Harness planner responses and Run smoke test](rfcs/projects/0123-reliable-harness-planner-responses-and-run-smoke-test.md)
   — nullable planner responses are classified safely, retried once, and
   covered by deterministic pipeline and opt-in live smoke tests.
3. [RFC 0122: Run finalization at the safe boundary](rfcs/projects/0122-run-finalization-safe-boundary.md)
   — Harness Runs remain active through reconciliation and only become
   terminal after Change Set, usage, reflection, and checkpoint facts are
   durable.
4. [RFC 0121: Production operational reflection](rfcs/projects/0121-production-operational-reflection.md)
   — production reconciliation now records validated operational reflections
   from actual telemetry and recurring observations create reviewable Harness
   improvements.
5. [RFC 0120: State-informed Run orientation](rfcs/projects/0120-state-informed-run-orientation.md)
   — each search is planned from its immutable Research State, prior next
   direction, and bounded same-Project operational observations.
6. [RFC 0108: Project-scoped autonomous research harness](rfcs/projects/0108-project-scoped-autonomous-research-harness.md)
   — the complete design is implemented through focused RFCs 0109–0122:
   bounded recurring Runs now reconcile Project knowledge, retain checkpoints,
   and expose the intended Project Research workspace.
7. [RFC 0119: Research workspace conformance and accessibility](rfcs/projects/0119-research-workspace-conformance-and-accessibility.md)
   — the Research workspace now provides counted and sortable State, Run/cycle
   filtering, keyboard focus restoration, grouped Run Activity, checkpoint
   review, authority inspection, and document citation provenance. Automated
   checks pass; sandbox localhost policy prevented runtime screenshots.
8. [RFC 0118: Durable Run checkpoints and structured Activity](rfcs/projects/0118-durable-run-checkpoints-and-structured-activity.md)
   — terminal Runs now retain actual usage, typed Activity, monotonic
   State/Vault/document revisions, complete checkpoint facts, and append-only
   same-Project State restoration.
9. [RFC 0117: Autonomous Run reconciliation and review](rfcs/projects/0117-autonomous-run-reconciliation-and-review.md)
   — completed searches now produce validated Change Sets with exhaustive
   candidate decisions, exact abstract evidence, review controls, and atomic
   Paper/State application.
10. [RFC 0116: Harness authority and effective instructions](rfcs/projects/0116-harness-authority-and-effective-instructions.md)
   — Harness Settings now persist bounded autonomy and write authority, every
   configuration version is immutable, and each Run exposes its effective
   policy, researcher instructions, structured settings, and bounded context.
11. [RFC 0115: Create Project documents from research](rfcs/projects/0115-create-project-documents-from-research.md)
   — selected entries from an exact Research State revision now create ordinary
   Markdown documents with epistemic qualification and durable citations.
12. [RFC 0114: Harness reflection and improvement proposals](rfcs/projects/0114-harness-reflection-and-improvement-proposals.md)
   — completed Runs now retain bounded operational reflections, and recurring
   telemetry can produce typed, reviewable improvements without silently
   changing policy or Project knowledge.
13. [RFC 0113: Scheduled Research Runs and stopping](rfcs/projects/0113-scheduled-research-runs-and-stopping.md)
   — Research Harnesses now persist daily or weekly local schedules, claim one
   bounded startup catch-up, recover interrupted Runs, and enforce explicit
   lifecycle and stop controls.
14. [RFC 0112: Typed Research State and evidence links](rfcs/projects/0112-typed-research-state-and-evidence-links.md)
   — Projects now have revisioned Findings, Questions, Gaps, Hypotheses, and
   Experiment Ideas with enforced epistemic status, durable Paper evidence,
   separate working context, history, and an interactive inspector.
15. [RFC 0111: Research Harness configuration and manual Runs](rfcs/projects/0111-research-harness-configuration-and-manual-runs.md)
   — every Project now has a persisted Harness with versioned settings,
   immutable manual Runs, cancellation, and append-only Activity.
16. [RFC 0110: Project Markdown documents](rfcs/projects/0110-project-markdown-documents.md)
   — Projects now contain ordinary Markdown documents with durable CRUD,
   explicit Harness write consent, and a Project Documents editor.
17. [RFC 0109: Project and Vault ownership migration](rfcs/projects/0109-project-vault-ownership-migration.md)
   — Projects now atomically own one Vault, existing Vaults migrate without
   identity or membership changes, and the Explorer uses Project lifecycle
   actions.
18. [RFC 0100: Eager Obscura startup and discovery gate](rfcs/source-acquisition/0100-eager-obscura-startup-and-discovery-gate.md)
   — implementation and automated lifecycle checks pass; manual verification
   of the Starting, Failed, Retry, and Ready UI states remains.
19. [RFC 0101: Honest chat web-search routing](rfcs/chat/0101-honest-chat-web-search-routing.md)
   — deterministic routing and typed lookup outcomes are implemented; one live
   successful lookup and one forced browser-failure answer check remain.
20. [RFC 0096: Fluid PDF highlight rendering](rfcs/reader/0096-fluid-pdf-highlight-rendering.md)
   — implementation is landed; manual PDF visual checks remain.
21. [RFC 0097: Research-connected chat with epistemic boundaries](rfcs/chat/0097-research-connected-chat-with-epistemic-boundaries.md)
   — production paths are landed; the live prompt-quality comparison remains.
22. [RFC 0098: Browser-first scholarly discovery](rfcs/discovery/0098-browser-first-scholarly-discovery.md)
   — production paths are landed; the fixed-corpus live migration benchmark
   remains.
23. [RFC 0099: Reliable math in chat and notes](rfcs/chat/0099-reliable-math-in-chat-and-notes.md)
   — implementation is landed; packaged-app offline visual verification
   remains.

## Superseded

1. [RFC 0052: Reliable Obscura sessions and web-reader fallback](rfcs/source-acquisition/0052-reliable-obscura-and-web-reader-fallback.md)
   — later Obscura lifecycle, stealth, discovery, HTML Reader, and durable web
   source RFCs delivered or replaced its active scope.
