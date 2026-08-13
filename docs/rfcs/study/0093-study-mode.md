# RFC 0093: What Study mode is for

Status: Proposed
Date: 2026-08-13
Product: i0i
Target: Tauri v2 + SvelteKit (Svelte 5), macOS first
Milestone: Release 0.0.1
Related: RFC 0086 §3 (the rail keeps STUDY and drops GRAPH/ASK), RFC 0089
(quiz), RFC 0092 (weekly digest).

## Summary

`ActivityRail.svelte:16` has had a **STUDY** button since the shell was built
(RFC 0001). Clicking it does nothing — `handleModeSelect` (`+page.svelte:837`)
handles `V`, `R`, `F` and no others. RFC 0086 §3 removes GRAPH and ASK for
exactly that reason and keeps STUDY only because this RFC exists.

So this RFC has to answer the question RFC 0086 declined to: **what is Study,
such that it deserves a mode when Graph and Ask did not?**

Today the honest answer is thin — the weekly digest is the only thing that would
live there. This document says what the mode is *for*, what belongs in it, and
what the smallest honest version is, so that STUDY stops being a button that
lies without becoming a room with one chair.

---

## 1. The claim

The other three modes are organised by **what you are handling**:

| Mode | Subject |
|---|---|
| VAULT | your collection |
| FIND | papers you do not have |
| READ | one document |

Study is the only mode whose subject is **you**: what you have understood, what
you have not, and what you did about it. That is a different axis, not a fourth
noun, and it is why the digest and the quiz do not fit anywhere else — RFC 0092
§1 had to invent a home for the digest precisely because no existing surface is
about the reader rather than the material.

**Study is where the app reports on the reader.** That is the whole membership
test. It is also why GRAPH failed the test (a graph is a view of the collection —
it belongs in VAULT) and why ASK failed it (asking is something you do to a
paper or a vault, from the surface where that thing already is — RFC 0087 puts
it in the vault inspector).

## 2. What is in it

Three things, of which one exists as an RFC, one is a natural neighbour, and one
is deliberately deferred.

R2.1 **The weekly digest** (RFC 0092) — the default landing surface. What you
read, marked and asked this week, with a written summary under the evidence.

R2.2 **Quiz history** (RFC 0089 §4) — sessions with their scores and, because
RFC 0089 R4.2 records where each question came from, *what kind* of thing you
tend to miss. This is the difference between "you took three quizzes" and "you
keep getting the passages you asked about wrong."

R2.3 **A start-a-quiz surface.** RFC 0089 puts entry points on the paper (the
reader toolbar) and on the vault list. Study is where you'd start one *without*
a paper in mind — "quiz me on something I read this month" — which is the mode's
own contribution rather than a copy of an entry point elsewhere.

### Explicitly not in it, yet

- **Spaced repetition / a review queue.** The obvious destination for this mode
  and much larger than everything above combined: scheduling, card state, a
  daily queue, and a retention model. RFC 0089 Open Decision A defers it and
  shapes the quiz schema so a scheduler could later read it. Study mode is where
  it would land; this RFC does not build it.
- **Reading statistics.** Time-on-paper, pages read, streaks. We do not record
  any of it (RFC 0092 R2.4), and adding tracking is a different RFC with a
  privacy discussion attached. A statistics page built from the timestamps we
  *do* have would be three numbers and a lie of implied precision.
- **Goals.** "Read two papers a week." A commitment device is a product opinion
  we have not formed.

## 3. Shape

R3.1 Study is a **mode that opens a tab**, not a mode that owns a full-screen
region: pressing `S` (RFC 0086 R3.3) opens or activates `digest:<isoWeek>`. This
keeps it inside the workspace model everything else uses, and means a digest sits
beside the paper you were reading rather than replacing it.

R3.2 The digest tab carries the mode's navigation in its own header — week
paging (RFC 0092 R1.4) and a link to quiz history — rather than the activity
rail growing sub-items. The rail selects a mode; the workspace holds the
surfaces.

R3.3 With no history at all — a fresh install, no annotations, no quizzes —
Study shows one line explaining what will appear here and how it gets there
("mark a passage, ask a question, take a quiz"). An empty mode should teach the
loop that fills it.

## 4. The smallest honest version

R4.1 If only one thing ships for 0.0.1, it is **R2.1 plus R3.3**: the digest,
and an empty state that does not pretend. That is enough to make the STUDY
button truthful, which is RFC 0086's bar.

R4.2 If the quiz (RFC 0089) does not make the release, R2.2 and R2.3 are
omitted rather than stubbed — no disabled tabs, no "coming soon". RFC 0086's
rule applies to this mode's own contents as much as to the rail.

---

## Task list

| # | Task | Ships alone | Size |
|---|---|---|---|
| 1 | R3.1–R3.3 mode wiring, `S` shortcut, empty state | no — wants RFC 0092 task 1 | S |
| 2 | R2.1 digest as the landing surface | = RFC 0092 | — |
| 3 | R2.2 quiz history view | no — wants RFC 0089 | M |
| 4 | R2.3 start-a-quiz-from-Study | no — wants RFC 0089 | S |

## Risks

- **A mode with one surface is a button with extra steps.** True today, and R4.1
  accepts it: the alternative is deleting STUDY along with GRAPH and ASK, and
  then re-adding a mode when the quiz lands. Keeping it is a bet that R2.2 and
  R2.3 arrive; if the quiz slips past 0.0.1, that bet should be revisited rather
  than defended.
- **"Study" invites feature creep** — every learning idea will want to live
  here. §1's membership test is the defence: does this surface report on the
  *reader*? A graph does not. A reading-list does not.

## Open Decisions

- **A. Should Study be per-vault or global?** The digest is global (your week,
  across everything); quiz history could sensibly be filtered by vault.
  Recommendation: global by default with a vault filter in the header, since the
  reader's week does not respect vault boundaries.
- **B. Does Study survive if the quiz does not ship?** Recommendation: yes for
  0.0.1 under R4.1, and reconsider at 0.0.2 if it is still one surface.

## Success criteria

1. Pressing `S`, or clicking STUDY, opens something real.
2. Everything in the mode reports on the reader, not on the collection.
3. Nothing in the mode is a stub, a placeholder, or a disabled tab.
