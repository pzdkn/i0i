use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Vault {
    pub id: String,
    pub title: String,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultDraft {
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Paper {
    pub id: String,
    pub title: String,
    pub authors: Vec<String>,
    pub venue: String,
    pub year: i32,
    pub citations: i32,
    pub tags: Vec<String>,
    pub note_count: i32,
    pub annotation_count: i32,
    pub status: String,
    #[serde(rename = "abstract")]
    pub abstract_text: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaperDraft {
    pub id: String,
    pub title: String,
    pub authors: Vec<String>,
    pub venue: String,
    pub year: i32,
    pub citations: i32,
    pub tags: Vec<String>,
    pub status: String,
    #[serde(rename = "abstract")]
    pub abstract_text: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultPaper {
    pub vault_id: String,
    pub paper_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibrarySnapshot {
    pub vaults: Vec<Vault>,
    pub papers: Vec<Paper>,
    pub vault_papers: Vec<VaultPaper>,
}
