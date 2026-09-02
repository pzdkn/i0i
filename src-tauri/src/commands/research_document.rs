//! Asynchronous Project-document generation commands (RFC 0115).

use tauri::Emitter;

use crate::domain::research_document::{CreateFromResearchRequest, ResearchDocumentGeneration};
use crate::storage::library_store::LibraryStore;

const GENERATION_UPDATED_EVENT: &str = "research_document_generation_updated";

fn start_generation(
    app: tauri::AppHandle,
    store: LibraryStore,
    generation: ResearchDocumentGeneration,
) {
    tauri::async_runtime::spawn(async move {
        let generation_id = generation.id.clone();
        let result = store.execute_research_document_generation(&generation_id);
        let final_generation = match result {
            Ok(generation) => generation,
            Err(error) => {
                let _ = store.fail_research_document_generation(&generation_id, &error);
                match store.get_research_document_generation(&generation_id) {
                    Ok(generation) => generation,
                    Err(_) => return,
                }
            }
        };
        let _ = app.emit(GENERATION_UPDATED_EVENT, final_generation);
    });
}

#[tauri::command]
/// Queues a pinned Research State selection and starts its generation job.
pub fn create_document_from_research(
    app: tauri::AppHandle,
    store: tauri::State<'_, LibraryStore>,
    request: CreateFromResearchRequest,
) -> Result<ResearchDocumentGeneration, String> {
    let generation = store.create_research_document_generation(&request, None)?;
    start_generation(app, store.inner().clone(), generation.clone());
    Ok(generation)
}

#[tauri::command]
/// Loads one immutable generation attempt and its current lifecycle status.
pub fn get_research_document_generation(
    store: tauri::State<'_, LibraryStore>,
    generation_id: String,
) -> Result<ResearchDocumentGeneration, String> {
    store.get_research_document_generation(&generation_id)
}

#[tauri::command]
/// Requests cancellation without creating or deleting a partial document.
pub fn cancel_research_document_generation(
    store: tauri::State<'_, LibraryStore>,
    generation_id: String,
) -> Result<ResearchDocumentGeneration, String> {
    store.cancel_research_document_generation(&generation_id)
}

#[tauri::command]
/// Creates and starts a new attempt linked to a failed or cancelled attempt.
pub fn retry_research_document_generation(
    app: tauri::AppHandle,
    store: tauri::State<'_, LibraryStore>,
    generation_id: String,
) -> Result<ResearchDocumentGeneration, String> {
    let generation = store.retry_research_document_generation(&generation_id)?;
    start_generation(app, store.inner().clone(), generation.clone());
    Ok(generation)
}
