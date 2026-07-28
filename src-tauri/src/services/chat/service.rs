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
    /// The model is offered the `highlight`/`note` tools (RFC 0059 Phase 2);
    /// each assembled tool call is parsed into a `HighlightIntentData` and
    /// delivered to `on_intent` (before this method returns), capped at
    /// `MAX_HIGHLIGHT_INTENTS_PER_TURN` per turn. Malformed or empty-quote
    /// tool calls are skipped.
    pub async fn ask_at_anchor_streamed<F, I>(
        &self,
        scope: &ChatScope,
        anchor: ThreadAnchor,
        body: String,
        on_delta: F,
        mut on_intent: I,
    ) -> Result<ChatThreadView, String>
    where
        F: FnMut(String),
        I: FnMut(HighlightIntentData),
    {
        let prep = self.prepare_ask_at_anchor(scope, &anchor, body).await?;
        let request = CompletionRequest {
            model: self.config.model.clone(),
            messages: prep.request_messages,
            stream: true,
            max_tokens: None,
            response_format: None,
            tools: Some(openrouter::highlight_tools()),
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

        let mut delivered = 0usize;
        let mut unresolved = 0usize;
        for tool_call in &outcome.tool_calls {
            // Past the per-turn cap, extra tool calls are dropped rather than
            // delivered — but they must still be counted as unresolved
            // (Minor finding 4, final review) rather than silently vanishing.
            if delivered >= MAX_HIGHLIGHT_INTENTS_PER_TURN {
                unresolved += 1;
                continue;
            }
            match intent_from_tool_call(&tool_call.name, &tool_call.arguments) {
                Some(intent) => {
                    on_intent(intent);
                    delivered += 1;
                }
                None => unresolved += 1,
            }
        }
        if unresolved > 0 {
            chat_title_log(format!(
                "ask_at_anchor_streamed: {unresolved} tool call(s) could not be parsed into a highlight intent"
            ));
        }

        let answer_text = answer_text_or_tool_only_fallback(outcome.text, !outcome.tool_calls.is_empty());

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

/// A tool-only reply can leave the model's text empty — persist a short
/// synthetic answer body instead of a blank bubble (Minor finding 2, final
/// review). Only substitutes when the turn actually produced tool calls;
/// an empty reply with no tool calls is left as-is (existing behavior).
fn answer_text_or_tool_only_fallback(text: String, had_tool_calls: bool) -> String {
    if text.trim().is_empty() && had_tool_calls {
        "Highlighted the requested passage(s).".to_string()
    } else {
        text
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
    fn tool_only_reply_falls_back_to_synthetic_answer_body() {
        assert_eq!(
            answer_text_or_tool_only_fallback(String::new(), true),
            "Highlighted the requested passage(s).".to_string()
        );
        assert_eq!(
            answer_text_or_tool_only_fallback("   ".to_string(), true),
            "Highlighted the requested passage(s).".to_string()
        );
    }

    #[test]
    fn empty_reply_without_tool_calls_is_left_as_is() {
        assert_eq!(answer_text_or_tool_only_fallback(String::new(), false), "");
    }

    #[test]
    fn non_empty_reply_passes_through_regardless_of_tool_calls() {
        assert_eq!(
            answer_text_or_tool_only_fallback("Here's the summary.".to_string(), true),
            "Here's the summary.".to_string()
        );
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
}
