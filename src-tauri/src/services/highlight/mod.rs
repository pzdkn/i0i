//! Author-agnostic capability layer for highlights (RFC 0058). Every verb takes
//! an explicit author; the UI (Tauri commands) and, later, the agent tool
//! registry (RFC 0059) call these same methods. The service takes an
//! already-resolved `Locator` — it never resolves quotes and never assumes an
//! operation completed synchronously on the client (PDF resolution is
//! client-side; see RFC 0058/0059).

use crate::domain::highlight::{Highlight, HighlightAuthor, HighlightColor, Locator};
use crate::storage::library_store::LibraryStore;

/// What a note/ask attaches to. `Document` is reserved for whole-paper
/// (unanchored) threads; wiring it up is deferred to Task 9.
#[derive(Debug, Clone)]
pub enum HighlightTarget {
    Highlight { id: String },
    Document { paper_id: String },
}

#[derive(Clone)]
pub struct HighlightService {
    store: LibraryStore,
}

impl HighlightService {
    pub fn new(store: LibraryStore) -> Self {
        Self { store }
    }

    pub async fn create_highlight(
        &self,
        paper_id: &str,
        locator: Locator,
        excerpt: &str,
        color: HighlightColor,
        label: Option<String>,
        author: HighlightAuthor,
    ) -> Result<Highlight, String> {
        self.store
            .insert_highlight(paper_id, &locator, excerpt, color, label.as_deref(), &author)
            .map_err(|e| e.to_string())
    }

    pub async fn recolor(&self, id: &str, color: HighlightColor) -> Result<(), String> {
        self.store
            .recolor_highlight(id, color)
            .map_err(|e| e.to_string())
    }

    pub async fn set_label(&self, id: &str, label: Option<String>) -> Result<(), String> {
        self.store
            .set_highlight_label(id, label.as_deref())
            .map_err(|e| e.to_string())
    }

    pub async fn remove(&self, id: &str) -> Result<(), String> {
        self.store.remove_highlight(id).map_err(|e| e.to_string())
    }

    pub async fn list(&self, paper_id: &str) -> Result<Vec<Highlight>, String> {
        self.store.list_highlights(paper_id).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::highlight::{HighlightAuthor, HighlightColor, Locator};
    use crate::storage::library_store::LibraryStore;

    fn service() -> (HighlightService, String) {
        let dir = std::env::temp_dir().join(format!(
            "i0i-highlight-service-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let store = LibraryStore::for_test(dir.join("library.sqlite"));
        store.init().unwrap();
        // init() seeds the default library, which includes "vaswani2017".
        let paper_id = "vaswani2017".to_string();
        (HighlightService::new(store), paper_id)
    }

    #[tokio::test]
    async fn create_is_author_agnostic() {
        let (svc, paper) = service();
        let loc = Locator::TextOffset {
            source_id: "s".into(),
            start_offset: 0,
            end_offset: 4,
        };
        let user = svc
            .create_highlight(
                &paper,
                loc.clone(),
                "quote",
                HighlightColor::Yellow,
                None,
                HighlightAuthor::User,
            )
            .await
            .unwrap();
        assert_eq!(user.author, HighlightAuthor::User);
        let agent = svc
            .create_highlight(
                &paper,
                loc,
                "quote",
                HighlightColor::Red,
                Some("exp".into()),
                HighlightAuthor::Agent { model: "m".into() },
            )
            .await
            .unwrap();
        assert!(matches!(agent.author, HighlightAuthor::Agent { .. }));
        assert_eq!(svc.list(&paper).await.unwrap().len(), 2);
    }
}
