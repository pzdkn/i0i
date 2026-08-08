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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::chat::{ChatContextSummary, ChatProgress};
    use crate::domain::context::ContextCitation;

    /// A camelCase mismatch across the Tauri boundary produces no error
    /// anywhere — the command just rejects, or the field reads `undefined` and
    /// the UI renders nothing. These pin both directions.
    #[test]
    fn the_context_key_deserializes_from_what_the_bridge_sends() {
        let key: ContextKey =
            serde_json::from_str(r#"{"kind":"item","id":"ctx_1"}"#).expect("item key parses");
        assert!(matches!(key, ContextKey::Item(id) if id == "ctx_1"));

        let key: ContextKey =
            serde_json::from_str(r#"{"kind":"chunk","id":"chunk_1"}"#).expect("chunk key parses");
        assert!(matches!(key, ContextKey::Chunk(id) if id == "chunk_1"));
    }

    #[test]
    fn the_context_summary_serializes_the_field_names_the_panel_reads() {
        let summary = ChatContextSummary {
            citations: vec![ContextCitation {
                handle: "C1".to_string(),
                item_id: "ctx_1".to_string(),
                paper_id: "vaswani2017".to_string(),
                page_start: 2,
                heading_path: Some("Method".to_string()),
                chunk_id: Some("chunk_1".to_string()),
                rects_json: "[]".to_string(),
            }],
            ..ChatContextSummary::default()
        };
        let json = serde_json::to_string(&summary).expect("serializes");

        for field in [
            "passages",
            "contextItems",
            "droppedItems",
            "unresolvedItems",
            "retrievalCapped",
            "citations",
            "pageStart",
            "headingPath",
            "chunkId",
            "rectsJson",
        ] {
            assert!(json.contains(&format!("\"{field}\"")), "missing {field}");
        }
    }

    #[test]
    fn progress_events_carry_the_tag_the_listener_switches_on() {
        let json = serde_json::to_string(&ChatProgress::Searching {
            query: "scaling".to_string(),
        })
        .expect("serializes");
        assert_eq!(json, r#"{"event":"searching","query":"scaling"}"#);

        let json = serde_json::to_string(&ChatProgress::Retrieved { count: 3 }).expect("serializes");
        assert_eq!(json, r#"{"event":"retrieved","count":3}"#);

        let json = serde_json::to_string(&ChatProgress::Deciding).expect("serializes");
        assert_eq!(json, r#"{"event":"deciding"}"#);
    }
}
