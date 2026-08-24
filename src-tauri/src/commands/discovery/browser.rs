//! Browser-first scholarly candidate discovery (RFC 0098).
//!
//! Obscura finds result links. Provider APIs are consulted only after a result
//! has a DOI, arXiv identifier, or an exact title that can be verified. The
//! output is the existing `PaperCandidate`, so every caller shares the current
//! deduplication and ranking pipeline.

use std::fs;
use std::path::PathBuf;

use futures_util::{stream, StreamExt};
use regex::Regex;
use scraper::{Html, Selector};
use sha2::{Digest, Sha256};
use tauri::Manager;
use url::Url;

use super::error::DiscoveryError;
use super::provider::{DiscoveryProvider, DiscoveryProviderId, ProviderSearchResult};
use super::providers::{arxiv::ArxivProvider, openalex::OpenAlexProvider};
use crate::domain::discovery::{
    CandidateMatch, DiscoverySearchRequest, DiscoverySort, PaperCandidate,
};
use crate::services::source_acquisition::SourceAcquisitionService;

const DEFAULT_SEARCH_URL: &str = "https://scholar.google.com/scholar?q={query}&start={offset}";
const RESOLUTION_CONCURRENCY: usize = 4;

#[derive(Debug, Clone)]
pub struct BrowserDiscoveryConfig {
    /// URL template for the browser search entry point.
    pub search_url_template: String,
    /// Number of result pages read for one query.
    pub pages_per_query: usize,
}

impl Default for BrowserDiscoveryConfig {
    fn default() -> Self {
        Self {
            search_url_template: DEFAULT_SEARCH_URL.to_string(),
            pages_per_query: 1,
        }
    }
}

impl BrowserDiscoveryConfig {
    pub fn load(app: &tauri::AppHandle) -> Self {
        let mut config = Self::default();
        for path in candidate_config_paths(app) {
            if let Ok(contents) = fs::read_to_string(path) {
                apply_config(&mut config, &contents);
                break;
            }
        }
        config
    }

    fn search_url(&self, query: &str, page: usize) -> Result<String, DiscoveryError> {
        if !self.search_url_template.contains("{query}") {
            return Err(DiscoveryError::new(
                "[discovery].browser_search_url must contain {query}",
            ));
        }
        let encoded: String = url::form_urlencoded::byte_serialize(query.as_bytes()).collect();
        Ok(self
            .search_url_template
            .replace("{query}", &encoded)
            .replace("{page}", &page.to_string())
            .replace("{offset}", &(page * 10).to_string()))
    }
}

#[derive(Debug, Clone)]
pub enum BrowserDiscoveryProgress {
    SearchingWeb,
    Provisional(Vec<PaperCandidate>),
    ResolvingMetadata { count: usize },
    Resolved { count: usize },
}

#[derive(Clone)]
pub struct BrowserDiscoverySource {
    browser: SourceAcquisitionService,
    openalex: OpenAlexProvider,
    arxiv: ArxivProvider,
    config: BrowserDiscoveryConfig,
}

impl BrowserDiscoverySource {
    pub fn new(
        browser: SourceAcquisitionService,
        openalex: OpenAlexProvider,
        arxiv: ArxivProvider,
        config: BrowserDiscoveryConfig,
    ) -> Self {
        Self {
            browser,
            openalex,
            arxiv,
            config,
        }
    }

    /// Discover provisional links, publish them, then enrich identities within
    /// a small concurrency bound. A failed page or resolver does not discard
    /// candidates obtained from the other pages.
    pub async fn discover(
        &self,
        query: &str,
        limit: usize,
        on_progress: &(dyn Fn(BrowserDiscoveryProgress) + Send + Sync),
    ) -> Result<Vec<PaperCandidate>, DiscoveryError> {
        on_progress(BrowserDiscoveryProgress::SearchingWeb);
        let mut provisional = Vec::new();
        let mut page_errors = Vec::new();

        for page in 0..self.config.pages_per_query {
            let url = self.config.search_url(query, page)?;
            match self.browser.inspect_browser_page(&url).await {
                Ok(inspection) => {
                    let html = inspection.snapshot.html.as_deref().unwrap_or_default();
                    provisional.extend(parse_search_results(html, query, limit));
                }
                Err(error) => page_errors.push(error.to_string()),
            }
            if provisional.len() >= limit {
                break;
            }
        }

        provisional = crate::services::research::dedup::dedup(provisional);
        provisional.truncate(limit);
        if provisional.is_empty() {
            return Err(DiscoveryError::new(if page_errors.is_empty() {
                "Browser search returned no scholarly links".to_string()
            } else {
                format!("Browser search failed: {}", page_errors.join("; "))
            }));
        }

        on_progress(BrowserDiscoveryProgress::Provisional(provisional.clone()));
        on_progress(BrowserDiscoveryProgress::ResolvingMetadata {
            count: provisional.len(),
        });

        let resolved = stream::iter(
            provisional
                .into_iter()
                .map(|candidate| async move { self.resolve_candidate(candidate).await }),
        )
        .buffer_unordered(RESOLUTION_CONCURRENCY)
        .collect::<Vec<_>>()
        .await;
        let resolved = crate::services::research::dedup::dedup(resolved);
        on_progress(BrowserDiscoveryProgress::Resolved {
            count: resolved.len(),
        });
        Ok(resolved)
    }

    async fn resolve_candidate(&self, provisional: PaperCandidate) -> PaperCandidate {
        let identifier = candidate_identifier(&provisional);
        let mut request = exact_request(
            identifier.as_deref().unwrap_or(&provisional.title),
            identifier.is_none(),
        );

        let resolved = if provisional.arxiv_id.is_some() {
            request.provider = crate::domain::discovery::DiscoveryProviderChoice::Arxiv;
            self.arxiv.search(&request).await.ok()
        } else {
            self.openalex.search(&request).await.ok()
        };

        let Some(candidate) = resolved
            .into_iter()
            .flat_map(|result| result.candidates)
            .find(|candidate| identity_matches(&provisional, candidate))
        else {
            return provisional;
        };

        merge_resolved_candidate(provisional, candidate)
    }

    pub fn as_provider_result(candidates: Vec<PaperCandidate>) -> ProviderSearchResult {
        ProviderSearchResult {
            provider: DiscoveryProviderId::Web,
            filters: vec![
                "browser-first".to_string(),
                "exact metadata resolution".to_string(),
            ],
            candidates,
        }
    }
}

fn exact_request(query: &str, quote: bool) -> DiscoverySearchRequest {
    DiscoverySearchRequest {
        query: if quote {
            format!("\"{}\"", query.trim().trim_matches('"'))
        } else {
            query.to_string()
        },
        year_from: None,
        year_to: None,
        result_limit: 5,
        sort_by: DiscoverySort::Relevance,
        provider: Default::default(),
        providers: Vec::new(),
        open_access: false,
        only_viewable: false,
        venues: Vec::new(),
        authors: Vec::new(),
        fields_of_study: Vec::new(),
    }
}

fn parse_search_results(html: &str, query: &str, limit: usize) -> Vec<PaperCandidate> {
    let document = Html::parse_document(html);
    let scholar_result = Selector::parse(".gs_ri").expect("valid selector");
    let scholar_title = Selector::parse(".gs_rt a").expect("valid selector");
    let scholar_snippet = Selector::parse(".gs_rs").expect("valid selector");
    let mut results = Vec::new();

    for row in document.select(&scholar_result) {
        let Some(link) = row.select(&scholar_title).next() else {
            continue;
        };
        let Some(url) = link.value().attr("href").and_then(normalize_result_url) else {
            continue;
        };
        let title = clean_title(&link.text().collect::<Vec<_>>().join(" "));
        let snippet = row
            .select(&scholar_snippet)
            .next()
            .map(|node| clean_text(&node.text().collect::<Vec<_>>().join(" ")));
        if let Some(candidate) = provisional_candidate(title, url, snippet, query, results.len()) {
            results.push(candidate);
        }
        if results.len() >= limit {
            return results;
        }
    }

    if !results.is_empty() {
        return results;
    }

    let anchors = Selector::parse("a[href]").expect("valid selector");
    for link in document.select(&anchors) {
        let Some(url) = link.value().attr("href").and_then(normalize_result_url) else {
            continue;
        };
        let title = clean_title(&link.text().collect::<Vec<_>>().join(" "));
        if let Some(candidate) = provisional_candidate(title, url, None, query, results.len()) {
            results.push(candidate);
        }
        if results.len() >= limit {
            break;
        }
    }
    results
}

fn provisional_candidate(
    title: String,
    url: String,
    snippet: Option<String>,
    query: &str,
    position: usize,
) -> Option<PaperCandidate> {
    if title.len() < 12 || !looks_like_external_result(&url) {
        return None;
    }
    let source_id = short_hash(&url);
    let arxiv_id = extract_arxiv_id(&url);
    let doi = extract_doi(&url);
    let pdf_url = is_pdf_url(&url).then(|| url.clone());
    Some(PaperCandidate {
        id: format!("web:{source_id}"),
        source_provider: "web".to_string(),
        source_id,
        title,
        authors: Vec::new(),
        abstract_text: snippet,
        year: None,
        publication_date: None,
        venue: None,
        citation_count: None,
        doi,
        openalex_id: None,
        arxiv_id,
        external_url: Some(url),
        pdf_url,
        open_access: None,
        match_summary: CandidateMatch {
            score: Some((1.0 - position as f64 * 0.03).max(0.1)),
            reasons: vec!["discovered:web".to_string()],
            matched_keywords: query.split_whitespace().map(str::to_string).collect(),
            from_seed_paper_ids: Vec::new(),
        },
        already_in_library: false,
    })
}

fn merge_resolved_candidate(
    provisional: PaperCandidate,
    mut resolved: PaperCandidate,
) -> PaperCandidate {
    // The row already exists before resolution. Keep its identity so richer
    // metadata updates that row instead of creating visible churn.
    resolved.id = provisional.id.clone();
    if resolved.external_url.is_none() {
        resolved.external_url = provisional.external_url;
    }
    if resolved.pdf_url.is_none() {
        resolved.pdf_url = provisional.pdf_url;
    }
    resolved
        .match_summary
        .reasons
        .retain(|reason| !reason.starts_with("provider:"));
    resolved.match_summary.reasons.extend([
        "discovered:web".to_string(),
        format!("resolved:{}", resolved.source_provider),
    ]);
    resolved
}

fn identity_matches(provisional: &PaperCandidate, resolved: &PaperCandidate) -> bool {
    if let (Some(expected), Some(actual)) = (&provisional.doi, &resolved.doi) {
        return normalized_identifier(expected) == normalized_identifier(actual);
    }
    if let (Some(expected), Some(actual)) = (&provisional.arxiv_id, &resolved.arxiv_id) {
        return normalized_identifier(expected) == normalized_identifier(actual);
    }
    normalize_title(&provisional.title) == normalize_title(&resolved.title)
}

fn candidate_identifier(candidate: &PaperCandidate) -> Option<String> {
    candidate
        .doi
        .as_ref()
        .map(|doi| format!("doi:{doi}"))
        .or_else(|| candidate.arxiv_id.as_ref().map(|id| format!("id:{id}")))
}

fn normalize_title(title: &str) -> String {
    title
        .chars()
        .filter(|character| character.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn normalized_identifier(identifier: &str) -> String {
    identifier
        .trim()
        .trim_start_matches("https://doi.org/")
        .to_lowercase()
}

fn normalize_result_url(raw: &str) -> Option<String> {
    let absolute = Url::parse(raw).ok()?;
    if let Some(target) = absolute
        .query_pairs()
        .find(|(key, _)| key == "q" || key == "url")
        .map(|(_, value)| value.into_owned())
        .filter(|value| value.starts_with("http://") || value.starts_with("https://"))
    {
        return Some(target);
    }
    Some(absolute.into())
}

fn looks_like_external_result(url: &str) -> bool {
    let Ok(url) = Url::parse(url) else {
        return false;
    };
    let host = url.host_str().unwrap_or_default();
    !host.is_empty()
        && !host.ends_with("google.com")
        && !host.ends_with("googleusercontent.com")
        && !host.ends_with("gstatic.com")
}

fn extract_doi(url: &str) -> Option<String> {
    let pattern = Regex::new(r"(?i)10\.\d{4,9}/[-._;()/:a-z0-9]+ ").expect("valid DOI regex");
    pattern
        .find(&format!("{url} "))
        .map(|matched| matched.as_str().trim().trim_end_matches('.').to_lowercase())
}

fn extract_arxiv_id(url: &str) -> Option<String> {
    let pattern = Regex::new(r"(?i)arxiv\.org/(?:abs|pdf)/([^?#]+)").expect("valid arXiv regex");
    pattern
        .captures(url)
        .and_then(|captures| captures.get(1))
        .map(|matched| matched.as_str().trim_end_matches(".pdf").to_string())
}

fn is_pdf_url(url: &str) -> bool {
    let lower = url.to_lowercase();
    lower.ends_with(".pdf") || lower.contains("/pdf/") || lower.contains("/pdf?")
}

fn clean_title(value: &str) -> String {
    clean_text(value)
        .trim_start_matches("[PDF]")
        .trim_start_matches("[HTML]")
        .trim()
        .to_string()
}

fn clean_text(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn short_hash(value: &str) -> String {
    let digest = Sha256::digest(value.as_bytes());
    digest[..6]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn candidate_config_paths(app: &tauri::AppHandle) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Ok(cwd) = std::env::current_dir() {
        paths.push(cwd.join("i0i.config.toml"));
    }
    if let Ok(config_dir) = app.path().app_config_dir() {
        paths.push(config_dir.join("i0i.config.toml"));
    }
    paths
}

fn apply_config(config: &mut BrowserDiscoveryConfig, contents: &str) {
    let mut in_discovery = false;
    for line in contents.lines().map(str::trim) {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            in_discovery = line == "[discovery]";
            continue;
        }
        if !in_discovery {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.trim().trim_matches('"');
        match key.trim() {
            "browser_search_url" => config.search_url_template = value.to_string(),
            "browser_pages_per_query" => {
                if let Ok(pages) = value.parse::<usize>() {
                    config.pages_per_query = pages.clamp(1, 2);
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Deserialize)]
    struct CorpusCase {
        goal: String,
        expected_title: String,
        html: String,
    }

    #[test]
    fn parses_google_scholar_results_as_provisional_candidates() {
        let html = r#"
            <div class="gs_ri">
              <h3 class="gs_rt"><a href="https://doi.org/10.1000/example">A Useful Research Paper</a></h3>
              <div class="gs_rs">A concise abstract snippet.</div>
            </div>
        "#;
        let candidates = parse_search_results(html, "useful research", 10);
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].doi.as_deref(), Some("10.1000/example"));
        assert_eq!(candidates[0].source_provider, "web");
        assert_eq!(
            candidates[0].abstract_text.as_deref(),
            Some("A concise abstract snippet.")
        );
    }

    #[test]
    fn exact_title_verification_ignores_case_and_punctuation() {
        let provisional = provisional_candidate(
            "Attention Is All You Need".to_string(),
            "https://example.test/paper".to_string(),
            None,
            "attention",
            0,
        )
        .unwrap();
        let mut resolved = provisional.clone();
        resolved.title = "Attention is all you need!".to_string();
        assert!(identity_matches(&provisional, &resolved));
    }

    #[test]
    fn config_builds_bounded_paginated_search_urls() {
        let config = BrowserDiscoveryConfig::default();
        let url = config.search_url("graph neural networks", 1).unwrap();
        assert!(url.contains("graph+neural+networks"));
        assert!(url.contains("start=10"));
    }

    #[test]
    fn recorded_corpus_retains_every_hand_marked_result() {
        let cases: Vec<CorpusCase> = serde_json::from_str(include_str!(
            "../../../tests/fixtures/browser_discovery_corpus.json"
        ))
        .expect("valid browser discovery corpus");
        assert_eq!(cases.len(), 10);

        let found = cases
            .iter()
            .filter(|case| {
                parse_search_results(&case.html, &case.goal, 10)
                    .iter()
                    .any(|candidate| {
                        normalize_title(&candidate.title) == normalize_title(&case.expected_title)
                    })
            })
            .count();

        assert_eq!(
            found,
            cases.len(),
            "recorded corpus recall must remain 100%"
        );
    }
}
