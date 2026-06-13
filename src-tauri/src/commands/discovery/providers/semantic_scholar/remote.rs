//! Semantic Scholar Graph API wire types.
//!
//! The API returns JSON. All fields are optional except `paperId` so callers
//! can handle partial responses without failing the whole batch.

use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub(super) struct SemanticScholarResponse {
    pub total: Option<u32>,
    pub data: Vec<SemanticScholarPaper>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct SemanticScholarPaper {
    pub paper_id: String,
    pub external_ids: Option<ExternalIds>,
    pub title: Option<String>,
    #[serde(rename = "abstract")]
    pub abstract_text: Option<String>,
    pub year: Option<i32>,
    pub publication_date: Option<String>,
    pub authors: Option<Vec<Author>>,
    pub venue: Option<String>,
    pub citation_count: Option<i32>,
    pub influential_citation_count: Option<i32>,
    pub is_open_access: Option<bool>,
    pub open_access_pdf: Option<OpenAccessPdf>,
    pub tldr: Option<Tldr>,
    pub url: Option<String>,
}

/// External identifier bag. Field names match the Semantic Scholar wire format
/// exactly — PascalCase keys are intentional.
#[derive(Debug, Deserialize)]
pub(super) struct ExternalIds {
    #[serde(rename = "DOI")]
    pub doi: Option<String>,
    #[serde(rename = "ArXiv")]
    pub arxiv: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct Author {
    pub name: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct OpenAccessPdf {
    pub url: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct Tldr {
    pub text: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    const FULL_PAPER_JSON: &str = r#"{
        "paperId": "a23b4c5d6e7f8g9h",
        "externalIds": { "DOI": "10.1234/example", "ArXiv": "2309.08600" },
        "title": "Sparse Autoencoders Find Highly Interpretable Features",
        "abstract": "We show that sparse autoencoders can find interpretable features.",
        "year": 2023,
        "publicationDate": "2023-09-15",
        "authors": [
            { "name": "Hoagy Cunningham" },
            { "name": "Aidan Beren" }
        ],
        "venue": "ICLR",
        "citationCount": 412,
        "influentialCitationCount": 38,
        "isOpenAccess": true,
        "openAccessPdf": { "url": "https://arxiv.org/pdf/2309.08600" },
        "tldr": { "model": "tldr@v2.0.0", "text": "Sparse autoencoders recover interpretable features." },
        "url": "https://www.semanticscholar.org/paper/a23b4c5d"
    }"#;

    #[test]
    fn full_paper_deserializes_all_fields() {
        let paper: SemanticScholarPaper = serde_json::from_str(FULL_PAPER_JSON).unwrap();
        assert_eq!(paper.paper_id, "a23b4c5d6e7f8g9h");
        assert_eq!(paper.title.as_deref(), Some("Sparse Autoencoders Find Highly Interpretable Features"));
        assert_eq!(paper.abstract_text.as_deref(), Some("We show that sparse autoencoders can find interpretable features."));
        assert_eq!(paper.year, Some(2023));
        assert_eq!(paper.publication_date.as_deref(), Some("2023-09-15"));
        assert_eq!(paper.venue.as_deref(), Some("ICLR"));
        assert_eq!(paper.citation_count, Some(412));
        assert_eq!(paper.influential_citation_count, Some(38));
        assert_eq!(paper.is_open_access, Some(true));
        assert_eq!(paper.url.as_deref(), Some("https://www.semanticscholar.org/paper/a23b4c5d"));
    }

    #[test]
    fn external_ids_reads_pascal_case_keys() {
        let paper: SemanticScholarPaper = serde_json::from_str(FULL_PAPER_JSON).unwrap();
        let ids = paper.external_ids.unwrap();
        assert_eq!(ids.doi.as_deref(), Some("10.1234/example"));
        assert_eq!(ids.arxiv.as_deref(), Some("2309.08600"));
    }

    #[test]
    fn authors_list_parses_correctly() {
        let paper: SemanticScholarPaper = serde_json::from_str(FULL_PAPER_JSON).unwrap();
        let authors = paper.authors.unwrap();
        assert_eq!(authors.len(), 2);
        assert_eq!(authors[0].name.as_deref(), Some("Hoagy Cunningham"));
        assert_eq!(authors[1].name.as_deref(), Some("Aidan Beren"));
    }

    #[test]
    fn open_access_pdf_url_parses() {
        let paper: SemanticScholarPaper = serde_json::from_str(FULL_PAPER_JSON).unwrap();
        let pdf = paper.open_access_pdf.unwrap();
        assert_eq!(pdf.url.as_deref(), Some("https://arxiv.org/pdf/2309.08600"));
    }

    #[test]
    fn tldr_text_parses() {
        let paper: SemanticScholarPaper = serde_json::from_str(FULL_PAPER_JSON).unwrap();
        let tldr = paper.tldr.unwrap();
        assert_eq!(tldr.text.as_deref(), Some("Sparse autoencoders recover interpretable features."));
    }

    #[test]
    fn paper_with_all_optional_fields_absent_deserializes() {
        let json = r#"{ "paperId": "minimal123" }"#;
        let paper: SemanticScholarPaper = serde_json::from_str(json).unwrap();
        assert_eq!(paper.paper_id, "minimal123");
        assert!(paper.title.is_none());
        assert!(paper.abstract_text.is_none());
        assert!(paper.year.is_none());
        assert!(paper.authors.is_none());
        assert!(paper.external_ids.is_none());
        assert!(paper.tldr.is_none());
        assert!(paper.is_open_access.is_none());
    }

    #[test]
    fn response_wrapper_deserializes() {
        let json = r#"{
            "total": 1234,
            "data": [{ "paperId": "abc" }]
        }"#;
        let resp: SemanticScholarResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.total, Some(1234));
        assert_eq!(resp.data.len(), 1);
    }
}
