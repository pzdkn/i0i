//! Local app-open scheduler for Project Research Harnesses (RFC 0113).

use std::time::Duration;

use chrono::Utc;

use crate::services::research::controller::ProjectResearchController;
use crate::storage::library_store::LibraryStore;

/// Recovers interrupted Runs and claims at most one due Project per minute.
#[derive(Clone)]
pub struct HarnessScheduler {
    store: LibraryStore,
    controller: ProjectResearchController,
}

impl HarnessScheduler {
    pub fn new(store: LibraryStore, controller: ProjectResearchController) -> Self {
        Self { store, controller }
    }

    pub fn start(&self) -> Result<(), String> {
        self.store.recover_interrupted_harness_runs()?;
        self.tick(true)?;
        let scheduler = self.clone();
        tauri::async_runtime::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(60)).await;
                if let Err(error) = scheduler.tick(false) {
                    eprintln!("[research-scheduler] tick failed: {error}");
                }
            }
        });
        Ok(())
    }

    pub fn tick(&self, startup: bool) -> Result<(), String> {
        let Some(claim) = self.store.claim_due_harness(Utc::now(), startup)? else {
            return Ok(());
        };
        self.controller
            .start(&claim.project_id, claim.trigger, Some(&claim.scheduled_for))?;
        Ok(())
    }
}
