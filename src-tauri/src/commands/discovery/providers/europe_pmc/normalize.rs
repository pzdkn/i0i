//! Europe PMC-to-domain normalization (RFC 0053).
//!
//! Converts a raw Europe PMC result into the shared PaperCandidate, choosing the
//! best obtainable full-text location (OA PDF > HTML > landing) so downstream
//! ranking and RFC 0051 acquisition start from a verified-openable URL.

use crate::{
    commands::discovery::providers::shared::extract_matched_keywords,
    domain::discovery::{CandidateMatch, OpenAccessSummary, PaperCandidate},
};

use super::remote::{EuropePmcResult, FullTextUrl};

pub(super) fn normalize_result(result: EuropePmcResult, query: &str) -> PaperCandidate {
    let is_open_access = result
        .is_open_access
        .as_deref()
        .map(|flag| flag.eq_ignore_ascii_case("y"))
        .unwrap_or(false);

    // Borrow the whole struct for author extraction before moving any field out.
    let authors = normalize_authors(&result);

    let full_text_urls = result
        .full_text_url_list
        .map(|list| list.full_text_url)
        .unwrap_or_default();
    let pdf_url = best_pdf_url(&full_text_urls);
    let external_url = best_landing_url(&full_text_urls).or_else(|| pdf_url.clone());

    let venue = result
        .journal_info
        .and_then(|info| info.journal)
        .and_then(|journal| journal.title);

    let source_id = europe_pmc_source_id(&result.pmcid, &result.pmid, &result.id);
    let id = format!("europe_pmc:{source_id}");

    PaperCandidate {
        id,
        source_provider: "europe_pmc".to_string(),
        source_id,
        title: result
            .title
            .filter(|title| !title.trim().is_empty())
            .unwrap_or_else(|| "Untitled Europe PMC work".to_string()),
        authors,
        abstract_text: result.abstract_text.filter(|text| !text.trim().is_empty()),
        year: result.pub_year.as_deref().and_then(parse_year),
        publication_date: None,
        venue,
        citation_count: None,
        doi: result.doi.filter(|doi| !doi.trim().is_empty()),
        openalex_id: None,
        arxiv_id: None,
        external_url,
        pdf_url,
        open_access: Some(OpenAccessSummary {
            is_open_access,
            status: Some("europe_pmc".to_string()),
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

/// Prefer author `fullName`s from the structured list; fall back to splitting
/// the flat `authorString` (`"Cheng Y, Zhu G, Zhou X."`).
fn normalize_authors(result: &EuropePmcResult) -> Vec<String> {
    if let Some(list) = &result.author_list {
        let names: Vec<String> = list
            .author
            .iter()
            .filter_map(|author| author.full_name.clone())
            .map(|name| name.trim().to_string())
            .filter(|name| !name.is_empty())
            .collect();
        if !names.is_empty() {
            return names;
        }
    }

    result
        .author_string
        .as_deref()
        .map(|raw| {
            raw.split(',')
                .map(|name| name.trim().trim_end_matches('.').trim().to_string())
                .filter(|name| !name.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

/// Choose the best directly-downloadable PDF: an open-access / free PDF link.
fn best_pdf_url(urls: &[FullTextUrl]) -> Option<String> {
    urls.iter()
        .find(|entry| is_pdf(entry) && is_free(entry))
        .and_then(|entry| entry.url.clone())
        .filter(|url| !url.trim().is_empty())
}

/// Choose a readable landing/HTML page, preferring free/open-access HTML.
fn best_landing_url(urls: &[FullTextUrl]) -> Option<String> {
    urls.iter()
        .find(|entry| is_html(entry) && is_free(entry))
        .or_else(|| urls.iter().find(|entry| is_html(entry)))
        .and_then(|entry| entry.url.clone())
        .filter(|url| !url.trim().is_empty())
}

fn is_pdf(entry: &FullTextUrl) -> bool {
    entry
        .document_style
        .as_deref()
        .map(|style| style.eq_ignore_ascii_case("pdf"))
        .unwrap_or(false)
}

fn is_html(entry: &FullTextUrl) -> bool {
    entry
        .document_style
        .as_deref()
        .map(|style| style.eq_ignore_ascii_case("html"))
        .unwrap_or(false)
}

fn is_free(entry: &FullTextUrl) -> bool {
    entry
        .availability
        .as_deref()
        .map(|availability| {
            let availability = availability.to_ascii_lowercase();
            availability.contains("open access") || availability.contains("free")
        })
        .unwrap_or(false)
}

/// Stable id: prefer PMCID (uniquely identifies an OA full-text copy), then
/// PMID, then the raw record id.
fn europe_pmc_source_id(pmcid: &Option<String>, pmid: &Option<String>, id: &str) -> String {
    pmcid
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| pmid.as_deref().filter(|value| !value.trim().is_empty()))
        .map(str::to_string)
        .unwrap_or_else(|| id.to_string())
}

fn parse_year(raw: &str) -> Option<i32> {
    raw.trim().get(..4).and_then(|year| year.parse().ok())
}

#[cfg(test)]
mod tests {
    use super::super::remote::{
        Author, AuthorList, FullTextUrl, FullTextUrlList, Journal, JournalInfo,
    };
    use super::*;

    fn ft(availability: &str, style: &str, url: &str) -> FullTextUrl {
        FullTextUrl {
            availability: Some(availability.to_string()),
            document_style: Some(style.to_string()),
            url: Some(url.to_string()),
        }
    }

    fn base_result() -> EuropePmcResult {
        EuropePmcResult {
            id: "42343087".to_string(),
            pmid: Some("42343087".to_string()),
            pmcid: Some("PMC13294356".to_string()),
            doi: Some("10.1038/s41421-026-00903-7".to_string()),
            title: Some("A CRISPR study".to_string()),
            author_string: Some("Cheng Y, Zhu G, Zhou X.".to_string()),
            author_list: None,
            journal_info: Some(JournalInfo {
                journal: Some(Journal {
                    title: Some("Cell Discovery".to_string()),
                }),
            }),
            pub_year: Some("2026".to_string()),
            abstract_text: Some("An abstract.".to_string()),
            is_open_access: Some("Y".to_string()),
            full_text_url_list: Some(FullTextUrlList {
                full_text_url: vec![
                    ft("Subscription required", "doi", "https://doi.org/10.1038/x"),
                    ft(
                        "Open access",
                        "html",
                        "https://europepmc.org/articles/PMC13294356",
                    ),
                    ft(
                        "Open access",
                        "pdf",
                        "https://europepmc.org/articles/PMC13294356?pdf=render",
                    ),
                ],
            }),
        }
    }

    #[test]
    fn picks_open_access_pdf_as_pdf_url() {
        let candidate = normalize_result(base_result(), "crispr");
        assert_eq!(
            candidate.pdf_url.as_deref(),
            Some("https://europepmc.org/articles/PMC13294356?pdf=render")
        );
    }

    #[test]
    fn picks_open_access_html_as_landing_url() {
        let candidate = normalize_result(base_result(), "crispr");
        assert_eq!(
            candidate.external_url.as_deref(),
            Some("https://europepmc.org/articles/PMC13294356")
        );
    }

    #[test]
    fn open_access_flag_parsed_from_y_string() {
        let candidate = normalize_result(base_result(), "crispr");
        assert!(candidate.open_access.unwrap().is_open_access);
    }

    #[test]
    fn subscription_only_record_has_no_pdf_url() {
        let mut result = base_result();
        result.is_open_access = Some("N".to_string());
        result.full_text_url_list = Some(FullTextUrlList {
            full_text_url: vec![ft("Subscription required", "pdf", "https://paywall/x.pdf")],
        });
        let candidate = normalize_result(result, "crispr");
        assert_eq!(candidate.pdf_url, None);
        assert!(!candidate.open_access.unwrap().is_open_access);
    }

    #[test]
    fn authors_split_from_author_string() {
        let candidate = normalize_result(base_result(), "crispr");
        assert_eq!(candidate.authors, vec!["Cheng Y", "Zhu G", "Zhou X"]);
    }

    #[test]
    fn authors_prefer_structured_full_names() {
        let mut result = base_result();
        result.author_list = Some(AuthorList {
            author: vec![
                Author {
                    full_name: Some("Yang Cheng".to_string()),
                },
                Author {
                    full_name: Some("Gang Zhu".to_string()),
                },
            ],
        });
        let candidate = normalize_result(result, "crispr");
        assert_eq!(candidate.authors, vec!["Yang Cheng", "Gang Zhu"]);
    }

    #[test]
    fn source_id_prefers_pmcid_and_year_parsed() {
        let candidate = normalize_result(base_result(), "crispr");
        assert_eq!(candidate.source_id, "PMC13294356");
        assert_eq!(candidate.id, "europe_pmc:PMC13294356");
        assert_eq!(candidate.year, Some(2026));
    }
}
