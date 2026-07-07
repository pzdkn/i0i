//! Pure set operations over a candidate pool.
//!
//! `dedup` collapses the same paper surfacing from two queries within a run;
//! `diff` returns only the candidates not already in the saved search's pool
//! (the stacking primitive).

use std::collections::HashSet;

use crate::domain::discovery::PaperCandidate;
use crate::domain::research::candidate_dedup_key;

/// Collapse duplicates by dedup key, keeping the first occurrence (which carries
/// the earliest/most-relevant provider order).
pub fn dedup(candidates: Vec<PaperCandidate>) -> Vec<PaperCandidate> {
    let mut seen = HashSet::new();
    let mut out = Vec::with_capacity(candidates.len());
    for candidate in candidates {
        if seen.insert(candidate_dedup_key(&candidate)) {
            out.push(candidate);
        }
    }
    out
}

/// Return only the candidates whose dedup key is not already in `existing`.
/// Also dedups within the incoming batch.
pub fn diff(candidates: Vec<PaperCandidate>, existing: &HashSet<String>) -> Vec<PaperCandidate> {
    let mut seen = existing.clone();
    let mut out = Vec::new();
    for candidate in candidates {
        if seen.insert(candidate_dedup_key(&candidate)) {
            out.push(candidate);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::discovery::{CandidateMatch, PaperCandidate};

    fn candidate(title: &str, doi: Option<&str>) -> PaperCandidate {
        PaperCandidate {
            id: title.to_string(),
            source_provider: "openalex".to_string(),
            source_id: title.to_string(),
            title: title.to_string(),
            authors: Vec::new(),
            abstract_text: None,
            year: None,
            publication_date: None,
            venue: None,
            citation_count: None,
            doi: doi.map(ToString::to_string),
            openalex_id: None,
            arxiv_id: None,
            external_url: None,
            pdf_url: None,
            open_access: None,
            match_summary: CandidateMatch {
                score: None,
                reasons: Vec::new(),
                matched_keywords: Vec::new(),
                from_seed_paper_ids: Vec::new(),
            },
            already_in_library: false,
        }
    }

    #[test]
    fn dedup_collapses_same_doi() {
        let pool = vec![
            candidate("A v1", Some("10.1/a")),
            candidate("A v2", Some("10.1/a")),
            candidate("B", Some("10.1/b")),
        ];
        let out = dedup(pool);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].title, "A v1"); // first occurrence wins
    }

    #[test]
    fn dedup_falls_back_to_title_when_no_ids() {
        let pool = vec![candidate("Same Title", None), candidate("same title", None)];
        // case-insensitive title key collapses these.
        assert_eq!(dedup(pool).len(), 1);
    }

    #[test]
    fn diff_removes_existing_and_dedups_batch() {
        let mut existing = HashSet::new();
        existing.insert("doi:10.1/a".to_string());
        let incoming = vec![
            candidate("A", Some("10.1/a")),     // already in pool
            candidate("B", Some("10.1/b")),     // new
            candidate("B dup", Some("10.1/b")), // dup within batch
        ];
        let out = diff(incoming, &existing);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].title, "B");
    }
}
