use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiscoverySort {
    Relevance,
    Newest,
    MostCited,
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
