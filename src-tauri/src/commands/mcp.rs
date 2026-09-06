//! Explicit app-session access for local external MCP clients.

use crate::services::mcp::{LocalMcpServer, McpConnectionGrant, VAULT_LIST, VAULT_LIST_PAPERS};
use crate::storage::library_store::LibraryStore;

/// Create a revocable connection scoped to one Project and its Vault.
#[tauri::command]
pub async fn create_external_mcp_grant(
    server: tauri::State<'_, LocalMcpServer>,
    store: tauri::State<'_, LibraryStore>,
    project_id: String,
) -> Result<McpConnectionGrant, String> {
    let snapshot = store.get_library()?;
    let vault = snapshot
        .vaults
        .iter()
        .find(|vault| vault.project_id == project_id)
        .ok_or("Project vault not found")?;
    server
        .issue_grant(
            &store,
            &project_id,
            &vault.id,
            "external-agent",
            None,
            [VAULT_LIST, VAULT_LIST_PAPERS],
        )
        .await
}

/// Revoke an external app-session credential.
#[tauri::command]
pub async fn revoke_external_mcp_grant(
    server: tauri::State<'_, LocalMcpServer>,
    bearer_token: String,
) -> Result<(), String> {
    server.revoke_grant(&bearer_token).await;
    Ok(())
}
