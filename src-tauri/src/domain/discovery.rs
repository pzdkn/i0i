use serde::{Deserialize, Deserializer, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum DiscoveryProviderChoice {
    // Aliases tolerate the LLM planner's natural spellings (RFC 0037).
    #[default]
    #[serde(alias = "openalex")]
    OpenAlex,
    #[serde(alias = "arXiv")]
    Arxiv,
}

impl DiscoveryProviderChoice {
    /// Map a wire string to a supported provider. Unknown or removed values
    /// (e.g. a stale `semantic_scholar` from RFC 0043 state) return `None` so
    /// callers can drop them instead of failing (RFC 0044).
    fn from_wire(raw: &str) -> Option<Self> {
        match raw {
            "open_alex" | "openalex" => Some(Self::OpenAlex),
            "arxiv" | "arXiv" => Some(Self::Arxiv),
            _ => None,
        }
    }
}

/// Tolerant single-provider deserialize: an unknown/removed value falls back to
/// the default (OpenAlex) rather than failing deserialization (RFC 0044). This
/// keeps stored search constraints and LLM-planned queries from crashing when
/// they still mention `semantic_scholar`.
pub fn deserialize_provider_lenient<'de, D>(
    deserializer: D,
) -> Result<DiscoveryProviderChoice, D::Error>
where
    D: Deserializer<'de>,
{
    let raw = String::deserialize(deserializer)?;
    Ok(DiscoveryProviderChoice::from_wire(&raw).unwrap_or_default())
}

/// Tolerant provider-list deserialize: silently drops unknown/removed values
/// (e.g. a stale `semantic_scholar`) instead of failing (RFC 0044). An empty or
/// fully-stale list deserializes to an empty vec; callers apply their own
/// default provider set.
pub fn deserialize_providers_lenient<'de, D>(
    deserializer: D,
) -> Result<Vec<DiscoveryProviderChoice>, D::Error>
where
    D: Deserializer<'de>,
{
    let raw = Vec::<String>::deserialize(deserializer)?;
    Ok(raw
        .iter()
        .filter_map(|value| DiscoveryProviderChoice::from_wire(value))
        .collect())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoverySearchRequest {
    pub query: String,
    pub year_from: Option<i32>,
    pub year_to: Option<i32>,
    pub result_limit: i32,
    pub sort_by: DiscoverySort,
    #[serde(default, deserialize_with = "deserialize_provider_lenient")]
    pub provider: DiscoveryProviderChoice,
    #[serde(default, deserialize_with = "deserialize_providers_lenient")]
    pub providers: Vec<DiscoveryProviderChoice>,
    #[serde(default)]
    pub open_access: bool,
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

/// Normalized dedup key for a candidate: prefer external scholarly identifiers
/// before falling back to a normalized title.
pub fn paper_candidate_dedup_key(candidate: &PaperCandidate) -> String {
    if let Some(doi) = candidate.doi.as_ref().filter(|d| !d.trim().is_empty()) {
        return format!("doi:{}", doi.trim().to_lowercase());
    }
    if let Some(arxiv) = candidate.arxiv_id.as_ref().filter(|a| !a.trim().is_empty()) {
        return format!("arxiv:{}", arxiv.trim().to_lowercase());
    }
    if let Some(openalex) = candidate
        .openalex_id
        .as_ref()
        .filter(|id| !id.trim().is_empty())
    {
        return format!("openalex:{}", openalex.trim().to_lowercase());
    }
    format!(
        "title:{}",
        candidate
            .title
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .to_lowercase()
    )
}
