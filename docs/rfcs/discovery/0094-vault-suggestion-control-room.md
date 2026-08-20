# RFC 0094: Vault suggestion control room

Status: Implemented
Date: 2026-08-20
Product: i0i
Target: Tauri v2 + SvelteKit (Svelte 5), macOS first
Builds on: RFC 0091 (vault paper suggestions), RFC 0039 (incremental deep
results), RFC 0051 (resizable workspace panels)

## Summary

The Suggestions view currently exposes a run button and a single status phrase,
but it does not make a long-running search legible or configurable. A refresh
also changes the refresh icon back into the text-labelled **Find similar
papers** control while it runs. That makes a command look like a newly inserted
search field and hides the fact that work is already in progress.

This RFC turns the contextual vault inspector into the Suggestions control
room. The centre pane remains the result surface; the inspector owns search
settings, live activity and selected-result evidence.

## Interaction design

### Two-stage search control

Suggestion search has an explicit planning boundary:

1. **Prepare queries** asks the planner for distinct lightweight query paths.
   It does not contact search providers.
2. The inspector shows the proposed paths with checkboxes. The user chooses
   which paths belong in the run.
3. **Run selected** starts concurrent provider searches for only the checked
   paths.

The Suggestions toolbar has one fixed-size preparation button in a stable
position:

- Before a query plan exists it uses a planning icon and the accessible name
  **Prepare suggestion queries**.
- Once a proposal exists it uses the refresh icon and the accessible name
  **Regenerate suggestion queries**. Regeneration produces a new proposal; it
  never silently runs an old query set against a changed vault.
- While either command runs, it keeps its previous identity and position but
  shows a spinner. It is disabled and exposes `aria-busy="true"`.
- The control never expands into a text-labelled toolbar element during a run.

The Suggestions tab also shows a small animated progress glyph while active.
Motion respects `prefers-reduced-motion`. A short status line above the results
shows the latest phase, so progress remains visible when the inspector is
collapsed.

The empty state may contain the full **Prepare suggestion queries** text
command. Once a proposal exists, the only **Run selected** command belongs at
the bottom of the query-path section because it confirms the choices
immediately above it. During retrieval the toolbar position shows progress, but
does not introduce a second Run command.

### Contextual inspector

When **Papers** is active, the inspector remains unchanged. When
**Suggestions** is active, it contains four compact, unframed sections:

1. **Search settings**
2. **Proposed queries**, after preparation
3. **Activity**, once retrieval starts
4. **Selected suggestion**, when a row is selected

This is a work panel, not a settings modal and not a stack of cards. Controls
use the existing restrained IDE visual language: short labels, native inputs,
small icons and hairline section boundaries.

```text
┌ Suggestions ────────────────────────────┬ Inspector ───────────────┐
│ Searching OpenAlex · 38 candidates  ⟳  │ SEARCH SETTINGS          │
├─────────────────────────────────────────┤ Focus  [                 ]│
│ paper title · authors · year            │ Years  [2021] — [2026]  │
│ why it belongs                    +  ×  │ Propose [ 3 ▾ ]          │
│                                         │ Results [ 5 ▾ ]          │
│ paper title · authors · year            │ [Prepare queries]        │
│ why it belongs                    +  ×  ├──────────────────────────┤
│                                         │ PROPOSED QUERIES         │
│                                         │ ☑ sparse attention       │
│                                         │ ☑ efficient inference    │
│                                         │ ☐ transformer surveys    │
│                                         │ [Run 2 selected]         │
│                                         ├──────────────────────────┤
│                                         │ ACTIVITY                 │
│ why it belongs                    +  ×  │ ● OpenAlex · 24 found    │
│                                         │ ● arXiv · 14 found       │
│                                         │ ◌ Ranking candidates     │
│                                         ├──────────────────────────┤
│                                         │ WHY THIS VAULT           │
│                                         │ Similar to 3 papers ...  │
└─────────────────────────────────────────┴──────────────────────────┘
```

### Search settings

The first version exposes only settings that directly answer the reported
problems:

- **Focus within this vault**: optional free text used to narrow the vault's
  subject, for example `empirical sparse-attention methods`. This is a real
  input and lives in the inspector, not beside the run icon.
- **Year from** and **Year to**: optional numeric fields. Empty means unbounded.
- **Proposed queries**: a compact select with `1`, `3`, and `5`; default `3`.
  This controls how many distinct lightweight search angles the planner should
  propose from the vault and optional focus.
- **Results**: a compact select with `3`, `5`, and `10`; default `5`.

`include_reviews` is reserved in the options snapshot for RFC 0095 but is not
shown yet. A visible control must not imply filtering behavior before the
relevance policy implements it.

`Proposed queries` controls the size of the reviewable search agenda, while
`Results` controls the maximum number of papers shown. Keep the labels separate
and add concise tooltips so users do not mistake five searches for five results.

Preparation returns plain query strings with short intent labels. Every path is
selected initially. The user may deselect paths but does not edit their text in
this first version. **Select all** appears only when at least one path is
disabled. A compact regenerate icon prepares a fresh set. At least one path
must be selected before Run is enabled.

Changing Focus or the proposed-query count marks the existing proposal stale
and disables Run until it is regenerated. Changing only year bounds, result
count or review inclusion does not invalidate query wording. Adding or removing
vault papers invalidates the proposal because its source profile changed.

Settings are saved per vault and apply to its next proposal and run. They do not
prepare or run automatically. While planning or retrieval is active they are
read-only, making it clear which configuration the visible work belongs to. The
run stores a snapshot of the settings and selected query strings so history
remains interpretable after preferences change.

Validation occurs at the input boundary: years must be plausible and `from`
must not exceed `to`. Invalid settings disable the run control and show one
inline message beside the fields.

### Live activity

The inspector shows what the suggestion service is doing now rather than only
the latest generic state. Keep the most recent eight entries in memory:

- `Planning focused searches`
- `Path 1 · sparse attention mechanisms`
- `Path 2 · efficient transformer inference`
- `OpenAlex · Path 1 · 24 found`
- `arXiv · Path 2 · 14 found`
- `Citation graph · 4 of 6 seeds`
- `Filtering · 31 eligible`
- `Ranking · semantic + graph`

New entries append at the bottom and the section follows them while the user has
not scrolled away. Provider failures use a short actionable line and do not
dump URLs, response bodies, prompts, API keys or backend diagnostics into the
UI. The durable run record keeps only the final summary and failure; this trace
is transient and starts empty after app restart.

The progress event gains structured fields rather than asking Svelte to parse a
status sentence:

```text
VaultSuggestionProgress
  vault_id
  run_id
  sequence
  phase       planning | provider | graph | filtering | ranking | complete
  query_path? stable identifier and display position for a generated query
  label
  detail?
  found
```

The service emits graph and filtering progress in addition to forwarding Deep
Research progress. Svelte rejects events for another vault or run and orders
accepted events by `sequence`.

### Result continuity

An initial run may populate provisional candidates as they arrive. A refresh
keeps the previous result list visible and actionable until the new run
finishes, as RFC 0091 specifies. Progress never replaces valid results with a
loading empty state. A failed refresh keeps the old results and leaves the
activity trace available for diagnosis.

## Data and command changes

Add a small per-vault settings record and snapshot it onto each run:

```text
VaultSuggestionOptions
  focus?: string
  year_from?: integer
  year_to?: integer
  query_path_count: 1 | 3 | 5
  result_count: 3 | 5 | 10
  include_reviews: boolean
```

`plan_vault_suggestion_queries(vault_id, options)` returns a transient proposal:

```text
VaultSuggestionQueryPlan
  id
  vault_revision
  queries[]
    id
    intent
    query
```

Planning makes one bounded LLM request and performs no OpenAlex or arXiv calls.
`run_vault_suggestions` accepts `VaultSuggestionOptions`, the plan id and the
selected query ids. The backend resolves those ids to the prepared query
strings and snapshots them onto the run. This prevents the frontend from
substituting arbitrary strings after review while keeping the bridge typed.

A separate read/write command loads and saves the per-vault preference. Query
plans remain transient and are regenerated after app restart. Do not create a
generic search-configuration framework for this feature.

RFC 0091's weekly startup retrieval is paused by this RFC. Automatically
selecting every generated path would violate the review boundary. Scheduled
search may later prepare a visible proposal, but only an explicit **Run
selected** action may contact providers.

## Accessibility and keyboard behavior

- The run button has an accessible name independent of its icon and reports
  busy state.
- Activity is one polite live region. It announces phase changes and completion,
  not every candidate.
- Settings follow normal tab order and retain focus after editing.
- The progress glyph is not the sole indication of state; status text remains.
- Narrowing or collapsing the inspector does not hide the centre-pane status.

## Acceptance criteria

1. Preparing queries makes no provider requests and returns the requested
   number of distinct, reviewable query paths.
2. Every proposed path is initially selected; the user can exclude paths and
   Run requires at least one selection.
3. Only explicitly selected paths are passed to provider retrieval.
4. Preparing, running or refreshing produces an immediate visible spinner in
   both the toolbar and Suggestions tab without changing the command's width.
5. The misleading toolbar text control never appears during a refresh.
6. Activating Suggestions changes the inspector to settings, query proposal,
   live activity and selected-result evidence; Papers retains its current
   inspector.
7. Focus, year range, proposed-query count and result count persist per vault
   and are passed to the run as a stable snapshot.
8. The planner produces the requested number of distinct query paths, executes
   selected provider searches concurrently within a fixed backend limit and
   merges their candidates before final ranking.
9. Query-path, provider, graph, filtering and ranking activity appears
   incrementally in the inspector, capped at eight entries.
10. A refresh preserves old results until successful replacement and preserves
    them on failure.
11. Tests cover option validation, proposal invalidation, query selection,
    per-vault persistence, event ordering, the stable run-control state machine
    and contextual inspector rendering.

## Out of scope

- Editing provider credentials or global provider availability
- A raw backend log viewer
- Pausing or resuming a run
- Automatically rerunning when a setting changes
- Scheduled preparation of a reviewable query proposal
- Editing or manually adding proposed query strings
- The relevance formula itself, which belongs to RFC 0095

## Verification

- `cargo test --lib`: 435 passed, 5 ignored
- `pnpm check`: 0 errors and 0 warnings
- `pnpm build`: passed

The existing Tauri development instance accepted the rebuilt frontend and Rust
backend. Automated screen capture was unavailable under the current macOS screen
recording permissions, so visual screenshot verification was not recorded.
