//! Multi-provider discovery orchestration (RFC 0043).
//!
//! Provider adapters still own HTTP/API details. This layer owns the search
//! quality work that should be shared by quick search and deep research:
//! provider selection, partial-failure tolerance, merge, dedupe, and
//! deterministic ranking.

use std::collections::HashMap;

use futures_util::future::join_all;

use super::error::DiscoveryError;
use super::provider::{DiscoveryProvider, DiscoveryProviderId, ProviderSearchResult};
use super::providers::{
    arxiv::ArxivProvider, core::CoreProvider, europe_pmc::EuropePmcProvider,
    openalex::OpenAlexProvider,
};
use crate::domain::discovery::{
    paper_candidate_dedup_key, CandidateMatch, DiscoveryProviderChoice, DiscoverySearchRequest,
    DiscoverySearchResponse, OpenAccessSummary, PaperCandidate,
};
use crate::services::embedding::EmbeddingReranker;

/// Legacy rank weights (no semantic signal). Used when the embedding reranker
/// is not ready, so ranking never regresses below today's behavior.
const KEYWORD_WEIGHT: f64 = 0.25;
const CITATION_WEIGHT: f64 = 0.15;
const RECENCY_WEIGHT: f64 = 0.10;
const AVAILABILITY_WEIGHT: f64 = 0.10;
const MULTI_PROVIDER_WEIGHT: f64 = 0.05;
const PROVIDER_SCORE_WEIGHT: f64 = 0.35;

/// Semantic-aware rank weights (RFC 0054). Applied when a per-candidate
/// embedding similarity is present. Meaning becomes the primary relevance term
/// and the substring keyword match drops to a lexical floor. Sums to 1.0.
const SEM_PROVIDER_WEIGHT: f64 = 0.25;
const SEM_SEMANTIC_WEIGHT: f64 = 0.30;
const SEM_KEYWORD_WEIGHT: f64 = 0.10;
const SEM_CITATION_WEIGHT: f64 = 0.10;
const SEM_RECENCY_WEIGHT: f64 = 0.10;
const SEM_AVAILABILITY_WEIGHT: f64 = 0.10;
const SEM_MULTI_PROVIDER_WEIGHT: f64 = 0.05;

/// Cap on how many candidates we embed for semantic reranking (RFC 0054). Above
/// this we keep the legacy-top-N as the working window — the tail can't survive
/// final truncation to `result_limit` anyway — which bounds embedding cost on
/// the expansion path (up to ~4 queries × 4 providers).
const SEMANTIC_RERANK_WINDOW: usize = 100;

#[derive(Clone)]
pub struct DiscoveryOrchestrator {
    openalex: OpenAlexProvider,
    arxiv: ArxivProvider,
    // Provider expansion (RFC 0053). Optional so deep research keeps its
    // OpenAlex + arXiv set without constructing providers it never queries.
    europe_pmc: Option<EuropePmcProvider>,
    core: Option<CoreProvider>,
    // Embedding reranker (RFC 0054). Defaults to disabled (legacy ranking);
    // the quick-search path attaches the app-state reranker.
    reranker: EmbeddingReranker,
}

impl DiscoveryOrchestrator {
    pub fn new(openalex: OpenAlexProvider, arxiv: ArxivProvider) -> Self {
        Self {
            openalex,
            arxiv,
            europe_pmc: None,
            core: None,
            reranker: EmbeddingReranker::disabled(),
        }
    }

    /// Attach the RFC 0053 expansion providers (Europe PMC + CORE) for the
    /// quick-search path. CORE is only queried when it reports itself
    /// configured (API key present).
    pub fn with_expansion_providers(
        mut self,
        europe_pmc: EuropePmcProvider,
        core: CoreProvider,
    ) -> Self {
        self.europe_pmc = Some(europe_pmc);
        self.core = Some(core);
        self
    }

    /// Attach the embedding reranker (RFC 0054). When the reranker is not ready
    /// (feature off / model missing), ranking transparently uses legacy weights.
    pub fn with_reranker(mut self, reranker: EmbeddingReranker) -> Self {
        self.reranker = reranker;
        self
    }

    pub async fn search(
        &self,
        request: DiscoverySearchRequest,
    ) -> Result<DiscoverySearchResponse, DiscoveryError> {
        self.search_expanded(request, Vec::new()).await
    }

    /// Like `search`, but also fans out `extra_queries` (RFC 0054 query
    /// expansion) and merges every result into one set. Ranking — keyword and
    /// semantic — is always measured against the **original** user query; the
    /// extra queries only widen recall, they don't change what "relevant" means.
    pub async fn search_expanded(
        &self,
        request: DiscoverySearchRequest,
        extra_queries: Vec<String>,
    ) -> Result<DiscoverySearchResponse, DiscoveryError> {
        let query = validated_query(&request)?;
        // Drop providers this orchestrator can't serve — an unattached Europe
        // PMC/CORE (deep research) or a CORE without an API key. Filtering here
        // (rather than erroring inside `search_one`) avoids a spurious
        // `provider_error:core` in the run summary (RFC 0053).
        let providers: Vec<DiscoveryProviderChoice> = selected_providers(&request)
            .into_iter()
            .filter(|&provider| self.provider_available(provider))
            .collect();

        if providers.is_empty() {
            return Err(DiscoveryError::new(
                "No configured providers for this search. Add an API key or enable another provider.",
            ));
        }

        // Fan out the original query plus each expansion variant. Deduplicated
        // variants that repeat the original are skipped so we don't double-fetch.
        let mut all_results = Vec::new();
        let mut errors = Vec::new();
        let (results, mut query_errors) = self.fan_out(&request, &providers).await;
        all_results.extend(results);
        errors.append(&mut query_errors);

        for extra in extra_queries {
            let trimmed = extra.trim();
            if trimmed.is_empty() || trimmed.eq_ignore_ascii_case(query.trim()) {
                continue;
            }
            let mut variant_request = request.clone();
            variant_request.query = trimmed.to_string();
            let (results, mut variant_errors) = self.fan_out(&variant_request, &providers).await;
            all_results.extend(results);
            errors.append(&mut variant_errors);
        }

        if all_results.is_empty() {
            return Err(DiscoveryError::new(format!(
                "All selected providers failed: {}",
                errors.join("; ")
            )));
        }

        let filters = merged_filters(&all_results, &errors);
        // Merge + dedupe + viewability filter, then score semantic similarity on
        // the surviving set (never on candidates we're about to drop), then rank
        // against the original query (RFC 0054). Empty score slice ⇒ legacy.
        let merged = merge_and_filter(all_results, request.only_viewable);
        // Bound embedding cost: on a large (expanded) set, keep only the
        // legacy-top-N as the working window before semantic scoring.
        let windowed = if self.reranker.is_ready() && merged.len() > SEMANTIC_RERANK_WINDOW {
            legacy_top_n(merged, &query, SEMANTIC_RERANK_WINDOW)
        } else {
            merged
        };
        let semantic_scores = self.reranker.semantic_scores(&query, &windowed).await;
        let candidates = rank_candidates(windowed, &query, request.result_limit, &semantic_scores);
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

    /// Fan one query out across the given providers concurrently (RFC 0044),
    /// returning each provider's results and any per-provider error strings.
    async fn fan_out(
        &self,
        request: &DiscoverySearchRequest,
        providers: &[DiscoveryProviderChoice],
    ) -> (Vec<ProviderSearchResult>, Vec<String>) {
        let dispatched = join_all(providers.iter().map(|&provider| {
            let provider_request = request_for_provider(request, provider);
            self.search_one(provider, provider_request)
        }))
        .await;

        let mut results = Vec::new();
        let mut errors = Vec::new();
        for (provider, result) in providers.iter().zip(dispatched) {
            match result {
                Ok(result) => results.push(result),
                Err(error) => errors.push(format!("{provider:?}: {error}")),
            }
        }
        (results, errors)
    }

    async fn search_one(
        &self,
        provider: DiscoveryProviderChoice,
        request: DiscoverySearchRequest,
    ) -> Result<ProviderSearchResult, DiscoveryError> {
        match provider {
            DiscoveryProviderChoice::OpenAlex => self.openalex.search(&request).await,
            DiscoveryProviderChoice::Arxiv => self.arxiv.search(&request).await,
            DiscoveryProviderChoice::EuropePmc => match &self.europe_pmc {
                Some(europe_pmc) => europe_pmc.search(&request).await,
                None => Err(DiscoveryError::new("Europe PMC provider is not attached.")),
            },
            DiscoveryProviderChoice::Core => match &self.core {
                Some(core) => core.search(&request).await,
                None => Err(DiscoveryError::new("CORE provider is not attached.")),
            },
        }
    }

    /// Whether this orchestrator can serve the given provider. Europe PMC and
    /// CORE must be attached (`with_expansion_providers`), and CORE must also
    /// have a resolvable API key (RFC 0053).
    fn provider_available(&self, provider: DiscoveryProviderChoice) -> bool {
        match provider {
            DiscoveryProviderChoice::OpenAlex | DiscoveryProviderChoice::Arxiv => true,
            DiscoveryProviderChoice::EuropePmc => self.europe_pmc.is_some(),
            DiscoveryProviderChoice::Core => self
                .core
                .as_ref()
                .map(CoreProvider::is_configured)
                .unwrap_or(false),
        }
    }
}

/// Merge provider results, collapse duplicates, and apply the viewability
/// filter — everything before ranking. Split out from ranking (RFC 0054) so the
/// async semantic-scoring step can run on this exact surviving set.
pub fn merge_and_filter(
    results: Vec<ProviderSearchResult>,
    only_viewable: bool,
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

    // Drop candidates with no obtainable view before ranking, so truncation to
    // `limit` never spends slots on results the user asked to hide (RFC 0053).
    if only_viewable {
        merged.retain(is_viewable);
    }

    merged
}

/// Merge, filter, and rank with no semantic signal (legacy weights). Test-only
/// convenience over `merge_and_filter` + `rank_candidates`.
#[cfg(test)]
pub fn merge_rank_and_limit(
    results: Vec<ProviderSearchResult>,
    query: &str,
    limit: i32,
    only_viewable: bool,
) -> Vec<PaperCandidate> {
    let merged = merge_and_filter(results, only_viewable);
    rank_candidates(merged, query, limit, &[])
}

/// Rank candidates in place. `semantic_scores` is index-aligned to
/// `candidates` (as produced by `EmbeddingReranker::semantic_scores`); an empty
/// or shorter slice means "no semantic signal" for the missing indices, which
/// falls back to legacy weights per candidate (RFC 0054).
/// Keep the `n` highest-scoring candidates by **legacy** rank score, without
/// annotating them (they'll be scored again for real by `rank_candidates`).
/// Used only to bound the semantic-embedding working set (RFC 0054).
fn legacy_top_n(candidates: Vec<PaperCandidate>, query: &str, n: usize) -> Vec<PaperCandidate> {
    let mut scored: Vec<(f64, PaperCandidate)> = candidates
        .into_iter()
        .map(|candidate| (rank_score(&candidate, query, None), candidate))
        .collect();
    scored.sort_by(|left, right| {
        right
            .0
            .partial_cmp(&left.0)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    scored
        .into_iter()
        .take(n)
        .map(|(_, candidate)| candidate)
        .collect()
}

pub fn rank_candidates(
    mut candidates: Vec<PaperCandidate>,
    query: &str,
    limit: i32,
    semantic_scores: &[f64],
) -> Vec<PaperCandidate> {
    for (index, candidate) in candidates.iter_mut().enumerate() {
        let semantic = semantic_scores.get(index).copied();
        let score = rank_score(candidate, query, semantic);
        candidate.match_summary.score = Some(score);
        candidate
            .match_summary
            .reasons
            .push(format!("rank_score:{score:.3}"));
        if let Some(semantic) = semantic {
            candidate
                .match_summary
                .reasons
                .push(format!("semantic:{semantic:.3}"));
        }
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

/// Blend the ranking sub-scores. When `semantic` is present (embedding reranker
/// ready) the semantic-aware weights apply, with cosine similarity as the
/// primary relevance term; otherwise the legacy weights apply so ranking never
/// regresses (RFC 0054).
fn rank_score(candidate: &PaperCandidate, query: &str, semantic: Option<f64>) -> f64 {
    let provider = candidate.match_summary.score.unwrap_or(0.0).clamp(0.0, 1.0);
    let keyword = keyword_score(candidate, query);
    let citations = citation_score(candidate.citation_count);
    let recency = recency_score(candidate.year);
    let availability = availability_score(candidate);
    let multi_provider = multi_provider_score(candidate);

    match semantic {
        Some(semantic) => {
            SEM_PROVIDER_WEIGHT * provider
                + SEM_SEMANTIC_WEIGHT * semantic.clamp(0.0, 1.0)
                + SEM_KEYWORD_WEIGHT * keyword
                + SEM_CITATION_WEIGHT * citations
                + SEM_RECENCY_WEIGHT * recency
                + SEM_AVAILABILITY_WEIGHT * availability
                + SEM_MULTI_PROVIDER_WEIGHT * multi_provider
        }
        None => {
            PROVIDER_SCORE_WEIGHT * provider
                + KEYWORD_WEIGHT * keyword
                + CITATION_WEIGHT * citations
                + RECENCY_WEIGHT * recency
                + AVAILABILITY_WEIGHT * availability
                + MULTI_PROVIDER_WEIGHT * multi_provider
        }
    }
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

/// Viewability tier as a ranking sub-score (RFC 0053):
/// Viewable (obtainable PDF/constructible arXiv copy) = 1.0, MaybeViewable
/// (open-access flag but only a landing URL) = 0.6, NotViewable = 0.0.
fn availability_score(candidate: &PaperCandidate) -> f64 {
    if candidate.pdf_url.is_some() || candidate.arxiv_id.is_some() {
        return 1.0;
    }
    if is_open_access_flagged(candidate) {
        return 0.6;
    }
    0.0
}

/// Whether we have any obtainable view for this candidate — used by the
/// `only_viewable` filter, which drops NotViewable candidates (RFC 0053).
fn is_viewable(candidate: &PaperCandidate) -> bool {
    candidate.pdf_url.is_some() || candidate.arxiv_id.is_some() || is_open_access_flagged(candidate)
}

fn is_open_access_flagged(candidate: &PaperCandidate) -> bool {
    candidate
        .open_access
        .as_ref()
        .map(|access| access.is_open_access)
        .unwrap_or(false)
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
            only_viewable: false,
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
            false,
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
            false,
        );

        assert_eq!(ranked[0].title, "Diffusion Protein Design");
    }

    #[test]
    fn both_weight_sets_sum_to_one() {
        let legacy = PROVIDER_SCORE_WEIGHT
            + KEYWORD_WEIGHT
            + CITATION_WEIGHT
            + RECENCY_WEIGHT
            + AVAILABILITY_WEIGHT
            + MULTI_PROVIDER_WEIGHT;
        let semantic = SEM_PROVIDER_WEIGHT
            + SEM_SEMANTIC_WEIGHT
            + SEM_KEYWORD_WEIGHT
            + SEM_CITATION_WEIGHT
            + SEM_RECENCY_WEIGHT
            + SEM_AVAILABILITY_WEIGHT
            + SEM_MULTI_PROVIDER_WEIGHT;
        assert!((legacy - 1.0).abs() < 1e-9, "legacy weights: {legacy}");
        assert!(
            (semantic - 1.0).abs() < 1e-9,
            "semantic weights: {semantic}"
        );
    }

    #[test]
    fn semantic_signal_lifts_an_otherwise_weak_candidate() {
        // A candidate with no keyword overlap and no citations should still
        // score higher when its semantic similarity is high vs. low.
        let mut paper = candidate("Retrieval over documents", Some("10.1/a"), "openalex");
        paper.abstract_text = Some("dense retrieval".to_string());
        paper.citation_count = Some(0);
        paper.pdf_url = None;
        paper.open_access = None;

        let high = rank_score(&paper, "llm document search", Some(0.95));
        let low = rank_score(&paper, "llm document search", Some(0.05));
        let legacy = rank_score(&paper, "llm document search", None);
        assert!(high > low, "high semantic should win: {high} vs {low}");
        assert!(high > legacy, "semantic signal should lift above legacy");
    }

    #[test]
    fn legacy_top_n_keeps_highest_scoring_and_caps() {
        // Strong candidate (keyword + pdf + citations) must survive the window;
        // weak one is dropped when n = 1.
        let strong = candidate("diffusion protein design", Some("10.1/a"), "openalex");
        let mut weak = candidate("unrelated topic", Some("10.1/b"), "openalex");
        weak.abstract_text = Some("nothing relevant".to_string());
        weak.pdf_url = None;
        weak.open_access = None;
        weak.citation_count = Some(0);

        let kept = legacy_top_n(vec![weak, strong], "diffusion protein", 1);
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].title, "diffusion protein design");
        // Windowing must not annotate reasons: real scoring happens later in
        // rank_candidates, which is where rank_score/semantic reasons get added.
        assert!(!kept[0]
            .match_summary
            .reasons
            .iter()
            .any(|reason| reason.starts_with("rank_score:")));
    }

    #[test]
    fn semantic_scores_are_index_aligned_in_rank_candidates() {
        // Two candidates; only the second gets a high semantic score. It should
        // rank first even though both are otherwise identical.
        let a = candidate("Paper A", Some("10.1/a"), "openalex");
        let b = candidate("Paper B", Some("10.1/b"), "openalex");
        let ranked = rank_candidates(vec![a, b], "query", 25, &[0.0, 1.0]);
        assert_eq!(ranked[0].title, "Paper B");
        assert!(ranked[0]
            .match_summary
            .reasons
            .iter()
            .any(|reason| reason.starts_with("semantic:")));
    }
}
