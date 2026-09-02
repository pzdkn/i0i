//! Review commands for typed Harness improvement proposals (RFC 0114).

use crate::domain::harness_improvement::{
    HarnessImprovement, HarnessImprovementStatus, HarnessImprovementValue,
};
use crate::storage::library_store::LibraryStore;

#[tauri::command]
pub fn list_harness_improvements(
    store: tauri::State<'_, LibraryStore>,
    project_id: String,
    status: Option<HarnessImprovementStatus>,
) -> Result<Vec<HarnessImprovement>, String> {
    store.list_harness_improvements(&project_id, status)
}

#[tauri::command]
pub fn get_harness_improvement(
    store: tauri::State<'_, LibraryStore>,
    improvement_id: String,
) -> Result<HarnessImprovement, String> {
    store.get_harness_improvement(&improvement_id)
}

#[tauri::command]
pub fn edit_harness_improvement(
    store: tauri::State<'_, LibraryStore>,
    improvement_id: String,
    proposed_value: HarnessImprovementValue,
) -> Result<HarnessImprovement, String> {
    store.edit_harness_improvement(&improvement_id, &proposed_value)
}

#[tauri::command]
pub fn accept_harness_improvement(
    store: tauri::State<'_, LibraryStore>,
    improvement_id: String,
) -> Result<HarnessImprovement, String> {
    store.accept_harness_improvement(&improvement_id)
}

#[tauri::command]
pub fn reject_harness_improvement(
    store: tauri::State<'_, LibraryStore>,
    improvement_id: String,
    reason: Option<String>,
) -> Result<HarnessImprovement, String> {
    store.reject_harness_improvement(&improvement_id, reason.as_deref())
}
