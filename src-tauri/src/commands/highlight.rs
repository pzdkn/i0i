//! Tauri commands: the UI front door to `HighlightService` (RFC 0058).

use crate::domain::highlight::{Highlight, HighlightAuthor, HighlightColor, Locator};
use crate::services::highlight::HighlightService;

#[tauri::command]
pub async fn create_highlight(
    service: tauri::State<'_, HighlightService>,
    paper_id: String,
    locator: Locator,
    excerpt: String,
    color: HighlightColor,
    label: Option<String>,
) -> Result<Highlight, String> {
    service
        .create_highlight(&paper_id, locator, &excerpt, color, label, HighlightAuthor::User)
        .await
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

#[tauri::command]
pub async fn list_highlights(
    service: tauri::State<'_, HighlightService>,
    paper_id: String,
) -> Result<Vec<Highlight>, String> {
    service.list(&paper_id).await
}
