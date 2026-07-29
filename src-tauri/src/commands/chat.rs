//! Tauri commands for anchored chat threads (RFC 0033).
//!
//! Thin wrappers over `ChatService`; all orchestration lives in the service.

use tauri::ipc::Channel;

use crate::domain::chat::{
    AnnotateEvent, ChatScope, ChatStreamEvent, ChatThreadSummary, ChatThreadView,
    HighlightIntentPayload, PinnedHighlight, ThreadAnchor,
};
use crate::services::chat::{AutoHighlightCategory, ChatService};

/// List a scope's threads with entry/pin counts.
#[tauri::command]
pub async fn list_chat_threads(
    chat_service: tauri::State<'_, ChatService>,
    scope: ChatScope,
) -> Result<Vec<ChatThreadSummary>, String> {
    chat_log(format!(
        "list_chat_threads scope={}:{}",
        scope.kind(),
        scope.id()
    ));
    chat_service.list_threads(&scope).await
}

/// Load a thread with its entries.
#[tauri::command]
pub async fn get_chat_thread(
    chat_service: tauri::State<'_, ChatService>,
    thread_id: String,
) -> Result<ChatThreadView, String> {
    chat_log(format!("get_chat_thread thread={thread_id}"));
    chat_service.get_thread(&thread_id).await
}

/// Add a self-authored note at an anchor, creating the thread lazily.
#[tauri::command]
pub async fn add_note_at_anchor(
    chat_service: tauri::State<'_, ChatService>,
    scope: ChatScope,
    anchor: ThreadAnchor,
    body: String,
) -> Result<ChatThreadView, String> {
    chat_log(format!(
        "add_note_at_anchor scope={}:{} anchor={} body_len={}",
        scope.kind(),
        scope.id(),
        anchor.storage_kind(),
        body.len()
    ));
    chat_service.add_note_at_anchor(&scope, anchor, body).await
}

/// Append a self-authored note (pinned by default) to a thread.
#[tauri::command]
pub async fn add_chat_note(
    chat_service: tauri::State<'_, ChatService>,
    thread_id: String,
    body: String,
) -> Result<ChatThreadView, String> {
    chat_log(format!(
        "add_chat_note thread={thread_id} body_len={}",
        body.len()
    ));
    chat_service.add_note(&thread_id, body).await
}

/// Ask in a thread (non-streaming) and return the updated thread.
#[tauri::command]
pub async fn ask_chat_thread(
    chat_service: tauri::State<'_, ChatService>,
    thread_id: String,
    body: String,
) -> Result<ChatThreadView, String> {
    chat_log(format!(
        "ask_chat_thread thread={thread_id} body_len={}",
        body.len()
    ));
    chat_service.ask_in_thread(&thread_id, body).await
}

/// Ask in a thread, streaming reply deltas over `on_event`.
///
/// Always resolves `Ok(())`; success and failure are reported as `Done` /
/// `Error` events so the frontend listens on a single channel.
#[tauri::command]
pub async fn ask_chat_thread_streamed(
    chat_service: tauri::State<'_, ChatService>,
    thread_id: String,
    body: String,
    on_event: Channel<ChatStreamEvent>,
) -> Result<(), String> {
    chat_log(format!(
        "ask_chat_thread_streamed thread={thread_id} body_len={}",
        body.len()
    ));

    let deltas = on_event.clone();
    let result = chat_service
        .ask_in_thread_streamed(&thread_id, body, move |text| {
            let _ = deltas.send(ChatStreamEvent::Delta { text });
        })
        .await;

    match result {
        Ok(thread) => {
            let _ = on_event.send(ChatStreamEvent::Done { thread });
        }
        Err(message) => {
            let _ = on_event.send(ChatStreamEvent::Error { message });
        }
    }
    Ok(())
}

/// Ask at an anchor, streaming reply deltas; the thread is created lazily on
/// success (RFC 0034). Like `ask_chat_thread_streamed`, always resolves
/// `Ok(())` and reports the outcome via `Done` / `Error` events.
#[tauri::command]
pub async fn ask_at_anchor_streamed(
    chat_service: tauri::State<'_, ChatService>,
    scope: ChatScope,
    anchor: ThreadAnchor,
    body: String,
    on_event: Channel<ChatStreamEvent>,
) -> Result<(), String> {
    chat_log(format!(
        "ask_at_anchor_streamed scope={}:{} anchor={} body_len={}",
        scope.kind(),
        scope.id(),
        anchor.storage_kind(),
        body.len()
    ));

    let deltas = on_event.clone();
    let result = chat_service
        .ask_at_anchor_streamed(&scope, anchor, body, move |text| {
            let _ = deltas.send(ChatStreamEvent::Delta { text });
        })
        .await;

    match result {
        Ok(thread) => {
            let _ = on_event.send(ChatStreamEvent::Done { thread });
        }
        Err(message) => {
            let _ = on_event.send(ChatStreamEvent::Error { message });
        }
    }
    Ok(())
}

/// Surface a frontend log line in the backend terminal at a chosen level, so
/// client-side detail (e.g. quote→locator resolution) shows up alongside the
/// backend logs. `level` is one of error|warn|info|debug|trace (default debug),
/// gated by the `I0I_LOG` threshold like every other leveled log.
#[tauri::command]
pub fn debug_log(level: Option<String>, message: String) {
    let level = level
        .as_deref()
        .map(crate::shared::log::Level::parse)
        .unwrap_or(crate::shared::log::Level::Debug);
    crate::shared::log::log(level, "frontend", &message);
}

/// Fast-model annotation pass (RFC 0059 follow-up): streams `HighlightIntent`
/// events for the passages the agent marks, on its own channel, independent of
/// the answer. The caller invokes this only for marking-intent requests.
#[tauri::command]
pub async fn annotate_streamed(
    chat_service: tauri::State<'_, ChatService>,
    scope: ChatScope,
    anchor: ThreadAnchor,
    body: String,
    on_event: Channel<AnnotateEvent>,
) -> Result<(), String> {
    chat_log(format!(
        "annotate_streamed scope={}:{} anchor={} body_len={}",
        scope.kind(),
        scope.id(),
        anchor.storage_kind(),
        body.len()
    ));

    let intents = on_event.clone();
    let result = chat_service
        .annotate_streamed(&scope, anchor, body, move |intent| {
            let _ = intents.send(AnnotateEvent::Intent {
                quote: intent.quote,
                color: intent.color,
                label: intent.label,
                note: intent.note,
            });
        })
        .await;

    match result {
        Ok(()) => {
            let _ = on_event.send(AnnotateEvent::Done);
        }
        Err(message) => {
            crate::shared::log::error("annotate", format!("annotate_streamed failed: {message}"));
            let _ = on_event.send(AnnotateEvent::Error { message });
        }
    }
    Ok(())
}

/// Explicit AI auto-highlight (RFC 0064): a command, not a conversation. Given
/// the chosen lens categories, return one parseable list of passages to mark
/// (structured output — no streaming, no prose). The frontend resolves each
/// quote and creates the AI highlights with Keep/Undo.
#[tauri::command]
pub async fn auto_highlight(
    chat_service: tauri::State<'_, ChatService>,
    scope: ChatScope,
    categories: Vec<AutoHighlightCategory>,
) -> Result<Vec<HighlightIntentPayload>, String> {
    chat_log(format!(
        "auto_highlight scope={}:{} categories={}",
        scope.kind(),
        scope.id(),
        categories.len()
    ));
    let intents = chat_service.auto_highlight(&scope, categories).await?;
    Ok(intents
        .into_iter()
        .map(|intent| HighlightIntentPayload {
            quote: intent.quote,
            color: intent.color,
            label: intent.label,
            note: intent.note,
        })
        .collect())
}

/// Pin or unpin an entry.
#[tauri::command]
pub async fn set_chat_entry_pinned(
    chat_service: tauri::State<'_, ChatService>,
    entry_id: String,
    pinned: bool,
) -> Result<(), String> {
    chat_log(format!(
        "set_chat_entry_pinned entry={entry_id} pinned={pinned}"
    ));
    chat_service.set_entry_pinned(&entry_id, pinned).await
}

/// List a scope's pinned entries (the Highlights view).
#[tauri::command]
pub async fn list_pinned_chat_entries(
    chat_service: tauri::State<'_, ChatService>,
    scope: ChatScope,
) -> Result<Vec<PinnedHighlight>, String> {
    chat_log(format!(
        "list_pinned_chat_entries scope={}:{}",
        scope.kind(),
        scope.id()
    ));
    chat_service.list_pinned(&scope).await
}

/// Rename a thread.
#[tauri::command]
pub async fn rename_chat_thread(
    chat_service: tauri::State<'_, ChatService>,
    thread_id: String,
    title: String,
) -> Result<(), String> {
    chat_log(format!("rename_chat_thread thread={thread_id}"));
    chat_service.rename_thread(&thread_id, title).await
}

/// Delete a thread and its entries.
#[tauri::command]
pub async fn delete_chat_thread(
    chat_service: tauri::State<'_, ChatService>,
    thread_id: String,
) -> Result<(), String> {
    chat_log(format!("delete_chat_thread thread={thread_id}"));
    chat_service.delete_thread(&thread_id).await
}

/// Log chat activity without ever emitting message bodies or the API key.
fn chat_log(message: impl AsRef<str>) {
    let timestamp_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0);
    eprintln!("[chat {timestamp_ms}] {}", message.as_ref());
}
