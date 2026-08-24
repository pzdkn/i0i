# RFC 0039: Discover Settings And Incremental Deep Results

Status: Stale
Date: 2026-07-06
Product: i0i
Target: Tauri v2 + Svelte, macOS first
Builds on: RFC 0038 (Discover search windows and deep-research UI), RFC 0037 (deep-research agentic search)

## Summary

Follow up on RFC 0038 with two small corrections:

1. Rename the Discover header's `tune` affordance to `settings`, or use a
   settings icon with accessible text.
2. Make the settings panel conditional on the `Deep` toggle.

Then add incremental deep-search result previews as a separate backend/event
follow-up. Deep research should feel live while it runs, but the durable
candidate pool should still be written only when the agent has finished ranking.

## Context

RFC 0038 reduced Discover to a command surface:

```text
search window -> query -> optional Deep toggle -> Run
```

The first implementation made the panel leaner, but two details still feel
wrong:

- `tune` is cute but unclear. The control is really search settings.
- The settings panel currently shows shallow-search controls even when `Deep`
  is enabled.

Deep research also currently behaves too batch-like from the user's point of
view: the run progresses in the inspector, but paper candidates only become
visible after final ranking.

## Decisions

1. **Rename `tune` to `settings`.** Prefer a settings/sliders icon once the app
   has a standard icon set. Until then, use the text label `settings`.
2. **Settings are conditional on `Deep`.** Shallow provider search and deep
   research have different controls and should not expose the same panel.
3. **Implement UI/settings cleanup first.** This is a small frontend pass and
   should land before backend event changes.
4. **Treat incremental deep candidates as a backend event-stream follow-up.**
   Do not fake streaming by polling if candidates are not persisted mid-run.
5. **Use transient preview snapshots for incremental results.** The backend can
   emit preview candidate snapshots while the agent is running; final ranked
   candidates still come from the durable search candidate pool.

## Goals

- Keep Discover lean while making the remaining controls more intuitive.
- Avoid exposing irrelevant controls when switching between shallow and deep
  search.
- Let deep research feel alive by showing candidate previews as soon as useful
  candidate pools exist.
- Preserve the durable storage model: only final ranked candidates are saved to
  the search candidate pool.

## Non-Goals

- No saved Scout/schedule settings in this RFC.
- No full visual redesign of the Discover feed.
- No provisional SQLite candidate rows.
- No candidate delta protocol.
- No live mutation of final ranks before ranking is complete.

## Milestone 1: Settings Cleanup

### Header Affordance

Replace:

```text
[ tune ]
```

with:

```text
[ settings ]
```

Later, once the app has a standard icon library:

```text
[ sliders icon ]
```

The accessible label should remain `Search settings`.

### Shallow Settings

When `Deep` is off, settings should show provider-search controls:

```text
Provider
Sort
Limit
Year from
Year to
```

Suggested defaults:

```text
provider: OpenAlex or Auto, depending on provider support
sort: Relevance
limit: 25
year: Any
```

### Deep Settings

When `Deep` is on, settings should show agentic-search controls:

```text
Depth
Providers
Target count
Year from
Year to
Seeds
```

For the next implementation pass, keep this minimal:

```text
Depth: Standard
Provider: current provider or Auto
Target count: existing resultLimit
Year from/to: existing year filters
Seeds: hidden or disabled until seed-paper support is wired
```

Do not show `Sort` in deep mode. Deep search ranks through the agent pipeline;
provider sort is not the user's direct control in that mode.

## Milestone 2: Incremental Deep Result Previews

### Chosen Approach

Use backend-emitted preview snapshots.

Do not start with frontend polling:

- the current deep-search manager persists ranked candidates only after the
  agent loop finishes
- polling `listSearchCandidates(...)` during a run would usually return nothing
  until the end
- polling adds repeated calls and UI complexity without real live results

Do not start with provisional DB rows:

- they complicate candidate lifecycle and cleanup
- they make unranked candidates look durable
- they force the storage model to know about temporary states too early

### Preview Event Shape

Add a new Tauri event or extend the existing research event stream with a
candidate preview payload.

Preferred separate event:

```ts
type SearchCandidatesPreview = {
  searchId: string;
  runId: string;
  candidates: PaperCandidate[];
  unique: number;
};
```

The event is transient. It is not a persistence contract.

### Backend Behavior

During deep research:

```text
provider results arrive
-> apply constraints
-> dedupe current pool
-> emit preview snapshot
-> continue planning/refining
-> final rank
-> persist ranked candidates
-> emit ready event
```

The simplest insertion point is after the agent loop has a deduped pool. The
preview should use the same normalized `PaperCandidate` shape as shallow
discovery.

### Frontend Behavior

While deep research is running:

```text
preview event arrives
-> map candidates into DiscoverCandidate rows
-> show rows in the normal Discover feed
-> mark them as reviewing
```

When the final `ready` event arrives:

```text
listSearchCandidates(searchId)
-> replace preview rows with final ranked rows
-> remove reviewing state
```

Preview rows should be visually distinct but quiet:

```text
Deep · reviewing
```

They should still be openable if they contain enough metadata and URLs. If a
preview candidate is saved before final ranking completes, the save path should
use the candidate payload in the preview row, not wait for the final search pool.

## Risks

- Preview candidates may disappear after final ranking. This is acceptable if
  they are marked as `reviewing` while provisional.
- Rows may reorder when the final ranked list arrives. The UI should tolerate
  this and avoid implying stable rank during preview.
- If preview events are too frequent, they may cause feed churn. Emit snapshots
  only after meaningful pool changes, not after every single candidate.

## Validation Plan

Milestone 1:

- With `Deep` off, settings shows provider/sort/limit/year controls.
- With `Deep` on, settings shows depth/provider/target/year controls and hides
  provider `Sort`.
- The settings control is labeled `settings` or has an accessible `Search
  settings` label.
- `pnpm check` passes.

Milestone 2:

- Starting deep research shows progress in the inspector.
- Candidate previews appear before the final ready event.
- Preview rows are marked `reviewing`.
- Final ready event replaces preview candidates with ranked durable candidates.
- Saving a preview candidate still works through the existing Discover add path.
- `cargo test`, `cargo check`, and `pnpm check` pass.
