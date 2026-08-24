use crate::services::source_acquisition::{
    BrowserEndpoint, BrowserPageSnapshot, BrowserRuntimeStatus, SourceAcquisitionService,
};

#[tauri::command]
pub fn get_browser_runtime_status(
    source_acquisition: tauri::State<'_, SourceAcquisitionService>,
) -> BrowserRuntimeStatus {
    source_acquisition.browser_status()
}

#[tauri::command]
pub async fn retry_browser_runtime(
    source_acquisition: tauri::State<'_, SourceAcquisitionService>,
) -> Result<BrowserRuntimeStatus, String> {
    let _ = source_acquisition.ensure_browser_ready().await;
    Ok(source_acquisition.browser_status())
}

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
