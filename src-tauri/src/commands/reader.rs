use crate::services::reader_service::ReaderService;

#[tauri::command]
pub fn get_reader_document(
    reader_service: tauri::State<'_, ReaderService>,
    paper_id: String,
    extraction_id: Option<String>,
) -> Result<crate::domain::reader::ReaderDocument, String> {
    reader_service.get_reader_document(&paper_id, extraction_id.as_deref())
}
