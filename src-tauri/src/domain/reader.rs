use serde::{Deserialize, Serialize};

/// Discovery candidate payload sent to the Reader before a paper is saved.
///
/// This keeps the Reader command boundary small: the frontend can pass the
/// metadata it already has from Discover, and the backend can adapt it into the
/// normal `ReaderDocument` shape without creating a durable library row first.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveryReaderCandidate {
    pub id: String,
    pub source_provider: Option<String>,
    pub source_id: Option<String>,
    pub title: String,
    pub authors: Vec<String>,
    pub venue: String,
    pub year: i32,
    pub citations: i32,
    pub tags: Vec<String>,
    #[serde(rename = "abstract")]
    pub abstract_text: Option<String>,
    pub external_url: Option<String>,
    pub pdf_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReaderTextBlock {
    pub id: String,
    pub kind: String,
    pub text: String,
    pub source_start: i64,
    pub highlight: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReaderParagraph {
    pub id: String,
    pub kind: String,
    pub text: String,
    pub highlight: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReaderMark {
    pub id: String,
    pub paragraph_id: String,
    pub kind: String,
    pub body: String,
    pub created_label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReaderPage {
    pub page_index: i32,
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReaderBlock {
    pub id: String,
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
pub struct ReaderSpan {
    pub id: String,
    pub block_id: String,
    pub page_index: i32,
    pub text: String,
    pub source_start: i64,
    pub source_end: i64,
    pub bbox_json: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReaderAsset {
    pub id: String,
    pub paper_id: String,
    pub source_id: String,
    pub extraction_id: String,
    #[serde(rename = "kind")]
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
pub struct ReaderDocument {
    pub paper_id: String,
    pub source_id: String,
    pub extraction_id: Option<String>,
    pub annotation_source_id: Option<String>,
    pub title: String,
    pub authors: Vec<String>,
    pub venue: String,
    pub year: i32,
    pub identifier: String,
    pub citation_key: String,
    pub tags: Vec<String>,
    pub pdf_local_path: Option<String>,
    pub pdf_source_url: Option<String>,
    pub pdf_error: Option<String>,
    pub source_text: String,
    pub pages: Vec<ReaderPage>,
    pub blocks: Vec<ReaderBlock>,
    pub spans: Vec<ReaderSpan>,
    pub assets: Vec<ReaderAsset>,
    pub text_blocks: Vec<ReaderTextBlock>,
    pub paragraphs: Vec<ReaderParagraph>,
    pub marks: Vec<ReaderMark>,
}
