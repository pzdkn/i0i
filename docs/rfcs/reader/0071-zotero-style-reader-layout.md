# RFC 0071: Zotero-style reader layout — collapsible section sidebar & tool panel

Status: Implemented (code complete, `pnpm check`/`build` green; manual QA pending)
Date: 2026-08-05
Product: i0i
Target: Tauri v2 + SvelteKit (Svelte 5), macOS first
Builds on: RFC 0045 (generic `ResizableSplit`), RFC 0048 (reader focus mode + its
threads-rail — the collapse precedent), RFC 0062 (annotations panel), RFC 0063
(reader toolbar), RFC 0067 (marks vs chats). **Supersedes** RFC 0068 R3 (the
note/chat segmented switch — see Design).

## Summary

Reshape the reader's right **inspector** and top **toolbar** into Zotero's
structure while keeping i0i's dark/amber chrome. The inspector becomes a
**collapsible sidebar** with a vertical **icon rail** that switches three
sections — **Info** (metadata), **Notes** (marks + note), **Chat**
(conversations) — and can collapse to a **full-width reading surface**,
re-expanded by a single toggle at the top-right of the toolbar.

This is a **frontend-only reorganization** of capabilities the reader already
has (highlight, note, chat, AI-highlight, zoom). It adds **no annotation tools**,
no new annotation types, no backend/bridge/schema changes.

## Problem

The reference is Zotero's reader: a persistent tool panel across the top, and a
collapsible right sidebar whose vertical icon rail flips between sections
(Info, Annotations, …); collapsing yields a full-width document, re-expanded from
a toolbar toggle.

i0i today is close in parts but not in shape:

- `ReaderInspector` has **two flat text tabs** — *Annotations* (marks **and** chat
  bundled together) and *Meta* (metadata) — always visible, **never collapsible**.
  The reading surface can't go full-width.
- **Chat is nested inside the Annotations tab.** A focused passage flips between
  its note and its conversation via a segmented switch (RFC 0068 R3), rather than
  chat being a first-class destination.
- There is no way to hide the panel for distraction-free reading.

The goal is to make metadata, notes, and chat **first-class switchable sections**
and let the reader go full-width — adopting Zotero's *structure* without touching
the annotation model or i0i's look.

## Scope

**In (v1):**

1. `ReaderInspector` becomes an **icon-rail sidebar** with three sections: **Info /
   Notes / Chat**.
2. The whole sidebar can **collapse** (full-width reading surface) and **expand**,
   toggled from a button at the **top-right of the toolbar**; expanding restores
   the user's previously dragged width, not the default.
3. `ReaderToolbar` gains that **sidebar toggle** plus light grouping so it reads as
   a tool panel.

**Out — explicit Non-Goals** (deliberate, per the guiding constraint "kind of the
same tools we already have: note, chat, zoom"):

- **No page navigation (`N / total`), no fit-width, no appearance/read-aloud.**
  Page nav needs current-page tracking off PDF scroll position (new plumbing
  through `ReaderView`/`PdfPage`), not a reorg. (Same deferral as RFC 0063.)
- **No text / area / ink annotation modes** and no new annotation type, storage,
  or canvas. i0i annotates by selecting text → picking a color (RFC 0058/0061);
  that is unchanged.
- **No light theme.** i0i keeps its dark/amber chrome; only Zotero's *structure*
  is borrowed. ("Do it like Zotero" referred to the collapse behavior, not the
  visual skin.)
- **No backend, bridge, command, or data-model change.** Frontend-only.
- **Focus mode (RFC 0048) is untouched** — it already collapses via its own
  `threads-rail`. This RFC targets **normal** reader mode.

## Design

### 1. Three-section sidebar (`ReaderInspector.svelte`)

A vertical **icon rail** hugs the right window edge (Lucide icons), with three
destinations; the active one is amber (matching the current tab-active style):

- **Info** — today's *Meta* section verbatim: the `meta-grid` (id / cite / marks)
  plus `MetadataPanel`.
- **Notes** — the **Marks** list + its Filter control, and the focused passage's
  **note editor** + color swatches.
- **Chat** — the **Chats** list, the **"Ask about this paper"** row, and the
  focused passage's **conversation** + composer.

Internals:

- `activeTab: "annotations" | "meta"` → `activeSection: "info" | "notes" | "chat"`.
- **Supersedes RFC 0068 R3.** The note/chat segmented switch (`detailView`) is
  **removed** — the rail *is* that switch now. The `{#if openPassage &&
  detailView === "note"}` / `detailView === "chat"` branches split across the
  Notes and Chat sections.
- **Shared focus survives section switches.** A focused passage (`openThread`)
  must **not** be cleared when the rail changes section: Notes shows that
  passage's note, Chat shows its conversation, and flipping between them keeps the
  passage. (This is the payoff of the split — the same passage, two lenses.)
- **Deterministic section on open.** The three call sites that today force
  `activeTab = "annotations"` each map to a section deliberately:
  - a fresh **selection** (`selection` effect) → **Notes** — jotting is the
    lightweight default;
  - a **mark / pin click** (`requestedThreadId` effect) → the section matching the
    passage, reusing RFC 0068's default rule: **Notes** if it has a note, else
    **Chat** if it has a conversation, else **Notes**;
  - **whole-paper** (`openWholePaper()`) → **Chat**. A document anchor has
    `openPassage === null`, so the Notes view would be empty for it; "Ask about
    this paper" must land on Chat.
- **Invariant note (RFC 0067).** "Every highlight appears in Marks ∪ Chats exactly
  once" still holds, but the two lists now live in **separate sections**. A
  note-only mark shows under **Notes**, not Chat — correct, but no longer
  self-evident from one tab. Documented here so it isn't "fixed" later.

### 2. Collapse / expand (`ReaderView.svelte` + `ReaderToolbar.svelte`)

- `ReaderView` owns a new `inspectorCollapsed` boolean, **persisted to
  `localStorage`** (e.g. `i0i.reader-inspector-collapsed`) and read on mount, so
  the choice survives reopening a paper.
- **Expanded** → the existing two-pane `ResizableSplit`
  (`storageKey="i0i.reader-split"`), whose persisted width already restores on
  mount.
- **Collapsed** → render the **single-pane** path (reader only) that the file
  already uses for the no-document case, giving a full-width reading surface.
- A **sidebar-toggle button** at the **top-right** of `ReaderToolbar` flips
  `inspectorCollapsed`; `ReaderView` passes `inspectorCollapsed` +
  `onToggleInspector` down. When collapsed, that toggle is the *only* affordance
  left on the right (matching the Zotero screenshot: full-width PDF, one icon
  top-right to bring the panel back). In focus mode the toggle is withheld
  (`onToggleInspector` passed as `undefined`), since that mode has its own
  threads-rail collapse.
- **Annotating while collapsed reveals the panel.** Selecting a passage or
  opening a mark's thread already routes through `openThreadsPanel()`; that
  chokepoint now un-collapses the inspector in normal mode, so the note/chat
  surface is never a dead end while collapsed.

**Implementation check (see Risks):** confirm `ResizableSplit` restores the
persisted width when the pane set flips from one pane back to two (a
collapse→expand round-trip). If it snaps to `default` instead, keep the split
mounted and collapse by hiding the inspector pane rather than swapping pane sets.

### 3. Top toolbar → full-width tool panel (`ReaderToolbar.svelte`, `ReaderView`)

The toolbar becomes the reader's single tool panel and spans the **full width**
above the split in normal mode (in focus mode it stays inside the reader pane).
Full-width placement is what keeps the collapse toggle **stationary** — otherwise
it rides the reader pane's right edge and shifts when the panel opens/closes. The
toolbar and its AI progress/Keep-Undo bar are one shared snippet (`toolbarStrip`)
rendered in exactly one place per mode.

Contents — all existing capabilities surfaced as icons; **no new annotation
types**:

- **Highlight selection** — marks the current selection with the sticky colour
  (reuses `pickColor`); disabled unless text is selected.
- **Note** / **Chat** — reveal the inspector and switch to that section, driven by
  a `requestedSection` request that also reveals a collapsed panel via
  `openThreadsPanel`.
- **Highlight with AI** — the existing menu, now icon-only.
- **Focus** — toggles reader focus mode (moved here from `ReaderHeader`); shows an
  exit-focus icon while in focus.
- **Zoom**, and the **sidebar toggle** at the far right (stationary).

`ReaderHeader` loses the dead **Graph** and **Ask** buttons and its Focus/Exit
controls (the toolbar owns focus now); its focus-mode threads toggle stays.

Note on semantics: **Note** navigates to the Notes section, **Highlight** acts on
the selection. In i0i both notes and highlights live in the same Notes/Marks
section, so these are intentionally different verbs (navigate vs act), not two
parallel annotation modes.

## Components touched

- `ReaderInspector.svelte` — the icon-rail + three-section refactor (largest
  change; pure view/state reshaping, no new data).
- `ReaderView.svelte` — `inspectorCollapsed` state + persistence, pane switching,
  toolbar wiring.
- `ReaderToolbar.svelte` — sidebar toggle + cluster grouping.
- `ResizableSplit.svelte` — only if the width-on-remount check fails (see Risks).

## Testing

Presentational Svelte with no unit harness (matching 0060–0063): verification is
`pnpm check` + `pnpm build` + a manual matrix. Any extracted pure helper is
trivial and gets a node test if it appears.

Manual matrix:

- Rail switches **Info / Notes / Chat**; active state and keyboard focus correct.
- Select a passage → **Notes** shows its note, **Chat** shows its conversation;
  switching sections **keeps** the passage; **"Ask about this paper"** opens
  **Chat**.
- A mark click / pin opens the passage in the right section (not an empty view).
- **Collapse** → reading surface goes full-width, one toggle top-right; **expand**
  restores the *previously dragged* width, not the default.
- Collapsed state **persists** across reopening the paper.
- **Focus mode** still collapses via its own threads-rail (unchanged).
- A note-only mark appears under **Notes**; an ask-only passage under **Chat**.

## Risks

- **Split width on remount.** If `ResizableSplit` doesn't restore its persisted
  width when the pane set changes, expand would snap to `default`. Mitigation:
  verify first; fall back to keeping the split mounted with a hidden/zero-width
  inspector pane.
- **Inspector state migration.** Folding the note/chat detail into rail sections
  touches the effects that force `activeTab`; each must map to the correct section
  or a focused passage lands on an empty view. Covered by the manual matrix above
  and by the explicit call-site mapping in Design §1.
- **Invariant legibility.** Note-only marks are no longer co-visible with chats;
  documented in Design §1 so a future reader doesn't treat it as a bug.

## Known limitations

- **Collapse unmounts the inspector, so its transient state resets.** Persisted
  state survives — the collapse flag (`i0i.reader-inspector-collapsed`) and the
  active rail section (`i0i.reader-inspector-section`). What does *not* survive an
  expand: an open passage/thread view falls back to its list, and an **unsaved**
  Note/Ask draft is lost. Saved notes and sent asks are unaffected (they live in
  the backend; a collapse mid-ask still persists the answer, though the panel
  won't reload it until the next chat reload). Acceptable for v1; if it proves
  annoying, lift the draft/open-thread state up to `ReaderView`.
- **Auto-expand does not change the saved preference.** Annotating or asking while
  collapsed reveals the panel for the session only; the toolbar toggle is the one
  control that writes the collapse preference.

## Rollout (slices)

1. ✅ **Inspector rail + three sections** (Info / Notes / Chat); removed
   `detailView` (supersedes 0068 R3); mapped the three `activeTab` call sites to
   sections. Behavior otherwise unchanged.
2. ✅ **Collapse/expand**: `inspectorCollapsed` state + localStorage persistence +
   full-width single-pane path + the toolbar sidebar toggle (+ auto-expand on
   annotate/ask via `openThreadsPanel`).
3. ✅ **Toolbar grouping/divider polish** (a divider before the sidebar toggle).
4. ✅ **Full tool panel**: full-width `toolbarStrip` (stationary toggle) with
   Highlight / Note / Chat / AI (icon-only) / Focus icons; `requestedSection`
   wiring; removed dead Graph/Ask and header Focus controls.

Each slice is independently shippable and `pnpm check`/`build`-clean.

## References

- Zotero reader (reference screenshots): persistent tool panel + collapsible
  section sidebar; collapsed = full-width document with one toggle top-right.
- RFC 0045 (`ResizableSplit`), 0048 (focus mode rail — collapse precedent), 0062
  (annotations panel), 0063 (reader toolbar), 0067 (marks vs chats), 0068 (note/
  chat detail switch — R3 superseded here).
