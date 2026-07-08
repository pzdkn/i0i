//! Multi-provider discovery orchestration (RFC 0043).
//!
//! Provider adapters still own HTTP/API details. This layer owns the search
//! quality work that should be shared by quick search and deep research:
//! provider selection, partial-failure tolerance, merge, dedupe, and
//! deterministic ranking.

use std::collections::HashMap;

use super::error::DiscoveryError;
use super::provider::{DiscoveryProvider, DiscoveryProviderId, ProviderSearchResult};
use super::providers::{
    arxiv::ArxivProvider, openalex::OpenAlexProvider, semantic_scholar::SemanticScholarProvider,
};
use crate::domain::discovery::{
    paper_candidate_dedup_key, CandidateMatch, DiscoveryProviderChoice, DiscoverySearchRequest,
    DiscoverySearchResponse, OpenAccessSummary, PaperCandidate,
};

const KEYWORD_WEIGHT: f64 = 0.25;
const CITATION_WEIGHT: f64 = 0.15;
const RECENCY_WEIGHT: f64 = 0.10;
const AVAILABILITY_WEIGHT: f64 = 0.10;
const MULTI_PROVIDER_WEIGHT: f64 = 0.05;
const PROVIDER_SCORE_WEIGHT: f64 = 0.35;

#[derive(Clone)]
pub struct DiscoveryOrchestrator {
    openalex: OpenAlexProvider,
    arxiv: ArxivProvider,
    semantic_scholar: SemanticScholarProvider,
}

impl DiscoveryOrchestrator {
    pub fn new(
        openalex: OpenAlexProvider,
        arxiv: ArxivProvider,
        semantic_scholar: SemanticScholarProvider,
    ) -> Self {
        Self {
            openalex,
            arxiv,
            semantic_scholar,
        }
    }

    pub async fn search(
        &self,
        request: DiscoverySearchRequest,
    ) -> Result<DiscoverySearchResponse, DiscoveryError> {
        let query = validated_query(&request)?;
        let providers = selected_providers(&request);
        let mut results = Vec::new();
        let mut errors = Vec::new();

        for provider in providers {
            let provider_request = request_for_provider(&request, provider);
            match self.search_one(provider, provider_request).await {
                Ok(result) => results.push(result),
                Err(error) => errors.push(format!("{provider:?}: {error}")),
            }
        }

        if results.is_empty() {
            return Err(DiscoveryError::new(format!(
                "All selected providers failed: {}",
                errors.join("; ")
            )));
        }

        let filters = merged_filters(&results, &errors);
        let candidates = merge_rank_and_limit(results, &query, request.result_limit);
        let result_count = candidates.len();

        Ok(DiscoverySearchResponse {
            provider: "multi".to_string(),
            query,
            filters,
            sort_by: request.sort_by,
            result_limit: request.result_limit,
            result_count,
            candidates,
        })
    }

    async fn search_one(
        &self,
        provider: DiscoveryProviderChoice,
        request: DiscoverySearchRequest,
    ) -> Result<ProviderSearchResult, DiscoveryError> {
        match provider {
            DiscoveryProviderChoice::OpenAlex => self.openalex.search(&request).await,
            DiscoveryProviderChoice::Arxiv => self.arxiv.search(&request).await,
            DiscoveryProviderChoice::SemanticScholar => {
                self.semantic_scholar.search(&request).await
            }
        }
    }
}

pub fn merge_rank_and_limit(
    results: Vec<ProviderSearchResult>,
    query: &str,
    limit: i32,
) -> Vec<PaperCandidate> {
    let mut merged: Vec<PaperCandidate> = Vec::new();
    let mut index_by_key: HashMap<String, usize> = HashMap::new();

    for result in results {
        let provider = result.provider;
        for mut candidate in result.candidates {
            add_provider_reason(&mut candidate, provider);
            let key = paper_candidate_dedup_key(&candidate);
            if let Some(index) = index_by_key.get(&key).copied() {
                merge_candidate(&mut merged[index], candidate, provider);
            } else {
                index_by_key.insert(key, merged.len());
                merged.push(candidate);
            }
        }
    }

    rank_candidates(merged, query, limit)
}

pub fn rank_candidates(
    mut candidates: Vec<PaperCandidate>,
    query: &str,
    limit: i32,
) -> Vec<PaperCandidate> {
    for candidate in &mut candidates {
        let score = rank_score(candidate, query);
        candidate.match_summary.score = Some(score);
        candidate
            .match_summary
            .reasons
            .push(format!("rank_score:{score:.3}"));
    }

    candidates.sort_by(|left, right| {
        right
            .match_summary
            .score
            .partial_cmp(&left.match_summary.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    candidates.truncate(limit.max(0) as usize);
    candidates
}

pub fn selected_providers(request: &DiscoverySearchRequest) -> Vec<DiscoveryProviderChoice> {
    if !request.providers.is_empty() {
        return dedup_providers(request.providers.clone());
    }

    vec![request.provider.clone()]
}

fn validated_query(request: &DiscoverySearchRequest) -> Result<String, DiscoveryError> {
    let query = request.query.trim().to_string();
    if query.is_empty() {
        return Err(DiscoveryError::new(
            "Enter a search query before running discovery.",
        ));
    }
    Ok(query)
}

fn request_for_provider(
    request: &DiscoverySearchRequest,
    provider: DiscoveryProviderChoice,
) -> DiscoverySearchRequest {
    let mut next = request.clone();
    next.provider = provider;
    next.providers = Vec::new();
    next
}

fn dedup_providers(providers: Vec<DiscoveryProviderChoice>) -> Vec<DiscoveryProviderChoice> {
    let mut out = Vec::new();
    for provider in providers {
        if !out.contains(&provider) {
            out.push(provider);
        }
    }
    out
}

fn merged_filters(results: &[ProviderSearchResult], errors: &[String]) -> Vec<String> {
    let mut filters = Vec::new();
    for result in results {
        filters.push(format!("provider:{}", result.provider));
        filters.extend(result.filters.iter().cloned());
    }
    filters.extend(errors.iter().map(|error| format!("provider_error:{error}")));
    filters
}

fn add_provider_reason(candidate: &mut PaperCandidate, provider: DiscoveryProviderId) {
    let reason = format!("found_by:{}", provider.as_str());
    if !candidate.match_summary.reasons.contains(&reason) {
        candidate.match_summary.reasons.push(reason);
    }
}

fn merge_candidate(
    target: &mut PaperCandidate,
    incoming: PaperCandidate,
    provider: DiscoveryProviderId,
) {
    add_provider_reason(target, provider);
    target.authors = richer_vec(&target.authors, &incoming.authors);
    target.abstract_text = richer_option(&target.abstract_text, &incoming.abstract_text);
    target.publication_date = target
        .publication_date
        .clone()
        .or(incoming.publication_date.clone());
    target.venue = richer_option(&target.venue, &incoming.venue);
    target.citation_count = max_option(target.citation_count, incoming.citation_count);
    target.doi = target.doi.clone().or(incoming.doi);
    target.openalex_id = target.openalex_id.clone().or(incoming.openalex_id);
    target.arxiv_id = target.arxiv_id.clone().or(incoming.arxiv_id);
    target.external_url = target.external_url.clone().or(incoming.external_url);
    target.pdf_url = target.pdf_url.clone().or(incoming.pdf_url);
    target.open_access = merge_open_access(&target.open_access, &incoming.open_access);
    target.already_in_library = target.already_in_library || incoming.already_in_library;
    merge_match(&mut target.match_summary, incoming.match_summary);
}

fn merge_match(target: &mut CandidateMatch, incoming: CandidateMatch) {
    target.score = max_f64(target.score, incoming.score);
    extend_unique(&mut target.reasons, incoming.reasons);
    extend_unique(&mut target.matched_keywords, incoming.matched_keywords);
    extend_unique(
        &mut target.from_seed_paper_ids,
        incoming.from_seed_paper_ids,
    );
}

fn merge_open_access(
    left: &Option<OpenAccessSummary>,
    right: &Option<OpenAccessSummary>,
) -> Option<OpenAccessSummary> {
    match (left, right) {
        (Some(left), Some(right)) => Some(OpenAccessSummary {
            is_open_access: left.is_open_access || right.is_open_access,
            status: left.status.clone().or_else(|| right.status.clone()),
        }),
        (Some(value), None) | (None, Some(value)) => Some(value.clone()),
        (None, None) => None,
    }
}

fn richer_vec(left: &[String], right: &[String]) -> Vec<String> {
    if left.len() >= right.len() {
        left.to_vec()
    } else {
        right.to_vec()
    }
}

fn richer_option(left: &Option<String>, right: &Option<String>) -> Option<String> {
    match (left, right) {
        (Some(left), Some(right)) if right.len() > left.len() => Some(right.clone()),
        (Some(left), _) => Some(left.clone()),
        (None, Some(right)) => Some(right.clone()),
        (None, None) => None,
    }
}

fn max_option(left: Option<i32>, right: Option<i32>) -> Option<i32> {
    match (left, right) {
        (Some(left), Some(right)) => Some(left.max(right)),
        (Some(value), None) | (None, Some(value)) => Some(value),
        (None, None) => None,
    }
}

fn max_f64(left: Option<f64>, right: Option<f64>) -> Option<f64> {
    match (left, right) {
        (Some(left), Some(right)) => Some(left.max(right)),
        (Some(value), None) | (None, Some(value)) => Some(value),
        (None, None) => None,
    }
}

fn extend_unique(target: &mut Vec<String>, incoming: Vec<String>) {
    for item in incoming {
        if !target.contains(&item) {
            target.push(item);
        }
    }
}

fn rank_score(candidate: &PaperCandidate, query: &str) -> f64 {
    let provider = candidate.match_summary.score.unwrap_or(0.0).clamp(0.0, 1.0);
    let keyword = keyword_score(candidate, query);
    let citations = citation_score(candidate.citation_count);
    let recency = recency_score(candidate.year);
    let availability = availability_score(candidate);
    let multi_provider = multi_provider_score(candidate);

    PROVIDER_SCORE_WEIGHT * provider
        + KEYWORD_WEIGHT * keyword
        + CITATION_WEIGHT * citations
        + RECENCY_WEIGHT * recency
        + AVAILABILITY_WEIGHT * availability
        + MULTI_PROVIDER_WEIGHT * multi_provider
}

fn keyword_score(candidate: &PaperCandidate, query: &str) -> f64 {
    let terms = query
        .split_whitespace()
        .map(|term| term.trim_matches(|ch: char| !ch.is_alphanumeric()))
        .filter(|term| term.len() > 2)
        .map(str::to_lowercase)
        .collect::<Vec<_>>();
    if terms.is_empty() {
        return 0.0;
    }

    let haystack = format!(
        "{} {}",
        candidate.title,
        candidate.abstract_text.clone().unwrap_or_default()
    )
    .to_lowercase();
    let hits = terms
        .iter()
        .filter(|term| haystack.contains(term.as_str()))
        .count();
    hits as f64 / terms.len() as f64
}

fn citation_score(citations: Option<i32>) -> f64 {
    let citations = citations.unwrap_or(0).max(0) as f64;
    (citations.ln_1p() / 1_000f64.ln_1p()).clamp(0.0, 1.0)
}

fn recency_score(year: Option<i32>) -> f64 {
    let Some(year) = year else {
        return 0.0;
    };
    ((year - 2015) as f64 / 10.0).clamp(0.0, 1.0)
}

fn availability_score(candidate: &PaperCandidate) -> f64 {
    if candidate.pdf_url.is_some() {
        return 1.0;
    }
    if candidate
        .open_access
        .as_ref()
        .map(|access| access.is_open_access)
        .unwrap_or(false)
    {
        return 0.7;
    }
    0.0
}

fn multi_provider_score(candidate: &PaperCandidate) -> f64 {
    let count = candidate
        .match_summary
        .reasons
        .iter()
        .filter(|reason| reason.starts_with("found_by:"))
        .count();
    if count > 1 {
        1.0
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::discovery::{DiscoverySort, OpenAccessSummary};

    fn request(providers: Vec<DiscoveryProviderChoice>) -> DiscoverySearchRequest {
        DiscoverySearchRequest {
            query: "diffusion protein".to_string(),
            year_from: None,
            year_to: None,
            result_limit: 25,
            sort_by: DiscoverySort::Relevance,
            provider: DiscoveryProviderChoice::OpenAlex,
            providers,
            open_access: true,
            venues: Vec::new(),
            authors: Vec::new(),
            fields_of_study: Vec::new(),
        }
    }

    fn candidate(title: &str, doi: Option<&str>, provider: &str) -> PaperCandidate {
        PaperCandidate {
            id: format!("{provider}:{title}"),
            source_provider: provider.to_string(),
            source_id: title.to_string(),
            title: title.to_string(),
            authors: Vec::new(),
            abstract_text: Some("diffusion models for protein design".to_string()),
            year: Some(2024),
            publication_date: None,
            venue: None,
            citation_count: Some(50),
            doi: doi.map(ToString::to_string),
            openalex_id: None,
            arxiv_id: None,
            external_url: None,
            pdf_url: Some("https://example.test/paper.pdf".to_string()),
            open_access: Some(OpenAccessSummary {
                is_open_access: true,
                status: Some("gold".to_string()),
            }),
            match_summary: CandidateMatch {
                score: Some(0.5),
                reasons: Vec::new(),
                matched_keywords: Vec::new(),
                from_seed_paper_ids: Vec::new(),
            },
            already_in_library: false,
        }
    }

    #[test]
    fn selected_providers_uses_multi_provider_setting() {
        assert_eq!(
            selected_providers(&request(vec![
                DiscoveryProviderChoice::OpenAlex,
                DiscoveryProviderChoice::Arxiv,
                DiscoveryProviderChoice::OpenAlex,
            ])),
            vec![
                DiscoveryProviderChoice::OpenAlex,
                DiscoveryProviderChoice::Arxiv
            ]
        );
    }

    #[test]
    fn selected_providers_falls_back_to_legacy_single_provider() {
        assert_eq!(
            selected_providers(&request(Vec::new())),
            vec![DiscoveryProviderChoice::OpenAlex]
        );
    }

    #[test]
    fn merge_collapses_duplicate_doi_and_combines_provider_reasons() {
        let ranked = merge_rank_and_limit(
            vec![
                ProviderSearchResult {
                    provider: DiscoveryProviderId::OpenAlex,
                    filters: Vec::new(),
                    candidates: vec![candidate("Paper", Some("10.1/a"), "openalex")],
                },
                ProviderSearchResult {
                    provider: DiscoveryProviderId::Arxiv,
                    filters: Vec::new(),
                    candidates: vec![candidate("Paper preprint", Some("10.1/a"), "arxiv")],
                },
            ],
            "diffusion protein",
            25,
        );

        assert_eq!(ranked.len(), 1);
        assert!(ranked[0]
            .match_summary
            .reasons
            .contains(&"found_by:openalex".to_string()));
        assert!(ranked[0]
            .match_summary
            .reasons
            .contains(&"found_by:arxiv".to_string()));
    }

    #[test]
    fn merged_filters_preserve_provider_errors_alongside_successes() {
        let filters = merged_filters(
            &[ProviderSearchResult {
                provider: DiscoveryProviderId::OpenAlex,
                filters: vec!["year:2024".to_string()],
                candidates: vec![candidate("Paper", Some("10.1/a"), "openalex")],
            }],
            &["Arxiv: rate limited".to_string()],
        );

        assert!(filters.contains(&"provider:openalex".to_string()));
        assert!(filters.contains(&"year:2024".to_string()));
        assert!(filters.contains(&"provider_error:Arxiv: rate limited".to_string()));
    }

    #[test]
    fn ranker_prefers_keyword_and_pdf_availability() {
        let strong = candidate("Diffusion Protein Design", Some("10.1/a"), "openalex");
        let mut weak = candidate("Unrelated", Some("10.1/b"), "openalex");
        weak.abstract_text = Some("nothing about the query".to_string());
        weak.pdf_url = None;
        weak.open_access = None;

        let ranked = merge_rank_and_limit(
            vec![ProviderSearchResult {
                provider: DiscoveryProviderId::OpenAlex,
                filters: Vec::new(),
                candidates: vec![weak, strong],
            }],
            "diffusion protein",
            25,
        );

        assert_eq!(ranked[0].title, "Diffusion Protein Design");
    }
}
