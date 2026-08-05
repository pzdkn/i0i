# RFC 0068: Passage detail — Note / Chat one-at-a-time

Status: Implemented
Date: 2026-07-31
Product: i0i
Target: Tauri v2 + SvelteKit (Svelte 5), macOS first
Builds on: `docs/design/pdf-reader-ux-refinement.md` (R3). Follows RFC 0067.

## Summary

The passage detail in `ReaderInspector` currently stacks a **Note** field *and*
a **Chat** composer, so it's unclear which input does what (refinement #5). This
RFC adds a **segmented switch `Note | Chat`** so exactly one input shows at a
time.

- **Note** sub-view: the note field (Enter saves). Nothing else.
- **Chat** sub-view: the conversation thread + ask composer (Enter sends).
- The **color swatches stay above** the switch — color is orthogonal to
  note/chat, and a fresh selection still colors in one click.
- **Default:** **Chat** if the passage already has a conversation (entries),
  otherwise **Note** (jotting is the lightweight default for a fresh passage).

One passage, one visible input, chosen deliberately.

## Scope / model

No data model change. The switch is pure view state (`detailView: "note" |
"chat"`) local to `ReaderInspector`, reset whenever the open passage changes.

The switch only appears for a **passage** thread (one with a real `openPassage`
excerpt). The **whole-paper** thread ("Ask about this paper") has no note, so it
shows the Chat composer directly with no switch.

## Behaviour

- Opening a passage seeds `detailView` from the passage: `chat` when its thread
  has entries, else `note`. Keyed on the same `passageKey` the note-draft effect
  uses, so it re-seeds once per passage, never mid-typing.
- Switching to **Chat** on a passage with no thread yet shows the empty-thread
  composer (asking creates the thread + marks the passage, unchanged from RFC
  0058/0064).
- Switching to **Note** shows the note field; saving a note does not create a
  thread (RFC 0061).

## Non-goals / deferred

- **Ask deep-link from the selection popup** (R3's "arrive via Ask → Chat"):
  requires threading a selection *intent* from the reader popup into the
  inspector. Deferred — the entries-based default covers the common case, and a
  one-click switch reaches Chat immediately. Tracked as a follow-up.
- Note/Chat page-marker distinction — RFC 0067 risk (one neutral marker).

## Testing

Presentational; `pnpm check` + `pnpm build` + manual:

- Select a fresh passage → **Note** sub-view active, only the note field shows.
- Flip to **Chat** → note field hidden, composer shows; ask → thread appears.
- Reopen a passage that has a conversation → opens on **Chat**.
- Color swatches work in both sub-views (a fresh selection still colors).
- Whole-paper "Ask about this paper" shows the composer with no switch.

## Rollout

Single slice — `ReaderInspector.svelte` only.
