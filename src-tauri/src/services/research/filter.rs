//! Post-hoc constraint guards (RFC 0037).
//!
//! Venue/author/field are applied at *query time* by the provider (see the
//! discovery layer), not here. This pure pass enforces the cheap belt-and-
//! suspenders guards: year clamp and open-access. Candidates with unknown years
//! are kept (we don't drop on missing metadata).

use crate::domain::discovery::PaperCandidate;
use crate::domain::research::SearchConstraints;

/// Drop candidates that fall outside the year range, or (when open-access is
/// required) that are known to be closed access. Unknown metadata is kept.
pub fn apply_constraints(
    candidates: Vec<PaperCandidate>,
    constraints: &SearchConstraints,
) -> Vec<PaperCandidate> {
    candidates
        .into_iter()
        .filter(|c| within_year(c, constraints))
        .filter(|c| !constraints.open_access || is_open_access(c))
        .collect()
}

/// Apply constraints after browser candidates have had a chance to resolve.
/// Unknown structured metadata cannot satisfy an explicit browser-search
/// filter; unconstrained searches still retain honest partial candidates.
pub fn apply_resolved_constraints(
    candidates: Vec<PaperCandidate>,
    constraints: &SearchConstraints,
) -> Vec<PaperCandidate> {
    candidates
        .into_iter()
        .filter(|candidate| within_resolved_year(candidate, constraints))
        .filter(|candidate| {
            constraints.venues.is_empty()
                || candidate
                    .venue
                    .as_deref()
                    .is_some_and(|venue| contains_any(venue, &constraints.venues))
        })
        .filter(|candidate| {
            constraints.authors.is_empty()
                || candidate
                    .authors
                    .iter()
                    .any(|author| contains_any(author, &constraints.authors))
        })
        .filter(|candidate| {
            !constraints.open_access
                || candidate
                    .open_access
                    .as_ref()
                    .is_some_and(|access| access.is_open_access)
        })
        .collect()
}

fn within_resolved_year(candidate: &PaperCandidate, constraints: &SearchConstraints) -> bool {
    if constraints.year_from.is_none() && constraints.year_to.is_none() {
        return true;
    }
    candidate.year.is_some_and(|year| {
        constraints.year_from.is_none_or(|from| year >= from)
            && constraints.year_to.is_none_or(|to| year <= to)
    })
}

fn contains_any(value: &str, filters: &[String]) -> bool {
    let value = value.to_lowercase();
    filters
        .iter()
        .any(|filter| value.contains(&filter.trim().to_lowercase()))
}

fn within_year(candidate: &PaperCandidate, constraints: &SearchConstraints) -> bool {
    let Some(year) = candidate.year else {
        return true; // unknown year: keep
    };
    if let Some(from) = constraints.year_from {
        if year < from {
            return false;
        }
    }
    if let Some(to) = constraints.year_to {
        if year > to {
            return false;
        }
    }
    true
}

fn is_open_access(candidate: &PaperCandidate) -> bool {
    // Unknown OA status is treated as allowed; only an explicit `false` drops it.
    candidate
        .open_access
        .as_ref()
        .map(|oa| oa.is_open_access)
        .unwrap_or(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::discovery::{CandidateMatch, OpenAccessSummary, PaperCandidate};

    fn candidate(title: &str, year: Option<i32>, oa: Option<bool>) -> PaperCandidate {
        PaperCandidate {
            id: title.to_string(),
            source_provider: "openalex".to_string(),
            source_id: title.to_string(),
            title: title.to_string(),
            authors: Vec::new(),
            abstract_text: None,
            year,
            publication_date: None,
            venue: None,
            citation_count: None,
            doi: None,
            openalex_id: None,
            arxiv_id: None,
            external_url: None,
            pdf_url: None,
            open_access: oa.map(|is_open_access| OpenAccessSummary {
                is_open_access,
                status: None,
            }),
            match_summary: CandidateMatch {
                score: None,
                reasons: Vec::new(),
                matched_keywords: Vec::new(),
                from_seed_paper_ids: Vec::new(),
            },
            already_in_library: false,
        }
    }

    fn constraints(
        year_from: Option<i32>,
        year_to: Option<i32>,
        open_access: bool,
    ) -> SearchConstraints {
        SearchConstraints {
            year_from,
            year_to,
            providers: Vec::new(),
            open_access,
            target_count: 20,
            venues: Vec::new(),
            authors: Vec::new(),
            fields_of_study: Vec::new(),
            seed_paper_ids: Vec::new(),
        }
    }

    #[test]
    fn drops_papers_before_year_from() {
        let pool = vec![
            candidate("old", Some(2019), None),
            candidate("new", Some(2024), None),
        ];
        let out = apply_constraints(pool, &constraints(Some(2023), None, false));
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].title, "new");
    }

    #[test]
    fn keeps_unknown_year() {
        let pool = vec![candidate("mystery", None, None)];
        assert_eq!(
            apply_constraints(pool, &constraints(Some(2023), None, false)).len(),
            1
        );
    }

    #[test]
    fn open_access_required_drops_closed_only() {
        let pool = vec![
            candidate("closed", Some(2024), Some(false)),
            candidate("open", Some(2024), Some(true)),
            candidate("unknown", Some(2024), None),
        ];
        let out = apply_constraints(pool, &constraints(None, None, true));
        let titles: Vec<&str> = out.iter().map(|c| c.title.as_str()).collect();
        assert_eq!(titles, vec!["open", "unknown"]);
    }

    #[test]
    fn resolved_filters_require_known_matching_metadata() {
        let mut matching = candidate("matching", Some(2024), Some(true));
        matching.venue = Some("NeurIPS".to_string());
        matching.authors = vec!["Ada Lovelace".to_string()];
        let unknown = candidate("unknown", None, None);
        let mut constraints = constraints(Some(2023), Some(2025), false);
        constraints.venues = vec!["neurips".to_string()];
        constraints.authors = vec!["Lovelace".to_string()];

        let out = apply_resolved_constraints(vec![matching, unknown], &constraints);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].title, "matching");
    }
}
