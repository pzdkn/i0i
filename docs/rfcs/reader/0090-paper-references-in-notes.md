# RFC 0090: `[@vault/paper]` — citing your own library from a note

Status: Proposed
Date: 2026-08-13
Product: i0i
Target: Tauri v2 + SvelteKit (Svelte 5), macOS first
Milestone: Release 0.0.1
Builds on: RFC 0061 (note as a highlight attachment), RFC 0078 (inline citation
nodes in `markdown.ts`), RFC 0079 §3 (note editor), RFC 0076 (library search).

## Summary

A note today is a plain string on a highlight. The report wants notes to link
papers: *"you should be able to reference other papers in the same vault, or
another vault, in your notes, for example with `[@vault/goodpaper]` notation."*

That turns a pile of per-paper notes into a graph — which is the point of the
vault, and the thing the app currently cannot express.

---

## 1. The notation

R1.1 The reference syntax is `[@vault/paper]`, matching the report:

| Written | Means |
|---|---|
| `[@transformers/attention-is-all-you-need]` | that paper, in that vault |
| `[@attention-is-all-you-need]` | that paper in **the current vault**, unqualified |
| `[@transformers/]` | the vault itself |

R1.2 The slug after the slash is the paper's **citation key** — `citationKey`
already exists on the reader document (`ReaderInspector.svelte:1327` renders it)
and is exactly the stable, typeable, human-meaningful handle this needs. Not the
paper id (`paper_1786…` is not something anyone types) and not the title (it
changes when metadata autofill runs).

R1.3 An unresolvable reference stays literal text. This is the same rule
`markdown.ts` already applies to citation markers — *"a `[n]` the assembly never
minted stays literal text"* — and for the same reason: a wrong render is worse
than an unrendered bracket.

## 2. Parsing

R2.1 `markdown.ts` gains one inline node kind, `paperRef`, beside the existing
`citation` node. The module's contract is unchanged and load-bearing: it emits
**typed nodes, never HTML**, so a note can never become markup. A reference is
resolved to a node during the inline pass, exactly as `[n]` is (RFC 0078).

R2.2 Resolution needs a lookup from `(vaultSlug, citationKey)` to a paper id.
The library cache in the frontend (`state/library-cache.svelte`) holds vaults and
papers already; resolution is a map built from it, not a backend round trip.

R2.3 Ambiguity — the same citation key in two vaults, unqualified — resolves to
the current vault if present, and otherwise stays literal. Guessing between two
papers is worse than not linking.

## 3. Where a reference renders, and where it does not

**This is the part that is not just a parser.** Notes surface in four places,
and only one of them renders markdown today:

| Surface | Today | After |
|---|---|---|
| Note editor (`ReaderInspector.svelte:1067`) | `<textarea>`, raw text | Unchanged — you edit source, and R4 autocompletes into it |
| Saved note (`ReaderInspector.svelte:1077`) | Plain text in a button | Rendered: references become clickable |
| Popover note (`HighlightPopover.svelte:109`) | `<textarea>` | Unchanged |
| Marks list row (`ReaderInspector.svelte:1481`) | `note ?? excerpt`, truncated | Reference renders as the paper's short title, not its raw slug |

R3.1 Clicking a rendered reference opens that paper — `reader:<paperId>`, a new
tab beside the current one (RFC 0084), never replacing what you were reading.
Following a reference out of a note is exactly the case tab eviction used to
punish.

R3.2 A reference to a vault opens that vault's tab.

R3.3 Hovering shows the paper's full title and year, because a citation key is
compact by design and unreadable by accident.

## 4. Writing one

R4.1 Typing `[@` in the note editor opens an inline autocomplete over the
library: fuzzy over title and citation key, current vault first, showing title +
year + vault. Selecting inserts the full `[@vault/key]` form.

R4.2 The pattern already exists in this codebase — `DiscoverTargetEditor.svelte`
does inline vault autocomplete (RFC 0006), including the keyboard handling
(`↑`/`↓`/`Enter`/`Esc`). Reuse its shape rather than inventing a second
autocomplete idiom.

R4.3 Autocomplete is a convenience, never a requirement: the notation is plain
text and typing it by hand must work.

## 5. What this is not

**Not backlinks.** "Which notes reference this paper" is the obvious next
feature and a different one: it needs a persisted edge table, extraction on
save, and a UI surface to show them. This RFC deliberately stops at the forward
reference, and R5.1 is the hook that makes the next step cheap.

R5.1 References are extracted **at render**, not stored. No schema change, no
migration, no risk of an index disagreeing with the note text. If backlinks land
later they can build an index from the same parser.

---

## Task list

| # | Task | Ships alone | Size |
|---|---|---|---|
| 1 | R2 `paperRef` node in `markdown.ts` + resolution map (pure, unit-testable) | yes | S |
| 2 | R3 render and open, in the saved-note and marks-list surfaces | no — wants 1 | M |
| 3 | R4 `[@` autocomplete | no — wants 1 | M |

## Risks

- **Citation keys are not guaranteed unique.** Two imports of the same paper, or
  two papers by the same author in a year, can collide. R2.3's rule (current
  vault wins, otherwise literal) keeps a collision from silently linking the
  wrong paper, but the real fix is uniqueness at import, which is out of scope
  here and worth its own look.
- **Rendering notes means notes stop being plain text.** A note containing
  `[@foo]` that the user did *not* mean as a reference will render as one if it
  happens to resolve. Accepted: the notation is explicit enough that this is
  rare, and R1.3 makes the non-resolving case invisible.

## Open Decisions

- **A. Should the reference render as the citation key or the title?** Key is
  compact and matches what you typed; title is readable. Recommendation: render
  the **short title** with the key in the tooltip — the note is prose, and prose
  reads better with names than with slugs.

## Success criteria

1. `[@vault/key]` typed in a note renders as a link and opens that paper in its
   own tab.
2. A reference that does not resolve is left exactly as typed.
3. No new table, no migration.
