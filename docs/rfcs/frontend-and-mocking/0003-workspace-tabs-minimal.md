# RFC 0003: Minimal Workspace Tabs

Status: Stale
Date: 2026-05-24  
Product: i0i  
Target: Tauri v2 + Svelte, macOS first

## Summary

Introduce a minimal workspace tab model so i0i behaves more like an IDE for knowledge curation.

The goal is to clean up the current Vault -> Reader navigation from RFC 0002:

- Remove the awkward row-level `dbl read` hint.
- Remove the page-like "Back to vault" Reader button.
- Use a stable tab strip to switch between the Vault folder and an opened Reader.
- Let the Reader tab be closed with an `x`.

This RFC does not implement a full tab system. It implements the smallest useful version that makes the current Vault and Reader workspaces feel intentional.

This RFC uses the product-wide architecture defined in [`docs/overview/architecture.md`](../overview/architecture.md).

## Context

RFC 0001 created the product shell and Vault/Home view.

RFC 0002 added the Reader skeleton:

- Single-click selects a paper.
- Double-click opens Reader.
- Reader uses mock text.
- Vault explorer remains visible.
- `TEXT` is active; `PDF` and `SPLIT` are visible but disabled.

The remaining awkwardness is navigation. The Reader currently has a content-level "Back to vault" affordance, and the paper row has a cryptic `dbl read` hint. Both fight the IDE metaphor.

i0i should feel closer to VS Code:

```text
[ /transformers/attention ] [ Attention Is All You Need  x ]
```

The shell stays stable. The center workspace changes based on the active tab.

Terminology:

- A **tab** is one open workspace, such as `# /transformers/attention`.
- The **tab strip** is the horizontal row that contains those tabs.
- The current `# /transformers/attention` tab is the Vault workspace for the attention folder.
- RFC 0003 turns the existing tab-looking row into the real shared workspace tab row.

## Goals

- Add a minimal shared workspace tab strip.
- Allow the Vault folder tab to be closed.
- Add a Reader tab when a paper is opened.
- Switch between Vault and Reader by clicking tabs.
- Close tabs with `x`.
- Reopen the Vault workspace from the left Vault/Explorer navigation.
- Remove the Reader header's "Back to vault" button.
- Remove the paper row's floating `dbl read` hint.
- Remove fake macOS window controls from the in-app chrome.
- Preserve current UI elements because they model the intended app.
- Make low-cost controls feel real where possible, even if behavior is mocked.
- Keep deeper future controls visible as design affordances.
- Use the same mock/lorem-ipsum-style Reader text for every current mock paper.
- Preserve double-click-to-open behavior.
- Keep all state local and easy to understand.

## Non-Goals

- No full multi-tab workspace manager.
- No persisted open tabs.
- No drag/drop tab reordering.
- No keyboard tab navigation yet.
- No routing changes.
- No app-wide store.
- No backend/Rust changes.
- No Discover tab yet.
- No real file/editor model.
- No real implementation of Add, Import, Export, Notes, Annotate, Graph, Ask, PDF, or SPLIT behavior.

## First Increment

Build this:

- A shared `WorkspaceTabs.svelte` component.
- A simple app-level workspace state in `+page.svelte`.
- One Vault tab for `/transformers/attention`, opened by default.
- One optional Reader tab.
- Tab titles based on the workspace they represent.
- Close button on closable tabs.
- Clicking the Vault tab activates Vault.
- Clicking the Reader tab activates Reader.
- Closing an active tab moves focus to the remaining tab if one exists.
- Closing the final tab shows an empty workspace state.
- Clicking the left Vault/Explorer selection reopens the Vault tab.
- Double-clicking any paper opens Reader.
- Any opened paper uses the same mock Reader body text for now.
- Reader metadata can reflect the selected paper, while the body text remains shared mock content.
- Fake traffic-light window controls are removed from the app chrome because macOS already provides native close/minimize/zoom controls.
- Search/filter fields accept typed text, but filtering/search behavior can remain unimplemented.
- The left Vault explorer can switch between mocked vault folders with default mock paper sets.
- Existing UI affordances remain visible even if their deeper behavior is not implemented yet.

## Proposed User Flow

```text
User starts in Vault
  -> sees one tab: [ /transformers/attention ]

User double-clicks a paper
  -> Reader tab appears
  -> active tab becomes Reader
  -> Reader workspace renders

User clicks Vault tab
  -> active tab becomes Vault
  -> Vault workspace renders
  -> Reader tab remains open

User clicks Reader tab
  -> active tab becomes Reader
  -> Reader workspace renders

User clicks x on Reader tab
  -> Reader tab closes
  -> active tab becomes Vault

User clicks x on Vault tab
  -> Vault tab closes
  -> active tab becomes Reader if Reader is open
  -> otherwise the center workspace shows an empty state

User clicks the attention folder in the left Explorer
  -> Vault tab reopens
  -> active tab becomes Vault
```

## Proposed State Model

Keep this local to `src/routes/+page.svelte` for now:

```ts
type WorkspaceKind = "vault" | "reader";

type WorkspaceTab = {
  id: string;
  kind: WorkspaceKind;
  title: string;
  paperId?: string;
};

let activeTabId = $state("vault:attention");
let tabs = $state<WorkspaceTab[]>([
  {
    id: "vault:attention",
    kind: "vault",
    title: "/transformers/attention",
  },
]);
let selectedPaperId = $state("vaswani2017");
```

Opening Vault from the left Explorer:

```ts
function openVault() {
  if (!tabs.some((tab) => tab.id === "vault:attention")) {
    tabs = [
      ...tabs,
      {
        id: "vault:attention",
        kind: "vault",
        title: "/transformers/attention",
      },
    ];
  }
  activeTabId = "vault:attention";
}
```

Opening a paper:

```ts
function openPaper(paperId: string) {
  selectedPaperId = paperId;
  const readerTab = {
    id: `reader:${paperId}`,
    kind: "reader",
    title: getPaperTitle(paperId),
    paperId,
  };
  tabs = [
    ...tabs.filter((tab) => tab.id !== readerTab.id),
    readerTab,
  ];
  activeTabId = `reader:${paperId}`;
}
```

Closing a tab:

```ts
function closeTab(tabId: string) {
  tabs = tabs.filter((tab) => tab.id !== tabId);
  if (activeTabId === tabId) {
    activeTabId = tabs[0]?.id ?? "";
  }
}
```

This is intentionally small. It gives us the mental model without committing to a general-purpose tab system.

## Proposed Frontend Structure

```text
src/lib/
  app/
    WorkspaceTabs.svelte

  features/
    vault/
      VaultHome.svelte
      PaperList.svelte

    reader/
      ReaderView.svelte
      ReaderHeader.svelte
```

## Component Responsibilities

`WorkspaceTabs.svelte`
: Presentational shared tab strip. Receives tabs, active tab ID, `onActivate`, and `onClose`.

`+page.svelte`
: Owns minimal workspace state and decides whether Vault or Reader is rendered.

`VaultExplorer.svelte`
: Reopens or activates the Vault tab when a vault folder is selected. It may expose multiple mocked folders with default mock paper sets.

`VaultHome.svelte`
: Emits `onOpenPaper(paperId)` on double-click.

`PaperList.svelte`
: Keeps single-click selection and double-click open. No floating read hint.

`ReaderHeader.svelte`
: Removes "Back to vault". It should focus on paper metadata and Reader controls only.

`TitleBar.svelte`
: Removes fake macOS close/minimize/zoom controls while the Tauri window uses native OS decorations.

## Data Boundary

For this increment:

- Tabs are frontend-only UI state.
- Reader content remains mock data.
- Vault content remains mock data.
- Rust remains unchanged.
- All current mock papers can open Reader using shared placeholder body text.
- Search/filter text can be local frontend state only.
- Mock folder selection can be local frontend state only.

Later:

- Open tabs may become persisted workspace state.
- Reader tabs may point to real paper IDs from storage.
- Discover can add a third tab kind.

## Design Notes

The tab strip should match the existing Layout A language:

- Compact height.
- Sharp rectangles.
- Hairline borders.
- Amber top border or amber text for active tab.
- Small `x` close affordance on closable tabs.
- No large cards or rounded pills.

The Vault tab represents the currently open folder workspace:

```text
[ /transformers/attention ]
```

The Reader tab should feel like an editor tab:

```text
[ Attention Is All You Need  x ]
```

### Interaction Model

Preserve the design surface, but make the current path feel coherent.

For this RFC:

- Keep workspace tabs clickable because they switch workspaces.
- Keep tab close buttons clickable because they close tabs.
- Keep paper rows double-clickable because they open Reader.
- Let search/filter fields accept text input even if nothing is filtered yet.
- Let the left Vault explorer switch between mocked folder selections and paper sets.
- Keep `TEXT` active.
- Keep `PDF` and `SPLIT` visible as future view-mode affordances.
- Keep future actions such as Add, Import, Export, Notes, Annotate, Graph, and Ask visible because they model the intended product.
- Remove fake macOS traffic-light buttons from the app chrome. The native macOS window already provides close/minimize/zoom controls unless we later deliberately build a custom titlebar.

This means not every visible control needs full behavior yet. The goal is to avoid breaking the mental model while making the implemented path feel less fake.

## Teaching Notes

This increment teaches:

- Local UI state as a simple workspace model.
- The difference between selected paper and opened paper.
- Why tab state can stay frontend-only for now.
- Why navigation should match the app metaphor.
- How to avoid prematurely building a full editor/tab framework.

## Risks

- We may accidentally build a generalized tab system too early.
- Closing the Reader tab could lose unsaved future state, once annotations exist.
- The Vault and Reader components currently have their own tab-strip-like chrome, which may need cleanup.

## Risk Mitigations

- Support only one optional Reader tab.
- Do not persist tabs yet.
- Keep the component API tiny.
- Remove duplicated tab strips from Vault/Reader as part of this cleanup.
- Keep the empty workspace state plain and obvious.
- Avoid fake controls that duplicate native OS chrome.
- Keep future controls visible when they help communicate the app model.
- Defer dirty-state/unsaved-change logic until annotations are real.

## Validation Plan

- `pnpm check`
- `pnpm build`
- `cargo check`
- `cargo fmt --check`
- `pnpm tauri dev`
- Manual flow check:
  - App starts with Vault tab only.
  - Single-click selects a paper.
  - Double-click opens Reader tab.
  - Clicking Vault tab returns to Vault.
  - Clicking Reader tab returns to Reader.
  - Clicking Reader tab `x` closes Reader and returns to Vault.
  - Clicking Vault tab `x` closes Vault.
  - Clicking the attention folder in Explorer reopens Vault.
  - Clicking other mocked vault folders updates the Vault tab/workspace with mocked paper sets.
  - Search/filter fields accept typed text without applying filtering yet.
  - Double-clicking any mock paper opens Reader with shared mock body text.
  - Fake macOS window controls are no longer rendered inside the app.
  - Future controls remain visible as design affordances.

## Open Questions

Resolved for this RFC:

- Use double-click to open Reader.
- Do not show `dbl read` or any cryptic double-click hint.
- Do not show "Back to vault" inside Reader content.
- Use a minimal tab strip instead of a separate Reader exit button.
- The Vault tab can be closed and reopened from the left Vault/Explorer navigation.
- All current mock papers use shared mock Reader body text.
- Remove fake in-app macOS window controls while using native window controls.
- Preserve current UI elements even when deep behavior is not implemented.
- Search/filter fields can accept text without filtering yet.
- Left Vault explorer can switch between mocked vault folders and paper sets.
- Keep the implementation frontend-only.

## Recommendation

Implement this before Discover.

Discover will be much easier to add once the workspace model is clear:

```text
[ /transformers/attention ] [ Attention Is All You Need x ] [ Discover: ssl+dino x ]
```
