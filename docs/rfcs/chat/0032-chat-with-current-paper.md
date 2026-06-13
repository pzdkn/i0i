# RFC 0032: Chat With the Current Paper

Status: Draft
Date: 2026-06-13
Product: i0i
Target: Tauri v2 + Svelte, macOS first

## Summary

Replace the static Ask-tab mock in the Reader Inspector with a working chat against the currently open paper, powered by OpenRouter's OpenAI-compatible chat completions API. The conversation is durable: messages persist in SQLite per paper, and a useful answer can be promoted into a paper note.

The design introduces a `ChatScope` boundary so that a later RFC can add vault-scope chat ("ask all papers in this vault") without reworking the command surface, the storage schema, or the context builder.

## Context

The Reader Inspector already has an `ask` tab (`ReaderInspector.svelte`), currently rendering hardcoded mock content. The Reader document model (`ReaderDocument`) carries `source_text` — the full extracted text of the paper — which is exactly the context a paper-scoped chat needs. Nothing has to be re-extracted.

The discovery providers established the config pattern this RFC reuses:

```text
app.conf.json block
  -> config struct with api_key as an env-var NAME
  -> resolve from process env, fall back to .env via read_dotenv_value()
```

`OPENROUTER_API_KEY` already exists in the repo-root `.env`, which `read_dotenv_value()` already searches (`manifest_dir/../.env`).

Two architectural patterns to follow:

- Long-lived services (`ReaderService`) are constructed in `lib.rs` and registered with `app.manage()`; commands receive them as `tauri::State`.
- The frontend calls commands only through small bridge functions in `src/lib/bridge/`.

This RFC also implements the first slice of two `slc.md` commitments: "ask a question about the current paper" and "allow saving a useful answer as a durable note", and it honors the `user-journey.md` requirement that AI work leaves durable, inspectable artifacts rather than vanishing chat history.

## Goals

- Send a chat message about the currently open paper and get a model answer in the Ask tab.
- Persist the conversation per paper in SQLite so it survives app restarts.
- Make the AI context inspectable: the UI shows what was sent (paper text, truncation state).
- Allow saving an assistant answer as a paper note.
- Stream tokens into the UI (second milestone) so long answers feel responsive.
- Shape `ChatScope` so vault-scope chat is an additive follow-up, not a rework.

## Non-Goals

- No vault-scope or multi-paper chat in this RFC (the boundary is designed, not implemented).
- No model picker UI; the model is a config value.
- No multiple conversation threads per paper; one transcript per paper.
- No retrieval, chunking, or embeddings; v1 context is the extracted full text under a char budget.
- No agents, scheduling, or tool use.
- No chat for transient discovery candidates (see Risks for why and the upgrade path).

## OpenRouter API Overview

OpenRouter exposes an OpenAI-compatible endpoint:

```
POST https://openrouter.ai/api/v1/chat/completions
Authorization: Bearer <OPENROUTER_API_KEY>
Content-Type: application/json
```

Request body:

```json
{
  "model": "anthropic/claude-sonnet-4.5",
  "messages": [
    { "role": "system", "content": "..." },
    { "role": "user", "content": "..." },
    { "role": "assistant", "content": "..." },
    { "role": "user", "content": "..." }
  ],
  "stream": false
}
```

Non-streaming response (the fields we read):

```json
{
  "id": "gen-...",
  "model": "anthropic/claude-sonnet-4.5",
  "choices": [
    { "message": { "role": "assistant", "content": "..." } }
  ]
}
```

With `"stream": true` the response is server-sent events (SSE): a stream of `data: {json}` lines where each chunk carries `choices[0].delta.content`, terminated by a literal `data: [DONE]` line. Milestone 2 parses this.

OpenRouter recommends two optional headers for app attribution: `HTTP-Referer` and `X-Title`. Send `X-Title: i0i`.

Error cases to surface clearly: `401` (bad/missing key), `402` (out of credits), `429` (rate limited). Each maps to a human-readable message in the Ask tab, not a raw status code.

## Design

### Scope model

The single most important shape in this RFC. Everything — commands, storage, context building — keys off it:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum ChatScope {
    Paper { paper_id: String },
    // Vault { vault_id: String },   // follow-up RFC, additive
}
```

The storage schema stores `scope_kind` + `scope_id` columns rather than a `paper_id` foreign key, so vault transcripts later reuse the same table. The context builder is the only component that must grow real new logic for vault scope (per-paper summaries, budget splitting); the rest is a new enum variant.

### Domain: `src-tauri/src/domain/chat.rs` (new)

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatMessage {
    pub id: String,
    pub role: String,            // "user" | "assistant"
    pub body: String,
    pub model: Option<String>,   // assistant messages only
    pub context_summary: Option<ChatContextSummary>, // assistant messages only
    pub created_at: String,
}

/// What the model actually saw — rendered in the UI so context is
/// inspectable, per the slc.md context-visibility requirement.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatContextSummary {
    pub paper_title: String,
    pub included_chars: usize,
    pub truncated: bool,
}
```

### Storage: one new table in `library_store.rs`

```sql
CREATE TABLE IF NOT EXISTS chat_messages (
    id            TEXT PRIMARY KEY,
    scope_kind    TEXT NOT NULL,       -- 'paper' (later: 'vault')
    scope_id      TEXT NOT NULL,       -- paper_id for paper scope
    role          TEXT NOT NULL,       -- 'user' | 'assistant'
    body          TEXT NOT NULL,
    model         TEXT,
    context_json  TEXT,                -- serialized ChatContextSummary
    created_at    TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_chat_messages_scope
    ON chat_messages (scope_kind, scope_id, created_at);
```

New `LibraryStore` methods, following the paper-notes pattern:

```rust
pub fn get_chat_messages(&self, scope_kind: &str, scope_id: &str)
    -> StoreResult<Vec<ChatMessage>>;
pub fn append_chat_message(&self, scope_kind: &str, scope_id: &str, draft: &ChatMessageDraft)
    -> StoreResult<ChatMessage>;
pub fn clear_chat_messages(&self, scope_kind: &str, scope_id: &str)
    -> StoreResult<()>;
```

`delete_paper_globally` gains a `DELETE FROM chat_messages WHERE scope_kind = 'paper' AND scope_id = ?` statement, mirroring how paper notes are cleaned up (no FK because `scope_id` is polymorphic).

Messages are returned oldest-first (a transcript reads top-down, unlike notes which read newest-first).

### Config: `chat` block in `app.conf.json`

```json
{
  "discovery": { "...": "unchanged" },
  "chat": {
    "provider": {
      "url": "https://openrouter.ai/api/v1/chat/completions",
      "api_key": "OPENROUTER_API_KEY",
      "model": "anthropic/claude-sonnet-4.5",
      "max_context_chars": 60000
    }
  }
}
```

`api_key` is the env-var name, resolved like OpenAlex: process env first, `.env` fallback. A missing key is a hard error at request time (chat cannot run unauthenticated), but it must NOT fail app startup — the error surfaces in the Ask tab when the user tries to send ("OpenRouter API key not found. Set OPENROUTER_API_KEY in the environment or .env.").

`read_dotenv_value` currently lives at `crate::commands::discovery::providers::shared`. It is `pub(crate)`, so chat can import it directly. Importing env plumbing from the discovery module is a wrong-shaped dependency, so this RFC moves `read_dotenv_value` / `parse_dotenv_value` (and their tests) to a new `src-tauri/src/shared/env.rs`, with the two discovery call sites updated. This is a mechanical move; if it bloats the diff, ship the chat milestone importing from the discovery path and do the move as an immediate follow-up commit.

### Service: `src-tauri/src/services/chat/` (new module)

```text
src-tauri/src/services/chat/
  mod.rs         re-exports ChatService
  config.rs      ChatConfig::load() from app.conf.json, key resolution
  context.rs     ChatScope -> ContextBundle
  openrouter.rs  wire types + HTTP call (and SSE parsing in M2)
  service.rs     ChatService orchestration
```

`ChatService` is constructed in `lib.rs` and registered with `app.manage()`, like `ReaderService`. It holds a `reqwest::Client`, the config, and handles to `LibraryStore` and `ReaderService` (matching however `ReaderService` currently receives the store).

The orchestration for one message:

```text
send(scope, user_body)
  1. append user message to store
  2. load transcript from store (history)
  3. build context:  ReaderService -> ReaderDocument -> source_text
       truncate to max_context_chars (keep head, drop tail, record truncated=true)
  4. assemble messages:
       system:  task framing + paper title/authors/venue + context text
       history: prior user/assistant turns (bodies only, no context re-inlining)
       user:    new message
  5. POST to OpenRouter
  6. append assistant message (body, model, context summary) to store
  7. return updated transcript
```

Two details that matter:

- **The paper text lives in the system message only, once.** History turns are stored and replayed as plain bodies. Re-inlining the paper into every turn would multiply token cost per message.
- **Do not hold the store lock across the network call.** Steps 1–2 and 6 each take the store briefly; step 5 is a multi-second await and must not block every other command in the app. The `LibraryStore` connection is behind a lock — acquire, write, release, then go to the network.

System prompt (v1, intentionally plain):

```text
You are a research assistant inside i0i. Answer questions about the
paper below. Ground every claim in the paper text. If the paper does
not contain the answer, say so explicitly.

Title: {title}
Authors: {authors}
Venue: {venue} {year}

Paper text{truncation_note}:
{context_text}
```

where `truncation_note` is ` (truncated to first {n} characters)` when the budget was hit.

### Commands: `src-tauri/src/commands/chat.rs` (new)

```rust
#[tauri::command]
pub async fn get_chat_messages(
    chat_service: tauri::State<'_, ChatService>,
    scope: ChatScope,
) -> Result<Vec<ChatMessage>, String>;

#[tauri::command]
pub async fn send_chat_message(
    chat_service: tauri::State<'_, ChatService>,
    scope: ChatScope,
    body: String,
) -> Result<Vec<ChatMessage>, String>;

#[tauri::command]
pub async fn clear_chat_messages(
    chat_service: tauri::State<'_, ChatService>,
    scope: ChatScope,
) -> Result<(), String>;
```

Registered in `lib.rs` alongside the existing handlers. `send_chat_message` returns the full updated transcript, matching the `create_paper_note -> Vec<PaperNote>` convention.

### Milestone 2: streaming

Non-streaming ships first: it exercises the whole pipeline (config, store, context, HTTP, UI) with the least new machinery, and a working-but-blocking chat is reviewable on its own. Streaming is a contained follow-up that changes only the transport.

Tauri v2 provides `tauri::ipc::Channel<T>` for exactly this: the frontend creates a channel, passes it as a command argument, and the backend pushes typed events through it while the command runs.

```rust
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase", tag = "event")]
pub enum ChatStreamEvent {
    Delta { text: String },
    Done { messages: Vec<ChatMessage> },
    Error { message: String },
}

#[tauri::command]
pub async fn send_chat_message_streamed(
    chat_service: tauri::State<'_, ChatService>,
    scope: ChatScope,
    body: String,
    on_event: tauri::ipc::Channel<ChatStreamEvent>,
) -> Result<(), String>;
```

Backend: set `"stream": true`, consume `response.bytes_stream()`, split on lines, parse `data: {json}` chunks, emit `Delta` per content fragment, persist the assembled message on `[DONE]`, emit `Done`. SSE chunks can split mid-line across network reads, so the parser must buffer partial lines — this is the fiddly part and gets unit tests against fixture byte sequences.

Dependency changes for M2 only:

```toml
reqwest = { version = "0.13.3", features = ["json", "stream"] }
futures-util = "0.3"
```

M1 has no dependency changes.

The non-streamed `send_chat_message` stays registered after M2 — it is the simpler debugging path and costs nothing to keep.

### Milestone 3: save answer as note

Per `slc.md`, the bridge from transient AI chat to durable knowledge. Each assistant message gets a "Save as note" action that creates a paper-level note:

```text
body          = assistant message body
quote_context = the user question that produced it
anchor_kind   = "chat"
```

The existing note validation requires selection offsets for text-anchored notes but already admits location anchors without selected text (see `create_pdf_note_allows_location_anchor_without_selected_text`). `anchor_kind = "chat"` is a new variant of that relaxation: no offsets, no rects, no page. The store validation in `create_paper_note` is extended to accept it; the Notes tab renders chat-born notes with a small "from chat" marker instead of an anchor snippet, and clicking them does not attempt to scroll the PDF.

If the validation change turns out to be invasive, M3 falls back to a "Copy answer" action and the note promotion becomes its own small RFC — but the intent is to ship it here.

## Frontend Changes

### `src/lib/domain/chat.ts` (new)

`ChatMessage`, `ChatContextSummary`, `ChatScope` mirroring the Rust types.

### `src/lib/bridge/chat.ts` (new)

```ts
export async function getChatMessages(scope: ChatScope): Promise<ChatMessage[]>;
export async function sendChatMessage(scope: ChatScope, body: string): Promise<ChatMessage[]>;
export async function clearChatMessages(scope: ChatScope): Promise<void>;
// M2:
export async function sendChatMessageStreamed(
  scope: ChatScope,
  body: string,
  onDelta: (text: string) => void,
): Promise<ChatMessage[]>;  // wraps Channel from @tauri-apps/api/core
```

### `ReaderInspector.svelte` — Ask tab

Replace the mock with:

- transcript list (user/assistant bubbles), loaded via `getChatMessages` when the tab first activates for a document
- under each assistant message: the context summary line ("Context: full paper text · 48,210 chars · truncated") and, in M3, "Save as note"
- input row: textarea + Send button; Send disables while a request is in flight; Enter sends, Shift+Enter newlines
- error state rendered inline in the transcript (missing key, 402, 429, network), never a silent failure
- "Clear chat" affordance (small, in the tab header area) calling `clearChatMessages`
- transcript state is component-local; no `library-cache` involvement in v1

For transient discovery documents (opened via `get_discovery_reader_document`, not in the library), the Ask tab shows the input disabled with: "Add this paper to a vault to chat with it." The component can detect this from how the document was opened (the discovery path is already distinguished in the Reader open flow).

## Files Affected

Rust:

- `src-tauri/app.conf.json` — add `chat` block
- `src-tauri/Cargo.toml` — M2 only: `stream` feature, `futures-util`
- `src-tauri/src/domain/chat.rs` — new
- `src-tauri/src/domain/mod.rs` — add `pub mod chat`
- `src-tauri/src/shared/env.rs` — new home for dotenv helpers (moved from discovery)
- `src-tauri/src/commands/discovery/providers/shared.rs` — drop moved helpers, update imports
- `src-tauri/src/services/chat/{mod,config,context,openrouter,service}.rs` — new
- `src-tauri/src/services/mod.rs` — add `pub mod chat`
- `src-tauri/src/commands/chat.rs` — new
- `src-tauri/src/commands/mod.rs` — add `pub mod chat`
- `src-tauri/src/lib.rs` — construct + manage `ChatService`, register commands
- `src-tauri/src/storage/library_store.rs` — `chat_messages` table, three methods, cleanup in `delete_paper_globally`; M3: accept `anchor_kind = "chat"`

Frontend:

- `src/lib/domain/chat.ts` — new
- `src/lib/bridge/chat.ts` — new
- `src/lib/features/reader/ReaderInspector.svelte` — replace Ask mock with real chat

## Risks

- **Long papers vs. char budget.** 60k chars (~15k tokens) covers most papers' bodies but truncates long ones silently from the model's perspective. The truncation flag in the context summary keeps the user informed, and the system prompt tells the model the text may be truncated. Real chunking/retrieval is explicitly deferred.
- **Cost.** Every turn resends the full paper text in the system message. With per-paper transcripts and manual sends this is acceptable for v1; prompt-caching or summarized context is a follow-up concern. The model is a config value, so the user can pick a cheap one.
- **Store lock across network await.** Called out in the service design; if a reviewer sees the store guard held across the OpenRouter await, that is a bug.
- **SSE parsing (M2).** Chunk boundaries split lines; comment lines (`: ...`) and `[DONE]` must be handled; a malformed chunk should fail that turn gracefully, not panic. Fixture-driven unit tests are mandatory before wiring to the UI.
- **Transient discovery papers.** Persisting chat keyed by candidate id would orphan rows for papers never saved. v1 sidesteps this by disabling chat for transient documents. If "chat before saving" proves important to the discovery journey, the upgrade path is ephemeral (non-persisted) chat for transient scopes — the service can skip store writes when the scope is transient, no schema change needed.
- **History growth.** Very long transcripts will eventually exceed the model context. v1 sends full history and relies on "Clear chat"; a sliding window over history is a one-line follow-up in the message assembly when needed.
- **Key in `.env`.** The key never appears in config, logs, or error messages — error text names the env var, not the value. `reader_log`-style logging in chat must log message ids and lengths, never bodies or keys.

## Validation Plan

```bash
cargo fmt --check
cargo clippy
cargo test
pnpm check
pnpm build
```

Unit tests:

- `config.rs`: chat block parses; missing key resolves to a request-time error, not a startup panic.
- `context.rs`: text under budget passes through untruncated; text over budget truncates at the boundary and sets `truncated = true`; summary char counts are accurate.
- `openrouter.rs`: non-streaming response deserializes; error statuses map to the right user-facing messages; (M2) SSE parser handles split-mid-line chunks, comment lines, `[DONE]`, and assembles deltas in order.
- `library_store.rs`: append/get round-trips messages oldest-first; clear removes only the given scope; `delete_paper_globally` removes the paper's chat; (M3) `anchor_kind = "chat"` notes are accepted without offsets.

Integration (manual):

- Open a library paper, ask a question, get a grounded answer; restart the app, transcript is still there.
- Ask a follow-up that depends on the first answer — history reaches the model.
- Context summary line shows plausible char count; open a very long paper and verify the truncation marker appears.
- Remove `OPENROUTER_API_KEY` from `.env`, send — friendly inline error, app does not crash, key restored chat works again without restart.
- Open a transient discovery candidate — input disabled with the save-to-vault hint.
- Clear chat, restart, transcript is empty.
- (M2) Tokens stream visibly; killing the network mid-stream yields an inline error and no half-persisted assistant message.
- (M3) Save an answer as a note; it appears in the Notes tab marked as chat-born; deleting it works.

## Follow-Up: Vault-Scope Chat (sketch, not in this RFC)

The pieces this RFC leaves ready:

```text
ChatScope::Vault { vault_id }     new enum variant
chat_messages                     already scope-polymorphic
commands / bridge                 already take ChatScope
```

The genuinely new work is the context builder: a vault of N papers cannot inline N full texts. The likely v1 there is per-paper context entries (title + abstract + the user's notes) under a shared budget, which is also where the durable-artifact thesis starts paying rent — saved notes and chat-born insights become the compressed representation of papers the user has processed. That deserves its own RFC once paper-scope chat is proven.

## Open Questions

- **Model default.** The RFC defaults to `anthropic/claude-sonnet-4.5` as a quality/cost balance. Fine to swap for any OpenRouter model id in config; is there a preferred default?
- **Selected-text scope.** `slc.md` lists "selected text" as a context scope. This RFC sends the whole paper regardless. Adding a "quote selection into the message" affordance (selection appended to the user message as a quoted block) is cheap and may be worth pulling into M1 — but it grows the Reader-selection plumbing, so it is left out pending discussion.
- **Where does the env-helper move land?** `src-tauri/src/shared/env.rs` is proposed; `src-tauri/src/config/` would also be reasonable if a broader config module is coming.
- **Transcript length cap.** Should the store cap messages per scope (e.g. keep last 200) or is unbounded fine until it visibly hurts? Proposal: unbounded, revisit with data.
