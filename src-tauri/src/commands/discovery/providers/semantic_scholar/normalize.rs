//! Semantic Scholar-to-domain normalization helpers.
//!
//! Converts raw SemanticScholarPaper wire values into the shared PaperCandidate
//! type. All open-access filtering happens upstream in search.rs before
//! normalization — every paper reaching this function is assumed OA.

use crate::{
    commands::discovery::providers::shared::extract_matched_keywords,
    domain::discovery::{CandidateMatch, OpenAccessSummary, PaperCandidate},
};

use super::remote::SemanticScholarPaper;

/// Convert one Semantic Scholar paper into the app's normalized candidate shape.
pub(super) fn normalize_paper(paper: SemanticScholarPaper, query: &str) -> PaperCandidate {
    let source_id = paper.paper_id.clone();
    let id = format!("semantic_scholar:{source_id}");

    let title = paper
        .title
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| "Untitled Semantic Scholar paper".to_string());

    let tldr_text = paper.tldr.and_then(|t| t.text);
    let abstract_text = paper.abstract_text.or(tldr_text);

    let authors = paper
        .authors
        .unwrap_or_default()
        .into_iter()
        .filter_map(|a| a.name)
        .collect();

    let venue = paper.venue.filter(|v| !v.is_empty());

    let doi = paper.external_ids.as_ref().and_then(|ids| ids.doi.clone());
    let arxiv_id = paper.external_ids.and_then(|ids| ids.arxiv);

    let pdf_url = paper.open_access_pdf.and_then(|pdf| pdf.url);
    let external_url = paper.url;

    let reasons = build_reasons(paper.influential_citation_count);
    let matched_keywords = extract_matched_keywords(query);

    PaperCandidate {
        id,
        source_provider: "semantic_scholar".to_string(),
        source_id,
        title,
        authors,
        abstract_text,
        year: paper.year,
        publication_date: paper.publication_date,
        venue,
        citation_count: paper.citation_count,
        doi,
        openalex_id: None,
        arxiv_id,
        external_url,
        pdf_url,
        open_access: Some(OpenAccessSummary {
            is_open_access: true,
            status: Some("open_access".to_string()),
        }),
        match_summary: CandidateMatch {
            score: None,
            reasons,
            matched_keywords,
            from_seed_paper_ids: Vec::new(),
        },
        already_in_library: false,
    }
}

/// Build human-readable match reasons from the influential citation count.
fn build_reasons(influential: Option<i32>) -> Vec<String> {
    let mut reasons = Vec::new();
    if let Some(count) = influential.filter(|&c| c > 0) {
        reasons.push(format!("{count} influential citations (Semantic Scholar)."));
    }
    reasons
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::discovery::providers::semantic_scholar::remote::{
        Author, ExternalIds, OpenAccessPdf, SemanticScholarPaper, Tldr,
    };

    fn minimal_paper(paper_id: &str) -> SemanticScholarPaper {
        SemanticScholarPaper {
            paper_id: paper_id.to_string(),
            external_ids: None,
            title: Some("Test Paper".to_string()),
            abstract_text: None,
            year: None,
            publication_date: None,
            authors: None,
            venue: None,
            citation_count: None,
            influential_citation_count: None,
            is_open_access: Some(true),
            open_access_pdf: None,
            tldr: None,
            url: None,
        }
    }

    #[test]
    fn id_and_source_id_derive_from_paper_id() {
        let paper = minimal_paper("abc123");
        let candidate = normalize_paper(paper, "test");
        assert_eq!(candidate.source_id, "abc123");
        assert_eq!(candidate.id, "semantic_scholar:abc123");
        assert_eq!(candidate.source_provider, "semantic_scholar");
    }

    #[test]
    fn abstract_used_when_present() {
        let mut paper = minimal_paper("x");
        paper.abstract_text = Some("The real abstract.".to_string());
        paper.tldr = Some(Tldr { text: Some("TLDR summary.".to_string()) });
        let candidate = normalize_paper(paper, "test");
        assert_eq!(candidate.abstract_text.as_deref(), Some("The real abstract."));
    }

    #[test]
    fn tldr_used_as_abstract_when_abstract_absent() {
        let mut paper = minimal_paper("x");
        paper.abstract_text = None;
        paper.tldr = Some(Tldr { text: Some("TLDR summary.".to_string()) });
        let candidate = normalize_paper(paper, "test");
        assert_eq!(candidate.abstract_text.as_deref(), Some("TLDR summary."));
    }

    #[test]
    fn abstract_none_when_both_absent() {
        let paper = minimal_paper("x");
        let candidate = normalize_paper(paper, "test");
        assert!(candidate.abstract_text.is_none());
    }

    #[test]
    fn authors_with_none_name_are_skipped() {
        let mut paper = minimal_paper("x");
        paper.authors = Some(vec![
            Author { name: Some("Alice".to_string()) },
            Author { name: None },
            Author { name: Some("Bob".to_string()) },
        ]);
        let candidate = normalize_paper(paper, "test");
        assert_eq!(candidate.authors, vec!["Alice", "Bob"]);
    }

    #[test]
    fn empty_venue_treated_as_none() {
        let mut paper = minimal_paper("x");
        paper.venue = Some("".to_string());
        let candidate = normalize_paper(paper, "test");
        assert!(candidate.venue.is_none());
    }

    #[test]
    fn non_empty_venue_passes_through() {
        let mut paper = minimal_paper("x");
        paper.venue = Some("ICLR".to_string());
        let candidate = normalize_paper(paper, "test");
        assert_eq!(candidate.venue.as_deref(), Some("ICLR"));
    }

    #[test]
    fn external_ids_extracted_correctly() {
        let mut paper = minimal_paper("x");
        paper.external_ids = Some(ExternalIds {
            doi: Some("10.1234/x".to_string()),
            arxiv: Some("2309.08600".to_string()),
        });
        let candidate = normalize_paper(paper, "test");
        assert_eq!(candidate.doi.as_deref(), Some("10.1234/x"));
        assert_eq!(candidate.arxiv_id.as_deref(), Some("2309.08600"));
    }

    #[test]
    fn pdf_url_comes_from_open_access_pdf() {
        let mut paper = minimal_paper("x");
        paper.open_access_pdf = Some(OpenAccessPdf {
            url: Some("https://arxiv.org/pdf/2309.08600".to_string()),
        });
        let candidate = normalize_paper(paper, "test");
        assert_eq!(candidate.pdf_url.as_deref(), Some("https://arxiv.org/pdf/2309.08600"));
    }

    #[test]
    fn candidate_is_always_marked_open_access() {
        let paper = minimal_paper("x");
        let candidate = normalize_paper(paper, "test");
        let oa = candidate.open_access.unwrap();
        assert!(oa.is_open_access);
        assert_eq!(oa.status.as_deref(), Some("open_access"));
    }

    #[test]
    fn influential_citations_appear_in_reasons() {
        let mut paper = minimal_paper("x");
        paper.influential_citation_count = Some(38);
        let candidate = normalize_paper(paper, "test");
        assert_eq!(candidate.match_summary.reasons, vec!["38 influential citations (Semantic Scholar)."]);
    }

    #[test]
    fn zero_influential_citations_produces_no_reason() {
        let mut paper = minimal_paper("x");
        paper.influential_citation_count = Some(0);
        let candidate = normalize_paper(paper, "test");
        assert!(candidate.match_summary.reasons.is_empty());
    }

    #[test]
    fn none_influential_citations_produces_no_reason() {
        let paper = minimal_paper("x");
        let candidate = normalize_paper(paper, "test");
        assert!(candidate.match_summary.reasons.is_empty());
    }

    #[test]
    fn openalex_id_is_always_none() {
        let paper = minimal_paper("x");
        let candidate = normalize_paper(paper, "test");
        assert!(candidate.openalex_id.is_none());
    }

    #[test]
    fn missing_title_uses_fallback() {
        let mut paper = minimal_paper("x");
        paper.title = None;
        let candidate = normalize_paper(paper, "test");
        assert_eq!(candidate.title, "Untitled Semantic Scholar paper");
    }
}
