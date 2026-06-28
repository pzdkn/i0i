use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum DiscoveryProviderChoice {
    #[default]
    OpenAlex,
    Arxiv,
    SemanticScholar,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoverySearchRequest {
    pub query: String,
    pub year_from: Option<i32>,
    pub year_to: Option<i32>,
    pub result_limit: i32,
    pub sort_by: DiscoverySort,
    #[serde(default)]
    pub provider: DiscoveryProviderChoice,
    /// Structured filters applied at query time (not post-filters). Support is
    /// per-provider and asymmetric: OpenAlex honors all three; arXiv supports
    /// author and category (≈ field) only. See RFC 0037.
    #[serde(default)]
    pub venues: Vec<String>,
    #[serde(default)]
    pub authors: Vec<String>,
    #[serde(default)]
    pub fields_of_study: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiscoverySort {
    Relevance,
    Newest,
    MostCited,
}

/// Direction of a citation-graph traversal from a seed paper. Naming is by
/// intent (not by OpenAlex's filter spelling, which is the inverse): `References`
/// are the works a paper cites; `Citations` are the works that cite it.
// Consumed by RealCandidateSource in the RFC 0037 seams layer.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Lineage {
    References,
    Citations,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoverySearchResponse {
    pub provider: String,
    pub query: String,
    pub filters: Vec<String>,
    pub sort_by: DiscoverySort,
    pub result_limit: i32,
    pub result_count: usize,
    pub candidates: Vec<PaperCandidate>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaperCandidate {
    pub id: String,
    pub source_provider: String,
    pub source_id: String,
    pub title: String,
    pub authors: Vec<String>,
    #[serde(rename = "abstract")]
    pub abstract_text: Option<String>,
    pub year: Option<i32>,
    pub publication_date: Option<String>,
    pub venue: Option<String>,
    pub citation_count: Option<i32>,
    pub doi: Option<String>,
    pub openalex_id: Option<String>,
    pub arxiv_id: Option<String>,
    pub external_url: Option<String>,
    pub pdf_url: Option<String>,
    pub open_access: Option<OpenAccessSummary>,
    pub match_summary: CandidateMatch,
    pub already_in_library: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenAccessSummary {
    pub is_open_access: bool,
    pub status: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CandidateMatch {
    pub score: Option<f64>,
    pub reasons: Vec<String>,
    pub matched_keywords: Vec<String>,
    pub from_seed_paper_ids: Vec<String>,
}
