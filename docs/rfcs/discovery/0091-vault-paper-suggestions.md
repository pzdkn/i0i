# RFC 0091: Papers this vault is missing

Status: Proposed
Date: 2026-08-13
Product: i0i
Target: Tauri v2 + SvelteKit (Svelte 5), macOS first
Milestone: Release 0.0.1
Builds on: RFC 0020 (scout discovery model), RFC 0037/0088 (deep research),
RFC 0057 (semantic ranking), RFC 0075 (chunk embeddings), RFC 0005 (add a
discovery candidate to a vault).

## Summary

Discover answers a question you asked. This feature answers one you did not:
*given what is in this vault, what should be in it?*

The report asks for weekly-or-manual suggestions and flags the UI as unresolved:
*"it's not clear how the UI should be connected / where should we select those
papers and add them to vault? Should it be connected to the Finder/Discover?"*
§1 answers that.

---

## 1. Where this lives

### The question, sharpened

Two things are being conflated and they want different surfaces:

- **The suggestion run** — computing candidates. This is a discovery search with
  a machine-written query, and it belongs to the machinery Discover already has.
- **The suggestion inbox** — where you meet the results and accept or reject
  them. This is *not* a search result list, because you did not search: it is a
  small, standing set of proposals with a decision attached to each.

### The decision

**Run it in Discover's machinery; surface it in the vault.**

R1.1 Suggestions appear as a **Suggestions section in the `VaultInspector`**,
directly under the vault's stats — the panel that already answers "how is this
vault doing" gets the row that answers "what is it missing." Each suggestion is
one row: title, year, one line of *why this vault*, and two buttons — **Add** and
**Dismiss**.

R1.2 **Not** a Discover tab. Discover is question-shaped: you type a target, it
searches, results are transient and scoped to that window. A suggestion has no
question, belongs to a vault rather than a search, and must survive until you
decide. Putting it in Discover would make it a search you did not run, sitting
in a list you would have to re-run to get back.

R1.3 **Not** a new mode or tab kind either. Three to five standing suggestions
do not need a workspace; they need to be where you already look at the vault.
This is the argument that fails for the quiz (RFC 0089 §1) and holds here — the
difference is session versus glance.

R1.4 **Add** reuses the existing add-candidate path (RFC 0005) exactly, so a
suggestion becomes a paper the same way a Discover result does. **Dismiss** is
persistent: a rejected suggestion never returns, which is the only thing that
makes a standing list tolerable.

### When it runs

R1.5 Manual is the primitive: a **Suggest** button in the section header. That
is the whole feature for 0.0.1 if the weekly part slips.

R1.6 Weekly is a schedule on top of it: on app start, if the vault's last
suggestion run is more than seven days old and the vault has at least three
papers, run once in the background. Never more than one vault per start, so
opening the app is never a burst of provider calls.

R1.7 The section header states when it last ran. A stale suggestion list that
does not say it is stale is worse than an empty one.

---

## 2. What makes a good suggestion

R2.1 The vault's **centroid** — the mean of its papers' chunk embeddings
(RFC 0075) — is the query. This is the one thing this feature has that a normal
search does not: it knows what the vault is *about* without anyone describing
it.

R2.2 Candidates come from two directions, merged and deduped by the existing
`dedup` primitive:

- **Citation neighbourhood.** Papers cited by, or citing, papers in the vault.
  This is the feature the release notes elsewhere call "find similar papers
  through the network of references," and it is the highest-precision source we
  have.
- **Semantic search.** Machine-written queries from the vault's dominant topics,
  through the existing multi-provider path.

R2.3 Ranked by the RFC 0057 semantic reranker against the centroid, then filtered
hard: **anything already in the vault, already dismissed, or already in the
library is removed** before ranking, not after. A suggestion you already own is
the fastest way to make the feature look broken.

R2.4 Each suggestion carries a one-line reason grounded in the vault — *"cited
by 3 papers here"*, *"closest to your work on sparse attention"* — not a generic
abstract summary. The reason is the difference between a recommendation and a
list.

R2.5 Cap at **five**. A suggestion list you scroll is a search result.

## 3. Storage

R3.1 `vault_suggestions` (`vault_id`, `paper_ref`, `reason`, `score`,
`created_at`, `state: pending | added | dismissed`) with an index on
`(vault_id, state)` and `on delete cascade` from `vaults` — indexed at creation,
per RFC 0079 §6.

R3.2 Dismissals are permanent and are consulted by the *next* run's filter
(R2.3), which is what stops the same paper being offered forever.

---

## Task list

| # | Task | Ships alone | Size |
|---|---|---|---|
| 1 | R3 schema + R2.1–R2.3 centroid + semantic candidates + filtering | yes | M |
| 2 | R1.1–R1.5 inspector section, Add / Dismiss, manual run | no — wants 1 | M |
| 3 | R2.2 citation-neighbourhood source | yes | M |
| 4 | R1.6–R1.7 weekly schedule | no — wants 2 | S |

## Risks

- **Suggestions are a background spend.** R1.6's one-vault-per-start rule and
  the existing research budget bound it, but this is the first feature that
  costs money without the user asking. It must be disable-able in Settings, and
  the RFC assumes it is.
- **A vault with three papers has no centroid worth the name.** R1.6's minimum
  is a floor, not a fix; below ~10 papers the suggestions will be broad. Say so
  in the empty state rather than shipping vague rows.
- **Citation-graph coverage is uneven** across the providers we use. This source
  may be strong for arXiv/OpenAlex and thin elsewhere.

## Open Decisions

- **A. Should suggestions also appear on the vault home, not just the
  inspector?** The inspector is only visible when the vault tab is open and the
  split is not collapsed. Recommendation: inspector only for 0.0.1, and revisit
  if the section goes unnoticed — a badge on the vault tab is the cheap escalation.
- **B. Weekly for every vault, or only the active one?** Recommendation: only
  vaults opened in the last 30 days. A vault you have not touched in a year does
  not need a weekly spend.

## Success criteria

1. From an open vault, suggestions are one click away and never include a paper
   the library already has.
2. Every suggestion says why *this vault*.
3. A dismissed paper never appears again.
