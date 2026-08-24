# RFC 0038: Discover Search Windows And Deep Research UI

Status: Stale
Date: 2026-07-06
Product: i0i
Target: Tauri v2 + Svelte, macOS first
Builds on: RFC 0020 (Scout discovery model), RFC 0029 (open discovery candidates in Reader), RFC 0037 (deep-research agentic search)

## Summary

Discover should stay one coherent workspace. Shallow provider search and deep
research are two run modes from the same search surface, not two separate
panels. A user starts a search from the Discover header; each new search
creates a new **search window**. Results from shallow and deep runs appear in
the same candidate list shape and can be opened inside i0i like any existing
Discover result.

The right-most panel becomes the run inspector when no candidate is selected:
it shows run status, progress, search stats, and deep-research trace. When a
candidate is selected, the right panel shows paper metadata. If a run is still
active while the user inspects a candidate, the panel keeps a compact active-run
strip so progress remains reachable.

## Decisions

1. **Deep-research papers open inside i0i.** A candidate is a candidate,
   regardless of whether it came from shallow provider search or deep research.
2. **New search means new search window, for now.** Starting another search does
   not append to or replace the active result list. It creates a new Discover
   search window.
3. **Deep research is a toggle, not a second primary button.** The header keeps
   one `Run` action. When `Deep` is toggled off, `Run` performs shallow
   provider search. When `Deep` is toggled on, `Run` starts agentic deep
   research.
4. **Search progress lives in the right-most panel.** The right inspector owns
   run state, stats, and trace rather than putting progress into the result
   list or a modal.
5. **The Discover panel should be reduced to command essentials.** The default
   visible workflow is search window -> query -> optional `Deep` toggle ->
   `Run`. Filters, provider details, run trace, and provenance stay available,
   but they should not compete with the primary command surface.

## Goals

- Make shallow search and deep research feel like one Discover workflow.
- Reduce visible controls while preserving the same functionality through clear
  defaults and progressive disclosure.
- Keep result exploration uninterrupted while deep research runs.
- Preserve provenance by separating searches into windows instead of silently
  appending mixed results.
- Let every paper candidate open in Reader and follow the existing add-to-vault
  path.
- Give deep research enough visibility to feel inspectable without turning the
  app into an agent-console-first interface.

## Non-Goals

- No append/replace mode in this RFC.
- No scheduled-search UI.
- No full saved-search management UI beyond the active search windows.
- No deep-research report/briefer UI; RFC 0037 keeps that separate.
- No provider/filter backend redesign here. This RFC only changes where those
  controls should live in the UI.

## Mental Model

The user is in Discover and asks: "find papers for this research intent." The
search may be shallow or deep, but the result is still a search window
containing paper candidates. A search window is the user's current working
surface for one question.

Starting a new query creates a new surface instead of mutating the old one. That
is the safest first behavior because the user can compare searches, go back to
prior results, and avoid confusion about why a result appeared.

## UX Design

### Design Principle

Discover should reduce elements while improving intuitiveness. The main panel
should expose only the controls needed to express intent and run the search.
Configuration that is useful but not always needed should move behind compact
controls, the right inspector, or later saved-search settings.

The default surface should feel like a command line inside the IDE:

```text
search window -> query -> optional deep toggle -> run
```

This preserves functionality without making the user scan a form before every
search.

The visual direction should follow the original i0i design language:

- **Retro terminal:** a prompt-like query row, compact state, trace output in
  the inspector, and keyboard-friendly command flow.
- **IDE:** stable panes, search-window tabs, result lists, and an inspector
  rather than modal-heavy workflows.
- **Solarpunk:** calm, warm, readable density. Avoid cyberpunk noise, neon
  clutter, dashboard cards, and decorative effects that make the research
  instrument feel less direct.

The working phrase is:

```text
query, mode, run, inspect
```

### Discover Header

The Discover header keeps one input, one primary run action, and one depth
toggle.

```text
Discover
> [ query input                                      ] [ Deep ] [ Run ] [ tune ]
```

`Deep` is a toggle. Off means shallow provider search. On means agentic
deep research. This keeps the panel closer to a command console: the user sets
the mode, then runs the command.

`Run` is the only primary action in the header. If a run is active, it may
become `Stop` or an equivalent stop icon. The `tune` control is progressive
disclosure for filters and run settings; it should be visually secondary.

Optional compact variant:

```text
Discover
> [ query input                                      ] [ Deep: off ] [ Run ]
```

The UI should avoid separate `Quick` and `Deep` primary buttons because that
makes the header feel like two competing workflows. The mental model should be:
one query, one run action, optional deeper execution.

### Search Windows

Discover has a lightweight window/tab strip for search windows.

```text
Discover
[ + ] [ Time to Fly ] [ SAEs for Interp ] [ Deep: Video Synthesis ]

> [ query input                                      ] [ Deep ] [ Run ] [ tune ]
```

Behavior:

- Clicking `+` opens an empty search window.
- Running a query in an empty search window names it from the query.
- Running a query when a search window already has results creates a new search
  window automatically.
- The old window remains available with its result list and run metadata.
- Closing a search window removes transient results from the UI. Persisted saved
  papers remain in the vault.

This avoids an early append/replace decision. Append may return later as an
explicit advanced action, but it should not be the default.

The `+` belongs with the search-window strip, not in the query row. Its job is
navigation/workspace creation, not query editing. Keeping it near the tabs makes
the action read as "new search window" instead of "add a query clause."

### Tune And Filters

Provider, sort, year, access, limit, seeds, and deep-search depth are useful,
but they should not all be visible in the default command surface. They belong
behind `tune` and should use defaults until changed.

Suggested shallow defaults:

```text
provider: Auto
sort: Relevance
access: Open access preferred
limit: 25
year: Any
```

Suggested deep-only controls:

```text
depth: Standard
providers: Auto
seeds: None
budget: Default
stop condition: Default
```

The header may show compact chips only for meaningful non-default state:

```text
filters: 2021-2026 · most cited
```

Avoid always rendering a second row of filter widgets. That recreates the form
problem this RFC is trying to remove.

### Result List

Shallow and deep results use the same candidate card/list row shape. Deep
results may add extra provenance fields:

- compact source signal, for example `Deep` or `OpenAlex`
- one-line rationale when it is actually useful
- provider/query provenance available in the inspector
- first-seen run available in metadata

Every candidate supports the existing actions:

- open in Reader
- inspect metadata
- add to vault
- download/cache PDF after save

Opening a candidate from deep research should use the same Reader bridge as
other Discover candidates. No separate "deep paper preview" mode.

The result row should have one obvious action model:

- clicking the row opens the candidate in Reader
- a compact `+` or save icon adds it to a vault
- secondary actions move to hover, overflow, or the inspector

The feed should not show verbose rationale text such as `Matched OpenAlex search
query ...`. Use compact metadata instead:

```text
OpenAlex · PDF · 114 cites
Deep · via citation expansion
```

Detailed provenance belongs in the inspector, not in every result row.

### Right Inspector

The right-most panel has two primary states.

When no paper is selected:

```text
Run
Deep research

Status: Searching
Queries tried: 7
Providers: OpenAlex, arXiv
Candidates found: 42
Added to results: 18

Current step
Searching citation trails around "..."

Trace
- planned 3 queries
- queried OpenAlex
- deduped 12 candidates
- ranked 18 candidates
```

When a paper is selected:

```text
[ Deep research running · 7 queries · 18 results ] [ View Run ]

Paper
Title
Authors
Venue
Why found
PDF status
Vault targets
```

Clicking `View Run` returns the right panel to progress. This keeps progress
reachable without blocking paper inspection.

Search progress should not be shown as progress bars or large banners inside
the result feed. The feed is for evidence; the inspector is for machinery.

### Empty State

An empty search window should feel like a quiet terminal prompt, not an
instructional card.

```text
Discover
[ + ] [ untitled ]

> [ find papers about...                         ] [ Deep ] [ Run ] [ tune ]
```

Avoid paragraphs of helper text in the main panel. If examples are needed, use
one quiet placeholder or ghost suggestion, and keep it dismissible by typing.

### Saved Scouts And Schedule

Saved Scouts, scheduled searches, and recurring discovery are important future
features, but they should not be visible in the default Discover header. They
belong in a saved-search detail surface or the right inspector once the user has
chosen to persist a search.

## Interaction Behavior

### Starting A Shallow Search

1. User types a query.
2. User leaves `Deep` toggled off.
3. User clicks `Run`.
4. If the current search window is empty, results load into it.
5. If the current search window already has results, i0i creates a new search
   window and runs there.
6. Right panel shows run status until a candidate is selected.

### Starting Deep Research

1. User types a goal/query.
2. User toggles `Deep` on.
3. User clicks `Run`.
4. i0i creates or uses an empty search window.
5. Deep research runs in the background.
6. Results appear incrementally as candidates are found/ranked.
7. Right panel shows progress and trace.
8. The user can open papers while the run continues.

### Opening A Candidate

Opening a candidate should never depend on search mode. A deep-research
candidate opens in Reader with the same behavior as shallow search:

- unsaved candidate opens transiently
- saved candidate opens as a vault paper
- PDF handling follows the existing Reader/PDF ingestion path

## Data/State Model

Frontend state needs a Discover search-window model:

```ts
type DiscoverSearchWindow = {
  id: string;
  title: string;
  mode: "quick" | "deep" | null;
  query: string;
  status: "idle" | "running" | "ready" | "failed" | "cancelled";
  candidates: PaperCandidate[];
  stats: SearchRunStats;
  trace: SearchRunTraceEvent[];
  selectedCandidateId?: string;
};
```

For v1 this can be transient frontend state. RFC 0037's durable saved-search
model can later persist the same concepts as searches/runs.

Deep research should produce the same `PaperCandidate` shape, plus optional
provenance/rationale fields. If the current backend type cannot hold those
fields yet, they should be additive optional fields.

## Backend Notes

- Shallow search can keep using the existing `search_papers` command.
- Deep research should expose a run command and emit progress events.
- Both modes should return/emit candidate payloads that the existing Reader
  candidate-open path can consume.
- Progress events should be scoped by `searchWindowId` or backend `runId`, so
  the right panel updates the correct window.

## Open Questions

- Should search windows survive app restart before RFC 0037 persistence lands?
  Proposal: no, not initially.
- Should empty search windows be closable automatically when the user starts a
  different search? Proposal: yes, if untouched.
- Should deep research results appear as soon as found or only after ranking?
  Proposal: show incrementally, but mark unranked results as "reviewing" until
  the rank/rationale arrives.
- Should `Deep` appear before or after `Run`? Proposal: before `Run`, because
  it reads as command configuration followed by execution.

## Validation Plan

- Starting a query in a populated search window creates a new window.
- Shallow and deep runs both populate the same candidate list component.
- Deep candidates open in Reader via the existing Discover candidate path.
- Selecting a candidate changes the right panel to metadata while preserving a
  visible active-run strip.
- Clicking `View Run` returns to progress for the correct run/window.
- Closing a search window does not affect papers already saved to a vault.
