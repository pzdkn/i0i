//! Commands for the agent's context (RFC 0077).
//!
//! Only *persistent* context is addressable here. The current selection and
//! page are ephemeral — passed into assembly at ask time and never stored — so
//! there is nothing to add or remove for them.

use crate::domain::context::{ContextItem, ContextItemView, ContextKey, ORIGIN_USER};
use crate::domain::chat::ChatThreadView;
use crate::services::chat::{ChatService, ContextManager};

/// Commit a chunk to a thread's persistent context.
///
/// Idempotent: adding a chunk already in context returns the existing item.
#[tauri::command]
pub fn add_chat_context(
    context: tauri::State<'_, ContextManager>,
    thread_id: String,
    chunk_id: String,
) -> Result<ContextItem, String> {
    context.add_context(&thread_id, &chunk_id, ORIGIN_USER)
}

/// Drop one item, by item id or by chunk id — whichever the caller holds.
#[tauri::command]
pub fn delete_chat_context(
    context: tauri::State<'_, ContextManager>,
    thread_id: String,
    key: ContextKey,
) -> Result<bool, String> {
    context.delete_context(&thread_id, &key)
}

/// A thread's persistent context, resolved to text.
///
/// Items whose chunk no longer resolves come back with `unresolved: true`
/// rather than being dropped: a silently shrinking context is worse than a
/// visible hole.
#[tauri::command]
pub fn list_chat_context(
    context: tauri::State<'_, ContextManager>,
    thread_id: String,
) -> Result<Vec<ContextItemView>, String> {
    context.list_context(&thread_id)
}

/// Summarize the thread's context and set a watermark.
///
/// The one context call that costs a model round-trip. `chat_entries` is not
/// touched — the thread view still shows every turn.
#[tauri::command]
pub async fn compact_chat_context(
    chat: tauri::State<'_, ChatService>,
    thread_id: String,
) -> Result<ChatThreadView, String> {
    chat.compact_context(&thread_id).await
}
