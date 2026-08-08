use crate::domain::reader::DiscoveryReaderCandidate;
use crate::pdf_extraction::PdfExtractionManager;
use crate::services::reader_service::ReaderService;

/// Load a Reader document for a durable library paper.
#[tauri::command]
pub async fn get_reader_document(
    reader_service: tauri::State<'_, ReaderService>,
    paper_id: String,
    extraction_id: Option<String>,
) -> Result<crate::domain::reader::ReaderDocument, String> {
    reader_log(format!(
        "get_reader_document start paper_id={paper_id} extraction_id={:?}",
        extraction_id
    ));
    let result = reader_service
        .get_reader_document(&paper_id, extraction_id.as_deref())
        .await;
    match &result {
        Ok(document) => reader_log(format!(
            "get_reader_document ok paper_id={} source_id={} has_pdf={} pdf_error={:?}",
            document.paper_id,
            document.source_id,
            document.pdf_local_path.is_some(),
            document.pdf_error
        )),
        Err(error) => reader_log(format!(
            "get_reader_document error paper_id={paper_id} error={error}"
        )),
    }
    result
}

/// Load a Reader document directly from a transient discovery candidate.
///
/// Returns immediately: when the PDF is not cached yet, acquisition runs in
/// the background and progress arrives via `reader_pdf_acquisition_progress`
/// events (RFC 0051). `force` bypasses the negative cache on Retry.
#[tauri::command]
pub async fn get_discovery_reader_document(
    reader_service: tauri::State<'_, ReaderService>,
    candidate: DiscoveryReaderCandidate,
    force: Option<bool>,
) -> Result<crate::domain::reader::ReaderDocument, String> {
    let force = force.unwrap_or(false);
    reader_log(format!(
        "get_discovery_reader_document start paper_id={} pdf_url={:?} doi={:?} arxiv_id={:?} force={force}",
        candidate.id, candidate.pdf_url, candidate.doi, candidate.arxiv_id
    ));
    let result = reader_service
        .get_discovery_reader_document(&candidate, force)
        .await;
    match &result {
        Ok(document) => reader_log(format!(
            "get_discovery_reader_document ok paper_id={} source_id={} has_pdf={} pdf_status={:?} pdf_error={:?}",
            document.paper_id,
            document.source_id,
            document.pdf_local_path.is_some(),
            document.pdf_status,
            document.pdf_error
        )),
        Err(error) => reader_log(format!(
            "get_discovery_reader_document error paper_id={} error={error}",
            candidate.id
        )),
    }
    result
}

/// Cancel an in-flight background discovery PDF acquisition.
#[tauri::command]
pub fn cancel_discovery_pdf_acquisition(
    reader_service: tauri::State<'_, ReaderService>,
    source_id: String,
    paper_id: String,
) -> Result<(), String> {
    reader_log(format!(
        "cancel_discovery_pdf_acquisition source_id={source_id} paper_id={paper_id}"
    ));
    reader_service.cancel_discovery_pdf_acquisition(&source_id, &paper_id)
}

/// Cheaply classify a discovery candidate's PDF availability (RFC 0051).
#[tauri::command]
pub async fn probe_discovery_candidate_pdf(
    reader_service: tauri::State<'_, ReaderService>,
    candidate: DiscoveryReaderCandidate,
) -> Result<String, String> {
    let availability = reader_service
        .probe_discovery_candidate_pdf(&candidate)
        .await;
    reader_log(format!(
        "probe_discovery_candidate_pdf paper_id={} availability={availability}",
        candidate.id
    ));
    Ok(availability.to_string())
}

/// Open an arbitrary URL as an HTML reader document (RFC 0056).
#[tauri::command]
pub async fn open_html_document(
    reader_service: tauri::State<'_, ReaderService>,
    url: String,
) -> Result<crate::domain::reader::ReaderDocument, String> {
    reader_log(format!("open_html_document start url={url}"));
    let result = reader_service.open_html_document(&url).await;
    match &result {
        Ok(document) => reader_log(format!(
            "open_html_document ok source_id={} title={}",
            document.source_id, document.title
        )),
        Err(error) => reader_log(format!("open_html_document error url={url} error={error}")),
    }
    result
}

/// Serve the sanitized HTML for a cached HTML source id (RFC 0056).
#[tauri::command]
pub fn get_reader_html(
    reader_service: tauri::State<'_, ReaderService>,
    source_id: String,
) -> Result<String, String> {
    reader_service.get_reader_html(&source_id)
}

/// Read PDF bytes for either a durable or temporary Reader source id.
///
/// Returns `tauri::ipc::Response`, NOT `Vec<u8>`. A `Vec<u8>` return value is
/// serialized by serde as a JSON array of integers, so a 28MB PDF crosses the
/// IPC as a 101MB string that the webview must then parse into 30M boxed
/// numbers. `Response` sends the bytes raw (`application/octet-stream`) and the
/// frontend receives an `ArrayBuffer` it hands straight to pdf.js.
///
/// Measured in the running app (`invoke` round-trip, warm page cache):
///
/// | PDF | `Vec<u8>` | `Response` |
/// | --- | --- | --- |
/// | 0.5MB | 78ms | 2ms |
/// | 3.7MB | 550ms | 3ms |
/// | 8.5MB | 1281ms | 6ms |
/// | 28.3MB | 4285ms | 19ms |
///
/// The cost is superlinear in file size, which is why this mattered far more
/// than an isolated `JSON.parse` benchmark suggested. Do not "simplify" this
/// back to `Vec<u8>`.
#[tauri::command]
pub fn get_reader_pdf_bytes(
    reader_service: tauri::State<'_, ReaderService>,
    source_id: String,
) -> Result<tauri::ipc::Response, String> {
    reader_log(format!("get_reader_pdf_bytes start source_id={source_id}"));
    let bytes = reader_service.get_reader_pdf_bytes(&source_id)?;
    reader_log(format!(
        "get_reader_pdf_bytes ok source_id={source_id} bytes={}",
        bytes.len()
    ));
    Ok(tauri::ipc::Response::new(bytes))
}

/// Queue text extraction for a saved paper's cached PDF.
#[tauri::command]
pub fn extract_paper_document(
    pdf_extractions: tauri::State<'_, PdfExtractionManager>,
    paper_id: String,
    source_id: Option<String>,
    force: Option<bool>,
) -> Result<(), String> {
    let force = force.unwrap_or(false);
    reader_log(format!(
        "extract_paper_document queued paper_id={paper_id} source_id={:?} force={force}",
        source_id
    ));
    pdf_extractions.queue_paper(paper_id, source_id, force)
}

fn reader_log(message: impl AsRef<str>) {
    let timestamp_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0);
    eprintln!("[reader {timestamp_ms}] {}", message.as_ref());
}
