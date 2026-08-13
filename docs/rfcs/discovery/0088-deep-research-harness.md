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
copy."* This RFC does the survey and answers **copy, do not adopt** — then names
the four techniques worth copying and the one architectural claim we should not.

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

Read from source, not from marketing:

- **[langchain-ai/open_deep_research](https://github.com/langchain-ai/open_deep_research)** —
  the reference open implementation. Its `deep_researcher.py` runs four nodes:
  `clarify_with_user` → `write_research_brief` → `research_supervisor` →
  `final_report_generation`. The supervisor holds exactly three tools:
  `think_tool` (reflection with no external call), `ConductResearch` (delegate a
  sub-topic to a researcher subgraph), and `ResearchComplete`. Researchers
  terminate on `max_react_tool_calls`, an explicit `ResearchComplete`, or a turn
  with no tool calls, then run a **compression** node that synthesises findings
  and, on token overflow, drops older messages via `remove_up_to_last_ai_message`.
  Concurrency is capped by `max_concurrent_research_units`.
- **Reflection as a first-class tool.** `think_tool` exists so the model can
  reason about *whether it has enough*, separately from acting. This is the
  single most-copied idea across current harnesses.
- **Clarify before planning.** The first node's whole job is deciding whether
  the request is answerable as stated.
- **Evaluation.** open_deep_research reports against Deep Research Bench (100
  PhD-level tasks, LLM-as-judge). The field treats "did the harness get better"
  as a measured question.

## The architectural claim we should *not* copy

These harnesses are LangGraph message-graph agents: state is a message list, and
control flow is what the model emits. RFC 0037 deliberately chose the opposite,
and the reasons still hold — Rust has no LangGraph, our primitives are typed and
unit-tested, and a message-list agent is exactly the thing whose cost RFC 0079
§5 caught (*"the round trip spent deciding whether to run it costs hundreds"*).

So: **keep the typed loop, borrow the techniques.**

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
tell "done" from "out of budget."

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
| 2 | R1 reflection primitive + `Converged` stop reason | yes | M |
| 3 | R4 Obscura `browse` primitive | yes | M |
| 4 | R2 clarify step | yes | S |
| 5 | R3 bounded sub-topic delegation | no — wants R1 | L |

## Risks

- **Reflection is another LLM call per round.** It replaces the refine call
  rather than adding to it (R1.2), so the round cost is roughly flat; the RFC is
  wrong if measurement says otherwise, which is why R5 lands first.
- **Browsing is an unbounded surface.** Arbitrary pages, arbitrary size,
  arbitrary latency. R4.3's budget and the existing `READER_MAX_PDF_BYTES`-style
  caps are the containment.
- **Delegation is the biggest change and the least certain payoff.** It is last
  in the task list for that reason, and it may not ship for 0.0.1.

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

## Sources

- [langchain-ai/open_deep_research](https://github.com/langchain-ai/open_deep_research) —
  supervisor/researcher structure, `think_tool` / `ConductResearch` /
  `ResearchComplete`, compression, iteration and concurrency caps.
- [Universal Deep Research: Bring Your Own Model and Strategy](https://arxiv.org/pdf/2509.00244) —
  strategy/harness separation.
- [Inside the Scaffold: A Source-Code Taxonomy of Coding Agent Architectures](https://arxiv.org/pdf/2604.03515) —
  control loop / tool definition / state management taxonomy.
