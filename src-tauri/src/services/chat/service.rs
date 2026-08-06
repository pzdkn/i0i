//! Chat orchestration over anchored threads (RFC 0033).
//!
//! A thread is a timeline of entries (notes + AI questions/answers) attached to
//! an anchor. Asking replays the thread's entries behind one system message
//! that carries the paper text (anchor foregrounded), so the paper text is
//! inlined once per turn rather than per entry. A turn is persisted only after
//! the reply succeeds, so a failed request leaves no orphaned entries.

use std::time::Duration;

use reqwest::Client;
use serde::Deserialize;
use tauri::{AppHandle, Emitter};

use super::config::ChatConfig;
use super::context::build_context;
use crate::domain::chat::{
    ChatContextSummary, ChatEntry, ChatEntryDraft, ChatScope, ChatThreadSummary, ChatThreadUpdated,
    ChatThreadView, PinnedHighlight, ThreadAnchor, ENTRY_ANSWER,
};
use crate::services::llm::{self as openrouter, CompletionRequest, WireMessage};
use crate::services::reader_service::ReaderService;
use crate::storage::library_store::LibraryStore;

#[derive(Clone)]
pub struct ChatService {
    app: AppHandle,
    client: Client,
    config: ChatConfig,
    store: LibraryStore,
    reader: ReaderService,
}

impl ChatService {
    pub fn new(
        app: AppHandle,
        config: ChatConfig,
        store: LibraryStore,
        reader: ReaderService,
    ) -> Self {
        let client = Client::builder()
            .user_agent(concat!(
                env!("CARGO_PKG_NAME"),
                "/",
                env!("CARGO_PKG_VERSION"),
                " chat"
            ))
            .build()
            .expect("reqwest client should build");
        Self {
            app,
            client,
            config,
            store,
            reader,
        }
    }

    pub fn from_app_config(
        app: AppHandle,
        store: LibraryStore,
        reader: ReaderService,
    ) -> Result<Self, String> {
        Ok(Self::new(app, ChatConfig::load()?, store, reader))
    }

    /// List a scope's threads with entry/pin counts (newest activity first).
    pub async fn list_threads(&self, scope: &ChatScope) -> Result<Vec<ChatThreadSummary>, String> {
        self.store.list_chat_threads(scope.kind(), scope.id())
    }

    /// Load a thread with its entries.
    pub async fn get_thread(&self, thread_id: &str) -> Result<ChatThreadView, String> {
        self.store.get_chat_thread(thread_id)
    }

    /// Add a self-authored note at an anchor, creating its thread lazily, and
    /// return the thread (RFC 0034). A `document` anchor reuses the whole-paper
    /// thread; a selection anchor starts a new one. Nothing is written for an
    /// empty note.
    pub async fn add_note_at_anchor(
        &self,
        scope: &ChatScope,
        anchor: ThreadAnchor,
        body: String,
    ) -> Result<ChatThreadView, String> {
        let body = body.trim();
        if body.is_empty() {
            return Err("Enter a note before saving.".to_string());
        }
        let default_title = anchor.default_title();
        let selected_text = anchor.selected_text().map(ToString::to_string);
        let write =
            self.store
                .add_note_at_anchor_with_creation(scope.kind(), scope.id(), &anchor, body)?;
        self.spawn_title_generation_if_created(
            write.created,
            write.view.thread.id.clone(),
            default_title,
            body.to_string(),
            selected_text,
        );
        Ok(write.view)
    }

    /// Append a self-authored note (pinned by default) and return the thread.
    pub async fn add_note(&self, thread_id: &str, body: String) -> Result<ChatThreadView, String> {
        let body = body.trim().to_string();
        if body.is_empty() {
            return Err("Enter a note before saving.".to_string());
        }
        self.store
            .append_chat_entry(thread_id, &ChatEntryDraft::note(body))?;
        self.store.get_chat_thread(thread_id)
    }

    /// Pin or unpin an entry.
    pub async fn set_entry_pinned(&self, entry_id: &str, pinned: bool) -> Result<(), String> {
        self.store.set_chat_entry_pinned(entry_id, pinned)
    }

    /// List a scope's pinned entries (the Highlights view).
    pub async fn list_pinned(&self, scope: &ChatScope) -> Result<Vec<PinnedHighlight>, String> {
        self.store
            .list_pinned_chat_entries(scope.kind(), scope.id())
    }

    /// Rename a thread.
    pub async fn rename_thread(&self, thread_id: &str, title: String) -> Result<(), String> {
        self.store.rename_chat_thread(thread_id, &title)
    }

    /// Delete a thread and its entries.
    pub async fn delete_thread(&self, thread_id: &str) -> Result<(), String> {
        self.store.delete_chat_thread(thread_id)
    }

    /// Ask in a thread (non-streaming) and return the updated thread.
    pub async fn ask_in_thread(
        &self,
        thread_id: &str,
        body: String,
    ) -> Result<ChatThreadView, String> {
        let prep = self.prepare_ask(thread_id, body).await?;
        let request = CompletionRequest {
            model: self.config.model.clone(),
            messages: prep.request_messages,
            stream: false,
            max_tokens: None,
            response_format: None,
            tools: None,
            tool_choice: None,
        };
        let answer =
            openrouter::complete(&self.client, &self.config.url, &prep.api_key, &request).await?;
        self.finalize_ask(thread_id, &prep.user_body, answer, prep.summary)
            .await
    }

    /// Ask in a thread, streaming reply deltas, and return the updated thread.
    pub async fn ask_in_thread_streamed<F>(
        &self,
        thread_id: &str,
        body: String,
        on_delta: F,
    ) -> Result<ChatThreadView, String>
    where
        F: FnMut(String),
    {
        let prep = self.prepare_ask(thread_id, body).await?;
        let request = CompletionRequest {
            model: self.config.model.clone(),
            messages: prep.request_messages,
            stream: true,
            max_tokens: None,
            response_format: None,
            tools: None,
            tool_choice: None,
        };
        let outcome = openrouter::complete_streamed(
            &self.client,
            &self.config.url,
            &prep.api_key,
            &request,
            on_delta,
        )
        .await?;
        self.finalize_ask(thread_id, &prep.user_body, outcome.text, prep.summary)
            .await
    }

    /// Ask at an anchor, creating the thread lazily on success (RFC 0034).
    ///
    /// Nothing is persisted until the reply arrives, so a failed ask leaves no
    /// thread. The passage (for a selection anchor) is foregrounded in the
    /// prompt, just as for a thread-scoped ask.
    ///
    /// Prose only — agent marking is a separate fast-model pass
    /// (`annotate_streamed`), so the answer streams clean and quick and is
    /// never slowed by tool-call generation (RFC 0059 follow-up).
    pub async fn ask_at_anchor_streamed<F>(
        &self,
        scope: &ChatScope,
        anchor: ThreadAnchor,
        body: String,
        on_delta: F,
    ) -> Result<ChatThreadView, String>
    where
        F: FnMut(String),
    {
        let prep = self.prepare_ask_at_anchor(scope, &anchor, body).await?;
        let request = CompletionRequest {
            model: self.config.model.clone(),
            messages: prep.request_messages,
            stream: true,
            max_tokens: None,
            response_format: None,
            tools: None,
            tool_choice: None,
        };
        let outcome = openrouter::complete_streamed(
            &self.client,
            &self.config.url,
            &prep.api_key,
            &request,
            on_delta,
        )
        .await?;

        let answer_text = outcome.text;

        let default_title = anchor.default_title();
        let selected_text = anchor.selected_text().map(ToString::to_string);
        let write = self.store.persist_anchored_turn_with_creation(
            scope.kind(),
            scope.id(),
            &anchor,
            &ChatEntryDraft::question(prep.user_body.clone()),
            &ChatEntryDraft::answer(answer_text, self.config.model.clone(), prep.summary),
        )?;
        self.spawn_title_generation_if_created(
            write.created,
            write.view.thread.id.clone(),
            default_title,
            prep.user_body,
            selected_text,
        );
        Ok(write.view)
    }

    /// Fast-model annotation pass (RFC 0059 follow-up): reuses the ask's paper
    /// context but runs the cheap `annotation_model` with ONLY the highlight
    /// tools, steered to mark rather than answer. Persists nothing — each parsed
    /// intent is handed to `on_intent`; the client resolves the quote and
    /// creates the agent highlight. Runs independently of the answer so the chat
    /// is never blocked behind marking. Capped at `MAX_HIGHLIGHT_INTENTS_PER_TURN`.
    pub async fn annotate_streamed<I>(
        &self,
        scope: &ChatScope,
        anchor: ThreadAnchor,
        body: String,
        mut on_intent: I,
    ) -> Result<(), String>
    where
        I: FnMut(HighlightIntentData),
    {
        let prep = self.prepare_ask_at_anchor(scope, &anchor, body).await?;
        let mut messages = Vec::with_capacity(prep.request_messages.len() + 1);
        messages.push(WireMessage {
            role: "system".to_string(),
            content: "You mark passages in a paper by calling the `highlight` tool. \
                      For each passage the user wants marked, quote a SHORT phrase — a \
                      single sentence or less — copied EXACTLY and VERBATIM from the \
                      paper text provided, character for character (do not paraphrase, \
                      shorten, or fix typos). Prefer a distinctive short span over a long \
                      one. Call `highlight` once per passage, choosing a fitting color. \
                      Do not answer in prose; only call the tool."
                .to_string(),
        });
        messages.extend(prep.request_messages);
        let request = CompletionRequest {
            model: self.config.annotation_model.clone(),
            messages,
            stream: true,
            max_tokens: None,
            response_format: None,
            tools: Some(openrouter::highlight_tools()),
            tool_choice: Some("auto".to_string()),
        };
        let outcome = openrouter::complete_streamed(
            &self.client,
            &self.config.url,
            &prep.api_key,
            &request,
            |_text| {}, // annotation yields tool calls, not prose
        )
        .await?;

        // Show exactly what the annotation model returned, so a "no marks"
        // outcome can be told apart — no tool calls vs. a missed assembly.
        crate::shared::log::debug(
            "annotate",
            format!(
                "model={} text_len={} tool_calls={}",
                self.config.annotation_model,
                outcome.text.len(),
                outcome.tool_calls.len(),
            ),
        );
        for tool_call in &outcome.tool_calls {
            let args: String = tool_call.arguments.chars().take(200).collect();
            crate::shared::log::debug(
                "annotate",
                format!("tool_call name={} args={args}", tool_call.name),
            );
        }

        let mut delivered = 0usize;
        for tool_call in &outcome.tool_calls {
            if delivered >= MAX_HIGHLIGHT_INTENTS_PER_TURN {
                break;
            }
            match intent_from_tool_call(&tool_call.name, &tool_call.arguments) {
                Some(intent) => {
                    on_intent(intent);
                    delivered += 1;
                }
                None => crate::shared::log::warn(
                    "annotate",
                    format!("tool_call '{}' did not parse into an intent", tool_call.name),
                ),
            }
        }
        crate::shared::log::info("annotate", format!("delivered {delivered} intent(s)"));
        Ok(())
    }

    /// Explicit AI auto-highlight (RFC 0064): a *command*, not a conversation.
    /// Given a set of categories (each with a color), ask the fast annotation
    /// model — with a strict `json_schema` structured output — for one parseable
    /// list of verbatim passages to mark. No streaming, no tool-call assembly,
    /// no prose. The caller resolves each quote and creates the AI highlights.
    pub async fn auto_highlight(
        &self,
        scope: &ChatScope,
        categories: Vec<AutoHighlightCategory>,
    ) -> Result<Vec<HighlightIntentData>, String> {
        if categories.is_empty() {
            return Err("Choose at least one category to highlight.".to_string());
        }
        let mut lines = String::new();
        for category in &categories {
            let label = category.label.trim();
            if label.is_empty() {
                continue;
            }
            let color = normalize_highlight_color(Some(&category.color));
            lines.push_str(&format!("- {label} (color: {color})"));
            if let Some(prompt) = category
                .prompt
                .as_deref()
                .map(str::trim)
                .filter(|prompt| !prompt.is_empty())
            {
                lines.push_str(&format!(": {prompt}"));
            }
            lines.push('\n');
        }
        if lines.is_empty() {
            return Err("Choose at least one category to highlight.".to_string());
        }

        let body =
            format!("Mark passages in this paper for these categories, using the given color for each:\n{lines}");
        let prep = self
            .prepare_ask_at_anchor(scope, &ThreadAnchor::Document, body)
            .await?;
        // RFC 0072: an unextracted PDF sends an empty paper body, so the model
        // can only ever return an empty list. Say why, instead of paying for a
        // call that cannot succeed. Deliberately not applied to the ask path —
        // an answer from title + metadata is degraded but not worthless, and the
        // inspector already labels that turn "Context: title + metadata only".
        if prep.summary.included_chars == 0 {
            return Err("This document has no extracted text yet — marking can't run until \
                        extraction finishes."
                .to_string());
        }
        let mut messages = Vec::with_capacity(prep.request_messages.len() + 1);
        messages.push(WireMessage {
            role: "system".to_string(),
            content: "You mark passages in a paper. Return ONLY a JSON object matching the schema \
                      { \"highlights\": [ { \"quote\", \"color\", \"label\" } ] }. Each `quote` must \
                      be a SHORT phrase — a single sentence or less — copied EXACTLY and VERBATIM \
                      from the paper text provided (character for character; do not paraphrase, \
                      shorten, or fix typos). Use the requested color for each category and set \
                      `label` to the category name. Prefer a distinctive short span. If nothing \
                      matches, return an empty list."
                .to_string(),
        });
        messages.extend(prep.request_messages);
        let request = CompletionRequest {
            model: self.config.annotation_model.clone(),
            messages,
            stream: false,
            max_tokens: None,
            response_format: Some(openrouter::ResponseFormat::json_schema(
                "highlights",
                auto_highlight_schema(),
            )),
            tools: None,
            tool_choice: None,
        };
        let text =
            openrouter::complete(&self.client, &self.config.url, &prep.api_key, &request).await?;
        let mut intents = parse_auto_highlights(&text)?;
        intents.truncate(MAX_HIGHLIGHT_INTENTS_PER_TURN);
        crate::shared::log::info(
            "auto_highlight",
            format!("parsed {} passage(s) from structured output", intents.len()),
        );
        Ok(intents)
    }

    /// Assemble the prompt for a brand-new anchored ask — persisting nothing.
    ///
    /// A new anchored thread has no prior entries; whole-paper continuity is
    /// handled by the thread-scoped ask path once the thread exists.
    async fn prepare_ask_at_anchor(
        &self,
        scope: &ChatScope,
        anchor: &ThreadAnchor,
        body: String,
    ) -> Result<PreparedAsk, String> {
        let user_body = body.trim().to_string();
        if user_body.is_empty() {
            return Err("Enter a message before sending.".to_string());
        }
        if scope.kind() != "paper" {
            return Err(format!("Unsupported chat scope: {}", scope.kind()));
        }
        let api_key = self.config.resolve_api_key()?;
        let document = self.reader.get_reader_document(scope.id(), None).await?;
        let bundle = build_context(
            &document.title,
            &document.authors,
            &document.venue,
            document.year,
            &document.source_text,
            self.config.max_context_chars,
            anchor.selected_text(),
        );
        Ok(PreparedAsk {
            api_key,
            request_messages: build_wire_messages(bundle.system_prompt, &[], &user_body),
            summary: bundle.summary,
            user_body,
        })
    }

    /// Validate, resolve the key, and assemble the prompt — persisting nothing.
    async fn prepare_ask(&self, thread_id: &str, body: String) -> Result<PreparedAsk, String> {
        let user_body = body.trim().to_string();
        if user_body.is_empty() {
            return Err("Enter a message before sending.".to_string());
        }
        let api_key = self.config.resolve_api_key()?;

        let view = self.store.get_chat_thread(thread_id)?;
        let (scope_kind, scope_id) = self.store.chat_thread_scope(thread_id)?;
        if scope_kind != "paper" {
            return Err(format!("Unsupported chat scope: {scope_kind}"));
        }
        let document = self.reader.get_reader_document(&scope_id, None).await?;

        let bundle = build_context(
            &document.title,
            &document.authors,
            &document.venue,
            document.year,
            &document.source_text,
            self.config.max_context_chars,
            view.thread.anchor.selected_text(),
        );

        Ok(PreparedAsk {
            api_key,
            request_messages: build_wire_messages(bundle.system_prompt, &view.entries, &user_body),
            summary: bundle.summary,
            user_body,
        })
    }

    /// Persist the completed turn (question then answer) and return the thread.
    async fn finalize_ask(
        &self,
        thread_id: &str,
        user_body: &str,
        answer: String,
        summary: ChatContextSummary,
    ) -> Result<ChatThreadView, String> {
        self.store
            .append_chat_entry(thread_id, &ChatEntryDraft::question(user_body.to_string()))?;
        self.store.append_chat_entry(
            thread_id,
            &ChatEntryDraft::answer(answer, self.config.model.clone(), summary),
        )?;
        self.store.get_chat_thread(thread_id)
    }

    fn spawn_title_generation_if_created(
        &self,
        created: bool,
        thread_id: String,
        default_title: String,
        first_entry_body: String,
        selected_text: Option<String>,
    ) {
        if !created || self.config.title_model.is_none() {
            return;
        }

        let service = self.clone();
        tauri::async_runtime::spawn(async move {
            if let Err(error) = service
                .generate_and_apply_thread_title(
                    thread_id,
                    default_title,
                    first_entry_body,
                    selected_text,
                )
                .await
            {
                chat_title_log(format!("title generation skipped/failed: {error}"));
            }
        });
    }

    async fn generate_and_apply_thread_title(
        &self,
        thread_id: String,
        default_title: String,
        first_entry_body: String,
        selected_text: Option<String>,
    ) -> Result<(), String> {
        let Some(model) = self.config.title_model.clone() else {
            return Ok(());
        };
        let api_key = self.config.resolve_api_key()?;
        let request = CompletionRequest {
            model,
            messages: build_title_messages(&first_entry_body, selected_text.as_deref()),
            stream: false,
            max_tokens: Some(self.config.title_max_tokens),
            response_format: None,
            tools: None,
            tool_choice: None,
        };

        let raw_title = tokio::time::timeout(
            Duration::from_millis(self.config.title_timeout_ms),
            openrouter::complete(&self.client, &self.config.url, &api_key, &request),
        )
        .await
        .map_err(|_| {
            format!(
                "Thread title generation timed out after {} ms",
                self.config.title_timeout_ms
            )
        })??;
        let title = clean_generated_title(&raw_title)
            .ok_or_else(|| "Generated title was empty".to_string())?;

        if let Some(thread) =
            self.store
                .rename_chat_thread_if_title_is(&thread_id, &default_title, &title)?
        {
            let _ = self.app.emit(
                "chat_thread_updated",
                ChatThreadUpdated {
                    thread_id: thread.id,
                    title: thread.title,
                },
            );
        }
        Ok(())
    }
}

/// Per-turn cap on how many highlight intents a single ask will deliver,
/// guarding against a model that calls the highlight/note tools excessively.
const MAX_HIGHLIGHT_INTENTS_PER_TURN: usize = 25;

/// A parsed, ready-to-emit highlight intent (RFC 0059 Phase 2): a quote to
/// highlight, its color, and optionally a label (from `highlight`) or a note
/// body (from `note`).
#[derive(Debug, Clone, PartialEq)]
pub struct HighlightIntentData {
    pub quote: String,
    pub color: String,
    pub label: Option<String>,
    pub note: Option<String>,
}

/// One category the user chose in the auto-highlight lens menu (RFC 0064): a
/// label, its color, and an optional free-text instruction (the "Custom…" row).
#[derive(Debug, Clone, Deserialize)]
pub struct AutoHighlightCategory {
    pub label: String,
    pub color: String,
    pub prompt: Option<String>,
}

/// Strict `json_schema` for the auto-highlight structured output (RFC 0064).
fn auto_highlight_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "highlights": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "quote": {"type": "string"},
                        "color": {"type": "string", "enum": HIGHLIGHT_COLOR_PALETTE},
                        "label": {"type": "string"}
                    },
                    "required": ["quote", "color"]
                }
            }
        },
        "required": ["highlights"]
    })
}

#[derive(Debug, Deserialize)]
struct AutoHighlightResponse {
    highlights: Vec<AutoHighlightItem>,
}

#[derive(Debug, Deserialize)]
struct AutoHighlightItem {
    quote: String,
    color: Option<String>,
    label: Option<String>,
}

/// Parse the auto-highlight structured output into ready-to-resolve intents
/// (RFC 0064). Malformed JSON is an error (so the UI can say "couldn't read the
/// AI's response" rather than silently marking nothing); empty-quote items are
/// dropped; a missing/off-palette color normalizes to yellow.
fn parse_auto_highlights(json: &str) -> Result<Vec<HighlightIntentData>, String> {
    let parsed: AutoHighlightResponse = serde_json::from_str(json.trim())
        .map_err(|error| format!("Couldn't read the AI's highlight list: {error}"))?;
    Ok(parsed
        .highlights
        .into_iter()
        .filter_map(|item| {
            let quote = item.quote.trim().to_string();
            if quote.is_empty() {
                return None;
            }
            Some(HighlightIntentData {
                color: normalize_highlight_color(item.color.as_deref()),
                quote,
                label: item.label.filter(|label| !label.trim().is_empty()),
                note: None,
            })
        })
        .collect())
}

/// The JSON arguments shape for both the `highlight` and `note` tools.
#[derive(Debug, Deserialize)]
struct HighlightArgs {
    quote: String,
    color: Option<String>,
    label: Option<String>,
    body: Option<String>,
}

/// Palette the `highlight`/`note` tools advertise (mirrors `highlight_tools()`
/// in `services/llm.rs`). Any color outside this set — including missing or
/// blank — is normalized to `"yellow"` rather than dropping the intent.
const HIGHLIGHT_COLOR_PALETTE: &[&str] = &["yellow", "green", "blue", "red", "purple", "orange"];
const DEFAULT_HIGHLIGHT_COLOR: &str = "yellow";

/// Normalize a raw color string to a valid palette member, defaulting to
/// `DEFAULT_HIGHLIGHT_COLOR` when missing, blank, or off-palette.
fn normalize_highlight_color(raw: Option<&str>) -> String {
    let candidate = raw.map(str::trim).unwrap_or("").to_lowercase();
    if HIGHLIGHT_COLOR_PALETTE.contains(&candidate.as_str()) {
        candidate
    } else {
        DEFAULT_HIGHLIGHT_COLOR.to_string()
    }
}

/// Parse one assembled tool call into a highlight intent, or `None` if the
/// tool is unrecognized, the arguments don't parse, or the quote is empty.
/// A missing/blank/off-palette color is normalized to `"yellow"` rather than
/// dropping the intent — `color` is required by the `highlight` tool schema
/// but optional for `note`, and a model may emit an off-palette string.
fn intent_from_tool_call(name: &str, arguments: &str) -> Option<HighlightIntentData> {
    let args: HighlightArgs = serde_json::from_str(arguments).ok()?;
    let quote = args.quote.trim();
    if quote.is_empty() {
        return None;
    }
    let color = normalize_highlight_color(args.color.as_deref());

    match name {
        "highlight" => Some(HighlightIntentData {
            quote: quote.to_string(),
            color,
            label: args.label.filter(|label| !label.trim().is_empty()),
            note: None,
        }),
        "note" => Some(HighlightIntentData {
            quote: quote.to_string(),
            color,
            label: None,
            note: args.body.filter(|body| !body.trim().is_empty()),
        }),
        _ => None,
    }
}

/// A prepared ask: the resolved key, the user's message (persisted only on
/// success), the prompt to send, and what the model will have seen.
struct PreparedAsk {
    api_key: String,
    user_body: String,
    request_messages: Vec<WireMessage>,
    summary: ChatContextSummary,
}

/// Assemble the wire messages: one system message, the thread's entries replayed
/// as plain turns (answers as `assistant`, everything else as `user`), then the
/// new question.
fn build_wire_messages(
    system_prompt: String,
    entries: &[ChatEntry],
    new_question: &str,
) -> Vec<WireMessage> {
    let mut messages = Vec::with_capacity(entries.len() + 2);
    messages.push(WireMessage {
        role: "system".to_string(),
        content: system_prompt,
    });
    for entry in entries {
        let role = if entry.kind == ENTRY_ANSWER {
            "assistant"
        } else {
            "user"
        };
        messages.push(WireMessage {
            role: role.to_string(),
            content: entry.body.clone(),
        });
    }
    messages.push(WireMessage {
        role: "user".to_string(),
        content: new_question.to_string(),
    });
    messages
}

fn build_title_messages(first_entry_body: &str, selected_text: Option<&str>) -> Vec<WireMessage> {
    let mut prompt = String::from(
        "Reply with a 3-6 word topic for this research chat thread. \
         Use Title Case. No quotes. No trailing punctuation.\n\n",
    );
    if let Some(selected_text) = selected_text {
        let selected_text = selected_text.trim();
        if !selected_text.is_empty() {
            prompt.push_str("Selected passage:\n");
            prompt.push_str(selected_text);
            prompt.push_str("\n\n");
        }
    }
    prompt.push_str("First user entry:\n");
    prompt.push_str(first_entry_body.trim());

    vec![WireMessage {
        role: "user".to_string(),
        content: prompt,
    }]
}

fn clean_generated_title(raw: &str) -> Option<String> {
    let one_line = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    let trimmed = one_line
        .trim()
        .trim_matches(['"', '\'', '`'])
        .trim_end_matches(['.', '!', '?', ':', ';'])
        .trim();
    if trimmed.is_empty() {
        return None;
    }

    Some(
        trimmed
            .split_whitespace()
            .take(6)
            .collect::<Vec<_>>()
            .join(" "),
    )
}

fn chat_title_log(message: impl AsRef<str>) {
    let timestamp_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0);
    eprintln!("[chat-title {timestamp_ms}] {}", message.as_ref());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(kind: &str, body: &str) -> ChatEntry {
        ChatEntry {
            id: format!("e_{body}"),
            thread_id: "t".to_string(),
            kind: kind.to_string(),
            body: body.to_string(),
            model: None,
            context_summary: None,
            pinned: false,
            created_at: "2026-06-14".to_string(),
        }
    }

    #[test]
    fn wire_messages_lead_with_system_replay_entries_then_new_question() {
        let entries = vec![
            entry("note", "a standalone note"),
            entry("question", "earlier question"),
            entry("answer", "earlier answer"),
        ];

        let wire = build_wire_messages("SYSTEM".to_string(), &entries, "the new question");

        assert_eq!(wire.len(), 5);
        assert_eq!(wire[0].role, "system");
        assert_eq!(wire[0].content, "SYSTEM");
        assert_eq!(wire[1].role, "user"); // note → user
        assert_eq!(wire[1].content, "a standalone note");
        assert_eq!(wire[2].role, "user"); // question → user
        assert_eq!(wire[3].role, "assistant"); // answer → assistant
        assert_eq!(wire[4].role, "user");
        assert_eq!(wire[4].content, "the new question");
    }

    #[test]
    fn generated_title_cleanup_removes_wrapping_and_caps_word_count() {
        assert_eq!(
            clean_generated_title(" \"scaled dot-product attention behavior.\" "),
            Some("scaled dot-product attention behavior".to_string())
        );
        assert_eq!(
            clean_generated_title("one two three four five six seven eight"),
            Some("one two three four five six".to_string())
        );
        assert_eq!(clean_generated_title("   "), None);
    }

    #[test]
    fn highlight_tool_call_parses_into_intent_with_label() {
        let intent = intent_from_tool_call(
            "highlight",
            r#"{"quote":"scaled dot-product","color":"yellow","label":"key idea"}"#,
        )
        .expect("valid highlight call parses");

        assert_eq!(intent.quote, "scaled dot-product");
        assert_eq!(intent.color, "yellow");
        assert_eq!(intent.label.as_deref(), Some("key idea"));
        assert_eq!(intent.note, None);
    }

    #[test]
    fn note_tool_call_parses_into_intent_with_note_body() {
        let intent = intent_from_tool_call(
            "note",
            r#"{"quote":"attention is all you need","color":"green","body":"the core claim"}"#,
        )
        .expect("valid note call parses");

        assert_eq!(intent.quote, "attention is all you need");
        assert_eq!(intent.color, "green");
        assert_eq!(intent.label, None);
        assert_eq!(intent.note.as_deref(), Some("the core claim"));
    }

    #[test]
    fn tool_call_with_empty_quote_is_skipped() {
        assert_eq!(
            intent_from_tool_call("highlight", r#"{"quote":"","color":"yellow"}"#),
            None
        );
    }

    #[test]
    fn tool_call_with_missing_or_blank_color_defaults_to_yellow() {
        assert_eq!(
            intent_from_tool_call("highlight", r#"{"quote":"some text"}"#)
                .expect("missing color still parses")
                .color,
            "yellow"
        );
        assert_eq!(
            intent_from_tool_call("highlight", r#"{"quote":"some text","color":"  "}"#)
                .expect("blank color still parses")
                .color,
            "yellow"
        );
    }

    #[test]
    fn note_tool_call_without_color_defaults_to_yellow() {
        let intent = intent_from_tool_call("note", r#"{"quote":"x","body":"b"}"#)
            .expect("note without color still parses");

        assert_eq!(intent.color, "yellow");
        assert_eq!(intent.note.as_deref(), Some("b"));
    }

    #[test]
    fn off_palette_color_defaults_to_yellow() {
        let intent = intent_from_tool_call(
            "highlight",
            r#"{"quote":"scaled dot-product","color":"crimson"}"#,
        )
        .expect("off-palette color still parses");

        assert_eq!(intent.color, "yellow");
    }

    #[test]
    fn malformed_json_is_skipped() {
        assert_eq!(intent_from_tool_call("highlight", "not json"), None);
    }

    #[test]
    fn unrecognized_tool_name_is_skipped() {
        assert_eq!(
            intent_from_tool_call("unknown_tool", r#"{"quote":"x","color":"red"}"#),
            None
        );
    }

    #[test]
    fn title_prompt_includes_anchor_passage_and_first_entry() {
        let messages = build_title_messages("why does this matter?", Some("scaled dot-product"));

        assert_eq!(messages.len(), 1);
        assert!(messages[0].content.contains("Selected passage"));
        assert!(messages[0].content.contains("scaled dot-product"));
        assert!(messages[0].content.contains("why does this matter?"));
    }

    #[test]
    fn parse_auto_highlights_reads_a_well_formed_list() {
        let json = r#"{"highlights":[
            {"quote":"attention is all you need","color":"yellow","label":"contribution"},
            {"quote":"we report BLEU","color":"green"}
        ]}"#;
        let intents = parse_auto_highlights(json).expect("valid list");
        assert_eq!(intents.len(), 2);
        assert_eq!(intents[0].quote, "attention is all you need");
        assert_eq!(intents[0].color, "yellow");
        assert_eq!(intents[0].label.as_deref(), Some("contribution"));
        // Missing label → None; color preserved.
        assert_eq!(intents[1].label, None);
        assert_eq!(intents[1].color, "green");
    }

    #[test]
    fn parse_auto_highlights_drops_empty_quotes_and_normalizes_color() {
        let json = r#"{"highlights":[
            {"quote":"   ","color":"blue"},
            {"quote":"kept","color":"chartreuse"}
        ]}"#;
        let intents = parse_auto_highlights(json).expect("valid list");
        assert_eq!(intents.len(), 1);
        assert_eq!(intents[0].quote, "kept");
        // Off-palette color normalizes to the default rather than dropping.
        assert_eq!(intents[0].color, "yellow");
    }

    #[test]
    fn parse_auto_highlights_empty_list_is_ok() {
        assert!(parse_auto_highlights(r#"{"highlights":[]}"#).unwrap().is_empty());
    }

    #[test]
    fn parse_auto_highlights_malformed_json_errors() {
        assert!(parse_auto_highlights("not json").is_err());
        assert!(parse_auto_highlights(r#"{"wrong":1}"#).is_err());
    }
}
