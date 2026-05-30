use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Vault {
    pub id: String,
    pub title: String,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultDraft {
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultRenameDraft {
    pub id: String,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Paper {
    pub id: String,
    pub title: String,
    pub authors: Vec<String>,
    pub venue: String,
    pub year: i32,
    pub citations: i32,
    pub tags: Vec<String>,
    pub note_count: i32,
    pub annotation_count: i32,
    pub status: String,
    #[serde(rename = "abstract")]
    pub abstract_text: Option<String>,
    pub active_source_id: Option<String>,
    pub active_extraction_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaperDraft {
    pub id: String,
    pub title: String,
    pub authors: Vec<String>,
    pub venue: String,
    pub year: i32,
    pub citations: i32,
    pub tags: Vec<String>,
    pub status: String,
    #[serde(rename = "abstract")]
    pub abstract_text: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaperNote {
    pub id: String,
    pub paper_id: String,
    pub source_id: String,
    pub start_offset: i64,
    pub end_offset: i64,
    pub selected_text: String,
    pub body: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaperNoteDraft {
    pub paper_id: String,
    pub source_id: String,
    pub start_offset: i64,
    pub end_offset: i64,
    pub selected_text: String,
    pub body: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultPaper {
    pub vault_id: String,
    pub paper_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentSource {
    pub id: String,
    pub paper_id: String,
    pub source_kind: String,
    pub source_url: Option<String>,
    pub local_path: Option<String>,
    pub status: String,
    pub error: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentExtraction {
    pub id: String,
    pub paper_id: String,
    pub source_id: String,
    pub extractor: String,
    pub extractor_version: String,
    pub annotation_source_id: String,
    pub status: String,
    pub error: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentPage {
    pub id: String,
    pub paper_id: String,
    pub source_id: String,
    pub extraction_id: String,
    pub page_index: i32,
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentBlock {
    pub id: String,
    pub paper_id: String,
    pub source_id: String,
    pub extraction_id: String,
    pub page_index: i32,
    pub block_index: i32,
    pub reading_order: i32,
    pub kind: String,
    pub text: Option<String>,
    pub asset_id: Option<String>,
    pub source_start: Option<i64>,
    pub source_end: Option<i64>,
    pub bbox_json: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentSpan {
    pub id: String,
    pub paper_id: String,
    pub source_id: String,
    pub extraction_id: String,
    pub block_id: String,
    pub page_index: i32,
    pub text: String,
    pub source_start: i64,
    pub source_end: i64,
    pub bbox_json: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentAsset {
    pub id: String,
    pub paper_id: String,
    pub source_id: String,
    pub extraction_id: String,
    pub asset_kind: String,
    pub page_index: i32,
    pub bbox_json: Option<String>,
    pub local_path: String,
    pub caption: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibrarySnapshot {
    pub vaults: Vec<Vault>,
    pub papers: Vec<Paper>,
    pub vault_papers: Vec<VaultPaper>,
    pub document_sources: Vec<DocumentSource>,
    pub document_extractions: Vec<DocumentExtraction>,
    pub document_pages: Vec<DocumentPage>,
    pub document_blocks: Vec<DocumentBlock>,
    pub document_spans: Vec<DocumentSpan>,
    pub document_assets: Vec<DocumentAsset>,
}
