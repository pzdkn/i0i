# RFC 0150: Readable Research Report Citations

- Status: Implemented
- Date: 2026-09-09
- Depends on: RFCs 0141, 0147, 0148, and 0149
- Scope: citations and internal identifiers in completed Research Run reports

## Problem

The latest Run report can display prose such as:

```text
FINDING Circuit discovery can answer a bounded causal-localization question
(passage_6808fafdf65043089eac2708db0363ea).
```

`passage_...` is a session/storage key. It is meaningful to finalization, but not
to a reader. The same class of leak previously exposed `research_entry_...` keys
in entry relationships; RFC 0149 repaired that surface.

The report leak has a different cause. `displayItems[].text`, `summary`, open
questions, and next direction are free model prose. `displayItems` has no
structured citation field, so the frontend cannot reliably distinguish a source
reference from ordinary text or navigate it to Reader evidence.

## Decision

Research reports use structured references and human-readable links. Internal
keys remain part of the persisted machine contract but never appear as display
labels or prose in the interface.

Render the example as:

```text
FINDING
Circuit discovery can answer a bounded causal-localization question. [1]

[1] Circuit Tracing: Revealing Computational Graphs in Language Models · p. 4
```

The numbered marker and source row are links. Activating either opens the paper,
scrolls to the exact captured passage, and paints the existing temporary Reader
highlight over that passage. The link therefore answers both "which paper?" and
"where in the paper?" without creating a permanent user annotation.

The durable passage anchor already carries the paper, source, page range, source
offsets, and exact quote. For a PDF, the Reader resolves that quote against the
page text layer to recover highlight rectangles. For HTML, it highlights the
stored source-offset range. If exact geometry can no longer be recovered, open
the cited page and show the quoted passage with an honest location-unavailable
notice; never highlight a fuzzy match elsewhere.

Use restrained inline numbers because a report item may cite multiple papers and
paper titles are too long to place inside prose. Do not use raw-id chips, full
quotes inside the compact report, or a modal citation viewer.

## Outcome Contract

Extend each `ResearchOutcomeItem` with:

```text
citedPassageRefs: string[]
stateEntryRef?: string
```

- `text` contains readable prose only.
- `citedPassageRefs` contains the exact passages supporting or motivating that
  displayed item. Empty is valid for clearly identified synthesis or speculation.
- `stateEntryRef` points to an existing entry id or a create handle from the same
  synthesis. Finalization resolves create handles to durable entry ids.
- These fields are distinct: the State-entry link opens the durable knowledge
  object; passage citations open the underlying source evidence.

Validate passage references against the Run's durable delivered anchors and
Project scope. Resolve citation presentation from stored source/chunk metadata:
paper title, optional page, excerpt, and the existing Reader navigation target.
The frontend must not infer those values from an identifier.

The model-facing schema and prompt must say explicitly: place references only in
structured reference fields; never write `passage_...`, `research_entry_...`,
search-run ids, or local create handles into prose.

## Presentation Model

Do not make `ProjectResearch.svelte` join storage records. The backend returns a
small report view model for each item:

```text
kind
text
stateEntryId?             // internal navigation target, never displayed
citations[]
  label                   // paper title plus optional page
  navigationTarget        // source, extraction, chunk, offsets, excerpt
```

Keep canonical persisted outcomes useful to the research continuation while
projecting a display-safe representation for the UI. Citation numbers follow the
item's structured citation order and restart for each item. Duplicate references
within one item are rejected or deduplicated before display.

## Existing Reports

Historical outcomes already contain inline passage ids and cannot be regenerated.
Apply a compatibility projection when reading them:

1. Detect only the application's exact internal-id forms.
2. Remove them from visible prose, including surrounding empty parentheses.
3. Resolve a detected passage through that Run's durable anchor table.
4. Show a numbered citation only when resolution succeeds.
5. If resolution fails, omit the link and show clean prose; never expose the id
   or invent a paper/page association.

This narrow parser is only a historical display migration. New outcomes must use
structured fields. Regex matching must never be the authority for source
navigation; the durable anchor and Project checks remain authoritative.

Apply the same display-safe prose projection to report summary, unanswered
questions, next direction, and continuation text so an internal key cannot leak
through a neighboring report surface. Those fields do not gain implicit
citations; only a structured or successfully resolved historical reference creates
a source link.

## Interaction And Accessibility

- Citation links use familiar numbered markers, with an accessible name such as
  "Open source 1: Circuit Tracing, page 4".
- Source rows display the paper title and page only when the page is real.
- Keyboard activation uses ordinary buttons or links and visible focus styling.
- Opening evidence follows the existing paper-opening callback and highlights the
  exact passage with the temporary citation highlight, matching Research State
  evidence behavior. The highlight fades after the existing Reader timeout.
- A linked State item uses its readable statement as tooltip/accessibility text
  and opens through RFC 0149's revision-aware entry navigation.
- Long titles wrap within the inspector and do not resize the panel.

## Verification

Use local fixtures only; no paid model or Research Run is required.

1. Deserialize a new structured outcome with one and multiple citations.
2. Reject or safely deduplicate unknown, unread, cross-Project, and duplicate refs.
3. Verify create handles become durable entry navigation targets after commit.
4. Project a historical inline passage id to clean prose and a resolvable source.
5. Project an unresolved historical id to clean prose with no fabricated link.
6. Verify internal-id forms never appear in summary, items, questions, direction,
   continuation, accessible names, or tooltips.
7. Component tests verify citation numbering, keyboard activation, paper opening,
   exact navigation payload, temporary passage highlighting, long titles, and
   absent page numbers.
8. Run focused Rust tests, `pnpm test:ui`, and `pnpm check`.

## Acceptance

- No `passage_...`, `research_entry_...`, search-run id, or local handle is shown
  as prose or a label anywhere in a Research Run report.
- New report items carry citations structurally and only durable, scoped evidence
  produces a clickable source link.
- Citation activation opens and highlights the exact Reader passage.
- Where an item corresponds to a State change, the report can open the readable
  Research Entry without displaying its id.
- Existing saved reports become readable without rewriting historical rows.
- Report-citation cleanup cannot fail or mutate Research State finalization.

## Non-Goals

- Renaming database ids or changing passage-anchor identity.
- Adding academic bibliography formatting or citation export.
- Redesigning the Research report, Reader, or State inspector.
- Inferring citations from arbitrary prose or matching quoted text heuristically.

## Implementation

- Managed outcomes carry passage and State-entry references in structured fields.
- Checkpoints project those references to readable, Project-scoped report links.
- Historical inline identifiers are removed from display and continuation prose;
  only durable passage anchors produce citation links.
- Citation markers and source rows open the Reader at the exact passage using its
  existing temporary evidence highlight.
- Verified with 641 local Rust tests, 8 UI tests, and a clean Svelte check.
