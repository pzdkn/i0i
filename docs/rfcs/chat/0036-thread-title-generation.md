# RFC 0036: Thread Title Generation

Status: Implemented
Date: 2026-06-15
Product: i0i
Target: Tauri v2 + Svelte, macOS first
Builds on: RFC 0033 (anchored threads + pins), RFC 0034 (notes unified into threads)

## Summary

Anchored threads currently need readable titles. The default title is useful as
a fallback: a selected passage, or "Whole paper" for the document thread. But in
the Threads list and open thread header, raw passages can be too long and
"Whole paper" is not descriptive.

This RFC adds background title generation for newly created threads. On the
first entry of a new thread, i0i asks a cheap configured model for a short topic
title, updates the thread title only if the user has not renamed it, and emits a
small event so the Reader can refresh the visible title.

## Goals

- Generate a short human-readable topic for a newly created thread.
- Run title generation in the background, off the Note/Ask critical path.
- Never overwrite a user rename.
- Keep failure silent: if title generation fails, the default title remains.
- Refresh the Threads list and open thread title when generation succeeds.

## Non-Goals

- No thread summarization.
- No title regeneration after every new message.
- No title generation for legacy/migrated threads in this RFC.
- No new thread model or schema change.
- No dependency on text extraction. This feature works from the first user entry
  and optional anchor passage.

## Design

When a thread's create-or-get path actually creates a row, the backend spawns a
background title job. The creation signal comes from the store: the thread
creation helper should return both the `thread_id` and whether it inserted a new
thread.

```text
input:  first user entry (note body or question) + anchor passage, if any
prompt: "Reply with a 3-6 word topic for this thread. Title Case, no quotes,
         no trailing punctuation."
write:  set chat_threads.title = generated_topic
        only if the current title still equals the auto default
emit:   chat_thread_updated { threadId, title }
```

The default-title guard is the safety mechanism:

- anchored selection default: the selected passage or existing anchor default
- whole-paper default: "Whole paper"
- if the current title differs from the default, assume the user renamed it and
  do nothing

Generation should happen once per created thread. Continuing an existing thread
does not retrigger title generation.

## Configuration

Add a cheap optional title model to the chat provider config.

If unset, skip title generation entirely. The initial implementation enables it
in `src-tauri/app.conf.json` with the existing chat model, a 24-token cap, and a
5s timeout. Swap `title_model` for a cheaper provider model, or set it to
`null`, without touching code.

The title model should reuse the existing chat provider plumbing where possible:

- same provider base URL
- same API key handling
- small token budget
- short timeout
- no streaming required

The RFC does not require a specific model default. If a default is provided, it
should live in config rather than being hardcoded in thread logic.

## Backend Changes

- Store create-or-get helpers return whether a thread was newly created.
- First-entry commands spawn title generation only when `created == true`.
- `ChatService` gains a small `generate_thread_title` path.
- The title update uses a rename-safe guard: update only when the title still
  equals the default.
- Emit `chat_thread_updated { threadId, title }` after a successful update.

Likely trigger points:

- `add_note_at_anchor`
- `ask_at_anchor_streamed`
- whole-paper first-message path

## Frontend Changes

`ReaderView` listens for `chat_thread_updated` and patches:

- the thread row in the Threads list
- the currently open thread title, if it matches the updated id

No new UI surface is required.

## UI

```text
Threads list
  ┌──────────────────────────────────────────────┐
  │ Scaled Dot-Product Scaling      ☆ 2          │   generated topic
  │ Whole paper                     ★ 5          │   fallback/default
  └──────────────────────────────────────────────┘

Open thread
  Scaled Dot-Product Scaling
  > we divide the dot products by sqrt(d_k) ...
  [ Note ] [ Ask ]
```

## Risks

- **Latency/cost.** One cheap background call per new thread; cap tokens and
  timeout.
- **Rename race.** The default-title guard prevents clobbering user renames.
- **Bad titles.** Titles are convenience labels; failure or low quality should
  not block thread use.
- **Event drift.** The event should be idempotent: it only sets the current title
  for one thread id.

## Validation Plan

Unit tests:

- generated title applies when current title still equals the default
- generated title does not apply after a user rename
- continuing an existing thread does not trigger generation
- missing title model skips generation without error

Manual checks:

- create a note on a passage; a topic title appears shortly after
- ask on a fresh passage; streaming starts immediately and title updates later
- rename a thread before title generation completes; rename sticks
- create a whole-paper thread; it can receive a generated title if enabled
