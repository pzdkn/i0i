//! arXiv-to-domain normalization helpers.
//!
//! Converts raw ArxivEntry wire values into the shared PaperCandidate type.

use crate::{
    commands::discovery::providers::shared::extract_matched_keywords,
    domain::discovery::{CandidateMatch, OpenAccessSummary, PaperCandidate},
};

use super::remote::ArxivEntry;

/// Convert one arXiv entry into the app's normalized candidate shape.
pub(super) fn normalize_entry(entry: ArxivEntry, query: &str) -> PaperCandidate {
    let arxiv_id = extract_arxiv_id(&entry.id);
    let id = format!("arxiv:{arxiv_id}");
    let pdf_url = Some(arxiv_pdf_url(&arxiv_id));
    let external_url = Some(arxiv_landing_url(&arxiv_id));

    PaperCandidate {
        id,
        source_provider: "arxiv".to_string(),
        source_id: arxiv_id.clone(),
        title: if entry.title.is_empty() {
            "Untitled arXiv work".to_string()
        } else {
            entry.title
        },
        authors: entry.authors,
        abstract_text: entry.summary,
        year: extract_year(entry.published.as_deref()),
        publication_date: extract_date(entry.published.as_deref()),
        venue: entry.primary_category,
        citation_count: None,
        doi: entry.doi,
        openalex_id: None,
        arxiv_id: Some(arxiv_id),
        external_url,
        pdf_url,
        open_access: Some(OpenAccessSummary {
            is_open_access: true,
            status: Some("green".to_string()),
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

/// Strip the `http(s)://arxiv.org/abs/` prefix and version suffix from a raw
/// arXiv `<id>` URL to produce a stable canonical ID like `2309.08600`.
fn extract_arxiv_id(raw_id: &str) -> String {
    // Remove the URL prefix, keeping everything after `/abs/`.
    let id = raw_id
        .find("/abs/")
        .map(|pos| &raw_id[pos + 5..])
        .unwrap_or(raw_id);

    // Strip the version suffix (e.g. `v2`). The version starts at the last `v`
    // that is followed only by digits — this avoids stripping `v` from old-style
    // category IDs like `hep-th`.
    if let Some(v_pos) = id.rfind('v') {
        let suffix = &id[v_pos + 1..];
        if !suffix.is_empty() && suffix.chars().all(|c| c.is_ascii_digit()) {
            return id[..v_pos].to_string();
        }
    }

    id.to_string()
}

/// Construct the direct PDF URL from a canonical arXiv ID.
fn arxiv_pdf_url(arxiv_id: &str) -> String {
    format!("https://arxiv.org/pdf/{arxiv_id}")
}

/// Construct the abstract landing page URL from a canonical arXiv ID.
fn arxiv_landing_url(arxiv_id: &str) -> String {
    format!("https://arxiv.org/abs/{arxiv_id}")
}

/// Parse the 4-digit publication year from an ISO 8601 timestamp.
fn extract_year(published: Option<&str>) -> Option<i32> {
    published
        .and_then(|s| s.get(..4))
        .and_then(|y| y.parse().ok())
}

/// Parse the `YYYY-MM-DD` date prefix from an ISO 8601 timestamp.
fn extract_date(published: Option<&str>) -> Option<String> {
    published.and_then(|s| s.get(..10)).map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arxiv_id_extracted_from_http_url_with_version() {
        assert_eq!(
            extract_arxiv_id("http://arxiv.org/abs/2309.08600v2"),
            "2309.08600"
        );
    }

    #[test]
    fn arxiv_id_extracted_from_https_url_without_version() {
        assert_eq!(
            extract_arxiv_id("https://arxiv.org/abs/1706.03762"),
            "1706.03762"
        );
    }

    #[test]
    fn arxiv_id_extracted_from_old_category_format() {
        // Old-style IDs like cs/0401023v1 should also strip the version.
        assert_eq!(
            extract_arxiv_id("http://arxiv.org/abs/cs/0401023v1"),
            "cs/0401023"
        );
    }

    #[test]
    fn pdf_url_constructed_from_arxiv_id() {
        assert_eq!(
            arxiv_pdf_url("2309.08600"),
            "https://arxiv.org/pdf/2309.08600"
        );
    }

    #[test]
    fn landing_url_constructed_from_arxiv_id() {
        assert_eq!(
            arxiv_landing_url("2309.08600"),
            "https://arxiv.org/abs/2309.08600"
        );
    }

    #[test]
    fn year_parsed_from_published_timestamp() {
        assert_eq!(extract_year(Some("2023-09-15T17:48:05Z")), Some(2023));
    }

    #[test]
    fn year_returns_none_when_published_is_missing() {
        assert_eq!(extract_year(None), None);
    }

    #[test]
    fn date_parsed_from_published_timestamp() {
        assert_eq!(
            extract_date(Some("2023-09-15T17:48:05Z")),
            Some("2023-09-15".to_string())
        );
    }

    #[test]
    fn date_returns_none_when_published_is_missing() {
        assert_eq!(extract_date(None), None);
    }

    #[test]
    fn normalized_entry_is_always_open_access() {
        let entry = make_minimal_entry("http://arxiv.org/abs/2309.08600v1");
        let candidate = normalize_entry(entry, "test");
        let oa = candidate.open_access.unwrap();
        assert!(oa.is_open_access);
        assert_eq!(oa.status, Some("green".to_string()));
    }

    #[test]
    fn normalized_entry_has_no_citation_count() {
        let entry = make_minimal_entry("http://arxiv.org/abs/2309.08600v1");
        let candidate = normalize_entry(entry, "test");
        assert_eq!(candidate.citation_count, None);
    }

    #[test]
    fn normalized_entry_derives_correct_arxiv_id_and_candidate_id() {
        let entry = make_minimal_entry("http://arxiv.org/abs/2309.08600v2");
        let candidate = normalize_entry(entry, "test");
        assert_eq!(candidate.arxiv_id, Some("2309.08600".to_string()));
        assert_eq!(candidate.id, "arxiv:2309.08600");
    }

    #[test]
    fn normalized_entry_has_correct_pdf_and_landing_urls() {
        let entry = make_minimal_entry("http://arxiv.org/abs/2309.08600v2");
        let candidate = normalize_entry(entry, "test");
        assert_eq!(
            candidate.pdf_url,
            Some("https://arxiv.org/pdf/2309.08600".to_string())
        );
        assert_eq!(
            candidate.external_url,
            Some("https://arxiv.org/abs/2309.08600".to_string())
        );
    }

    fn make_minimal_entry(id: &str) -> ArxivEntry {
        ArxivEntry {
            id: id.to_string(),
            title: "Test Paper".to_string(),
            ..ArxivEntry::default()
        }
    }
}
