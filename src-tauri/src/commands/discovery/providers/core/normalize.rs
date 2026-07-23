//! CORE-to-domain normalization (RFC 0053).
//!
//! CORE aggregates open-access works, so a result's `downloadUrl` is a direct
//! obtainable PDF that feeds ranking and RFC 0051 acquisition.

use crate::{
    commands::discovery::providers::shared::extract_matched_keywords,
    domain::discovery::{CandidateMatch, OpenAccessSummary, PaperCandidate},
};

use super::remote::CoreWork;

pub(super) fn normalize_work(work: CoreWork, query: &str) -> PaperCandidate {
    let source_id = core_id(&work.id);
    let id = format!("core:{source_id}");

    let download_url = work
        .download_url
        .filter(|url| !url.trim().is_empty())
        .map(|url| url.trim().to_string());

    let authors = work
        .authors
        .into_iter()
        .filter_map(|author| author.name)
        .map(|name| name.trim().to_string())
        .filter(|name| !name.is_empty())
        .collect();

    PaperCandidate {
        id,
        source_provider: "core".to_string(),
        source_id,
        title: work
            .title
            .filter(|title| !title.trim().is_empty())
            .unwrap_or_else(|| "Untitled CORE work".to_string()),
        authors,
        abstract_text: work.abstract_text.filter(|text| !text.trim().is_empty()),
        year: work.year_published,
        publication_date: work
            .published_date
            .and_then(|date| date.get(..10).map(str::to_string)),
        venue: work.publisher.filter(|value| !value.trim().is_empty()),
        citation_count: None,
        doi: work.doi.filter(|doi| !doi.trim().is_empty()),
        openalex_id: None,
        arxiv_id: None,
        external_url: download_url.clone(),
        pdf_url: download_url,
        // CORE aggregates open-access works exclusively.
        open_access: Some(OpenAccessSummary {
            is_open_access: true,
            status: Some("core".to_string()),
        }),
        match_summary: CandidateMatch {
            score: None,
            reasons: Vec::new(),
            matched_keywords: extract_matched_keywords(query),
            from_seed_paper_ids: Vec::new(),
        },
        already_in_library: false,
    }
}

/// CORE ids may serialize as a number or a string; normalize either to a
/// stable string, falling back to a random-free placeholder when absent.
fn core_id(raw: &Option<serde_json::Value>) -> String {
    match raw {
        Some(serde_json::Value::String(value)) if !value.trim().is_empty() => {
            value.trim().to_string()
        }
        Some(serde_json::Value::Number(number)) => number.to_string(),
        _ => "unknown".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::super::remote::CoreAuthor;
    use super::*;

    fn base_work() -> CoreWork {
        CoreWork {
            id: Some(serde_json::json!(12345)),
            title: Some("Deep learning survey".to_string()),
            authors: vec![
                CoreAuthor {
                    name: Some("Yann LeCun".to_string()),
                },
                CoreAuthor {
                    name: Some("Yoshua Bengio".to_string()),
                },
            ],
            abstract_text: Some("A survey.".to_string()),
            year_published: Some(2015),
            published_date: Some("2015-05-27T00:00:00".to_string()),
            doi: Some("10.1038/nature14539".to_string()),
            download_url: Some("https://core.ac.uk/download/12345.pdf".to_string()),
            publisher: Some("Nature".to_string()),
        }
    }

    #[test]
    fn download_url_becomes_pdf_url() {
        let candidate = normalize_work(base_work(), "deep learning");
        assert_eq!(
            candidate.pdf_url.as_deref(),
            Some("https://core.ac.uk/download/12345.pdf")
        );
    }

    #[test]
    fn results_are_marked_open_access() {
        let candidate = normalize_work(base_work(), "deep learning");
        assert!(candidate.open_access.unwrap().is_open_access);
    }

    #[test]
    fn numeric_id_normalized_to_string() {
        let candidate = normalize_work(base_work(), "deep learning");
        assert_eq!(candidate.source_id, "12345");
        assert_eq!(candidate.id, "core:12345");
    }

    #[test]
    fn authors_and_year_mapped() {
        let candidate = normalize_work(base_work(), "deep learning");
        assert_eq!(candidate.authors, vec!["Yann LeCun", "Yoshua Bengio"]);
        assert_eq!(candidate.year, Some(2015));
        assert_eq!(candidate.publication_date.as_deref(), Some("2015-05-27"));
    }

    #[test]
    fn missing_download_url_leaves_pdf_none() {
        let mut work = base_work();
        work.download_url = None;
        let candidate = normalize_work(work, "deep learning");
        assert_eq!(candidate.pdf_url, None);
        // Still open access (repository record without a direct file link).
        assert!(candidate.open_access.unwrap().is_open_access);
    }
}
