use crate::services::reader_service::ReaderService;
use crate::storage::library_store::LibraryStore;

#[tauri::command]
pub fn get_reader_document(
    reader_service: tauri::State<'_, ReaderService>,
    paper_id: String,
    extraction_id: Option<String>,
) -> Result<crate::domain::reader::ReaderDocument, String> {
    reader_log(format!(
        "get_reader_document start paper_id={paper_id} extraction_id={:?}",
        extraction_id
    ));
    let result = reader_service.get_reader_document(&paper_id, extraction_id.as_deref());
    match &result {
        Ok(document) => reader_log(format!(
            "get_reader_document ok paper_id={} source_id={} has_pdf={}",
            document.paper_id,
            document.source_id,
            document.pdf_local_path.is_some()
        )),
        Err(error) => reader_log(format!(
            "get_reader_document error paper_id={paper_id} error={error}"
        )),
    }
    result
}

#[tauri::command]
pub fn get_reader_pdf_bytes(
    store: tauri::State<'_, LibraryStore>,
    source_id: String,
) -> Result<Vec<u8>, String> {
    reader_log(format!("get_reader_pdf_bytes start source_id={source_id}"));
    let source = store.get_document_source(&source_id)?;
    if source.source_kind != "pdf" {
        return Err(format!("Document source is not a PDF: {source_id}"));
    }
    if source.status != "cached" {
        return Err(format!(
            "PDF source is not cached yet: {source_id} ({})",
            source.status
        ));
    }

    let local_path = source
        .local_path
        .ok_or_else(|| format!("Cached PDF source has no local path: {source_id}"))?;
    let bytes = std::fs::read(&local_path)
        .map_err(|error| format!("Failed to read cached PDF {local_path}: {error}"))?;
    reader_log(format!(
        "get_reader_pdf_bytes ok source_id={source_id} bytes={}",
        bytes.len()
    ));
    Ok(bytes)
}

fn reader_log(message: impl AsRef<str>) {
    let timestamp_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0);
    eprintln!("[reader {timestamp_ms}] {}", message.as_ref());
}
