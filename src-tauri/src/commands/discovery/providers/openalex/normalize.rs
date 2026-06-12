//! OpenAlex-to-domain normalization helpers.
//!
//! This module translates provider-local OpenAlex wire types into the app's
//! shared discovery domain type, `PaperCandidate`. The goal is to keep OpenAlex
//! naming and fallback rules contained here so the rest of the app can work
//! with one stable candidate shape.

use std::collections::HashMap;

use crate::domain::discovery::{CandidateMatch, OpenAccessSummary, PaperCandidate};

use super::remote::OpenAlexWork;

/// Convert one raw OpenAlex work record into the app's normalized candidate
/// shape used by discovery UI and save flows.
pub fn normalize_work(work: OpenAlexWork, query: &str) -> PaperCandidate {
    // Derive the stable provider-local identity first so downstream fields can
    // reuse it consistently.
    let source_id = derive_source_id(&work);
    let id = format!("openalex:{source_id}");

    // Pull the user-facing metadata into local values before assembling the
    // final PaperCandidate. This keeps the struct literal compact and readable.
    let title = work
        .display_name
        .clone()
        .unwrap_or_else(|| "Untitled OpenAlex work".to_string());
    let authors = extract_authors(&work);
    let venue = extract_venue(&work);
    let external_url = extract_external_url(&work);
    let pdf_url = choose_openalex_pdf_url(&work);
    let open_access = extract_open_access(&work);
    let abstract_text = work
        .abstract_inverted_index
        .clone()
        .map(reconstruct_abstract);
    let matched_keywords = extract_matched_keywords(query);
    let reasons = build_match_reasons(query, work.cited_by_count);
    let doi = extract_doi(&work);
    let openalex_id = extract_openalex_id(&work);
    let arxiv_id = extract_arxiv_id(&work);

    PaperCandidate {
        id,
        source_provider: "openalex".to_string(),
        source_id,
        title,
        authors,
        abstract_text,
        year: work.publication_year,
        publication_date: work.publication_date,
        venue,
        citation_count: work.cited_by_count,
        doi,
        openalex_id,
        arxiv_id,
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

/// Prefer the top-level work id and fall back to the ids bag when deriving the
/// short OpenAlex work suffix like `W123456`.
fn derive_source_id(work: &OpenAlexWork) -> String {
    openalex_work_suffix(work.id.as_deref())
        .or_else(|| {
            work.ids
                .as_ref()
                .and_then(|ids| openalex_work_suffix(ids.openalex.as_deref()))
        })
        .unwrap_or_else(|| "unknown".to_string())
}

/// Flatten OpenAlex authorship entries into a simple list of author names.
fn extract_authors(work: &OpenAlexWork) -> Vec<String> {
    work.authorships
        .as_deref()
        .unwrap_or_default()
        .iter()
        .filter_map(|authorship| authorship.author.as_ref()?.display_name.clone())
        .collect()
}

/// Pull the venue name from the primary location source when OpenAlex provides one.
fn extract_venue(work: &OpenAlexWork) -> Option<String> {
    work.primary_location
        .as_ref()
        .and_then(|location| location.source.as_ref())
        .and_then(|source| source.display_name.clone())
}

/// Prefer the primary landing page, then the best OA landing page, then the
/// canonical OpenAlex work url as the external destination.
fn extract_external_url(work: &OpenAlexWork) -> Option<String> {
    work.primary_location
        .as_ref()
        .and_then(|location| location.landing_page_url.clone())
        .or_else(|| {
            work.best_oa_location
                .as_ref()
                .and_then(|location| location.landing_page_url.clone())
        })
        .or_else(|| work.id.clone())
}

/// Convert the OpenAlex open-access block into the smaller app-level summary.
fn extract_open_access(work: &OpenAlexWork) -> Option<OpenAccessSummary> {
    work.open_access
        .as_ref()
        .map(|open_access| OpenAccessSummary {
            is_open_access: open_access.is_oa.unwrap_or(false),
            status: open_access.oa_status.clone(),
        })
}

/// Tokenize the freeform query into lowercase keywords used in the match summary.
fn extract_matched_keywords(query: &str) -> Vec<String> {
    query
        .split_whitespace()
        .map(|part| part.trim_matches(|ch: char| !ch.is_alphanumeric()))
        .filter(|part| !part.is_empty())
        .map(str::to_lowercase)
        .collect()
}

/// Build the human-readable explanation shown for why a candidate appeared.
/// TOOD: Might deprecate
fn build_match_reasons(query: &str, citation_count: Option<i32>) -> Vec<String> {
    let mut reasons = vec![format!("Matched OpenAlex search query \"{query}\".")];

    if let Some(citation_count) = citation_count.filter(|count| *count > 0) {
        reasons.push(format!("{citation_count} OpenAlex citations."));
    }

    reasons
}

/// Prefer the top-level DOI and fall back to the ids bag when needed.
fn extract_doi(work: &OpenAlexWork) -> Option<String> {
    work.doi
        .clone()
        .or_else(|| work.ids.as_ref().and_then(|ids| ids.doi.clone()))
}

/// Prefer ids.openalex and fall back to the top-level id field.
fn extract_openalex_id(work: &OpenAlexWork) -> Option<String> {
    work.ids
        .as_ref()
        .and_then(|ids| ids.openalex.clone())
        .or_else(|| work.id.clone())
}

/// Extract the linked arXiv id when OpenAlex exposes one.
fn extract_arxiv_id(work: &OpenAlexWork) -> Option<String> {
    work.ids.as_ref().and_then(|ids| ids.arxiv.clone())
}

/// Prefer the best OA pdf and fall back to the primary location pdf.
fn choose_openalex_pdf_url(work: &OpenAlexWork) -> Option<String> {
    work.best_oa_location
        .as_ref()
        .and_then(|location| location.pdf_url.clone())
        .or_else(|| {
            work.primary_location
                .as_ref()
                .and_then(|location| location.pdf_url.clone())
        })
}

/// Strip an OpenAlex url down to its final work suffix segment.
fn openalex_work_suffix(value: Option<&str>) -> Option<String> {
    let value = value?;
    value
        .rsplit('/')
        .next()
        .filter(|part| !part.is_empty())
        .map(str::to_string)
}

/// Reconstruct plain abstract text from OpenAlex's inverted-index encoding.
///
/// OpenAlex stores abstracts as `word -> [positions]` instead of one flat
/// string. This helper flips that back into `position -> word` order and joins
/// the words into readable text.
fn reconstruct_abstract(index: HashMap<String, Vec<usize>>) -> String {
    let max_position = index
        .values()
        .flat_map(|positions| positions.iter())
        .copied()
        .max();
    let Some(max_position) = max_position else {
        return String::new();
    };

    // Pre-size the word slots by maximum observed position so each word can be
    // written back to its original abstract position.
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
