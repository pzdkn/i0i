use crate::services::source_acquisition::{
    BrowserEndpoint, BrowserPageSnapshot, SourceAcquisitionService,
};

#[tauri::command]
pub async fn debug_obscura_start(
    source_acquisition: tauri::State<'_, SourceAcquisitionService>,
) -> Result<BrowserEndpoint, String> {
    source_acquisition
        .ensure_browser_ready()
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn debug_obscura_fetch(
    source_acquisition: tauri::State<'_, SourceAcquisitionService>,
    url: String,
) -> Result<BrowserPageSnapshot, String> {
    source_acquisition
        .acquire_web_page(&url)
        .await
        .map_err(|error| error.to_string())
}
