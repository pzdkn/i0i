# RFC 0087: Three vault surfaces that look real and are not

Status: Partially implemented — §2 and §3 landed; §1 (Ask this vault) not started
Date: 2026-08-13
Product: i0i
Target: Tauri v2 + SvelteKit (Svelte 5), macOS first
Milestone: Release 0.0.1
Builds on: RFC 0001 (app shell), RFC 0076 (hybrid chunk retrieval),
RFC 0077 (ContextManager), RFC 0079 §5 (retrieval is the floor).

## Summary

Three surfaces in the vault view are finished-looking mockups from the shell
era (RFC 0001–0004) that were never wired. They are not broken — they were never
connected — and each one currently tells the user something false.

| Surface | What it shows | What it is |
|---|---|---|
| "Ask this folder" (`VaultInspector.svelte:90`) | A question about attention complexity, a **Run** button, a **cite 22** button | Hardcoded prose. Neither button has an `onclick`. |
| Paper filter (`VaultHome.svelte:199`) | A `filter` input over the paper list | `localFilter` is declared at `:41`, bound to the input, and **never read**. |
| Status bar counts (`StatusBar.svelte:16`) | `vault / 234 papers / 12 unread`, `sync: local` | `get_vault_status()` (`commands/vault.rs:4`) returns those three literals. Always. |

---

## 1. "Ask this folder" should be "Ask this vault", and should ask

### Diagnosis

The panel renders a fixed sample question and two dead buttons. The word
"folder" is left over from the shell mock; everywhere else the domain calls this
a **vault**.

The machinery it needs already exists and is used by the reader. `SearchRequest`
(`services/search/mod.rs:57`) takes `vault_ids: Vec<String>` and resolves scope
through `resolve_search_scope` (`:138`), so vault-scoped hybrid retrieval is a
solved problem — RFC 0076 built it and the title-bar search already calls it
across a scope. The chat side has `ContextManager` (RFC 0077) and the
always-retrieve rule (RFC 0079 R5.1).

So this is **wiring, not new machinery**: a composer that runs a vault-scoped
ask and renders a cited answer with the `CitedAnswer` component the reader
already uses.

### Change

R1.1 Rename the section to **Ask this vault** and replace the sample text with
an empty composer (`placeholder: "Ask across this vault…"`).

R1.2 Submitting runs the existing chat path with `vault_ids: [activeVaultId]`
and no `paper_ids`, so retrieval spans the vault. Answers render through
`CitedAnswer`, and a citation opens the cited paper at the cited passage — the
same jump `onOpenSearchResult` already performs from the title bar.

R1.3 The `cite 22` button goes. The citation count belongs to an answer, not to
an empty composer.

R1.4 A vault ask is a conversation, not a one-shot: it gets a thread with
`anchor.kind = "vault"`, so it survives a re-open and appears in the vault's own
history. **This is the one piece of new domain here** — today `ThreadAnchor` is
`Document` or a passage, both paper-scoped.

## 2. The filter box filters nothing

### Diagnosis

`VaultHome.svelte:41` declares `localFilter`; `:199` binds an input to it; no
other line mentions it. The paper list is rendered from `workspace.papers`
unfiltered.

### Change

R2.1 Filter the list on the trimmed, case-folded value across **title, authors
and year** — the three columns the list already shows. Substring, not fuzzy: the
box says `filter`, and a filter that reorders results is a search.

R2.2 An empty result shows "No papers match *<query>*" with a clear affordance,
matching the annotations list's empty state (RFC 0062).

R2.3 The filter is view state, not persisted, and resets when the vault changes.

This is small enough that the alternative — deleting the box — is worse only
because the behaviour is obvious. Where the report says *"if we have no good
idea, keep them dead"*, this is a case where the good idea is unambiguous.

## 3. The status bar reports a fictional library

### Diagnosis

```rust
// src-tauri/src/commands/vault.rs:4
pub fn get_vault_status() -> VaultStatus {
    VaultStatus { paper_count: 234, unread_count: 12, sync_state: "local".to_string() }
}
```

It takes no arguments — not even a vault id — and the frontend calls it exactly
once, in `onMount` (`+page.svelte:151`). So the bar shows 234 papers on an empty
library, and it would still say 234 after you imported a thousand.

### Change

R3.1 `get_vault_status(vault_id: Option<String>)` counts from `LibraryStore`:
papers in the vault (or the whole library when `None`), and unread as
`status != 'READ'`. Two `count(*)` queries against tables the store already
indexes.

R3.2 The frontend refreshes it wherever the library changes — the same places
that already call `hydrateLibrary` / `syncPaperMetadata`: import, delete,
add-to-vault, status change, and vault switch. A `$derived` over the library
cache is simpler than a command per event and cannot go stale; the command then
exists only for first paint.

R3.3 `sync_state` is `"local"` because there is no sync. Say **`local-only`**
until there is something to sync, rather than implying a sync that is idle.

---

## Task list

| # | Task | Ships alone | Size |
|---|---|---|---|
| 1 | R3 live status counts | yes | S |
| 2 | R2 paper filter | yes | XS |
| 3 | R1 vault-scoped ask (R1.4 vault-anchored threads is the substance) | yes | M |

## Risks

- **A vault ask is a much larger retrieval scope than a paper ask.** RFC 0079's
  per-turn budget applies unchanged; the scope summary must say how many papers
  and chunks were in range, or the answer's confidence will not be readable.
- **Counting on every library mutation** is cheap now and stays cheap: two
  indexed counts. If it ever is not, it becomes a cached value invalidated by
  the same events.

## Success criteria

1. No control in the vault view is inert, and no number in the status bar is a
   literal.
2. Asking a question about a vault produces an answer citing papers from that
   vault, reachable again after a restart.
