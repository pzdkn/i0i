# RFC 0092: The week you actually had

Status: Proposed
Date: 2026-08-13
Product: i0i
Target: Tauri v2 + SvelteKit (Svelte 5), macOS first
Milestone: Release 0.0.1
Builds on: RFC 0062 (annotations), RFC 0067 (marks vs chats), RFC 0075
(chunking), RFC 0089 (quiz sessions), RFC 0079 R7 (cheap model slots).

## Summary

*"Summarize what you have studied and learned this week."* The report flags the
UI as unresolved; §1 is that pass.

The app already records everything this needs and shows none of it back. Every
table carries a `created_at` written by SQLite's `datetime('now')`
(`library_store.rs:2047` and throughout) — `YYYY-MM-DD HH:MM:SS` in UTC, which
sorts lexicographically, so a week is a `where created_at >= ?` and nothing more
clever. That is worth stating up front because it is the difference between this
feature being a query and being a migration.

---

## 1. Where this lives

### The decision

**A tab kind: `digest`, opened from the activity rail's Study mode.**

R1.1 The digest is a **document you read**, not a panel you glance at. It has
prose, sections, a list of papers, and quotations from your own notes. That is a
full-width surface, and the tab model already gives it one — the same argument
RFC 0089 §1 makes for the quiz, and the same reason a rail section is wrong.

R1.2 It is *not* per-paper and not per-vault, so it cannot hang off the reader
inspector or the vault inspector — the two places RFC 0087 and RFC 0091 use.
Its subject is **you, over a week**, which is a scope nothing in the shell
currently addresses. That is what justifies a new attachment rather than reusing
one.

R1.3 Entry point: a **Study** entry in the `ActivityRail` (`+page.svelte:837`
handles V/R/F; this adds one), opening `digest:<isoWeek>`. It is also where
RFC 0089's quiz history belongs, which is why both RFCs live under
`docs/rfcs/study/` — this is one product area, arriving in two pieces.

R1.4 Weeks are navigable: `‹ this week ›`. A diary you cannot page back through
is a dashboard.

### What it contains

Ordered by how much it tells you, not by how easy it is to compute:

R1.5 **What you read.** Papers with any activity this week — an annotation, a
chat turn, a status change — with time-on-paper unavailable (we do not track it)
and therefore not faked. Each row: title, what happened (`4 marks, 1
conversation`), and a link that opens it.

R1.6 **What you marked.** Your highlights and notes from the week, grouped by
paper, quoted verbatim. This is the part that makes the digest feel like a diary
rather than a report: it is your own words, back in front of you.

R1.7 **What you asked.** Questions from the week's threads, listed as questions.
The answers are one click away in the reader; the digest shows what you *wanted
to know*, which is a better record of a week's thinking than what the model said.

R1.8 **What you learned — a written summary.** One model call over R1.5–R1.7,
producing three to five sentences in the second person: what you were working
on, what changed, what you left open. This is the only generated prose in the
digest and it comes last, under the evidence, never above it.

R1.9 **What you retained.** Quiz sessions from the week with their scores
(RFC 0089 R3.2). Absent until quizzes exist; the section is omitted, not empty.

R1.10 An empty week says so plainly and does not generate prose about nothing.

---

## 2. Computing it

R2.1 One query per section, all of the form
`where created_at >= :week_start and created_at < :week_end`, against
`highlights`, `chat_entries`, `papers`, and `quiz_sessions`. Weeks start Monday
00:00 **local** time, converted to UTC for the bound — the user's week is a
human week, not a UTC one.

R2.2 The summary (R1.8) is one call on the cheap model slot
(`model.annotation`, RFC 0079 R7.3) over a compact rendering of R1.5–R1.7. It
never sees paper full text: the digest summarises *your activity*, and feeding
it the papers would produce a literature review instead of a diary.

R2.3 The summary is **cached per week** once generated, and regenerated only on
request. A week that has ended cannot change; the current week's digest offers a
Refresh.

R2.4 No new activity tracking. If it is not already in a table with a
`created_at`, this RFC does not invent it — that is what keeps this feature a
query. Reading time, scroll depth and session length are all things we do not
record, and adding them is a different RFC with a privacy discussion attached.

---

## Task list

| # | Task | Ships alone | Size |
|---|---|---|---|
| 1 | R2.1 week queries (pure, testable against a seeded store) | yes | S |
| 2 | R1.3–R1.7 the digest tab, evidence sections only, no prose | no — wants 1 | M |
| 3 | R1.8 + R2.2–R2.3 generated summary with caching | no — wants 2 | S |
| 4 | R1.9 quiz section | no — wants RFC 0089 | XS |

Task 2 is deliberately shippable **without** the model: a digest that only shows
what you did is already useful, and it is the half that cannot be wrong.

## Risks

- **A digest of a quiet week is a reproach.** The tone of R1.8 matters more than
  its accuracy: it describes, it does not encourage or grade. An app that tells
  you off for a slow week is one you stop opening.
- **`created_at` is written by SQLite, not by the app**, so a machine with a
  wrong clock produces a wrong week. Acceptable — the alternative is an app
  clock that disagrees with the database's own defaults.
- **Second-person generated prose is the highest-risk text in the product.** It
  is about the user, and it will occasionally be wrong about them. R1.8 places
  it under the evidence for exactly that reason: the facts are checkable above
  it.

## Open Decisions

- **A. Should the digest be pushed (a notification, a badge on Monday) or
  pulled?** Recommendation: pulled for 0.0.1. A weekly notification is a product
  commitment about attention, and it should not be made by default in a first
  release.
- **B. Does "Study" mode hold anything else?** It is the natural home for the
  quiz history, spaced repetition if it ever lands, and reading stats. This RFC
  claims the mode and fills one tab in it; RFC 0089 fills the other.

## Success criteria

1. Opening Study shows what you read, marked and asked this week, drawn from
   real timestamps, with every item linking back to the thing it describes.
2. The summary sits under the evidence and can be regenerated.
3. No new tracking table, and no fabricated metric.
