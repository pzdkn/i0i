use serde::{Deserialize, Serialize};

use crate::domain::research_document::ProjectDocumentCitation;

/// A goal-directed workspace that owns exactly one Vault.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub id: String,
    pub title: String,
    pub goal: Option<String>,
}

/// User input for atomically creating a Project and its Vault.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectDraft {
    pub title: String,
    pub goal: Option<String>,
}

/// User input for changing Project metadata without renaming its Vault.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectRenameDraft {
    pub id: String,
    pub title: String,
}

/// Navigation metadata for a Project document; content is loaded on demand.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectDocumentSummary {
    pub id: String,
    pub project_id: String,
    pub title: String,
    pub format: String,
    pub harness_writable: bool,
    pub content_revision: i64,
    pub updated_at: String,
}

/// A Markdown artifact authored by a researcher or, when allowed, the Harness.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectDocument {
    pub id: String,
    pub project_id: String,
    pub title: String,
    pub format: String,
    pub content: String,
    pub harness_writable: bool,
    pub content_revision: i64,
    pub created_from_run_id: Option<String>,
    pub created_from_state_revision: Option<i64>,
    pub generation_id: Option<String>,
    pub output_shape: Option<String>,
    pub citations: Vec<ProjectDocumentCitation>,
    pub created_at: String,
    pub updated_at: String,
}

/// User-authored document creation input.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectDocumentDraft {
    pub project_id: String,
    pub title: String,
    #[serde(default)]
    pub content: String,
}

/// Complete editable state for one Project document.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectDocumentUpdate {
    pub id: String,
    pub title: String,
    pub content: String,
    pub harness_writable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Vault {
    pub id: String,
    pub project_id: String,
    pub title: String,
    pub path: String,
    pub membership_revision: i64,
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
    /// Count of pinned chat entries across this paper's threads (RFC 0034).
    /// Computed on read from the chat tables; the legacy `note_count` column is
    /// no longer surfaced.
    pub highlight_count: i32,
    pub annotation_count: i32,
    pub status: String,
    #[serde(rename = "abstract")]
    pub abstract_text: Option<String>,
    pub active_source_id: Option<String>,
    pub active_extraction_id: Option<String>,
}

/// The minimal per-paper metadata needed to emit one BibTeX entry (RFC 0070).
///
/// Deliberately narrower than `Paper`: it omits the counts that `Paper` computes
/// on read from the chat tables, so a citation export never has to touch those
/// tables. Populated by `LibraryStore::cite_records_for_vault`.
#[derive(Debug, Clone)]
pub struct CiteRecord {
    pub title: String,
    pub authors: Vec<String>,
    pub venue: String,
    pub year: i32,
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
    #[serde(default)]
    pub sources: Vec<PaperSourceDraft>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaperSourceDraft {
    pub source_kind: String,
    pub source_url: String,
    pub landing_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalPdfImport {
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalPdfImportResult {
    pub snapshot: LibrarySnapshot,
    pub imported_paper_ids: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct PaperMetadataEnrichment {
    pub title: Option<String>,
    pub authors: Option<Vec<String>>,
    pub venue: Option<String>,
    pub year: Option<i32>,
    pub citations: Option<i32>,
    pub abstract_text: Option<String>,
    pub confident: bool,
}

/// Manual metadata edit payload from the right panel (RFC 0049). `None`
/// leaves a field untouched.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaperMetadataUpdate {
    pub title: Option<String>,
    pub authors: Option<Vec<String>>,
    pub venue: Option<String>,
    pub year: Option<i32>,
    #[serde(rename = "abstract")]
    pub abstract_text: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MetadataCandidate {
    pub id: String,
    pub title: String,
    pub authors: Vec<String>,
    pub venue: Option<String>,
    pub year: Option<i32>,
    pub doi: Option<String>,
    pub arxiv_id: Option<String>,
    pub abstract_text: Option<String>,
    pub providers: Vec<String>,
    pub confidence: f64,
    pub evidence: Vec<String>,
}

impl From<MetadataCandidate> for PaperMetadataEnrichment {
    fn from(candidate: MetadataCandidate) -> Self {
        Self {
            title: Some(candidate.title),
            authors: Some(candidate.authors),
            venue: candidate.venue,
            year: candidate.year,
            citations: None,
            abstract_text: candidate.abstract_text,
            confident: true,
        }
    }
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
    pub landing_url: Option<String>,
    pub final_url: Option<String>,
    pub acquisition_method: Option<String>,
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

/// A retrieval-sized slice of an extraction (RFC 0075).
///
/// Derived data: everything here is regenerable from pages/blocks/spans, and is
/// torn down with them on re-extraction. `source_start`/`source_end` index the
/// canonical `source_text` — blocks joined by `"\n\n"` — because a chunk can
/// cover part of its first or last block, which `document_chunk_blocks` alone
/// cannot express.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentChunk {
    pub id: String,
    pub paper_id: String,
    pub source_id: String,
    pub extraction_id: String,
    /// Reading order within the extraction, dense from 0.
    pub chunk_index: i32,
    pub chunker: String,
    pub chunk_version: i32,
    pub page_start: i32,
    pub page_end: i32,
    /// `"Introduction > Motivation"`, or `None` above the first heading.
    pub heading_path: Option<String>,
    pub text: String,
    pub token_estimate: i32,
    pub source_start: i64,
    pub source_end: i64,
    /// Blocks this chunk covers, in reading order. Not a database column —
    /// carried alongside so a chunk and its provenance write as one unit.
    pub block_ids: Vec<String>,
}

/// Coverage of chunk embeddings for one paper (RFC 0075).
///
/// A missing embedding is a retrieval hole the user cannot see on their own,
/// so it is counted rather than swallowed.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmbeddingCoverage {
    pub chunks: i64,
    pub embedded: i64,
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
    pub projects: Vec<Project>,
    pub project_documents: Vec<ProjectDocumentSummary>,
    pub vaults: Vec<Vault>,
    pub papers: Vec<Paper>,
    pub vault_papers: Vec<VaultPaper>,
    pub document_sources: Vec<DocumentSource>,
    pub document_extractions: Vec<DocumentExtraction>,
    pub document_pages: Vec<DocumentPage>,
    pub document_assets: Vec<DocumentAsset>,
}

/// One extraction's blocks and spans, loaded on demand (RFC 0075 R2).
///
/// Deliberately *not* part of `LibrarySnapshot`: this is document-scoped data,
/// and every consumer narrows to a single extraction anyway. Carrying it in the
/// library snapshot meant serializing every paper's full text across IPC to
/// render a list of titles.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtractionStructure {
    pub blocks: Vec<DocumentBlock>,
    pub spans: Vec<DocumentSpan>,
}
