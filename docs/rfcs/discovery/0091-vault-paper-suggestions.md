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
| **Semantic search** | textual | Machine-written queries from the vault centroid, through the existing multi-provider path |

R2.2 **The traversal primitive already exists and is unused.**
`openalex_lineage_filter` (`commands/discovery/providers/openalex/search.rs:186`)
builds `cited_by:<id>` for references and `cites:<id>` for citations. It is
marked `#[allow(dead_code)]` with the comment *"Wired into RealCandidateSource in
the RFC 0037 seams layer"* — written for deep research and never connected. This
RFC is its first caller.

R2.3 **k = 2, and the fan-out is bounded per hop.** Hop 1 from a 40-paper vault
is already thousands of works; hop 2 is unbounded in practice. So: seed with the
vault's *most central* papers (most-cited within the vault's own hop-1 graph, not
globally), cap hop-1 expansion per seed, and take hop 2 only from hop-1 papers
that were reached more than once. **Frequency of arrival is the ranking signal
that makes hop 2 affordable** — it is also exactly the co-citation evidence R2.1
wants, so the bound and the quality measure are the same number.

R2.4 **Fusion, not concatenation.** A candidate's score combines: how many vault
papers reach it (structural), its cosine similarity to the vault centroid
(textual, RFC 0075 chunk embeddings), and its own citation count as a weak prior.
Reciprocal-rank fusion is the obvious combiner and the codebase already has one —
`services/search/fusion.rs`, built for hybrid retrieval in RFC 0076. Reuse it
rather than inventing a scoring formula.

R2.5 **The vault centroid** — the mean of its papers' chunk embeddings — is what
the semantic generator queries with, and what the textual half of R2.4 measures
against. This is the thing this feature has that a normal search does not: it
knows what the vault is *about* without anyone describing it.

R2.6 **Filter before ranking, hard**: anything already in the vault, already in
the library, or previously dismissed is removed before scoring. A suggestion you
already own is the fastest way to make the feature look broken.

R2.7 **Every suggestion states its evidence**, and the evidence differs by
generator: *"cited by 4 papers in this vault"*, *"cites 3 of the same works as
Vaswani 2017"*, *"closest to your work on sparse attention"*. The reason is not
decoration — it is how you decide without opening the paper, and a fused score
with no explanation is a number nobody can act on.

R2.8 Cap at **five**. A suggestion list you scroll is a search result.

### Provider reality

R2.9 OpenAlex is the only provider we use with a traversable citation graph, so
the structural generators are OpenAlex-only for now and the RFC says so rather
than implying coverage we lack. arXiv and Europe PMC contribute through the
semantic generator. A vault of papers with no OpenAlex ids degrades to R2.1's
third row alone — which is exactly the situation the fusion has to survive
gracefully.

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
| 1 | R3 schema + R2.5/R2.6 centroid, semantic generator, filtering | yes | M |
| 2 | R1.1–R1.5 inspector section, Add / Dismiss, manual run | no — wants 1 | M |
| 3 | **R2.2–R2.3 k-hop traversal** — wire the dead `openalex_lineage_filter` | yes | M |
| 4 | R2.4 RRF fusion over the three generators + R2.7 evidence lines | no — wants 1, 3 | M |
| 5 | R1.6–R1.7 weekly schedule | no — wants 2 | S |

## Risks

- **Suggestions are a background spend.** R1.6's one-vault-per-start rule and
  the existing research budget bound it, but this is the first feature that
  costs money without the user asking. It must be disable-able in Settings, and
  the RFC assumes it is.
- **A vault with three papers has no centroid worth the name.** R1.6's minimum
  is a floor, not a fix; below ~10 papers the suggestions will be broad. Say so
  in the empty state rather than shipping vague rows.
- **Citation-graph coverage is uneven.** R2.9 states it: OpenAlex only. A vault
  whose papers lack OpenAlex ids gets semantic suggestions and nothing else.
- **Hop 2 is a fan-out hazard.** R2.3's arrival-frequency gate is what keeps it
  finite; if the numbers in practice say otherwise, the answer is a smaller k,
  not a bigger budget.

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
2. Every suggestion says why *this vault*, in the terms of the generator that
   found it.
3. A suggestion reached from several of your papers outranks one reached from
   one, all else equal.
4. A dismissed paper never appears again.

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
