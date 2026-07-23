//! CORE v3 JSON wire types (RFC 0053).
//!
//! Modeled from the CORE v3 `search/works` schema. Every field is optional and
//! `#[serde(default)]` so schema drift degrades to missing data rather than a
//! hard parse failure. Not live-verified in this environment (no API key
//! present); the tolerant shape is deliberate.

use serde::Deserialize;

#[derive(Debug, Default, Deserialize)]
pub(super) struct CoreSearchResponse {
    #[serde(default)]
    pub results: Vec<CoreWork>,
}

#[derive(Debug, Default, Deserialize)]
pub(super) struct CoreWork {
    #[serde(default)]
    pub id: Option<serde_json::Value>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub authors: Vec<CoreAuthor>,
    #[serde(rename = "abstract", default)]
    pub abstract_text: Option<String>,
    #[serde(rename = "yearPublished", default)]
    pub year_published: Option<i32>,
    #[serde(rename = "publishedDate", default)]
    pub published_date: Option<String>,
    #[serde(default)]
    pub doi: Option<String>,
    #[serde(rename = "downloadUrl", default)]
    pub download_url: Option<String>,
    #[serde(rename = "publisher", default)]
    pub publisher: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
pub(super) struct CoreAuthor {
    #[serde(default)]
    pub name: Option<String>,
}
