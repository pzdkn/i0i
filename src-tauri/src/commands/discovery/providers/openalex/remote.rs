//! OpenAlex wire-format types used only for deserializing API responses.
//!
//! These structs mirror the subset of the OpenAlex JSON schema that our
//! discovery flow requests with `select=...`. They are provider-local types:
//! the rest of the app should work with normalized domain types like
//! `PaperCandidate`, not with these raw API payload structs.
use serde::Deserialize;
use std::collections::HashMap;

#[derive(Debug, Deserialize)]
pub(super) struct OpenAlexWork {
    // Canonical OpenAlex work id, usually like `https://openalex.org/W123456`.
    pub(super) id: Option<String>,
    // Canonical DOI for the work, if OpenAlex has one.
    pub(super) doi: Option<String>,
    // Human-readable title used by OpenAlex for display.
    pub(super) display_name: Option<String>,
    // Publication year of the work.
    pub(super) publication_year: Option<i32>,
    // Full publication date in ISO 8601 format, for example `2024-05-01`.
    pub(super) publication_date: Option<String>,
    // Total number of citations counted by OpenAlex.
    pub(super) cited_by_count: Option<i32>,
    // Nested authorship records, which include author identity and affiliation data.
    pub(super) authorships: Option<Vec<OpenAlexAuthorship>>,
    // OpenAlex's primary location for the work, usually the best version-of-record location.
    pub(super) primary_location: Option<OpenAlexLocation>,
    // Best open-access location for the work when one is available.
    pub(super) best_oa_location: Option<OpenAlexLocation>,
    // Summary of whether the work is open access and what OA status it has.
    pub(super) open_access: Option<OpenAlexAccess>,
    // Abstract stored as an inverted index rather than plain text.
    pub(super) abstract_inverted_index: Option<HashMap<String, Vec<usize>>>,
    // Search relevance score returned by OpenAlex for this query.
    pub(super) relevance_score: Option<f64>,
    // Bag of external ids and alternate ids such as OpenAlex, DOI, and arXiv.
    pub(super) ids: Option<OpenAlexIds>,
}

#[derive(Debug, Deserialize)]
pub(super) struct OpenAlexWorksResponse {
    // The list of work records returned for the current search request.
    pub(super) results: Vec<OpenAlexWork>,
}

#[derive(Debug, Deserialize)]
pub(super) struct OpenAlexAuthorship {
    // Nested author identity for one authorship entry.
    pub(super) author: Option<OpenAlexAuthor>,
}

#[derive(Debug, Deserialize)]
pub(super) struct OpenAlexAuthor {
    // Display name of the author as returned by OpenAlex.
    pub(super) display_name: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct OpenAlexLocation {
    // Human-facing landing page for this location, often a publisher or repository page.
    pub(super) landing_page_url: Option<String>,
    // Direct PDF URL for this location when OpenAlex knows one.
    pub(super) pdf_url: Option<String>,
    // Source metadata for this location, such as the journal or repository name.
    pub(super) source: Option<OpenAlexSource>,
}

#[derive(Debug, Deserialize)]
pub(super) struct OpenAlexSource {
    // Human-readable source name, for example a journal or repository title.
    pub(super) display_name: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct OpenAlexAccess {
    // Whether OpenAlex considers this work open access.
    pub(super) is_oa: Option<bool>,
    // Open-access status label such as `gold`, `green`, `bronze`, or `closed`.
    pub(super) oa_status: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct OpenAlexIds {
    // Canonical OpenAlex id for the work.
    pub(super) openalex: Option<String>,
    // DOI as stored in OpenAlex's ids bag.
    pub(super) doi: Option<String>,
    // arXiv identifier when OpenAlex has linked one to the work.
    pub(super) arxiv: Option<String>,
}
