use crate::domain::vault::VaultStatus;

#[tauri::command]
pub fn get_vault_status() -> VaultStatus {
    VaultStatus {
        paper_count: 234,
        unread_count: 12,
        sync_state: "local".to_string(),
    }
}
