use crate::domain::discovery::{
    CandidateMatch, DiscoverySearchRequest, DiscoverySearchResponse, DiscoverySort,
    OpenAccessSummary, PaperCandidate,
};
use reqwest::StatusCode;
use serde::Deserialize;
use std::{
    collections::HashMap,
    env, fs,
    path::{Path, PathBuf},
};
use url::Url;

const OPENALEX_WORKS_URL: &str = "https://api.openalex.org/works";
const OPENALEX_API_KEY_ENV: &str = "OPENALEX_API_KEY";
const MAX_RESULT_LIMIT: i32 = 50;

#[tauri::command]
pub async fn search_papers(
    request: DiscoverySearchRequest,
) -> Result<DiscoverySearchResponse, String> {
    let query = request.query.trim().to_string();
    if query.is_empty() {
        return Err("Enter a search query before running discovery.".to_string());
    }

    let api_key = openalex_api_key()?;
    let result_limit = request.result_limit.clamp(1, MAX_RESULT_LIMIT);
    let filters = openalex_filters(&request);
    let mut query_params = vec![
        ("api_key", api_key),
        ("search", query.clone()),
        ("per-page", result_limit.to_string()),
        (
            "select",
            [
                "id",
                "doi",
                "display_name",
                "publication_year",
                "publication_date",
                "cited_by_count",
                "authorships",
                "primary_location",
                "open_access",
                "abstract_inverted_index",
                "relevance_score",
                "ids",
            ]
            .join(","),
        ),
    ];

    if !filters.is_empty() {
        query_params.push(("filter", filters.join(",")));
    }

    if let Some(sort) = openalex_sort(&request.sort_by) {
        query_params.push(("sort", sort.to_string()));
    }

    let url = openalex_url(&query_params)?;
    let response = reqwest::Client::new()
        .get(url)
        .header("User-Agent", "i0i/0.1 local Tauri discovery")
        .send()
        .await
        .map_err(|error| format!("OpenAlex request failed: {error}"))?;

    let status = response.status();
    if status != StatusCode::OK {
        let body = response.text().await.unwrap_or_default();
        return Err(openalex_error_message(status, &body));
    }

    let payload = response
        .json::<OpenAlexWorksResponse>()
        .await
        .map_err(|error| format!("OpenAlex response could not be read: {error}"))?;
    let candidates = payload
        .results
        .into_iter()
        .map(|work| normalize_openalex_work(work, &query, request.open_access_only))
        .collect::<Vec<_>>();

    Ok(DiscoverySearchResponse {
        provider: "openalex".to_string(),
        query,
        filters,
        sort_by: request.sort_by,
        result_limit,
        result_count: candidates.len(),
        candidates,
    })
}

fn openalex_url(query_params: &[(&str, String)]) -> Result<Url, String> {
    let mut url = Url::parse(OPENALEX_WORKS_URL).map_err(|error| error.to_string())?;
    {
        let mut pairs = url.query_pairs_mut();
        for (key, value) in query_params {
            pairs.append_pair(key, value);
        }
    }
    Ok(url)
}

fn openalex_api_key() -> Result<String, String> {
    if let Ok(value) = env::var(OPENALEX_API_KEY_ENV) {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return Ok(trimmed.to_string());
        }
    }

    for path in candidate_env_paths() {
        if let Some(value) = read_env_value(&path, OPENALEX_API_KEY_ENV) {
            return Ok(value);
        }
    }

    Err("OPENALEX_API_KEY is required for OpenAlex discovery. Add it to .env or the process environment.".to_string())
}

fn candidate_env_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();

    if let Ok(current_dir) = env::current_dir() {
        paths.push(current_dir.join(".env"));
        if let Some(parent) = current_dir.parent() {
            paths.push(parent.join(".env"));
        }
    }

    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    paths.push(manifest_dir.join(".env"));
    if let Some(parent) = manifest_dir.parent() {
        paths.push(parent.join(".env"));
    }

    paths
}

fn read_env_value(path: &Path, key: &str) -> Option<String> {
    let content = fs::read_to_string(path).ok()?;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        let (name, value) = trimmed.split_once('=')?;
        if name.trim() != key {
            continue;
        }

        let value = value.trim().trim_matches('"').trim_matches('\'');
        if !value.is_empty() {
            return Some(value.to_string());
        }
    }

    None
}

fn openalex_filters(request: &DiscoverySearchRequest) -> Vec<String> {
    let mut filters = Vec::new();

    if let Some(year_from) = request.year_from {
        filters.push(format!("from_publication_date:{year_from}-01-01"));
    }

    if let Some(year_to) = request.year_to {
        filters.push(format!("to_publication_date:{year_to}-12-31"));
    }

    if request.open_access_only {
        filters.push("is_oa:true".to_string());
    }

    filters
}

fn openalex_sort(sort: &DiscoverySort) -> Option<&'static str> {
    match sort {
        DiscoverySort::Relevance => None,
        DiscoverySort::Newest => Some("publication_date:desc"),
        DiscoverySort::MostCited => Some("cited_by_count:desc"),
    }
}

fn openalex_error_message(status: StatusCode, body: &str) -> String {
    if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
        return "OpenAlex rejected the API key. Check OPENALEX_API_KEY.".to_string();
    }

    if body.trim().is_empty() {
        return format!("OpenAlex returned HTTP {status}.");
    }

    format!("OpenAlex returned HTTP {status}: {body}")
}

fn normalize_openalex_work(
    work: OpenAlexWork,
    query: &str,
    open_access_only: bool,
) -> PaperCandidate {
    let source_id = openalex_work_suffix(work.id.as_deref()).unwrap_or_else(|| {
        work.ids
            .as_ref()
            .and_then(|ids| openalex_work_suffix(ids.openalex.as_deref()))
            .unwrap_or_else(|| "unknown".to_string())
    });
    let id = format!("openalex:{source_id}");
    let authors = work
        .authorships
        .unwrap_or_default()
        .into_iter()
        .filter_map(|authorship| authorship.author?.display_name)
        .collect::<Vec<_>>();
    let venue = work
        .primary_location
        .as_ref()
        .and_then(|location| location.source.as_ref())
        .and_then(|source| source.display_name.clone());
    let external_url = work
        .primary_location
        .as_ref()
        .and_then(|location| location.landing_page_url.clone())
        .or_else(|| work.id.clone());
    let pdf_url = work
        .primary_location
        .as_ref()
        .and_then(|location| location.pdf_url.clone())
        .or_else(|| {
            work.open_access
                .as_ref()
                .and_then(|open_access| open_access.oa_url.clone())
        });
    let open_access = work.open_access.map(|open_access| OpenAccessSummary {
        is_open_access: open_access.is_oa.unwrap_or(false),
        status: open_access.oa_status,
    });
    let abstract_text = work.abstract_inverted_index.map(reconstruct_abstract);
    let matched_keywords = query
        .split_whitespace()
        .map(|part| part.trim_matches(|ch: char| !ch.is_alphanumeric()))
        .filter(|part| !part.is_empty())
        .map(str::to_lowercase)
        .collect::<Vec<_>>();
    let mut reasons = vec![format!("Matched OpenAlex search query \"{query}\".")];

    if open_access_only {
        reasons.push("Open-access filter was applied.".to_string());
    }

    if let Some(citation_count) = work.cited_by_count {
        if citation_count > 0 {
            reasons.push(format!("{citation_count} OpenAlex citations."));
        }
    }

    PaperCandidate {
        id,
        source_provider: "openalex".to_string(),
        source_id,
        title: work
            .display_name
            .unwrap_or_else(|| "Untitled OpenAlex work".to_string()),
        authors,
        abstract_text,
        year: work.publication_year,
        publication_date: work.publication_date,
        venue,
        citation_count: work.cited_by_count,
        doi: work
            .doi
            .or_else(|| work.ids.as_ref().and_then(|ids| ids.doi.clone())),
        openalex_id: work
            .ids
            .as_ref()
            .and_then(|ids| ids.openalex.clone())
            .or(work.id),
        arxiv_id: work.ids.and_then(|ids| ids.arxiv),
        external_url,
        pdf_url,
        open_access,
        match_summary: CandidateMatch {
            score: work.relevance_score,
            reasons,
            matched_keywords,
            from_seed_paper_ids: Vec::new(),
        },
        already_in_library: false,
    }
}

fn openalex_work_suffix(value: Option<&str>) -> Option<String> {
    let value = value?;
    value
        .rsplit('/')
        .next()
        .filter(|part| !part.is_empty())
        .map(str::to_string)
}

fn reconstruct_abstract(index: HashMap<String, Vec<usize>>) -> String {
    let max_position = index
        .values()
        .flat_map(|positions| positions.iter())
        .copied()
        .max();
    let Some(max_position) = max_position else {
        return String::new();
    };
    let mut words = vec![String::new(); max_position + 1];

    for (word, positions) in index {
        for position in positions {
            if let Some(slot) = words.get_mut(position) {
                *slot = word.clone();
            }
        }
    }

    words
        .into_iter()
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

#[derive(Debug, Deserialize)]
struct OpenAlexWorksResponse {
    results: Vec<OpenAlexWork>,
}

#[derive(Debug, Deserialize)]
struct OpenAlexWork {
    id: Option<String>,
    doi: Option<String>,
    display_name: Option<String>,
    publication_year: Option<i32>,
    publication_date: Option<String>,
    cited_by_count: Option<i32>,
    authorships: Option<Vec<OpenAlexAuthorship>>,
    primary_location: Option<OpenAlexLocation>,
    open_access: Option<OpenAlexAccess>,
    abstract_inverted_index: Option<HashMap<String, Vec<usize>>>,
    relevance_score: Option<f64>,
    ids: Option<OpenAlexIds>,
}

#[derive(Debug, Deserialize)]
struct OpenAlexAuthorship {
    author: Option<OpenAlexAuthor>,
}

#[derive(Debug, Deserialize)]
struct OpenAlexAuthor {
    display_name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OpenAlexLocation {
    landing_page_url: Option<String>,
    pdf_url: Option<String>,
    source: Option<OpenAlexSource>,
}

#[derive(Debug, Deserialize)]
struct OpenAlexSource {
    display_name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OpenAlexAccess {
    is_oa: Option<bool>,
    oa_status: Option<String>,
    oa_url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OpenAlexIds {
    openalex: Option<String>,
    doi: Option<String>,
    arxiv: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn abstract_reconstruction_orders_words_by_position() {
        let mut index = HashMap::new();
        index.insert("world".to_string(), vec![1]);
        index.insert("hello".to_string(), vec![0]);

        assert_eq!(reconstruct_abstract(index), "hello world");
    }

    #[test]
    fn filters_include_year_range_and_open_access() {
        let request = DiscoverySearchRequest {
            query: "sparse autoencoder".to_string(),
            year_from: Some(2023),
            year_to: Some(2026),
            result_limit: 25,
            sort_by: DiscoverySort::Relevance,
            open_access_only: true,
        };

        assert_eq!(
            openalex_filters(&request),
            vec![
                "from_publication_date:2023-01-01",
                "to_publication_date:2026-12-31",
                "is_oa:true"
            ]
        );
    }

    #[test]
    fn openalex_id_uses_work_suffix() {
        assert_eq!(
            openalex_work_suffix(Some("https://openalex.org/W123456")),
            Some("W123456".to_string())
        );
    }
}
