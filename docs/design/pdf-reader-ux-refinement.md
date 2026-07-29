# PDF Reader UX — Refinement Pass (Amendment 1)

Status: Draft (amendment to `pdf-reader-ux.md` — carves refinement RFCs)
Date: 2026-07-29
Product: i0i
Scope: The roadmap (RFC 0060–0064) shipped and builds are green, but first real
use exposed **clarity and correctness gaps**. This amendment records what a user
actually experiences, fixes the model where it confused them, and defines the
refinement RFCs. It amends — does not replace — the north star.

## Why this pass

The north star was declared "reached" on green builds. Green ≠ usable. Using the
reader on a real PDF surfaced six problems (all user-reported or log-confirmed):

1. **Auto-highlight produces nothing on PDFs.** The structured output *works*
   (`auto_highlight: parsed 7 passage(s)`), but every quote fails to resolve
   (`resolve FAILED: quote not located in the rendered document`). The AI feature
   looks broken because the **PDF quote→locator resolution** can't find the
   model's quotes.
2. **"Read as HTML" is offered — and errors — on local PDFs.** The toolbar shows
   it whenever any source URL exists; a local PDF's URL is `local://sha256/…`,
   which isn't fetchable (`open_html_document error … builder error for url`).
3. **The "Highlight with AI" panel is malformed** (layout).
4. **A passage you note/ask stops being visibly marked.** You lose track of *what
   you're working on* — the thing you just acted on disappears from the page.
5. **Note and Ask inputs are shown at the same time.** In one passage you see a
   Note field *and* a chat composer stacked; it's unclear which does what.
6. **The controls don't explain themselves.** "All / You / AI" and "Note / Chat /
   Starred" read like actions, not filters. And **every conversation is dumped
   into the Annotations list**, so "everything I marked" is polluted by chats.

## Putting myself in the user's seat (walk-through)

> I open a PDF. There's a top bar: a **PDF** badge, **Read as HTML** (why? this
> *is* the document — clicking it errors), a search box (greyed — "HTML only"),
> zoom, and **✨ Highlight with AI**.
>
> I select a sentence. A little popup lets me pick a color, or **Note**, or
> **Ask**. I click **Ask**, type a question. My sentence's highlight… vanishes.
> Which sentence was I asking about? I scroll back to check.
>
> In the right panel I clicked into a passage. There's a **Note** box *and* a
> chat box. I meant to just jot a note — do I type in the top one or the bottom
> one? I press Enter in the wrong one.
>
> Back in the list, there's a row of buttons: **All · You · AI**, six color dots,
> **Note · Chat · Starred**. Are these things I *do* to the selected item, or
> ways to *filter*? They look identical to the action buttons. And my three AI
> chats are sitting in the same list as my highlights, so the "index of what I
> marked" is mostly conversations.
>
> I click ✨ **Highlight with AI**, pick *Results*, hit Highlight. "Marking…"
> then nothing appears on the page. Did it fail? (It did — silently on the page,
> though the bar now says "couldn't locate.")

The through-line: **too many controls with unlabeled, overloaded roles, and the
thing I'm working on doesn't stay visible.**

## Principles for this pass (button legibility)

1. **Every control announces its role.** A filter looks like a filter (grouped,
   labeled "Filter", or behind a funnel icon); an action looks like an action.
   No naked verb-nouns floating in a row.
2. **What you act on stays visible.** Coloring, noting, *or* asking a passage
   leaves a mark on the page, so you never lose the thing you just worked on.
   (This pass decides **Ask leaves a light marker** — see R2.)
3. **One input at a time.** A passage shows **Note** or **Chat**, chosen by an
   explicit switch — never both stacked.
4. **Marks and Chats are different things.** "What I highlighted/noted" and "my
   conversations" are separate lists. A conversation is not an annotation of the
   margin; it's a side channel.
5. **A feature that can't deliver says so at the point of use** (auto-highlight
   that resolves nothing must not look like success).

## Refinements

### R1 — Separate **Marks** from **Chats** (the panel)

**Problem:** conversations flood the Annotations index (#6).

**Change:** the Annotations tab becomes two clearly-separated regions:

- **Marks** (default, primary): passages with a **color or a note** — the true
  "everything I marked." Each row: chip/marker, excerpt or note, badges (AI /
  note). This is the annotation index the north star wanted.
- **Chats** (a **collapsible section** below, collapsed by default): every
  conversation — the whole-paper "Ask about this paper" plus any passage
  conversations — as a compact list. Clicking one opens that conversation.

Because R2 gives an ask-only passage a light on-page marker, it now shows in
**Marks** too (with a chat badge) *and* under **Chats** — the badge distinguishes
it, and the "Has: Chat" filter isolates conversations. A note-with-chat passage
shows in Marks with both badges. Filters (R4) apply to **Marks**; Chats is just a
list. *(Open question: default-hide ask-only passages from Marks — see below.)*

*(This directly answers "why are all existing chats stored in annotation — maybe
a collapsible list under Chats.")*

### R2 — A passage you ask about stays marked (amends the north star)

**Problem:** noting/asking a passage leaves no page mark, so you lose it (#4).

**Decision (reverses a north-star call):** **Ask now leaves a light permanent
marker** on the passage — a subtle "conversation" marker (thin low-opacity
underline, visually distinct from the note marker), so an asked passage stays
findable on the page and appears in the index, just like a note does. This
supersedes the north-star rule *"Ask leaves no page mark."*

Revised page-visibility table (amends `pdf-reader-ux.md`):

| The passage has… | On the page |
|---|---|
| a color | that color highlight |
| a note only | the neutral **note** marker |
| a conversation only | a light **conversation** marker (was: nothing) |
| a combination | the color if set, else the strongest marker present |

Rationale: in practice "I asked about this and now it's gone" is disorienting
(#4); a findable trace is worth more than a pristine page. The two markers are
distinguishable so you can tell a private note from a passage with an AI thread.

**Interaction with R1:** because an ask-only passage now has an on-page mark, it
*does* show in **Marks** — but tagged with the chat badge, and it still appears
in **Chats**. Marks = "anything visible on the page"; Chats = "anything with a
conversation." Overlap is expected and fine; the badge tells them apart. (Open
question: should ask-only passages be *hidden* from Marks by default and shown
only via the "Has: Chat" filter? — flagged below.)

*(Optional add-on, not required by this decision: a brighter temporary outline on
the passage whose detail is currently open, for extra "you are here" feedback.
Deferred unless the marker alone proves insufficient.)*

### R3 — Passage detail: **Note** and **Chat** are one-at-a-time sub-views

**Problem:** Note field and Ask composer show together; unclear which is which
(#5), and "click Ask should show only ask or note."

**Change:** the passage detail gets a small **segmented switch: `Note | Chat`**.

- **Note** view: the note field (Enter saves). Nothing else.
- **Chat** view: the conversation + ask composer (Enter sends). Nothing else.
- Default: **Note** if the passage has/wants a note; **Chat** if you arrived by
  clicking **Ask** (the selection popup's Ask deep-links to the Chat sub-view).
- The color swatches stay above the switch (color is orthogonal to note/chat).

One passage, one visible input, chosen deliberately.

### R4 — Legible controls (labels, grouping, naming)

**Problem:** "All / You / AI" and "Note / Chat / Starred" are unlabeled and look
like actions (#6).

**Change:**

- Put filters behind a single **Filter** control (funnel icon + "Filter"); when
  open it shows **labeled** groups: **Author** (All · Me · AI), **Color**,
  **Has** (Note · Chat · Starred). Collapsed by default so the list breathes.
- Rename for consistency with the rest of the UI: **You → Me**; the "Chat" facet
  matches the **Chats** section (R1); "Starred" keeps the `Star` icon **and** the
  word until it's obviously learned.
- The **detail** action row (swatches, Note/Chat switch, Remove) is visually
  distinct from any filter affordance, so *do* vs *filter* never look alike.

### R5 — Fix the ✨ menu layout (#3)

Rework `AiHighlightMenu` so it renders correctly within the toolbar's positioned
slot (bounded width, no clipping/overflow, proper anchoring under the button).
Treat as a straight layout fix.

### R6 — Correctness bugs

- **R6a — Read as HTML only for real web pages.** Gate the toolbar action on an
  `http(s)://` source URL (a saved web page or a discovery candidate), never a
  `local://…` PDF. `canReadAsHtml` must test the scheme, not mere presence.
- **R6b — PDF quote resolution reliability.** Auto-highlight parses correctly but
  the PDF resolver can't locate the quotes (#1). Investigate the PDF
  quote→locator path (text-layer span joining, the alphanumeric matcher's
  coverage across line/column breaks, quote length). Auto-highlight is only as
  good as this resolver; it's the highest-value fix in this pass. Consider:
  shorter model quotes (tighten the prompt), and a fuzzier PDF matcher that
  spans text-item boundaries.

## Button role reference (target — every control, one line)

**Reader toolbar (document-level):**

| Control | Role |
|---|---|
| `PDF`/`HTML` badge | Which format you're reading. Not a button. |
| Read as HTML | Re-open a **web** source as an in-app article. *Hidden for local PDFs (R6a).* |
| Search | Find text in the document. *(HTML now; PDF pending.)* |
| Zoom −/% /+ | Scale the PDF. |
| ✨ Highlight with AI | Open the lens menu; AI marks passages by category. |

**Annotations panel:**

| Control | Role |
|---|---|
| Annotations / Meta tabs | Your marks & chats · paper metadata. |
| **Marks** list | Everything you highlighted or noted. |
| **Chats** (collapsible) | Your conversations (whole-paper + per-passage). |
| Filter (funnel) | Narrow **Marks** by Author / Color / Has. |
| Ask about this paper | Start/opens the document-wide conversation. |

**Passage detail:**

| Control | Role |
|---|---|
| Color swatches | Set/clear this passage's highlight color. |
| `Note | Chat` switch | Choose the note field **or** the conversation. |
| Note field | Your private text; Enter saves. |
| Chat (composer) | Ask the AI about this passage; Enter sends. |
| Remove | Delete the passage's annotation. |
| Star (in chat) | Keep an answer/note (feeds the Starred filter). |

**Selection popup (on the page):**

| Control | Role |
|---|---|
| Color swatches | One-click color highlight. |
| Note | Create the passage + open its **Note**; leaves the note marker. |
| Ask | Create the passage + open its **Chat**; leaves a light **conversation** marker (no color). |

## User journeys (target experience)

**J1 — Quick color highlight.** I select a sentence → the popup appears → I click
🟡. Done: the sentence is yellow on the page and appears under **Marks**. No panel
spin-up, no ambiguity.

**J2 — Jot a private note.** I select a phrase → **Note** → the passage outlines
on the page (R2) and the detail opens on the **Note** sub-view → I type, press
**Enter** → saved. The passage now shows the neutral note marker and a row under
**Marks**. The AI was never involved.

**J3 — Ask about a figure.** I select the caption → **Ask** → the passage gets a
**light conversation marker** (R2) and the detail opens on the **Chat** sub-view
(only the chat box, no note field) → I ask, **Enter** sends → the answer streams.
Afterward the caption still shows its subtle marker, so tomorrow I can see I
discussed it; the conversation is listed under **Chats**, and the passage row
carries a chat badge in **Marks**.

**J4 — Have the AI mark all results.** Toolbar → **✨ Highlight with AI** → the
menu opens cleanly (R5) → I tick **Results** (green) → **Highlight**. "Marking… N"
→ green marks appear as they resolve → **Keep · Undo all**. If none resolve, the
bar says "Couldn't locate N passages" (never silent), and — once R6b lands — this
is the common case *fixed*, not disclosed.

**J5 — Review what I marked.** Annotations → **Marks** shows my highlights and
notes. I click **Filter** → Author: **AI** → only the AI's marks. I clear it. My
three conversations are tucked under **Chats**, one click away, not cluttering the
index.

**J6 — Come back tomorrow.** I open the paper; my colors and notes are on the
page and in **Marks**. I expand **Chats** to reread a conversation; clicking it
jumps to the passage (still outlined while open). I search a term (HTML today).

## Proposed refinement RFCs

Ordered by user-visible payoff and independence:

1. **RFC 0066 — Reader control legibility & bug fixes.** R6a (Read-as-HTML
   scheme gate), R5 (AI menu layout), R4 (labeled Filter, rename You→Me). Small,
   high-clarity, low-risk. *Do first.*
2. **RFC 0067 — Marks vs Chats + focus outline.** R1 (split the panel) and R2
   (active-passage outline). The structural clarity win.
3. **RFC 0068 — Passage detail Note/Chat switch.** R3 (one input at a time).
4. **RFC 0069 — PDF quote-resolution hardening.** R6b — make auto-highlight (and
   any quote-anchored feature) actually land on PDFs. Highest engineering value;
   sized on its own because it's an algorithm problem, not UI.

## Open questions

- **Ask-only passages in Marks (R1×R2):** now that asking leaves a marker, should
  ask-only passages appear in the **Marks** list by default (with a chat badge),
  or be **hidden** from Marks and reachable only via the "Has: Chat" filter and
  the **Chats** section? (Decision pending your review — this is the main knob the
  "Ask leaves a marker" choice opens up.)
- Should the two markers (note vs conversation) be visually distinct enough at a
  glance, or is one shared "annotated, no color" marker + a panel badge enough?
- Optional focus outline (R2 add-on): worth adding a brighter "you are here"
  outline on the open passage on top of the marker, or is the marker enough?
- Chats (R1): a passage conversation reachable from both its Marks row (chat
  badge) and the Chats section? (Assume both.)
- Filter (R4): funnel-behind-a-button vs. always-visible-but-labeled — which
  reads faster in practice? (Prototype both.)
- Does "Ask about this paper" belong at the top of **Chats** (its natural home)
  rather than above the Marks list? (Leaning yes.)
