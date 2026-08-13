# RFC 0089: Quiz the reader on the paper they just read

Status: Proposed
Date: 2026-08-13
Product: i0i
Target: Tauri v2 + SvelteKit (Svelte 5), macOS first
Milestone: Release 0.0.1
Builds on: RFC 0075 (structural chunking + embeddings), RFC 0076 (hybrid
retrieval), RFC 0077 (ContextManager), RFC 0079 §5 (retrieval is the floor),
RFC 0064 (structured outputs).

## Summary

The product's stated goal is a learning IDE; today the app helps you *collect*
and *interrogate* papers and never asks whether you understood one. This RFC
adds a quiz: the app asks, you answer, it grades against the paper and shows you
the passage you missed.

The report flags this as needing a UI/UX pass. §1 is that pass — it is the
substance of this RFC, and the model plumbing (§2) is the easy half.

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

## 2. Generating and grading

R2.1 Questions are generated from the paper's **chunks** (RFC 0075), not from
its full text: five chunks sampled across the document's structure — never five
from the introduction — each carrying the section it came from.

R2.2 Generation uses structured outputs, the mechanism RFC 0064 established for
auto-highlighting: a typed `QuizQuestion { kind, prompt, options, answer,
source_chunk_id }`, so a malformed generation fails to parse rather than
rendering as a broken question.

R2.3 **A question whose source chunk is not verbatim in the document is
discarded**, the same integrity rule RFC 0059 applies to agent highlights. A
quiz that asks about something the paper does not say is worse than a short
quiz.

R2.4 Grading a free-text answer is one call with the question, the answer and
the source passage, returning `{ verdict: correct | partial | incorrect,
explanation }`. Multiple choice never calls a model.

R2.5 Both use the cheap model slot (`model.annotation`, DeepSeek flash per RFC
0079 R7.3) — this is extraction and comparison, not reasoning about a hard
question.

## 3. Storage

R3.1 A `quiz_sessions` table (`id`, `paper_id`, `created_at`, `score`) and a
`quiz_answers` table (`session_id`, `question_json`, `user_answer`, `verdict`),
with the same `on delete cascade` and index-the-foreign-key discipline RFC 0079
§6 had to retrofit. Do it right the first time here.

R3.2 Sessions are kept, not discarded: RFC 0092's weekly digest wants "what did
you actually retain this week," and a scored session is the only signal in the
app that answers it.

---

## Task list

| # | Task | Ships alone | Size |
|---|---|---|---|
| 1 | R3 schema + R2.1–R2.3 generation from chunks | yes | M |
| 2 | R1.3–R1.6 the quiz tab and session UI | no — wants 1 | L |
| 3 | R2.4 free-text grading | yes | S |
| 4 | R1.5 mark-what-I-missed | no — wants 2 | S |

## Risks

- **Generated questions can be trivial** ("What does the paper propose?"). The
  mitigation is R2.1's structural sampling plus a prompt that demands the answer
  be a specific claim in the passage — and R5-style spot-checking before this
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
- **B. Quiz a vault, not just a paper?** Falls out of R2.1 by widening the chunk
  scope to `vault_ids`, which RFC 0076's search request already supports.
  Recommendation: ship paper-scoped, then measure whether anyone asks.

## Success criteria

1. From an open paper, a quiz is two clicks away and five questions long.
2. Every question can be traced to a passage that verbatim exists in the paper.
3. Finishing a quiz leaves annotations on what you missed, not just a number.
