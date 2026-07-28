# PDF Reader UX — North Star Design

Status: Draft (design north star — RFCs carve increments out of this)
Date: 2026-07-28
Product: i0i
Scope: How a user reads and annotates a paper (PDF and HTML) in i0i — layout,
annotation model, AI collaboration, and the interaction details that currently
cause friction.

This document is the **vision**. It is not an implementation plan; each section
that changes behavior becomes an RFC (see "Increment plan"). Where it describes
today's behavior, it does so only to contrast with the target.

## Why this document exists

The reader grew feature-first: chat came before highlights, so highlighting was
bolted onto chat. That produced three concrete smells the user flagged:

1. **Note and Ask are the same gesture.** Selecting text opens one composer where
   **Enter sends your text to the AI**. A quick margin note becomes an expensive,
   surprising AI question. Notes and AI answers also interleave in one thread.
2. **AI marking is hidden inside chat.** To have the AI highlight passages you
   type "highlight the results" into the chat box; intent is guessed by a keyword
   gate, the answer model *also* emits redundant prose, and the feature is
   undiscoverable.
3. **The panel is organized around conversations, not annotations.** There is no
   single place to see "everything I marked."

The fixes below are not cosmetic — they change what the primitives are and where
they live.

## Principles

1. **The annotated passage is the primitive.** A color highlight, a note, and a
   conversation are three *independent* attachments on a passage — any can exist
   alone, each added in one click. (Highlight is not a prerequisite for noting or
   asking.)
2. **Note ≠ Ask.** A note is *your* text — cheap, private, the default. Ask is a
   *deliberate* conversation with the AI. They have separate inputs and never
   share an Enter key.
3. **The AI is an explicit collaborator, not a hidden chat trick.** "Highlight
   with AI" is a named action with concrete inputs and a visible result, not a
   sentence you hope the chat interprets.
4. **Annotation comes to the text; document tools stay put.** Contextual actions
   (highlight/note/ask a selection) appear at the selection. Document-level tools
   (search, AI auto-highlight, zoom) live in a persistent toolbar.
5. **One place to see everything you marked.** The panel is annotation-first.

## The core model

The primitive is the **annotated passage** (an anchor into the text). Three
**independent** attachments hang off it — a color highlight, a note, and a
conversation. Any can exist alone; none requires the others; each is added in a
single click. There is never a "highlight first, then note/ask" step.

```
AnnotatedPassage { anchor, author: you | AI }            ← the primitive
  ├─ color?        a highlight color (optional)   → renders as that color
  ├─ note?         your text (optional, Enter-saves) → renders as a subtle marker
  └─ conversation? an AI thread (optional)         → NOT rendered on the page;
                                                      lives in the panel, anchored

Document
  └─ conversation  whole-paper AI thread ("ask about this paper")
```

**Page visibility, by attachment:**

| The passage has… | On the page |
|---|---|
| a color | that color highlight |
| a note only (no color) | a subtle neutral marker (so it's findable) |
| a conversation only | **nothing** — a side conversation belongs in the panel, anchored so you can jump back to the passage |
| any combination | the color if set, else the note marker; a 💬 badge in the panel |

So **Ask leaves no page mark** (that's "ask without a highlight"); **Note** leaves
a light marker without you picking a color or doing a separate step; and a color
highlight is its own one-click action. Want color on a noted/asked passage? Click
a swatch — independently, anytime.

Two AI actions, deliberately different shapes:
- **Auto-highlight** — a *command*: "mark passages matching X in color Y" → returns
  a set of highlights. No prose, no conversation.
- **Ask** — a *conversation*: prose Q&A about a passage or the document.

## Layout

```
┌──────────────────────────────────────────────┬───────────────────────┐
│  READER TOOLBAR (persistent, document-level)  │                       │
│  ◀ 3/24 ▶  ⊖ 115% ⊕   🔍 search   [PDF|HTML]  │   RIGHT PANEL         │
│  ✨ Highlight with AI   🖍 highlighter-mode    │  ┌─────────────────┐  │
├──────────────────────────────────────────────┤  │ Annotations | Meta│  │
│                                                │  ├─────────────────┤  │
│   READING SURFACE (PDF / HTML)                 │  │ 💬 Ask about     │  │
│   highlights rendered inline                   │  │    this paper    │  │
│                                                │  │ ─ filters ─      │  │
│   ┌── selection popup (contextual) ──┐         │  │ 🟡 "…" 📝        │  │
│   │  🟡🟢🔵🔴🟣🟠   Note   Ask       │         │  │ 🔴 "…" 💬 ✨     │  │
│   └───────────────────────────────────┘        │  │ 🟢 "…"           │  │
│                                                │  │  … (list ⇄ detail)│  │
└──────────────────────────────────────────────┴──└─────────────────┘──┘
```

- **Reader toolbar (persistent):** page nav, zoom, in-document search, PDF/HTML
  view toggle, **✨ Highlight with AI**, **🖍 highlighter-mode** toggle.
  Everything here is document-level.
- **Reading surface:** the paper, with highlights drawn inline (tight boxes over
  the text — the resolver/geometry work already done).
- **Selection popup (contextual):** appears at a text selection — color swatches ·
  **Note** · **Ask**. This is where manual annotation happens; it comes to the
  text rather than living in a fixed bar.
- **Right panel:** **Annotations** and **Meta** tabs (details below).

## Where the conversation lives (resolving "where's the chat window?")

Collapsing the old Threads/Pins/Meta into **Annotations + Meta** does **not** remove
chat — it relocates it to where it belongs:

- **Auto-highlight needs no chat surface.** It is a toolbar command; its output is
  marks on the page plus a **Keep · Undo** bar. Nothing to converse with.
- **Ask needs a conversation surface**, and that surface is the **detail view of
  the focused thing**:
  - The **Annotations** tab is a **list ⇄ detail**. The list is the index of
    annotated passages; clicking one opens its **detail**: the excerpt, its
    **note** (inline,
    editable), and its **conversation** (the passage-scoped AI thread). *That
    conversation region is the chat window* — contextual to the passage.
  - A persistent **"💬 Ask about this paper"** entry at the top opens the
    **document-scoped** conversation (the same detail view, scoped to the whole
    paper) for questions not tied to a passage.

So there is exactly one conversation surface; its *scope* follows what you're
looking at (a highlight, or the document). No conversation tab is lost — it stops
being a top-level silo and becomes contextual.

## The Annotations panel (annotation-first)

- **List (index):** every annotated passage — color chip (or note marker),
  excerpt, badges: 📝 has a note, 💬 has a conversation, ✨ AI-authored, ⭐
  starred. Filters: **color · author (you / AI) · has-note · has-conversation ·
  starred**. (Old *Pins* = the ⭐ filter; old *Threads* = the 💬 filter.) Click a
  row → jump to the passage on the page **and** open its detail.
- **Detail (one passage, or the document):** excerpt · **note field** (Enter
  saves) · **conversation** (Ask). Back returns to the list.

**Meta** stays a separate tab (paper metadata is unrelated to annotation).

## Note vs Ask — the behavioral fix

Selecting text shows the popup: `🟡🟢🔵🔴🟣🟠  Note  Ask`.

| Action | Result | Enter behavior |
|---|---|---|
| **Color swatch** | Color highlight only, in that color. Done. | — |
| **Note** | An inline **note field** on the passage; on save, a subtle neutral marker (no color picked, no separate step). | **Enter SAVES the note.** Shift+Enter = newline. Never asks the AI. |
| **Ask** | Opens the passage **conversation** in the panel. **No page mark** — the passage is anchored (jump-back) but not highlighted. | Enter sends the message to the AI (in the ask input only). |

A note is your text, saved instantly and locally; Ask is the deliberate,
networked action. They never share an Enter key or a composer again. Each is one
click from the selection — no "highlight first." A passage displays its color (if
any) and its note inline; its conversation is reached via the 💬 badge in the
panel.

## AI features

### ✨ Highlight with AI (auto-highlight) — a command

- Toolbar → a small popover with a **discrete, multi-select menu** (primary UX —
  a concrete tool, not a text prompt):
  ```
  ☐ Key contributions   🟡      ☐ Limitations   🔴
  ☐ Methods             🔵      ☐ Definitions   🟣
  ☐ Results             🟢      ☐ Custom…       (free text + color)
                    [ Highlight ]
  ```
  - **Multi-select**, each category with a default color — so one action can mark
    several categories at once (contributions=yellow, results=green,
    limitations=red), which is where per-color meaning pays off.
  - **Custom…** is a single row (free text + color) for off-menu requests — the
    secondary path, not the headline.
  - The category set is editable; colors are adjustable.
- Runs the **fast annotation model** (cheap, separate from the chat model) with
  **structured outputs** — a strict `json_schema` returning
  `{ "highlights": [ { "quote", "color", "label" } ] }` — rather than streamed
  tool-calls. One clean JSON object → parse the list → resolve each quote to a
  locator (the alphanumeric matcher) → create AI-authored highlights.
- UX: a **"Marking… N"** progress indicator, marks appear as they resolve, then a
  **Keep · Undo** bar for the batch. **No prose answer, no chat.**

### Ask — a conversation

- On a highlight or the document. The **chat model**, streamed prose. Lives in the
  detail conversation surface described above. This is the *only* place prose
  answers appear — so asking to "highlight" never produces a stray answer, because
  highlighting is no longer an ask.

## Iconography

Emojis in this document (📝 💬 ✨ ⭐ 🔍 🖍) are **placeholders**. The UI uses a
real icon set: **`@lucide/svelte`** — the official Lucide package for Svelte 5,
tree-shakeable, MIT, and the same visual language as the icons already
hand-inlined in `ActivityRail`. Color swatches stay as CSS circles (not icons).

| Placeholder | Lucide icon | Where |
|---|---|---|
| ✨ Highlight with AI / AI-authored | `Sparkles` | toolbar action, AI badge |
| 📝 note | `StickyNote` (or `NotebookPen`) | highlight badge, Note action |
| 💬 conversation / Ask | `MessageSquare` | highlight badge, Ask action, "Ask about this paper" |
| ⭐ starred | `Star` | pin/keeper filter |
| 🔍 search | `Search` | toolbar |
| 🖍 highlighter-mode | `Highlighter` | toolbar toggle |
| zoom | `ZoomIn` / `ZoomOut` | toolbar |
| page nav | `ChevronLeft` / `ChevronRight` | toolbar |
| remove | `Trash2` | popover |
| recolor / palette | (color swatches, CSS) | popover, selection popup |

Adopting `@lucide/svelte` (replacing the hand-inlined SVGs) is small enough to
land with RFC 1, or as a standalone tidy-up first.

## User journey (target)

1. **Open** → paper renders; toolbar and empty Annotations panel ready.
2. **Mark a sentence** → select → click 🟡. One click, no panel spin-up.
3. **Jot a thought** → select → **Note** → type → **Enter** saves. Private, instant.
4. **Ask about a figure** → select → **Ask** → conversation opens for that passage.
5. **Have the AI mark all results** → toolbar **✨ Highlight with AI** → *Results* →
   marks appear with **Keep · Undo**. No chat, no guessing.
6. **Review everything** → Annotations panel; filter by ✨ AI or 🔴 or ⭐.
7. **Come back tomorrow** → search the doc, or scan the Annotations list; click →
   jump to the passage.

## Inconveniences → resolutions

| Pain (today) | Resolution |
|---|---|
| Enter on a selection *asks the AI* instead of noting | Note field: Enter saves; Ask is a separate deliberate action |
| Notes and AI answers interleave in one thread | Note (your text) and Conversation (AI) are distinct layers on a highlight |
| Selecting text spins up a heavy thread panel | Selection → lightweight popup; the panel only engages on Note/Ask |
| AI marking hidden in chat, fragile keyword gate, redundant prose | Explicit ✨ toolbar action; command not conversation; no prose |
| No overview of what I marked | Annotations panel = annotation index with filters |
| Streamed tool-call assembly is fiddly/unreliable | Structured outputs (json_schema) → one parseable list |
| PDF marks mis-shaped / quotes not found | Text-layer geometry + alphanumeric matcher (already fixed) |

## RFC roadmap (the sequence to the north star)

RFCs are ordered so each rests on the one before it. Numbers are provisional
(next free is 0060). Dependency chain:

```
Foundation (shipped) ─┬─ 0060 Icons ────────────────────────────┐
                      ├─ 0061 Passage model + Note/Ask ─┬─ 0062 Annotations panel
                      │                                 └─ 0064 AI auto-highlight
                      └─ 0063 Reader toolbar ─────────────── 0064 (button lives here)
```

### Foundation — already shipped (Phase 1 + Phase 2)

In place, and reused by everything below: the highlight primitive + storage +
migration; multi-color rendering (HTML + PDF); the client-side quote→locator
resolvers with the alphanumeric matcher; the fast annotation model; non-blocking
background marking with Keep/Undo; and leveled logging. Current agent marking is
chat-driven — RFC 0064 replaces that trigger, not the machinery.

### RFC 0060 — Iconography: adopt `@lucide/svelte`

- **Goal:** replace emojis and hand-inlined SVGs with the Lucide component set.
- **Depends on:** nothing.
- **Why first:** cheap, isolated, zero behavioral risk; every UI RFC after this
  uses icons, so clear the deck once.

### RFC 0061 — Passage-primitive model + Note/Ask separation *(highest-value)*

- **Goal:** make the **annotated passage** the primitive with three independent
  optional attachments (color / note / conversation). Split Note from Ask: the
  Note field's **Enter saves a note**; **Ask** is a separate action that leaves
  **no page mark**; a note-only passage renders a subtle neutral marker.
- **Delivers:** the selection popup behavior, the storage change (color becomes
  optional; note and conversation are independent), and the rendering rules.
- **Depends on:** the shipped highlight primitive (extends it).
- **Why here:** it's both the foundation the panel and AI tool assume *and* the
  single biggest quality-of-life fix. Nothing above the foundation should land
  before this, because it changes what an "annotation" is.

### RFC 0062 — Annotations panel (list ⇄ detail + filters)

- **Goal:** restructure the right panel from Threads/Pins/Meta to **Annotations +
  Meta**: a filterable index (color · author · has-note · has-conversation ·
  starred) that drills into a per-passage detail (excerpt · note · conversation),
  plus a document-level "Ask about this paper" entry. Pins/Threads become filters.
- **Depends on:** RFC 0061 (the passage model + attachments).
- **Why here:** it surfaces the model 0061 introduces; pointless before it exists.

### RFC 0063 — Reader toolbar & navigation

- **Goal:** the persistent document-level toolbar shell — in-document search,
  highlighter-mode (click-drag continuous marking), PDF/HTML toggle,
  page/zoom consolidation.
- **Depends on:** nothing hard (can proceed in parallel with 0061/0062).
- **Why here:** it's the home the AI auto-highlight button (0064) needs, so it
  should exist by the time 0064 lands.

### RFC 0064 — Explicit AI auto-highlight + structured outputs

- **Goal:** retire chat-driven marking. Add the toolbar **✨ Highlight with AI**
  action — a discrete multi-select lens menu (per-category colors) + a Custom row
  — and switch the annotation call from streamed tool-calls to **`json_schema`
  structured outputs** (one parseable passage list). No prose, no keyword-guessing.
- **Depends on:** RFC 0061 (annotations can be AI-authored without a conversation)
  and RFC 0063 (a toolbar to host the button). Reuses the shipped annotation
  model, resolvers, `create_agent_highlight`, and Keep/Undo.
- **Why last:** highest total surface, but it leans on the model, the toolbar, and
  all the already-built plumbing — so by this point it's mostly wiring + the
  structured-output swap.

**Ordering rationale:** cosmetics first (0060), then the foundational
model/interaction (0061), then the panel that reveals it (0062), then the toolbar
shell (0063), then the AI tool that lives in the toolbar and consumes the model
(0064). 0060 and 0063 can slot in opportunistically since they don't depend on
0061.

## Open questions (to revisit as RFCs are written)

- Does a highlight allow exactly one conversation, or several? (Assume one for
  now — matches "a passage, a discussion.")
- Highlighter-mode (click-drag continuous marking) — v1 or later?
- Should document-level chat be reachable from anywhere (a persistent affordance),
  or only via the Annotations panel entry?
