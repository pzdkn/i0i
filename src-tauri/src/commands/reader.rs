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

/// Read PDF bytes for either a durable or temporary Reader source id.
#[tauri::command]
pub fn get_reader_pdf_bytes(
    reader_service: tauri::State<'_, ReaderService>,
    source_id: String,
) -> Result<Vec<u8>, String> {
    reader_log(format!("get_reader_pdf_bytes start source_id={source_id}"));
    let bytes = reader_service.get_reader_pdf_bytes(&source_id)?;
    reader_log(format!(
        "get_reader_pdf_bytes ok source_id={source_id} bytes={}",
        bytes.len()
    ));
    Ok(bytes)
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
