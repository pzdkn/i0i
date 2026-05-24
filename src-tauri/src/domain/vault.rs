#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultStatus {
    pub paper_count: usize,
    pub unread_count: usize,
    pub sync_state: String,
}
