//! Chat orchestration over anchored threads (RFC 0033).
//!
//! A thread is a timeline of entries (notes + AI questions/answers) attached to
//! an anchor. Asking replays the thread's entries behind one system message
//! that carries the paper text (anchor foregrounded), so the paper text is
//! inlined once per turn rather than per entry. A turn is persisted only after
//! the reply succeeds, so a failed request leaves no orphaned entries.

use reqwest::Client;

use super::config::ChatConfig;
use super::context::build_context;
use super::openrouter::{self, CompletionRequest, WireMessage};
use crate::domain::chat::{
    ChatContextSummary, ChatEntry, ChatEntryDraft, ChatScope, ChatThreadSummary, ChatThreadView,
    PinnedHighlight, ThreadAnchor, ENTRY_ANSWER,
};
use crate::services::reader_service::ReaderService;
use crate::storage::library_store::LibraryStore;

pub struct ChatService {
    client: Client,
    config: ChatConfig,
    store: LibraryStore,
    reader: ReaderService,
}

impl ChatService {
    pub fn new(config: ChatConfig, store: LibraryStore, reader: ReaderService) -> Self {
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
            client,
            config,
            store,
            reader,
        }
    }

    pub fn from_app_config(store: LibraryStore, reader: ReaderService) -> Result<Self, String> {
        Ok(Self::new(ChatConfig::load()?, store, reader))
    }

    /// List a scope's threads with entry/pin counts (newest activity first).
    pub async fn list_threads(&self, scope: &ChatScope) -> Result<Vec<ChatThreadSummary>, String> {
        self.store.list_chat_threads(scope.kind(), scope.id())
    }

    /// Load a thread with its entries.
    pub async fn get_thread(&self, thread_id: &str) -> Result<ChatThreadView, String> {
        self.store.get_chat_thread(thread_id)
    }

    /// Open (creating if needed) the scope's whole-paper thread.
    pub async fn open_document_thread(&self, scope: &ChatScope) -> Result<ChatThreadView, String> {
        let thread = self
            .store
            .ensure_document_thread(scope.kind(), scope.id())?;
        self.store.get_chat_thread(&thread.id)
    }

    /// Create a thread for an anchor (e.g. a reader selection) and return it.
    pub async fn create_thread(
        &self,
        scope: &ChatScope,
        anchor: ThreadAnchor,
        title: Option<String>,
    ) -> Result<ChatThreadView, String> {
        let thread =
            self.store
                .create_chat_thread(scope.kind(), scope.id(), &anchor, title.as_deref())?;
        self.store.get_chat_thread(&thread.id)
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
        };
        let answer = openrouter::complete_streamed(
            &self.client,
            &self.config.url,
            &prep.api_key,
            &request,
            on_delta,
        )
        .await?;
        self.finalize_ask(thread_id, &prep.user_body, answer, prep.summary)
            .await
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
}
