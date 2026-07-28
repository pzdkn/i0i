# Agent-Authored Highlights (RFC 0059, Phase 2) — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax.

**Goal:** Let the chat agent mark passages in the paper while it answers — via LLM tool-calls that become client-resolved, author-tagged highlights.

**Architecture:** Add OpenAI-compatible `tools` to the chat request; assemble streamed `tool_calls` in the decoder; the chat service streams prose deltas AND emits a `HighlightIntent` per assembled tool call over the existing `Channel<ChatStreamEvent>`; the reader resolves each intent's quote → `Locator` (HTML: text search; PDF: PDF.js text-layer) and calls a new `create_agent_highlight` command (author = Agent). Accept/undo operates on the turn's agent-authored rows.

**Tech Stack:** Rust (reqwest streaming, serde), Tauri `ipc::Channel`, Svelte 5 runes, PDF.js text layer.

## Global Constraints

- Builds on merged Phase 1 (RFC 0058). Reuse: `HighlightService::create_highlight(paper_id, Locator, excerpt, HighlightColor, label, HighlightAuthor)`, `domain::highlight::{HighlightColor, Locator, HighlightAuthor}`, the frontend `$lib/domain/highlight` types + `highlightFill`.
- Author-tagging is server-side and trustworthy: agent highlights are created with `HighlightAuthor::Agent { model }` by a dedicated command; the existing `create_highlight` command (User) is untouched. The agent may create/note only; it never touches user highlights.
- Named color palette (closed enum, verbatim): yellow, green, blue, red, purple, orange. The tool schema exposes exactly these.
- A single assistant message may contain BOTH `content` and `tool_calls`; highlight tools are fire-and-forget (NO tool-result round-trip / no second model call).
- Streamed `tool_calls` arrive fragmented: each SSE `delta.tool_calls[]` carries an `index`, optionally `id`/`function.name`, and a `function.arguments` string FRAGMENT. Assemble by `index` across chunks; parse arguments JSON only once fully assembled.
- Resolution is client-side for BOTH surfaces (RFC 0059 §2). The backend forwards intents; it never resolves quotes.
- Unresolved quotes are reported to the user ("couldn't locate N passages"), never silently dropped, never mis-placed.
- Backend tests: `cd src-tauri && cargo test --no-default-features --features embeddings`. Frontend gate: `pnpm check`.
- Do NOT commit per task (controller reviews working-tree diffs; user commits at checkpoints). Leave the tree clean per task.

---

## SLICE 1 — Backend tool-calling infrastructure (`services/llm.rs`)

### Task 1: Tool schema types + `tools` on `CompletionRequest`

**Files:** Modify `src-tauri/src/services/llm.rs`

**Interfaces produced:**
- `pub(crate) struct Tool` (serde: `{ "type": "function", "function": { name, description, parameters } }`), a builder `highlight_tools() -> Vec<Tool>` returning the two tools (`highlight`, `note`) with a `color` enum constrained to the six palette names.
- `CompletionRequest` gains `#[serde(skip_serializing_if = "Option::is_none")] pub tools: Option<Vec<Tool>>` and `pub tool_choice: Option<String>`.

- [ ] **Step 1 — failing test** (in llm.rs tests): assert `serde_json::to_value(&CompletionRequest{ tools: Some(highlight_tools()), .. })` contains `tools[0].function.name == "highlight"`, that the `color` param has `enum` = the six names, and that a request with `tools: None` serializes WITHOUT a `tools` key.
- [ ] **Step 2 — run, expect FAIL** (`cargo test ... llm::`).
- [ ] **Step 3 — implement.** Add:

```rust
#[derive(Debug, Clone, Serialize)]
pub(crate) struct Tool {
    #[serde(rename = "type")]
    pub kind: String, // "function"
    pub function: ToolFunction,
}
#[derive(Debug, Clone, Serialize)]
pub(crate) struct ToolFunction {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value, // JSON Schema
}

pub(crate) fn highlight_tools() -> Vec<Tool> {
    let colors = serde_json::json!(["yellow","green","blue","red","purple","orange"]);
    vec![
        Tool { kind: "function".into(), function: ToolFunction {
            name: "highlight".into(),
            description: "Highlight a verbatim passage from the paper in a color. Quote the exact text.".into(),
            parameters: serde_json::json!({
                "type":"object",
                "properties":{
                    "quote":{"type":"string","description":"Exact verbatim text from the paper to highlight."},
                    "color":{"type":"string","enum":colors},
                    "label":{"type":"string","description":"Optional short category, e.g. 'experimental result'."}
                },
                "required":["quote","color"]
            }),
        }},
        Tool { kind: "function".into(), function: ToolFunction {
            name: "note".into(),
            description: "Highlight a verbatim passage and attach a short note to it.".into(),
            parameters: serde_json::json!({
                "type":"object",
                "properties":{
                    "quote":{"type":"string"},
                    "body":{"type":"string","description":"The note text."},
                    "color":{"type":"string","enum":colors}
                },
                "required":["quote","body"]
            }),
        }},
    ]
}
```

Add to `CompletionRequest`: `#[serde(skip_serializing_if="Option::is_none")] pub tools: Option<Vec<Tool>>,` and `#[serde(skip_serializing_if="Option::is_none")] pub tool_choice: Option<String>,`. Update EVERY existing `CompletionRequest { .. }` literal (grep `CompletionRequest {` — there are ~4 in service.rs, plus planner/query_expansion) to add `tools: None, tool_choice: None,`.

- [ ] **Step 4 — run, expect PASS** + full suite (`cargo test ...`) so the added-fields don't break existing request literals.
- [ ] **Step 5 — clean tree for review.**

### Task 2: Assemble streamed `tool_calls` in the decoder

**Files:** Modify `src-tauri/src/services/llm.rs` (the streaming SSE decoder + `StreamDelta`)

**Interfaces produced:**
- `pub(crate) struct AssembledToolCall { pub name: String, pub arguments: String }` (arguments = the fully-assembled JSON string).
- A decoder that, in addition to content, accumulates tool-call fragments by index and can yield the completed `Vec<AssembledToolCall>` at stream end.

- [ ] **Step 1 — failing tests** (llm.rs tests): feed the decoder a sequence of SSE lines representing ONE tool call split across chunks — e.g. chunk A: `{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"c1","function":{"name":"highlight","arguments":"{\"quote\":\"a"}}]}}]}`, chunk B: `{"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"bc\",\"color\":\"red\"}"}}]}}]}` — and assert the decoder yields one `AssembledToolCall { name:"highlight", arguments:"{\"quote\":\"abc\",\"color\":\"red\"}" }`. Add a second test with TWO interleaved tool calls (index 0 and 1) asserting both assemble independently. Add a test that plain content deltas still assemble as before (regression).
- [ ] **Step 2 — run, expect FAIL.**
- [ ] **Step 3 — implement.** Extend the `StreamDelta` deserialization to include optional `tool_calls: Option<Vec<ToolCallDelta>>` where `ToolCallDelta { index: usize, id: Option<String>, function: Option<FunctionDelta{ name: Option<String>, arguments: Option<String> }> }`. In the decoder state, keep a `HashMap<usize, (String name, String args)>`; on each tool_call delta, set name when present and append the arguments fragment. Expose `fn finish(&mut self) -> Vec<AssembledToolCall>` (ordered by index) that the streaming loop calls at `[DONE]`/stream end. Keep content-delta behavior unchanged.
- [ ] **Step 4 — run, expect PASS** + full suite.
- [ ] **Step 5 — clean tree.**

### Task 3: `complete_streamed` returns assembled tool calls

**Files:** Modify `src-tauri/src/services/llm.rs`; update callers in `services/chat/service.rs`

**Interfaces produced:**
- Change `complete_streamed` to return `Result<StreamOutcome, String>` where `pub(crate) struct StreamOutcome { pub text: String, pub tool_calls: Vec<AssembledToolCall> }` (instead of `Result<String, String>`). Content deltas still fire `on_delta` live.

- [ ] **Step 1 — failing test:** a fake SSE body mixing content deltas and one tool-call (using the existing test transport pattern) → assert `outcome.text` is the concatenated content AND `outcome.tool_calls.len() == 1`.
- [ ] **Step 2 — run, expect FAIL.**
- [ ] **Step 3 — implement:** thread the decoder's `finish()` into the return value; wrap old text return in `StreamOutcome`. Update the two callers in `service.rs` (`ask_in_thread_streamed`, `ask_at_anchor_streamed`) to use `outcome.text` (they ignore `tool_calls` for now — Task 5 consumes them).
- [ ] **Step 4 — run, expect PASS** + full suite.
- [ ] **Step 5 — clean tree.**

---

## SLICE 2 — Chat turn emits intents + agent-create command

### Task 4: `HighlightIntent` event + `create_agent_highlight` command

**Files:** Modify `src-tauri/src/domain/chat.rs` (event); create `src-tauri/src/commands/highlight.rs` additions (agent command); modify `src-tauri/src/lib.rs` (register).

**Interfaces produced:**
- `ChatStreamEvent::HighlightIntent { quote: String, color: String, label: Option<String>, note: Option<String> }` (serde rename `highlightIntent`).
- Command `create_agent_highlight(service, paper_id, locator: Locator, excerpt, color: HighlightColor, label: Option<String>, model: String, thread_id: Option<String>) -> Result<Highlight, String>` → `HighlightService.create_highlight(.., HighlightAuthor::Agent { model })` (and, if `thread_id` present, links via the existing thread mechanism — else no thread).

- [ ] **Step 1 — failing test** (services/highlight test): `create` with `HighlightAuthor::Agent{model:"m"}` yields a row whose `author` is Agent — already covered by Phase 1's `create_is_author_agnostic`; ADD a command-level shape check is unnecessary. Instead test that the new `list_agent_highlights(paper)` (add it) returns only Agent-authored rows. Write that failing test.
- [ ] **Step 2 — FAIL.**
- [ ] **Step 3 — implement:** add the `HighlightIntent` variant; add `create_agent_highlight` command + `list_agent_highlights` (store: `list_highlights` filtered by `author_kind='agent'`) + register both in `lib.rs generate_handler!`. Frontend bridge additions go in Task 8/9.
- [ ] **Step 4 — PASS** + build clean.
- [ ] **Step 5 — clean tree.**

### Task 5: Agent-enabled ask path emits intents

**Files:** Modify `src-tauri/src/services/chat/service.rs` + `src-tauri/src/commands/chat.rs`

**Interfaces produced:**
- A new streamed method (or a `tools: bool` param on the existing anchor-ask) that sets `request.tools = Some(highlight_tools())`, streams content, and after the stream maps each `AssembledToolCall` → a parsed intent, invoking a callback `on_intent(HighlightIntentPayload)`.
- Command `ask_at_anchor_streamed` (or a new `ask_agent_streamed`) forwards intents as `ChatStreamEvent::HighlightIntent` over the channel, before `Done`.

- [ ] **Step 1 — failing test:** with a fake transport returning a tool-call for `highlight(quote,color)`, the service invokes `on_intent` once with the parsed quote+color. (Parse: `serde_json::from_str::<HighlightArgs>(&tc.arguments)`, tolerate a missing/invalid color by skipping + counting unresolved.)
- [ ] **Step 2 — FAIL.**
- [ ] **Step 3 — implement:** define `HighlightArgs { quote, color: Option<String>, label: Option<String>, body: Option<String> }`; parse each assembled tool call; map `note` tool → intent with `note` set. Enforce a per-turn cap (e.g. 25). Send intents over the channel in the command.
- [ ] **Step 4 — PASS** + full suite.
- [ ] **Step 5 — clean tree.**

---

## SLICE 3 — Client resolvers + wiring

### Task 6: HTML quote → `TextOffset` resolver (pure util + tests)

**Files:** Create `src/lib/features/reader/resolve-quote-html.ts`

**Interfaces produced:** `resolveQuoteInText(fullText: string, quote: string): { start: number; end: number } | null` — exact match first, then whitespace-normalized fuzzy match mapping back to original offsets.

- [ ] **Step 1 — failing tests** (vitest if present, else a `.test.ts` the repo runs; check how frontend tests run — `grep -r vitest package.json`). Cases: exact substring; quote with collapsed/extra whitespace vs source; not-found → null; multi-line quote.
- [ ] **Step 2 — FAIL.** **Step 3 — implement** normalize-and-map. **Step 4 — PASS.** **Step 5 — clean tree.**

### Task 7: PDF quote → `PdfRect` resolver (PDF.js text layer)

**Files:** Create `src/lib/features/reader/resolve-quote-pdf.ts`; wire into the PDF reader components.

**Interfaces produced:** `resolveQuoteInPdf(page, quote): { pageIndex, rectsJson } | null` — concatenate the page's text-layer items into a string with an index map back to items, find the quote span (exact then whitespace-normalized), union the covering items' rects.

- [ ] **Step 1:** unit-test the string-assembly + span-finding half with a synthetic text-item array (item text + rects); assert the covering items are selected for a multi-item quote and rects unioned; not-found → null.
- [ ] **Steps 2-5** as standard. (Rendering/geometry integration is verified via `pnpm check` + manual; the matcher logic is unit-tested.)

### Task 8: Consume intents in the ask flow → resolve → create; unresolved reporting

**Files:** Modify `src/lib/features/reader/ReaderInspector.svelte` (the streamed ask handler) + `ReaderView.svelte`; add bridge fns in `src/lib/bridge/highlight.ts` (`createAgentHighlight`, `listAgentHighlights`).

**Interfaces consumed:** the `HighlightIntent` channel event; `resolveQuoteInText`/`resolveQuoteInPdf`; `create_agent_highlight`.

- [ ] Handle `HighlightIntent` events in the streamed-ask `on_event` handler: for the active source, resolve the quote (HTML via rendered text; PDF via the page text layers), and on success call `createAgentHighlight({ paperId, locator, excerpt: quote, color, label, model, threadId })`, then reload highlights. Track unresolved count and surface "couldn't locate N passages" in the thread. `pnpm check` 0/0. Manual smoke: "mark the key claims in yellow" on an HTML article marks passages.

---

## SLICE 4 — Accept/undo + agent styling

### Task 9: Agent mark styling + batch Keep/Undo

**Files:** `HighlightPopover.svelte`, `HtmlReader.svelte`/PDF mark render (author badge), `ReaderInspector.svelte` (turn-level Keep all / Undo all), bridge `removeHighlight` (exists).

- [ ] Render agent-authored highlights with a subtle author badge (mark + rail row). After an agent turn that created marks, show **Keep all · Undo all**; Undo removes the highlight ids created in that turn (collect the ids returned by `createAgentHighlight`); Keep dismisses the affordance. `pnpm check` 0/0.

### Task 10: End-to-end verification + docs

- [ ] Run the full backend suite + `pnpm check`. Manually verify the three RFC examples on an HTML article and a PDF: "mark in yellow" (no prose), "mark experimental results in red" (label), "answer and mark" (prose + marks linked to the answer). Update RFC 0059 Status to "Implemented". Clean tree.

---

## Self-Review Notes
- **Spec coverage:** tools (T1), streamed tool-call assembly (T2-T3), intents over the channel (T4-T5), client resolvers HTML+PDF (T6-T7), wiring + unresolved reporting (T8), accept/undo + agent styling (T9), the three worked examples (T10). Author-tagging server-side via `create_agent_highlight` (T4).
- **Riskiest tasks:** T2 (streamed tool-call fragment assembly) and T7 (PDF text-layer matching) — both isolated behind unit-tested pure functions so the risk is contained and testable without a live model or a rendered PDF.
- **Deferred (RFC non-goals):** agent editing user highlights; cross-paper marking; adversarial-PDF perfection.
- **Grounding note:** frontend test runner — before T6, `grep -r "vitest\|\"test\"" package.json` to confirm how `.test.ts` runs; if there is no JS test runner, implement the resolver logic with an inline `if (import.meta.vitest)` block or a tiny node script the plan's verify step runs, and note the substitution.
