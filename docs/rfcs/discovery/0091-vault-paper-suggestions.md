# RFC 0091: Papers this vault is missing

Status: Implemented
Date: 2026-08-13 (revised 2026-08-20)
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

**Run it with Discover's machinery; work with it in the vault's centre pane.**

R1.1 Suggestions appear as a **Suggestions view tab in `VaultHome`**, beside
Papers and below the folder header. Selecting it replaces the paper list in the
centre pane; it does not open another workspace tab or change the left-hand
Vault Explorer.

The current `workspace.tabs` row is display-only: it renders `<span>` elements,
always marks the first one active and includes views that do not exist. This
feature must not add another inert label to that row. Task 3 introduces a real
`activeVaultView: papers | suggestions` state and renders implemented views as
actual tab controls. Planned labels such as Graph and Q&A remain hidden until
they have a view to open.

```text
┌ Vault Explorer ┬──────────────────────────────────┬ Inspector ┐
│                │ Folder: interpretability         │           │
│                │ Papers 42 | Suggestions 5        │ run state │
│                ├──────────────────────────────────│ or detail │
│                │ suggested paper rows             │           │
│                │ title · year · why this vault    │           │
│                │                         [+] [×]   │           │
└────────────────┴──────────────────────────────────┴───────────┘
```

The centre pane is the work surface: it has enough width to scan titles,
authors, venues and evidence, and it already owns paper-list interactions. The
inspector is contextual and narrow; making it hold the inbox would hide
suggestions whenever selected-paper metadata occupies it.

R1.2 **Not** a Discover tab. Discover is question-shaped: you type a target, it
searches, results are transient and scoped to that window. A suggestion has no
question, belongs to a vault rather than a search, and must survive until you
decide. Putting it in Discover would make it a search you did not run, sitting
in a list you would have to re-run to get back.

R1.3 **Not** a new workspace tab kind either. `Suggestions` is a local view of
the already-open vault, analogous to changing the list being shown rather than
opening a new document. Three to five standing proposals do not need their own
workspace lifecycle.

R1.4 The `VaultInspector` carries only a compact status row: suggestion count,
last-run time and current run state. Activating that row selects the Suggestions
view. While a run is active, the inspector shows the RFC 0088 phase and counts;
when a suggestion row is selected, it shows that paper's metadata and complete
evidence. It never duplicates the suggestion list.

The inspector is supplementary, not the only place progress appears. An active
run is visible on the Suggestions tab, in a compact status line above the
suggestion rows and in greater detail in the inspector. Collapsing or narrowing
the inspector must not make a running search look idle. User-facing phases are
plain — `Searching 2/4`, `Ranking`, `5 found` — rather than provider or
reflection terminology.

R1.5 **Add** reuses the existing add-candidate path (RFC 0005) exactly, so a
suggestion becomes a paper the same way a Discover result does. **Dismiss** is
persistent: a rejected suggestion never returns, which is the only thing that
makes a standing list tolerable. Dismiss removes the row immediately and offers
`Dismissed · Undo`; Undo returns its state to `pending`. Permanent suppression
must not turn one accidental click into an irreversible recommendation change.

### When it runs

R1.6 Manual is the primitive. The Suggestions tab contains one
**Find similar papers** action in its toolbar; after a successful run the same
control becomes the refresh action. It does not join Import PDF, Add web page
and Export in the folder header, because those are collection-management
commands while this action belongs to one local view.

R1.7 Weekly is a schedule on top of it: on app start, if the vault's last
suggestion run is more than seven days old and the vault has at least three
papers, run once in the background. Never more than one vault per start, so
opening the app is never a burst of provider calls.

R1.8 The tab toolbar states when it last ran. The first run streams up to five
provisional rows in stable arrival order as RFC 0088 emits candidate previews;
it does not reshuffle the list after every event. Add and Dismiss already work
on provisional rows, and finalisation respects those decisions. When the run
finishes, the remaining rows take their final fused order once.

A refresh behaves differently: the current five suggestions stay visible and
actionable while progress updates around them, then are replaced atomically
when the new run succeeds. A failed or cancelled refresh leaves the previous
list untouched. Added and dismissed decisions remain durable in either path.

### Interaction contract

R1.9 Suggestions use a dedicated, stable two-line row, approximately 48–52px
high: year and venue; title and authors; one evidence line; Add and Dismiss
actions. They are not cards. `PaperList` cannot be reused unchanged because its
entire row is a `<button>`; placing actions inside it would create nested
interactive controls. The suggestion row has a primary title/select control and
separate sibling action buttons.

R1.10 A single click selects a suggestion and updates the inspector. Double
click, or Enter while its primary control is focused, opens the candidate in
the existing reader path. Add does not open it. After Add or Dismiss, focus
moves to the next suggestion, or to the Suggestions tab when none remain.

R1.11 The visible tab label is **Suggestions** because the set includes citation
neighbours as well as semantically similar papers. The first command is
**Find similar papers**; after a successful run it becomes a refresh icon with
the tooltip and accessible name **Refresh suggestions**. Add and Dismiss use
the familiar plus and close icons, each with a paper-specific accessible name.

R1.12 The tab strip is a keyboard-operable `tablist`: real buttons with
`role="tab"`, `aria-selected`, linked `tabpanel` ids and Left/Right arrow
navigation. Run progress is announced without repeatedly interrupting screen
readers; completion and failure use a polite live region. Undo receives focus
only when Dismiss was keyboard-triggered.

R1.13 The vault's tag chips may remain visible in both views, but paper-list
controls do not leak into Suggestions. The title/author/year filter and
`sort recent` are hidden there: five ranked proposals need neither filtering
nor a second sort. The Suggestions tab owns only last-run state and its
find/refresh command.

R1.14 The tab count is the number of pending, actionable suggestions; added and
dismissed rows leave the count. A vault with no papers shows no run command and
the compact empty state `No papers to match yet`. One- and two-paper vaults may
run manually, with evidence stating the small basis rather than claiming strong
vault similarity. A successful run with no candidates says `No new suggestions`
and keeps Refresh available. Failures show Retry and preserve any previous
results.

---

## 2. What makes a good suggestion

The report asks for this to be done *cleverly*, and points at two levers: the
methods Discover already has, and k-hop traversal of the citation graph. Both
are right, and the literature says why — plus what neither of them does alone.

### What the field does

- **Citation-informed embeddings.** [SPECTER](https://arxiv.org/abs/2004.07180)
  and its successors (SPECTER2, SciNCL) train document embeddings so that
  *cited-together* papers land near each other, then recommend by plain cosine
  similarity with no reranking stage. The lesson is not "use SPECTER" — we embed
  chunks locally (RFC 0075) — but that **citation structure and text similarity
  are different signals**, and the strong systems fuse them rather than picking
  one.
- **Co-citation and bibliographic coupling.** Two papers cited *by* the same
  work (co-citation), or citing the same works (bibliographic coupling), are
  related even with no direct edge and no textual overlap. Accumulating this
  evidence across a whole library is what
  [citation-community work](https://arxiv.org/pdf/2605.07158) finds captures
  agenda-level relatedness that text embeddings miss.
- **Agentic retrieval over the graph.** Recent citation-graph-aware retrievers
  run a query-generating agent over multiple facets of a topic and re-rank by
  internal citation count within the retrieved set — which is close to what
  RFC 0088 is building for deep research.

### The design

R2.1 **Three candidate generators, fused.** None of them alone is good enough,
and the fusion is the cleverness:

| Generator | Signal | Why |
|---|---|---|
| **k-hop citation neighbourhood** | structural | Papers your vault's papers cite (hop 1 out), papers citing them (hop 1 in), and hop 2 of both |
| **Co-citation / coupling frequency** | structural, accumulated | A hop-2 paper reached from *five* of your papers is a different proposition from one reached from one |
| **Deep Research search** | textual | Machine-written queries from the bounded vault profile, through the existing multi-provider path |

R2.2 **RFC 0088 Deep Research is the semantic candidate generator.** This RFC
does not implement a second agent loop. A `VaultSuggestionService` builds a
bounded textual vault profile from paper titles, abstracts and representative
extracted passages, then supplies that profile as the goal to RFC 0088's
`agent::run`. Its planner fans out provider queries, reflects on missing facets,
retires unproductive providers and streams `CandidatePreview` updates.

Reuse means the typed research loop and provider seams, not the Discover UI: a
suggestion run must not create a visible Discover window or an entry in the
user's saved-search list. Vault-specific orchestration maps research progress to
events carrying `vault_id`, then persists the final filtered candidates as
suggestions. Before a preview reaches Svelte, the service applies R2.8's hard
filter and preliminary vault-similarity ordering, so the list does not flash
papers the user already owns or papers that disappear only at completion.

R2.3 **Retrieval and similarity have different jobs.** Deep Research produces a
broad candidate pool from the textual vault profile. It does not decide the
final order. After retrieval, candidates are embedded and compared with the
vault centroid; this vault-specific score feeds the final fusion in R2.6. This
prevents the machine-written query from becoming the sole definition of what
the vault is about.

R2.4 **The traversal primitive already exists and is unused.**
`openalex_lineage_filter` (`commands/discovery/providers/openalex/search.rs:186`)
builds `cited_by:<id>` for references and `cites:<id>` for citations. It is
marked `#[allow(dead_code)]` with the comment *"Wired into RealCandidateSource in
the RFC 0037 seams layer"* — written for deep research and never connected. This
RFC is its first caller.

R2.5 **k = 2, and the fan-out is bounded per hop.** Hop 1 from a 40-paper vault
is already thousands of works; hop 2 is unbounded in practice. So: seed with the
vault's *most central* papers (most-cited within the vault's own hop-1 graph, not
globally), cap hop-1 expansion per seed, and take hop 2 only from hop-1 papers
that were reached more than once. **Frequency of arrival is the ranking signal
that makes hop 2 affordable** — it is also exactly the co-citation evidence R2.1
wants, so the bound and the quality measure are the same number.

R2.6 **Fusion, not concatenation.** A candidate's score combines: how many vault
papers reach it (structural), its cosine similarity to the vault centroid
(textual, RFC 0075 chunk embeddings), and its own citation count as a weak prior.
Reciprocal-rank fusion is the obvious combiner and the codebase already has one —
`services/search/fusion.rs`, built for hybrid retrieval in RFC 0076. Reuse it
rather than inventing a scoring formula.

R2.7 **The vault centroid** — the mean of its papers' chunk embeddings — is what
the textual half of R2.6 measures candidates against. Provider APIs cannot be
queried with a local vector, so the bounded textual profile drives retrieval
while the centroid drives similarity ranking. This is the thing this feature
has that a normal search does not: it knows what the vault is *about* without
anyone describing it.

Only papers with ready chunk embeddings contribute to the centroid. Titles and
abstracts from every paper may still contribute to the textual vault profile.
If no vault embeddings are available, the run still retrieves candidates and
labels vault similarity unavailable; final ordering then uses Deep Research and
citation ranks rather than manufacturing a similarity score.

R2.8 **Filter before ranking, hard**: anything already in the vault, already in
the library, or previously dismissed is removed before scoring. A suggestion you
already own is the fastest way to make the feature look broken.

R2.9 **Every suggestion states its evidence**, and the evidence differs by
generator: *"cited by 4 papers in this vault"*, *"cites 3 of the same works as
Vaswani 2017"*, *"closest to your work on sparse attention"*. The reason is not
decoration — it is how you decide without opening the paper, and a fused score
with no explanation is a number nobody can act on.

R2.10 Cap at **five**. A suggestion list you scroll is a search result.

### Provider reality

R2.11 OpenAlex is the only provider we use with a traversable citation graph, so
the structural generators are OpenAlex-only for now and the RFC says so rather
than implying coverage we lack. RFC 0088's current `RealCandidateSource`
searches OpenAlex and arXiv; Europe PMC and CORE join suggestions only after
that source supports them. A vault of papers with no OpenAlex ids degrades to
R2.1's Deep Research row alone — which is exactly the situation the fusion has
to survive gracefully.

## 3. Storage

R3.1 `vault_suggestions` (`vault_id`, `paper_ref`, `reason`, `score`,
`created_at`, `state: pending | added | dismissed`) with an index on
`(vault_id, state)` and `on delete cascade` from `vaults` — indexed at creation,
per RFC 0079 §6.

R3.2 Dismissals are durable and are consulted by the *next* run's filter
(R2.8), which is what stops the same paper being offered forever. The immediate
Undo action is the only path that restores a dismissal to `pending`.

R3.3 `vault_suggestion_runs` records `id`, `vault_id`, status, start/finish
times, stop reason and error. It gives the inspector and weekly scheduler one
durable source of truth without pretending a suggestion run is a user-created
Discover search. Detailed per-round state remains transient and arrives through
progress events.

---

## Task list

| # | Task | Ships alone | Size |
|---|---|---|---|
| 1 | R3 schema + R2.7/R2.8 vault centroid, filtering and run ledger | yes | M |
| 2 | R2.2–R2.3 vault profile + RFC 0088 Deep Research candidate generation and progress events | no — wants 1 | M |
| 3 | R1.1–R1.6 + R1.8–R1.14 accessible centre-pane Suggestions view, dedicated rows, progress, Undo, Add / Dismiss and manual run | no — wants 1, 2 | M |
| 4 | **R2.4–R2.5 k-hop traversal** — wire the dead `openalex_lineage_filter` | yes | M |
| 5 | R2.6 RRF fusion over the three generators + R2.9 evidence lines | no — wants 1, 2, 4 | M |
| 6 | R1.7 weekly schedule | no — wants 3 | S |

## Risks

- **Suggestions are a background spend.** R1.7's one-vault-per-start rule and
  the existing research budget bound it, but this is the first feature that
  costs money without the user asking. It must be disable-able in Settings, and
  the RFC assumes it is.
- **A vault with three papers has no centroid worth the name.** R1.7's minimum
  is a floor, not a fix; below ~10 papers the suggestions will be broad. Say so
  in the empty state rather than shipping vague rows.
- **Citation-graph coverage is uneven.** R2.11 states it: OpenAlex only. A vault
  whose papers lack OpenAlex ids gets semantic suggestions and nothing else.
- **Hop 2 is a fan-out hazard.** R2.5's arrival-frequency gate is what keeps it
  finite; if the numbers in practice say otherwise, the answer is a smaller k,
  not a bigger budget.

## Resolved Decisions

- **A. Weekly scope:** at startup, choose at most one due vault, preferring the
  most recently updated vault with at least three papers. The schedule is
  enabled by default and can be disabled under Settings → Search. Tracking
  precise last-open time remains unnecessary until the app has a general recent
  workspace history.

## Implementation

Implemented 2026-08-20. The production path includes the durable run ledger and
inbox, bounded vault profile, RFC 0088 research loop, OpenAlex two-hop expansion,
vault-centroid scoring, multi-signal RRF, provisional progress events, the
accessible centre-pane Suggestions view, Add/Dismiss/Undo, and the configurable
weekly startup schedule.

## Success criteria

1. From an open vault, Suggestions is a visible centre-pane view and never
   includes a paper the library already has.
2. Every suggestion says why *this vault*, in the terms of the generator that
   found it.
3. A suggestion reached from several of your papers outranks one reached from
   one, all else equal.
4. A dismissed paper never appears again.
5. A manual run uses the RFC 0088 loop, streams candidates before completion and
   creates no Discover workspace or saved-search entry.
6. Papers and Suggestions are real keyboard-operable tabs; no unimplemented
   vault view is presented as an active control.
7. Dismiss offers Undo, and Add or Dismiss never strands keyboard focus.
8. Run state remains visible with the inspector narrowed, and a failed refresh
   preserves the previous suggestion list.
9. Empty-vault, zero-result and one-to-two-paper states remain honest and offer
   only actions that can succeed.

## Sources

- [SPECTER: Document-level Representation Learning using Citation-informed
  Transformers](https://arxiv.org/abs/2004.07180) — citation-informed embeddings,
  cosine similarity without reranking.
- [Topic Is Not Agenda: A Citation-Community Audit of Text
  Embeddings](https://arxiv.org/pdf/2605.07158) — co-citation and bibliographic
  coupling capture relatedness text embeddings miss.
- [Citation Recommendation for Research Papers via Knowledge
  Graphs](https://arxiv.org/pdf/2106.05633) — graph traversal for citation
  recommendation.
