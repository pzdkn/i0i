# RFC 0084: Opening a paper should not close the last one

Status: Implemented (pending manual verification)
Date: 2026-08-13
Product: i0i
Target: Tauri v2 + SvelteKit (Svelte 5), macOS first
Builds on: RFC 0029 (open discovery candidates in the reader).

## Summary

There is a tab bar, tabs have close buttons, and there can only ever be one
paper open. Opening a second paper silently closes the first.

## Diagnosis

`openPaper` (`src/routes/+page.svelte:263`) builds a tab and then:

```ts
tabs = [...tabs.filter((tab) => tab.kind !== "reader"), readerTab];
```

Every existing reader tab is evicted. `openCandidate` (:598) does the same at
:615. The tab id is *already* per paper — `reader:${paper.id}` — so the model
supports many; only these two lines do not.

Two consequences beyond the obvious one: comparing two papers is impossible, and
the close button on a reader tab is decorative, since the next paper you open
would have closed it anyway.

## Change

R1.1 `openPaper` and `openCandidate` **activate** an existing tab for that paper
when one is open, and otherwise **append** a new one. No reader tab is ever
evicted by opening another.

R1.2 Reopening a discovery candidate for a paper that already has a plain reader
tab refreshes that tab's `readerCandidate` rather than adding a second tab for
the same paper. One paper, one tab, keyed by the id it already has.

R1.3 The new tab becomes active. Opening a paper means wanting to read it.

R1.4 The tab strip scrolls horizontally once tabs overflow, and a tab has a
floor width (140px) so ten open papers are ten legible tabs rather than ten
slivers. `WorkspaceTabs.svelte` has no overflow rule today because it never had
to hold more than three tabs.

R1.5 **The open gesture stays double-click**, and this is a decision, not an
omission. The report said "clicking on a paper should open a new tab"; in
`VaultHome.svelte:221` a single click is what selects the row, and the selection
drives the `VaultInspector` metadata pane beside the list. Making single-click
open would activate the reader tab on every click, which leaves that pane
reachable only by returning to the vault tab — where the next click leaves
again. The complaint this RFC answers is the eviction (R1.1), which is
gesture-independent. If double-click still feels wrong once tabs accumulate, the
change is one line in `PaperList.svelte:120`.

**Out of scope:** tab reordering, tab persistence across restarts, splits (the
`split` / `layout` words in the tab bar are still decoration), a limit on how
many papers can be open.

## Task list

| # | Task | Ships alone | Size |
|---|---|---|---|
| 1 | R1.1–R1.3 activate-or-append in both open paths | yes | XS |
| 2 | R1.4 tab strip overflow | yes | XS |

## Risks

- **Unbounded tabs.** Ten papers is ten tabs and no eviction. That is what the
  close button is for; a cap would be a second surprise on top of the one this
  RFC removes.

## Verification

Manual:

1. Open paper A, then paper B: two reader tabs, B active, A still open.
2. Open A again from the vault: no third tab, A's existing tab activates.
3. Close A: B stays open and active.
4. Open a discovery candidate for a paper already open: the same tab, now
   carrying the candidate.
5. Open eight papers: the strip scrolls; every tab is still readable.

## Success criteria

1. Opening a paper never closes another.
2. A paper open twice is one tab, not two.
