# RFC 0004: Discover View Skeleton

Status: Stale
Date: 2026-05-24  
Product: i0i  
Target: Tauri v2 + Svelte, macOS first

## Summary

Add the first Discover workspace skeleton to i0i.

Discover should reuse the app shell and workspace tab model from RFC 0003, follow the existing Layout A design language, and use mock data only. The goal is to model paper discovery as a first-class workspace without building real search, ranking, agents, or external API integrations yet.

This RFC uses the product-wide architecture defined in [`docs/overview/architecture.md`](../overview/architecture.md).

## Context

RFC 0001 created the app shell and Vault/Home view.

RFC 0002 added the Reader skeleton.

RFC 0003 introduced the minimal workspace tab model:

- Vault folders open as workspace tabs.
- Reader opens as a workspace tab.
- Tabs can be closed.
- The left Explorer can reopen Vault workspaces.
- The left Activity Rail can activate supported workspaces.

The next major product surface from the existing design is Discover. In the mockups, Discover is a ranked candidate feed driven by seed chips, with a Search Console inspector for filters, signal mix, and scheduled/agent-like discovery.

## Goals

- Add a Discover workspace tab kind.
- Clicking `FIND` in the Activity Rail opens or activates Discover.
- Add a Discover feature folder under `src/lib/features/discover`.
- Add mock Discover data under `src/lib/mock/discover.ts`.
- Add Discover domain types under `src/lib/domain/discover.ts`.
- Render a Layout A-style Discover workspace:
  - Seed/chip input area
  - Search text input that accepts typing
  - Saved-search/status strip
  - Ranked candidate feed
  - Right Search Console inspector
- Keep candidate actions visible as design affordances.
- Allow double-clicking a candidate to open Reader with shared mock Reader body text.
- Keep everything frontend-only and mock-driven.

## Non-Goals

- No real search.
- No real ranking.
- No embeddings.
- No citation graph lookup.
- No Semantic Scholar/arXiv/OpenAlex integration.
- No agents.
- No scheduled background jobs.
- No saved-search persistence.
- No backend/Rust changes.
- No database changes.
- No real Add/Preview/Graph behavior.

## First Increment

Build this:

- A `discover` workspace kind in the tab model.
- A Discover tab with a title like `Discover: ssl + dino`.
- Activity Rail `FIND` opens/activates that Discover tab.
- `DiscoverView.svelte` as the main workspace.
- `DiscoverSeedBar.svelte` for seed chips and a text input.
- `DiscoverFeed.svelte` for ranked candidates.
- `DiscoverInspector.svelte` for the Search Console.
- Mock candidates with:
  - title
  - authors
  - venue/year
  - citation count
  - score
  - why/reason string
  - tags
  - owned/in-vault flag
- Candidate rows preserve visible actions:
  - Add
  - Preview
  - Graph
  - Open, if already in vault
- Double-clicking any candidate opens Reader using shared mock body text.

## Proposed User Flow

```text
User starts in Vault
  -> clicks FIND in Activity Rail
  -> Discover tab opens
  -> Discover workspace renders

User types in seed/search field
  -> text appears
  -> no real search is applied yet

User clicks Discover tab / Vault tab / Reader tab
  -> active workspace changes

User double-clicks a candidate
  -> Reader tab opens for that candidate
  -> Reader uses shared mock body text

User closes Discover tab
  -> tab closes
  -> focus moves to another open tab, or empty workspace
```

## Proposed State Model

Extend the RFC 0003 local tab state:

```ts
type WorkspaceKind = "vault" | "reader" | "discover";

type WorkspaceTab = {
  id: string;
  kind: WorkspaceKind;
  title: string;
  vaultId?: string;
  paperId?: string;
  discoverId?: string;
};
```

Opening Discover:

```ts
function openDiscover(discoverId = "ssl-dino") {
  const discoverTab = {
    id: `discover:${discoverId}`,
    kind: "discover",
    title: "Discover: ssl + dino",
    discoverId,
  };
  tabs = [
    ...tabs.filter((tab) => tab.id !== discoverTab.id),
    discoverTab,
  ];
  activeTabId = discoverTab.id;
}
```

Clicking `FIND` in the Activity Rail should call `openDiscover()`.

## Proposed Frontend Structure

```text
src/lib/
  domain/
    discover.ts

  mock/
    discover.ts

  features/
    discover/
      DiscoverView.svelte
      DiscoverSeedBar.svelte
      DiscoverFeed.svelte
      DiscoverInspector.svelte
```

## Proposed Domain Types

```ts
export type DiscoverCandidate = {
  id: string;
  title: string;
  authors: string[];
  venue: string;
  year: number;
  citations: number;
  score: number;
  why: string;
  tags: string[];
  owned?: boolean;
  isNew?: boolean;
};

export type DiscoverWorkspace = {
  id: string;
  title: string;
  seeds: string[];
  candidates: DiscoverCandidate[];
};
```

These types model frontend mock data only. They are not final backend search contracts.

## Component Responsibilities

`+page.svelte`
: Owns local tab state and adds `discover` as a workspace kind.

`ActivityRail.svelte`
: Calls `onSelectMode("F")`; the page handles this by opening/activating Discover.

`DiscoverView.svelte`
: Owns Discover workspace layout and local UI state for text inputs.

`DiscoverSeedBar.svelte`
: Renders seed chips and accepts typed text without applying real search.

`DiscoverFeed.svelte`
: Renders ranked candidates. Emits `onOpenCandidate(candidateId)` on double-click.

`DiscoverInspector.svelte`
: Renders static Search Console controls: year range, venues, signal mix, agents, schedule, and batch actions.

## Data Boundary

For this increment:

- Discover data is frontend mock data.
- Search input text is local frontend state.
- Seed chips are local/frontend mock state.
- Candidate actions are visible affordances only.
- Double-click candidate -> Reader is the only meaningful candidate interaction.
- Rust remains unchanged.

Later:

- Discovery can move behind a Rust search service.
- Ranking can combine semantic similarity, citation graph signals, author networks, venue filters, and novelty.
- Saved searches can become persisted entities.
- Background discovery/agents can become scheduled tasks.

## Design Notes

Discover should preserve the fixed i0i design language:

- Lowercase `i0i` in app chrome.
- Amber-on-black shell.
- IBM Plex Mono.
- Sharp rectangles.
- Hairline borders.
- Dense ranked feed.
- Search Console inspector on the right.
- Seed chips / pill input at the top.
- Keep future controls visible as design affordances.

Discover should feel like another IDE workspace, not a separate app page.

The tab should look like:

```text
[ Discover: ssl + dino  x ]
```

## Interaction Model

For this RFC:

- `FIND` in the Activity Rail opens/activates Discover.
- Discover tab can be clicked and closed.
- Search/seed input accepts text.
- Candidate feed is static.
- Candidate double-click opens Reader.
- Add/Preview/Graph/Batch/Agent/Schedule controls remain visible as design affordances.
- Search Console sliders/toggles can be static or accept trivial local state if cheap.

## Teaching Notes

This increment teaches:

- How to add a third workspace kind without changing the overall architecture.
- Why mock data is still useful before real search.
- How workspace tabs scale from Vault/Reader to Discover.
- How to separate visual product affordances from real backend behavior.
- Why search/ranking should not be designed from UI code alone.

## Risks

- Discover has many visible controls and can become too large quickly.
- Mock controls may imply real discovery if the UI is too polished.
- Adding Discover may expose weaknesses in the current tab state model.
- Candidate data may overlap awkwardly with vault paper data.

## Risk Mitigations

- Keep components pane-sized and named by responsibility.
- Keep all data in `mock/discover.ts`.
- Keep one Discover workspace only.
- Keep the candidate model simple.
- Let candidate double-click reuse the existing Reader mock body.
- Do not introduce stores, routing, or backend services yet.

## Validation Plan

- `pnpm check`
- `pnpm build`
- `cargo check`
- `cargo fmt --check`
- `pnpm tauri dev`
- Manual flow check:
  - App starts with existing workspace behavior.
  - Clicking `FIND` opens Discover tab.
  - Clicking Discover/Vault/Reader tabs switches workspace.
  - Discover search/seed field accepts text.
  - Candidate feed renders.
  - Search Console inspector renders.
  - Double-clicking a candidate opens Reader.
  - Closing Discover tab works.

## Open Questions

Resolved for this RFC:

- Keep Discover frontend-only.
- Keep Discover mock-driven.
- Clicking `FIND` opens Discover.
- Preserve visible future controls as design affordances.
- Candidate double-click can open Reader with shared mock body text.

## Recommendation

Implement Discover skeleton next.

After Vault, Reader, and Discover all exist as workspace tabs, pause for a small architecture cleanup pass before adding more product surfaces.
