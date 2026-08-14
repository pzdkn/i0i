# RFC 0088: Borrow the parts of an established deep-research harness that we lack

Status: Proposed
Date: 2026-08-13
Product: i0i
Target: Tauri v2 + SvelteKit (Svelte 5), macOS first
Milestone: Release 0.0.1
Builds on: RFC 0037 (deep-research scout agent), RFC 0043 (multi-provider),
RFC 0054 (search relevance), RFC 0057 (semantic ranking), RFC 0041/0052
(Obscura-backed acquisition), RFC 0078 (agentic context).

## Summary

The report asks to *"find in github what other deep research agents use as
scaffolding, what are their key techniques and features, either use them or
copy."* This RFC surveys **five** of them — 80k, 29k, 19.5k, 5.2k stars and LangChain's
reference implementation — and answers **copy the techniques, do not adopt the
architecture**. §0 is the reflection: what we steal whole, what we copy and
reshape, what we can do *better* than a web harness because we hold a paper
library, and what we refuse because it only makes sense for a report generator.

The filter throughout is that **our goal is improved paper search — Perplexity
over your library, not a literature review**. Four of the five end in a written
report. We end in a ranked set of papers.

### What we have

`services/research/agent.rs:60` runs a bounded loop over typed primitives:

```
plan_queries → search (multi-provider) → dedup → rank (embedding rerank) → refine_queries → …
```

with `max_iterations`, `max_provider_queries`, and a `BudgetUsage` counter,
cancellation, and progress events. RFC 0037's premise — **Rust orchestrates,
the LLM is called inside primitives and only parameterises them** — is why this
is legible and testable (`agent.rs` has a full fake-planner test suite at :250+).

### What the field does

Five harnesses, read from source or from their own docs rather than from
coverage of them. Stars are a proxy for how much scrutiny each design has had,
not for quality.

| Harness | Scale | Shape | Terminates on | Produces |
|---|---|---|---|---|
| [bytedance/deer-flow](https://github.com/bytedance/deer-flow) | 80k★, Python | Lead agent spawning **dynamic** sub-agents with scoped contexts and their own termination conditions | Per-agent conditions; goal tracking | Report / artifacts |
| [assafelovic/gpt-researcher](https://github.com/assafelovic/gpt-researcher) | 29k★, Python | **Planner / executor** split; planner writes sub-questions, crawler agents gather, each source summarised then aggregated | Fixed tree depth × breadth | Report over 20+ sources |
| [dzhng/deep-research](https://github.com/dzhng/deep-research) | 19.5k★, TS | **Recursive tree**: generate SERP queries, extract `learnings` + `followUpQuestions`, recurse | `depth` hits 0 | Report from accumulated learnings |
| [langchain-ai/open_deep_research](https://github.com/langchain-ai/open_deep_research) | LangChain's reference | `clarify_with_user` → `write_research_brief` → **supervisor** → report; supervisor holds `think_tool`, `ConductResearch`, `ResearchComplete` | Tool-call caps, explicit `ResearchComplete`, or a turn with no tool calls | Report |
| [jina-ai/node-DeepResearch](https://github.com/jina-ai/node-DeepResearch) | 5.2k★, TS | Flat loop over four actions — **search / read / reason / reflect** — with knowledge accumulation | **Token budget**, then "Beast Mode" forces an answer from what it has | A cited *answer*, explicitly not a report |

Four patterns recur across all five, and each has a name here:

- **Query fan-out before retrieval.** Every one of them turns the goal into
  several distinct queries rather than one. gpt-researcher calls it planning,
  dzhng calls it `generateSerpQueries`, open_deep_research calls it a research
  brief. We do this (`plan_queries`).
- **Gap-driven iteration.** The second round is aimed at what the first round
  missed: `followUpQuestions` (dzhng), `think_tool` (open_deep_research),
  `reflect` (jina). We do a weak version — gaps are computed and passed to
  `refine_queries` as strings.
- **A hard budget, and a graceful end at it.** jina's is the sharpest: a token
  budget, and when it is spent, "Beast Mode" stops searching and answers from
  accumulated knowledge rather than failing. We have budgets and stop, but with
  a `StopReason` that cannot distinguish success from exhaustion.
- **Read the page, not just the index.** All five fetch and extract page
  content. We do not — we only query metadata APIs.

And two structural choices they disagree about, which is itself informative:

- **Recursion schedule.** dzhng *halves breadth at each depth*
  (`Math.ceil(breadth / 2)`) — a deliberate narrowing as the tree deepens.
  gpt-researcher uses a fixed depth × breadth. Ours is flat: `max_iterations`
  with the same query count each round.
- **Delegation.** open_deep_research and deer-flow both delegate to sub-agents,
  but deer-flow's own guidance is the cautious one: *"sub-agents are an
  optimization, not the default response to a complex request"*, and the lead
  *"uses the fewest useful sub-agents"*. The 80k-star project argues against
  reaching for delegation first.

### The filter: we are building search, not a report

**Our goal is improved paper search — closer to Perplexity than to a report
generator.** Four of these five end in a written report; jina's ends in a cited
answer. We end in **a ranked set of papers the user chooses from**. That
difference decides what is worth copying:

- Their **synthesis** stages — summarise each source, compress findings, write
  sections — are the majority of their token spend and are *irrelevant to us*.
  We do not write prose about the papers; we rank them and hand them over.
- Their **retrieval** stages — fan-out, gap-driven iteration, reading pages,
  budget discipline — are exactly our problem, and are where they are ahead of
  us.
- We have two things none of them have: a **citation graph** with real paper
  identity (DOI/OpenAlex id, so dedup is exact rather than URL-based) and a
  **local embedding reranker** (RFC 0057). A web harness cannot rank by
  semantic proximity to your library; we can.

## 0. What we steal, copy, improve, and refuse

Four verbs, used precisely. **Steal** = take the idea whole, it is right and we
have nothing like it. **Copy** = take the mechanism but reshape it to a typed
Rust primitive. **Improve** = they do it, we can do it better because of what a
paper library gives us. **Refuse** = it is load-bearing for a report generator
and dead weight for a search engine.

### Steal

| From | Idea | Why it is right for us |
|---|---|---|
| jina | **Budget-forcing with a graceful end.** When the budget is spent, stop searching and return the best you have, deliberately, rather than treating exhaustion as failure. | We already accumulate a ranked pool, so our version is nearly free: exhaustion returns the pool and *says* it was exhausted. Today `StopReason::MaxIterations` cannot distinguish that from convergence. → §1 R1.3 |
| jina | **Disable an action that stops paying.** jina disables actions that yield nothing new. | We already compute `new_count(pool, existing)` (`agent.rs:199`) and throw it away. A provider that returns zero new candidates two rounds running should stop being queried this run. → **new R1.4** |
| dzhng | **Narrow as you deepen** — `Math.ceil(breadth / 2)` per level. | Our loop issues the same number of queries every round, so round 4 costs what round 1 did while returning far less. A decaying query budget is three lines and directly reduces spend. → **new R1.5** |

### Copy

| From | Mechanism | Our shape |
|---|---|---|
| open_deep_research | `think_tool` — reflection as a first-class step, separate from acting | A `reflect` **primitive** returning typed `Reflection { gaps, should_continue, next_queries }`, not a tool the model may skip. Rust decides; the model only fills the struct. → §1 |
| all five | Fetch and read pages, not just query indexes | A `browse` primitive over the existing `html_ingestion` pipeline, tightly budgeted. → §4 |
| open_deep_research | `clarify_with_user` before spending | One pre-round call, default **off**. → §2, Open Decision A |
| deer-flow / open_deep_research | Sub-agent delegation | Bounded fan-out over sub-topics with the budget split up front — and demoted, on deer-flow's own advice. → §3 |

### Improve

Three places where a paper library beats a web harness, and we should not
imitate their workarounds:

1. **Identity.** They dedup by URL and by fuzzy title. We have DOIs and
   OpenAlex ids; `dedup` is exact. Their duplicate-suppression heuristics are a
   symptom of a problem we do not have.
2. **Ranking.** They rank by what the model says. We rank by embedding
   proximity (RFC 0057) against a local reranker — and, per RFC 0091, could rank
   against the *vault centroid*, which is a signal no general harness can
   compute. Perplexity-style search over your own library is exactly this.
3. **Coverage as the stop condition.** Their loops ask *"can I answer yet?"*
   Ours should ask *"is the candidate set complete?"* — a different question with
   a measurable answer (§5's recall fixtures), and the honest goal for search.

### Refuse

- **Report and section generation** (gpt-researcher, dzhng, deer-flow,
  open_deep_research). We return papers. A generated literature review would be
  the most expensive and least trustworthy thing in the product.
- **Per-source summarisation** (gpt-researcher summarises every scraped source).
  Our unit is the paper, and the user reads it in the reader we already built.
- **Context compression** (open_deep_research's `compress_research`,
  `remove_up_to_last_ai_message`). That exists because a message-list agent
  overflows its window. Our loop holds a typed `Vec<PaperCandidate>`, which does
  not grow like a transcript.
- **Dynamic agent spawning** (deer-flow). The most-starred design here, and the
  furthest from RFC 0037's premise. Even deer-flow says sub-agents are an
  optimisation, not a default.

### What this changes about the plan

The survey moved two things. **Budget shaping (R1.4, R1.5) is new and is the
cheapest win in the RFC** — it comes from the two harnesses closest to our
actual problem, needs no new model call, and reduces cost rather than adding it.
And **delegation (§3) drops further down**: three of the five either avoid it or
warn against reaching for it first.

---

## 1. A reflection step, spent on the gap and not on the plan

R1.1 After each round's rank, one `reflect` primitive answers three questions
against the current pool: *what does the goal ask for that this pool does not
cover; is the marginal round likely to add anything; what queries would close
the gap.* It returns a typed `Reflection { gaps, should_continue, next_queries }`
— the same shape as `think_tool`'s intent, but as a primitive rather than a tool
the model may or may not call.

R1.2 `refine_queries` (`planner.rs`) folds into R1.1. Today gaps are computed
and passed to refinement as strings (`agent.rs:94`); reflection makes the
decision to continue an explicit, logged output rather than a budget side
effect.

R1.3 `StopReason` gains `Converged` — the run stopped because reflection said
the pool answers the goal — distinct from today's `MaxIterations`, which cannot
tell "done" from "out of budget." Exhaustion is never a failure: like jina's
Beast Mode, a spent budget returns the ranked pool and labels it *exhausted*, so
the UI can say "here is what I found, and I stopped early" rather than implying
completeness.

R1.4 **Retire a provider that has stopped paying.** `new_count(pool, existing)`
(`agent.rs:199`) already measures how many candidates a round actually added and
the result is discarded. A provider returning zero new candidates for two
consecutive rounds is skipped for the rest of the run. Stolen from jina's
action-disabling; costs one counter per provider.

R1.5 **Narrow as the run deepens.** Query count per round decays —
`ceil(previous / 2)`, floored at one — instead of issuing the same fan-out every
round. dzhng halves breadth at each level of recursion for the same reason: the
later rounds are refinements, and paying round-one prices for them is how a
bounded loop still gets expensive.

## 2. Clarify before spending the budget

R2.1 Before round 0, one call decides whether the goal is answerable as written.
If not, the run stops immediately with up to three questions and **no provider
queries spent**. Discover already has a target editor
(`DiscoverTargetEditor.svelte`) that can host the answer.

R2.2 Clarification is skippable and remembered per search window: a user who has
answered once, or who dismisses it, proceeds. A harness that interrogates you
every time is worse than one that guesses.

## 3. Sub-topic delegation, bounded

R3.1 When the brief decomposes into independent sub-topics, the loop runs one
**bounded** researcher per sub-topic and merges their pools through the existing
`dedup`. This is `ConductResearch`, minus the graph: a `Vec<SubTopic>` and a
`join_all` with a concurrency cap, in the shape `commands/discovery/orchestrator.rs`
already uses for provider fan-out (RFC 0044).

R3.2 The per-turn budget is split across sub-topics up front, so delegation
cannot multiply cost. A harness whose parallelism is unbounded is how a $0.30
search becomes $6.

## 4. Obscura browsing as a research primitive

R4.1 Today Obscura is an *acquisition* backend (RFC 0041/0052): it fetches a PDF
we already decided to want. Deep research needs the other direction — reading a
landing page, a lab's publication list, a survey's reference section — which is
how these harnesses find what a metadata API does not index.

R4.2 A new `browse` primitive alongside `search`: given a URL, return extracted
readable text through the existing `html_ingestion` pipeline (RFC 0056), which
already sanitises and extracts. Candidates discovered by browsing enter the same
pool and the same dedup as API candidates, tagged with their provenance.

R4.3 Browsing is budgeted separately and more tightly than search — it is the
slowest primitive and the easiest to loop on. Reflection (R1) is what decides a
page is worth fetching.

## 5. Say whether it got better

R5.1 A fixture set of ~10 real goals with hand-marked "should find" papers,
run offline against recorded provider responses, reporting recall@target and
provider calls spent. Not Deep Research Bench — we are not writing reports, we
are finding papers — but the same principle: the harness change is a measured
claim, not a vibe.

R5.2 Every change in §1–§4 reports its before/after on that set. RFC 0079 §6
established this habit for performance; this extends it to quality.

---

## Task list

| # | Task | Ships alone | Size |
|---|---|---|---|
| 1 | R5 evaluation fixtures — **first**, so the rest can be judged | yes | M |
| 2 | **R1.4 + R1.5 budget shaping** — decaying fan-out, retire dead providers | yes | S |
| 3 | R1.1–R1.3 reflection primitive + `Converged` / exhausted stop reasons | yes | M |
| 4 | R4 Obscura `browse` primitive | yes | M |
| 5 | R2 clarify step (default off) | yes | S |
| 6 | R3 bounded sub-topic delegation | no — wants R1 | L |

Task 2 moved ahead of reflection after the survey: it is the smallest change,
it needs no model call, and it *reduces* spend, so it makes every later
measurement cheaper to run.

## Risks

- **Reflection is another LLM call per round.** It replaces the refine call
  rather than adding to it (R1.2), so the round cost is roughly flat; the RFC is
  wrong if measurement says otherwise, which is why R5 lands first.
- **Browsing is an unbounded surface.** Arbitrary pages, arbitrary size,
  arbitrary latency. R4.3's budget and the existing `READER_MAX_PDF_BYTES`-style
  caps are the containment.
- **Delegation is the biggest change and the least certain payoff.** It is last
  in the task list for that reason, and it may not ship for 0.0.1. deer-flow —
  the largest project surveyed and one that *does* delegate — says sub-agents
  are an optimisation rather than a default, which is the outside view agreeing.
- **A decaying fan-out (R1.5) can under-search a broad goal.** It is a schedule,
  not a law: the decay floor and the starting breadth are both settings, and R5
  is what says whether the schedule costs recall.

## Open Decisions

- **A. Does clarification belong in a research run at all?** For a
  paper-discovery tool the goal is usually a topic, not an ambiguous request.
  Recommendation: implement it as R2 describes but default it **off** behind a
  setting until the evaluation set shows it changes recall.

## Success criteria

1. A run that has found what the goal asked for stops and says so, rather than
   exhausting `max_iterations`.
2. Deep research can read a web page it was not handed, and papers found that
   way are indistinguishable downstream from API-found ones.
3. Every claim that the harness improved is backed by a number from R5.
4. A run that spends its budget returns its ranked pool and says it stopped
   early — exhaustion is reported, never silently dressed as completeness.
5. Cost per run falls, not rises: the decaying fan-out and provider retirement
   land before anything that adds a model call.

## Sources

Harnesses, read from source or from their own documentation:

- [bytedance/deer-flow](https://github.com/bytedance/deer-flow) (80k★) — lead
  agent with dynamic sub-agent spawning, scoped contexts, and the warning that
  sub-agents are an optimisation rather than a default.
- [assafelovic/gpt-researcher](https://github.com/assafelovic/gpt-researcher)
  (29k★) — planner/executor split, 20+ sources per run, tree-like depth ×
  breadth exploration; reports ~5 minutes and ~$0.40 per deep research run,
  which is a useful yardstick for our own budget defaults.
- [dzhng/deep-research](https://github.com/dzhng/deep-research) (19.5k★) —
  `deepResearch()` recursion over `breadth`/`depth`, `generateSerpQueries`,
  `processSerpResult` returning learnings + follow-up questions, breadth halved
  per level, `pLimit` concurrency.
- [jina-ai/node-DeepResearch](https://github.com/jina-ai/node-DeepResearch)
  (5.2k★) — search / read / reason / reflect loop, token-budget termination with
  Beast Mode, action disabling, definitive-answer test, citations preferred.
- [langchain-ai/open_deep_research](https://github.com/langchain-ai/open_deep_research) —
  supervisor/researcher structure, `think_tool` / `ConductResearch` /
  `ResearchComplete`, compression, iteration and concurrency caps.
- [Universal Deep Research: Bring Your Own Model and Strategy](https://arxiv.org/pdf/2509.00244) —
  strategy/harness separation.
- [Inside the Scaffold: A Source-Code Taxonomy of Coding Agent Architectures](https://arxiv.org/pdf/2604.03515) —
  control loop / tool definition / state management taxonomy.
