# RFC 0089: Quiz the reader on the paper they just read

Status: Proposed
Date: 2026-08-13
Product: i0i
Target: Tauri v2 + SvelteKit (Svelte 5), macOS first
Milestone: Release 0.0.1
Related: RFC 0092 (weekly digest reads §4's sessions), RFC 0093 (Study mode
hosts the quiz history).
Builds on: RFC 0075 (structural chunking + embeddings), RFC 0076 (hybrid
retrieval), RFC 0077 (ContextManager), RFC 0079 §5 (retrieval is the floor),
RFC 0064 (structured outputs).

## Summary

The product's stated goal is a learning IDE; today the app helps you *collect*
and *interrogate* papers and never asks whether you understood one. This RFC
adds a quiz: the app asks, you answer, it grades against the paper and shows you
the passage you missed.

Two things make it *this* app's quiz rather than a generic one, and both are
sections of this RFC rather than notes for later:

- **§1 — where it lives.** The UI/UX pass the report asked for.
- **§2 — what it asks about.** The pedagogy. The app already knows what you
  highlighted, what you noted and what you asked; a quiz that ignores that is a
  reading comprehension test, and a quiz that uses it is a study tool.

§4 keeps the results, which is what lets RFC 0092's weekly digest report
retention rather than only activity.

---

## 1. Where this lives

### The constraint

The shell offers three places a feature can attach, and the choice determines
what the feature can be:

| Attachment | Precedent | What it implies |
|---|---|---|
| A **rail section** in the reader inspector | Info / Notes / Chat (`ReaderInspector.svelte:1565`) | Paper-scoped, always beside the text, competing for a 340px column |
| A **tab kind** | `reader` / `vault` / `discover` (`domain/workspace.ts:5`) | Full workspace width, its own lifecycle, closable, one per subject |
| A **mode** in the activity rail | V / R / F (`+page.svelte:837`) | A place you *go*, not a thing you open |

### The decision

**A tab kind: `quiz`, with `paperId`.** Reasons, in order:

1. **A quiz needs the width.** A question, four options, your answer, the graded
   verdict and the cited passage do not fit a 340px inspector column beside a
   paper. The annotations panel is already crowded enough that RFC 0080 had to
   shrink a delete button to fit.
2. **A quiz is a session, not a view.** It has a start, a sequence, and a score.
   Tabs already model exactly that — they open, they persist while you work, you
   close them when done. A rail section would reset every time you switched to
   Notes and back.
3. **Closing it must not close the paper.** RFC 0084 made tabs one-per-paper and
   non-evicting, so `quiz:<paperId>` sits beside `reader:<paperId>` and you can
   flip between the question and the text. That is the core interaction and it
   only works if both are open at once.

A mode is wrong: quizzing is something you do *to a paper you have*, not a
region of the app you inhabit. It would be an empty room until you picked a
paper, which is the vault's job.

### The entry point

R1.1 A **Quiz** action in the reader toolbar (`ReaderToolbar.svelte`), beside
the existing AI auto-highlight, enabled only when the paper is in the library
and has chunks — the same `chatEnabled` + indexed condition the ask path uses
(RFC 0079 R5.4). Disabled with the reason, not hidden.

R1.2 A **Quiz** row in the vault's paper context menu (`PaperList.svelte:143`),
so you can quiz yourself without opening the paper — which is the honest test of
whether you remember it.

### The session

R1.3 A quiz is **five questions**, presented one at a time. Five because it is
short enough to finish in a break and long enough to cover a paper's sections;
one at a time because a list of five invites skimming for the easy one.

R1.4 Each question shows: the question, an answer area, and — only after you
answer — the verdict, a one-line explanation, and **the passage it came from**,
rendered as a quotation with a "Show me in the paper" link that opens
`reader:<paperId>` at that chunk and flashes it (the citation-jump path RFC 0077
already built).

R1.5 The end of the session shows the five questions, your verdict on each, and
a single action: **Mark the ones I missed**, which creates a note-only
annotation on each missed passage. A quiz that ends in a score is a test; a quiz
that ends in annotations is study. This is the feature's whole reason to exist.

### Question shape

R1.6 Two kinds, mixed:

- **Recall** — free text, graded by the model against the source passage.
- **Multiple choice** — four options, one correct, graded locally.

Free text is where learning happens; multiple choice is where you can be honest
about not knowing. A quiz of only free text is exhausting; only multiple choice
is guessable.

---

## 2. What the quiz is *about* — the pedagogy

A quiz generated from a paper's chunks tests the paper. A quiz worth taking
tests **your reading of it**, and the app already knows what that was: what you
highlighted, what you wrote in the margin, and what you asked. Those three are
the record of where your attention went and where it faltered, and they are the
whole reason this feature can be better than a generic comprehension test.

### The principle

**Ask about what the reader engaged with, and about what they engaged with
*wrongly*.** Three sources, in priority order:

R2.1 **Your marks are the syllabus.** A highlighted passage is a claim you
decided mattered. Questions drawn from highlighted and noted chunks come first,
because recall of what you chose to mark is the thing you actually wanted to
retain. RFC 0085 partitions Notes from Chat; both partitions feed this.

R2.2 **Your questions are the gaps.** A passage you *asked about* is one you did
not follow on the first pass. Chat threads anchored to a passage
(`passageChats`, `ReaderInspector.svelte:302`) mark exactly those, and a question
generated there is aimed at the place comprehension actually broke. This is the
highest-value source in the feature and the one a generic quiz cannot have.

R2.3 **What you skipped is the blind spot.** Sections of the paper with no
marks, no notes and no questions. One or two questions from unmarked territory
keep the quiz from only ever confirming what you already attended to — a quiz
built purely from your highlights would grade your highlighting, not your
understanding.

R2.4 The mix for a five-question quiz: **two from marks, two from asked
passages, one from unmarked territory** — degrading gracefully. A paper with no
annotations falls back entirely to R2.5's structural sampling, which is the
generic quiz, and that is the *floor* rather than the design.

R2.5 Structural sampling (the fallback, and the source of R2.3's questions):
chunks drawn across the document's structure (RFC 0075), never five from the
introduction, each carrying the section it came from.

### Feeding the answer back

R2.6 A question you get wrong on a passage you had already asked about is the
most useful signal the app can produce, and it goes back where it came from:
the missed-passage annotation (R1.5) attaches to the passage's existing thread
rather than creating a second one.

R2.7 Questions never quote your note back as the answer. A note is *your*
phrasing of the claim; grading against it would reward remembering what you
wrote rather than what the paper said. The source of truth is always the
passage.

## 3. Generating and grading

R3.1 Generation uses structured outputs, the mechanism RFC 0064 established for
auto-highlighting: a typed `QuizQuestion { kind, prompt, options, answer,
source_chunk_id }`, so a malformed generation fails to parse rather than
rendering as a broken question.

R3.2 **A question whose source chunk is not verbatim in the document is
discarded**, the same integrity rule RFC 0059 applies to agent highlights. A
quiz that asks about something the paper does not say is worse than a short
quiz.

R3.3 Grading a free-text answer is one call with the question, the answer and
the source passage, returning `{ verdict: correct | partial | incorrect,
explanation }`. Multiple choice never calls a model.

R3.4 Both use the cheap model slot (`model.annotation`, DeepSeek flash per RFC
0079 R7.3) — this is extraction and comparison, not reasoning about a hard
question.

## 4. Storage — and why the digest needs it

R4.1 Two tables, indexed on their foreign keys at creation rather than
retrofitted (the mistake RFC 0079 §6 had to pay for):

| Table | Columns |
|---|---|
| `quiz_sessions` | `id`, `paper_id`, `created_at`, `question_count`, `correct_count` |
| `quiz_answers` | `id`, `session_id`, `question_json`, `source_chunk_id`, `source` (`mark` / `ask` / `unmarked`), `user_answer`, `verdict`, `created_at` |

`on delete cascade` from `papers`, and an index on `quiz_answers(session_id)`
and `quiz_sessions(paper_id, created_at)`.

R4.2 **`source` is the column that makes the history worth keeping.** It records
which of §2's three lanes each question came from, so "you got the passages you
asked about wrong twice this month" is a query rather than a guess.

R4.3 **Sessions are the retention signal the rest of the product lacks.**
RFC 0092's weekly digest asks "what did you learn this week" and can otherwise
only answer with activity — what you read, marked and asked. A scored session is
the only record in the app of whether any of it stuck. The digest reads
`quiz_sessions` for the week by `created_at` (RFC 0092 R1.9) and, because of
R4.2, can say *what kind* of thing you missed.

R4.4 Sessions are never silently deleted. Deleting a paper cascades them, which
is correct; nothing else removes them.

---

## Task list

| # | Task | Ships alone | Size |
|---|---|---|---|
| 1 | R4 schema + R2.5/R3.1–R3.2 structural generation (the fallback quiz) | yes | M |
| 2 | R1.3–R1.6 the quiz tab and session UI | no — wants 1 | L |
| 3 | R3.3 free-text grading | yes | S |
| 4 | **R2.1–R2.4 mark-, ask- and gap-driven question selection** | no — wants 1 | M |
| 5 | R1.5 + R2.6 mark-what-I-missed, onto the existing thread | no — wants 2, 4 | S |

Task 4 is the feature. Tasks 1–3 build a competent generic quiz; task 4 is what
makes it *this* app's quiz, and it should not be dropped to make a release.

## Risks

- **Generated questions can be trivial** ("What does the paper propose?"). The
  mitigation is §2's sourcing plus a prompt that demands the answer be a
  specific claim in the passage — and R5-style spot-checking before this
  ships to a release.
- **Grading free text is a judgement call the model will sometimes get wrong.**
  Showing the source passage with every verdict (R1.4) means the user can
  overrule it; the verdict is never the last word on screen.
- **A new tab kind touches the workspace switch** in `+page.svelte`. Small, but
  it is the third `kind` and the `{#if}` chain there is already long.

## Open Decisions

- **A. Should a quiz be re-takeable, and should it repeat questions?** Spaced
  repetition is the obvious next step and a much larger feature (scheduling,
  cards, a review queue). Recommendation: 0.0.1 generates a fresh quiz each
  time and stores the sessions; the schema in R3 is deliberately shaped so a
  scheduler could later read it, but this RFC does not build one.
- **B. Quiz a vault, not just a paper?** Falls out of R2.5 by widening the chunk
  scope to `vault_ids`, which RFC 0076's search request already supports.
  Recommendation: ship paper-scoped, then measure whether anyone asks.

## Success criteria

1. From an open paper, a quiz is two clicks away and five questions long.
2. Every question can be traced to a passage that verbatim exists in the paper.
3. On an annotated paper, most questions come from what the reader marked or
   asked about — not from a uniform sample of the text.
4. Finishing a quiz leaves annotations on what you missed, not just a number.
5. A week's sessions are queryable by date and by question source, so RFC 0092
   can report retention rather than only activity.
