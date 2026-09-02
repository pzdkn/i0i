//! Tauri commands: the UI front door to `HighlightService` (RFC 0058).

use crate::domain::highlight::{Highlight, HighlightAuthor, HighlightColor, Locator};
use crate::services::highlight::HighlightService;

#[tauri::command]
pub async fn create_highlight(
    service: tauri::State<'_, HighlightService>,
    paper_id: String,
    locator: Locator,
    excerpt: String,
    // Optional (RFC 0061): a null color creates a note-/ask-only passage.
    color: Option<HighlightColor>,
    label: Option<String>,
) -> Result<Highlight, String> {
    service
        .create_highlight(
            &paper_id,
            locator,
            &excerpt,
            color,
            label,
            HighlightAuthor::User,
        )
        .await
}

#[tauri::command]
pub async fn set_highlight_note(
    service: tauri::State<'_, HighlightService>,
    id: String,
    note: Option<String>,
) -> Result<(), String> {
    service.set_note(&id, note).await
}

#[tauri::command]
pub async fn recolor_highlight(
    service: tauri::State<'_, HighlightService>,
    id: String,
    color: HighlightColor,
) -> Result<(), String> {
    service.recolor(&id, color).await
}

#[tauri::command]
pub async fn set_highlight_label(
    service: tauri::State<'_, HighlightService>,
    id: String,
    label: Option<String>,
) -> Result<(), String> {
    service.set_label(&id, label).await
}

#[tauri::command]
pub async fn remove_highlight(
    service: tauri::State<'_, HighlightService>,
    id: String,
) -> Result<(), String> {
    service.remove(&id).await
}

/// Delete a passage outright: the mark, its note, and its conversation
/// (RFC 0079 R1.3). Returns how many threads went with it, which is what the
/// UI confirms against before calling.
#[tauri::command]
pub async fn remove_annotation(
    service: tauri::State<'_, HighlightService>,
    id: String,
) -> Result<usize, String> {
    service.remove_annotation(&id).await
}

#[tauri::command]
pub async fn list_highlights(
    service: tauri::State<'_, HighlightService>,
    paper_id: String,
) -> Result<Vec<Highlight>, String> {
    service.list(&paper_id).await
}

/// Agent-authored counterpart to `create_highlight` (RFC 0059). Thread
/// linkage is deferred; the agent tool passes no thread id yet.
#[tauri::command]
pub async fn create_agent_highlight(
    service: tauri::State<'_, HighlightService>,
    paper_id: String,
    locator: Locator,
    excerpt: String,
    color: HighlightColor,
    label: Option<String>,
    model: String,
) -> Result<Highlight, String> {
    service
        .create_highlight(
            &paper_id,
            locator,
            &excerpt,
            Some(color), // agent marks always carry a color
            label,
            HighlightAuthor::Agent { model },
        )
        .await
}

#[tauri::command]
pub async fn list_agent_highlights(
    service: tauri::State<'_, HighlightService>,
    paper_id: String,
) -> Result<Vec<Highlight>, String> {
    service.list_agent(&paper_id).await
}
