# Today's RFC List

Updated: 2026-09-02

This is the working index for active RFCs and older documents reviewed on
2026-08-24.

- **Open** means meaningful implementation or verification work remains.
- **Superseded** means later RFCs delivered or replaced the document's active
  scope; any remaining ideas require new focused RFCs.
- **Stale** means the document has been superseded by later RFCs or the current
  implementation. It remains useful as design history, but is not a backlog
  item without a fresh review.

## Open

1. [RFC 0108: Project-scoped autonomous research harness](rfcs/projects/0108-project-scoped-autonomous-research-harness.md)
   — establish Projects with one Vault, ordinary Project documents, and a
   project-scoped Harness with typed Research State, Runs, and Activity; delivery
   is split into focused follow-up RFCs.
2. [RFC 0107: Relevant research history for paper chats](rfcs/chat/0107-relevant-research-history-for-paper-chats.md)
   — search same-paper notes and earlier conversations into each chat's bounded,
   inspectable context.
3. [RFC 0087: Dead paths in vault surfaces](rfcs/frontend-and-mocking/0087-dead-paths-in-the-vault-surfaces.md)
   — implement **Ask this vault**; the other sections have landed.
4. [RFC 0088: Deep Research harness](rfcs/discovery/0088-deep-research-harness.md)
   — finish evaluation fixtures and decide which remaining harness tasks still
   justify implementation; browser candidate generation moves to RFC 0098.

## Defered
5. [RFC 0089: Paper quiz](rfcs/study/0089-paper-quiz.md)
   — add paper-grounded quiz generation, grading, and quiz sessions.
6. [RFC 0090: Paper references in notes](rfcs/reader/0090-paper-references-in-notes.md)
   — finish the `[@` paper autocomplete interaction.
7. [RFC 0092: Weekly digest](rfcs/study/0092-weekly-digest.md)
   — summarize the user's actual reading, notes, questions, and quiz activity.
8. [RFC 0093: Study mode](rfcs/study/0093-study-mode.md)
   — define and implement the home for quizzes, digests, and learning history.

## Finished

Implementation is landed for these RFCs. Any remaining manual or live
verification is noted below and remains reflected in each RFC's formal status.

1. [RFC 0110: Project Markdown documents](rfcs/projects/0110-project-markdown-documents.md)
   — Projects now contain ordinary Markdown documents with durable CRUD,
   explicit Harness write consent, and a Project Documents editor.
2. [RFC 0109: Project and Vault ownership migration](rfcs/projects/0109-project-vault-ownership-migration.md)
   — Projects now atomically own one Vault, existing Vaults migrate without
   identity or membership changes, and the Explorer uses Project lifecycle
   actions.
3. [RFC 0100: Eager Obscura startup and discovery gate](rfcs/source-acquisition/0100-eager-obscura-startup-and-discovery-gate.md)
   — implementation and automated lifecycle checks pass; manual verification
   of the Starting, Failed, Retry, and Ready UI states remains.
4. [RFC 0101: Honest chat web-search routing](rfcs/chat/0101-honest-chat-web-search-routing.md)
   — deterministic routing and typed lookup outcomes are implemented; one live
   successful lookup and one forced browser-failure answer check remain.
5. [RFC 0096: Fluid PDF highlight rendering](rfcs/reader/0096-fluid-pdf-highlight-rendering.md)
   — implementation is landed; manual PDF visual checks remain.
6. [RFC 0097: Research-connected chat with epistemic boundaries](rfcs/chat/0097-research-connected-chat-with-epistemic-boundaries.md)
   — production paths are landed; the live prompt-quality comparison remains.
7. [RFC 0098: Browser-first scholarly discovery](rfcs/discovery/0098-browser-first-scholarly-discovery.md)
   — production paths are landed; the fixed-corpus live migration benchmark
   remains.
8. [RFC 0099: Reliable math in chat and notes](rfcs/chat/0099-reliable-math-in-chat-and-notes.md)
   — implementation is landed; packaged-app offline visual verification
   remains.

## Superseded

1. [RFC 0052: Reliable Obscura sessions and web-reader fallback](rfcs/source-acquisition/0052-reliable-obscura-and-web-reader-fallback.md)
   — later Obscura lifecycle, stealth, discovery, HTML Reader, and durable web
   source RFCs delivered or replaced its active scope.
