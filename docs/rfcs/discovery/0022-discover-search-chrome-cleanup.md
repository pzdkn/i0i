# RFC 0022: Discover Search Chrome Cleanup

Status: Draft
Date: 2026-05-29

## Summary

The first live Discover slice made OpenAlex search work, but some of the visible copy still exposes implementation details instead of helping the researcher scan results. This RFC proposes a small cosmetic pass over the Discover search chrome, candidate rationale text, and inspector copy.

The goal is to make Discover feel like a compact research console again without changing search behavior, providers, persistence, or result storage.

## Problems

The current UI is too verbose in places:

- Candidate/inspector rationale such as `Matched OpenAlex search query "Time to fly".` repeats the user's own query and does not explain why the paper is useful.
- Inspector text such as `Open-access filter was applied.` and `114 OpenAlex citations.` reads like debug output.
- The static header text `OpenAlex / transient search / explicit run` is accurate, but it is implementation language rather than product language.
- The query field stretches across too much of the header, making the interface feel flatter and less intentional.
- The `+` button sits before the query input, which makes it look related to adding query terms rather than creating a new Discover search tab/workspace.

## Goals

- Remove verbose OpenAlex match sentences from visible UI.
- Replace implementation-language status text with compact source/status chips.
- Make the search field visually bounded, not full-width by default.
- Move the new-search affordance away from the query input.
- Restore more of the dense "Search Console" feel from the original Discover design.

## Non-Goals

- No change to the OpenAlex request semantics.
- No new discovery provider.
- No persistent Scout/search history.
- No PDF storage or caching.
- No large redesign of the global workspace tab system.

## Proposed UX

### Header

Keep the Discover heading, but remove the static text:

```text
Discover
Search Title
[OpenAlex] [Manual run] [Open access] [25 max] [Completed]
```

The chips should be derived from the current workspace state:

- provider: `OpenAlex`
- run mode: `Manual run`
- open-access filter: `Open access` or hidden when off
- limit: `25 max`
- status: `Idle`, `Running`, `Completed`, or `Failed`

Avoid showing `transient search` and `explicit run` in the UI. Those are implementation decisions.

### Search Command Row

Use a compact command strip:

```text
[ query input, max width ~560-680px ] [Run]
```

The input should remain prominent, but it should not consume the entire horizontal region. Filters can stay in the secondary row.

### New Search Placement

Remove the `+` button from the query row.

For this cosmetic pass, place `New Search` as a small header action near the Discover title, preferably on the right side of the header. This makes it clear that the action creates a new Discover workspace/tab, not a query clause.

Longer term, the most natural home is probably next to the Discover tab in the workspace tab strip. That requires touching shared workspace/tab chrome, so it should be treated as a later interaction pass.

### Candidate and Inspector Rationale

Do not show sentence-style OpenAlex match reasons:

```text
Matched OpenAlex search query "Time to fly".
Open-access filter was applied.
114 OpenAlex citations.
```

Instead, use compact metadata signals:

```text
OpenAlex · 114 citations · Open access
```

In the inspector, replace the current reason list with a small `Signals` section:

```text
Signals
[OpenAlex] [114 citations] [Open access] [PDF available]
```

The abstract, venue, year, authors, citation count, and PDF availability remain the useful inspector content.

## Files Likely Affected

- `src/lib/features/discover/DiscoverSeedBar.svelte`
- `src/lib/features/discover/DiscoverInspector.svelte`
- `src/lib/state/library-cache.svelte.ts`

Optionally:

- `src-tauri/src/commands/discovery.rs`, if we decide to stop emitting sentence-style `reasons` from Rust instead of only hiding them in Svelte.

## Risks

- Removing the current reason sentences slightly reduces explicit provenance, but the current text does not add meaningful explainability.
- Moving `New Search` to the header is an interim placement. It is clearer than the query-row `+`, but the eventual best place may be the workspace tab strip.
- Compact chips can become noisy if too many are shown. Keep them short and state-derived.

## Validation Plan

- Run `pnpm check`.
- Run `pnpm build`.
- Manually inspect Discover at empty, idle, running, completed, and failed states.
- Verify the add-to-vault inspector area is not regressed by the cosmetic changes.
