# Today's RFC List

Updated: 2026-08-24

This is the working index for active RFCs and older documents reviewed on
2026-08-24.

- **Open** means meaningful implementation or verification work remains.
- **Stale** means the document has been superseded by later RFCs or the current
  implementation. It remains useful as design history, but is not a backlog
  item without a fresh review.

## Open

1. [RFC 0100: Eager Obscura startup and discovery gate](rfcs/source-acquisition/0100-eager-obscura-startup-and-discovery-gate.md)
   — implementation and automated lifecycle checks pass; manually verify the
   Starting, Failed, Retry, and Ready states in the running app.
2. [RFC 0101: Honest chat web-search routing](rfcs/chat/0101-honest-chat-web-search-routing.md)
   — deterministic routing and typed lookup outcomes are implemented; run one
   live successful lookup and one forced browser-failure answer check.
3. [RFC 0096: Fluid PDF highlight rendering](rfcs/reader/0096-fluid-pdf-highlight-rendering.md)
   — preserve precise anchors while rendering each passage as soft, unified
   bands and one logical interaction target. Implementation is landed; manual
   PDF visual checks remain.
4. [RFC 0097: Research-connected chat with epistemic boundaries](rfcs/chat/0097-research-connected-chat-with-epistemic-boundaries.md)
   — connect chat to library search, bounded web evidence, and asynchronous
   Deep Research while distinguishing evidence, inference, and hypothesis.
   Production paths are landed; the live prompt-quality comparison remains.
5. [RFC 0098: Browser-first scholarly discovery](rfcs/discovery/0098-browser-first-scholarly-discovery.md)
   — discover through Obscura by default and retain provider APIs only for
   identifiers and verified exact-title metadata resolution. Production paths
   are landed; the fixed-corpus live migration benchmark remains.
6. [RFC 0099: Reliable math in chat and notes](rfcs/chat/0099-reliable-math-in-chat-and-notes.md)
   — add typed math nodes and locally bundled KaTeX with stable streaming and
   readable failure behavior. Implementation is landed; packaged-app offline
   visual verification remains.
7. [RFC 0052: Reliable Obscura sessions and web-reader fallback](rfcs/source-acquisition/0052-reliable-obscura-and-web-reader-fallback.md)
   — finish the shared persistent browser session required by RFCs 0097 and
   0098.
8. [RFC 0087: Dead paths in vault surfaces](rfcs/frontend-and-mocking/0087-dead-paths-in-the-vault-surfaces.md)
   — implement **Ask this vault**; the other sections have landed.
9. [RFC 0088: Deep Research harness](rfcs/discovery/0088-deep-research-harness.md)
   — finish evaluation fixtures and decide which remaining harness tasks still
   justify implementation; browser candidate generation moves to RFC 0098.
10. [RFC 0089: Paper quiz](rfcs/study/0089-paper-quiz.md)
   — add paper-grounded quiz generation, grading, and quiz sessions.
11. [RFC 0090: Paper references in notes](rfcs/reader/0090-paper-references-in-notes.md)
   — finish the `[@` paper autocomplete interaction.
12. [RFC 0092: Weekly digest](rfcs/study/0092-weekly-digest.md)
   — summarize the user's actual reading, notes, questions, and quiz activity.
13. [RFC 0093: Study mode](rfcs/study/0093-study-mode.md)
   — define and implement the home for quizzes, digests, and learning history.
