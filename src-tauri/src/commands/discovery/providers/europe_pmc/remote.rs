//! Europe PMC JSON wire types (RFC 0053).
//!
//! Only the fields the normalizer reads are modeled; everything is optional and
//! `#[serde(default)]` so a shape change in Europe PMC's response degrades to
//! missing data rather than a hard parse failure. Field paths were confirmed
//! against a live `resultType=core` response.

use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub(super) struct EuropePmcResponse {
    #[serde(rename = "resultList", default)]
    pub result_list: ResultList,
}

#[derive(Debug, Default, Deserialize)]
pub(super) struct ResultList {
    #[serde(default)]
    pub result: Vec<EuropePmcResult>,
}

#[derive(Debug, Default, Deserialize)]
pub(super) struct EuropePmcResult {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub pmid: Option<String>,
    #[serde(default)]
    pub pmcid: Option<String>,
    #[serde(default)]
    pub doi: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    /// Comma-separated author names, e.g. `"Cheng Y, Zhu G, Zhou X."`.
    #[serde(rename = "authorString", default)]
    pub author_string: Option<String>,
    #[serde(rename = "authorList", default)]
    pub author_list: Option<AuthorList>,
    #[serde(rename = "journalInfo", default)]
    pub journal_info: Option<JournalInfo>,
    /// Publication year as a string, e.g. `"2026"`.
    #[serde(rename = "pubYear", default)]
    pub pub_year: Option<String>,
    #[serde(rename = "abstractText", default)]
    pub abstract_text: Option<String>,
    /// Open-access flag encoded as `"Y"` / `"N"`.
    #[serde(rename = "isOpenAccess", default)]
    pub is_open_access: Option<String>,
    #[serde(rename = "fullTextUrlList", default)]
    pub full_text_url_list: Option<FullTextUrlList>,
}

#[derive(Debug, Default, Deserialize)]
pub(super) struct AuthorList {
    #[serde(default)]
    pub author: Vec<Author>,
}

#[derive(Debug, Default, Deserialize)]
pub(super) struct Author {
    #[serde(rename = "fullName", default)]
    pub full_name: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
pub(super) struct JournalInfo {
    #[serde(default)]
    pub journal: Option<Journal>,
}

#[derive(Debug, Default, Deserialize)]
pub(super) struct Journal {
    #[serde(default)]
    pub title: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
pub(super) struct FullTextUrlList {
    #[serde(rename = "fullTextUrl", default)]
    pub full_text_url: Vec<FullTextUrl>,
}

#[derive(Debug, Default, Deserialize)]
pub(super) struct FullTextUrl {
    /// e.g. `"Open access"`, `"Free"`, `"Subscription required"`.
    #[serde(default)]
    pub availability: Option<String>,
    /// e.g. `"pdf"`, `"html"`, `"doi"`.
    #[serde(rename = "documentStyle", default)]
    pub document_style: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
}
