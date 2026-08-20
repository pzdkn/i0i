//! Tauri commands for the vault Suggestions inbox (RFC 0091).

use crate::domain::library::{LibrarySnapshot, PaperDraft, PaperSourceDraft};
use crate::domain::vault_suggestion::{VaultSuggestion, VaultSuggestionSnapshot};
use crate::pdf_ingestion::PdfDownloadManager;
use crate::services::reader_service::ReaderService;
use crate::services::vault_suggestions::VaultSuggestionManager;
use crate::storage::library_store::LibraryStore;

/// Load pending suggestions and the latest durable run status.
#[tauri::command]
pub fn get_vault_suggestions(
    store: tauri::State<'_, LibraryStore>,
    vault_id: String,
) -> Result<VaultSuggestionSnapshot, String> {
    store.get_vault_suggestions(&vault_id)
}

/// Queue a manual suggestion run.
#[tauri::command]
pub fn run_vault_suggestions(
    manager: tauri::State<'_, VaultSuggestionManager>,
    vault_id: String,
) -> Result<String, String> {
    manager.run(vault_id)
}

/// Cancel one active suggestion run.
#[tauri::command]
pub fn cancel_vault_suggestion_run(
    manager: tauri::State<'_, VaultSuggestionManager>,
    run_id: String,
) {
    manager.cancel(&run_id);
}

/// Persist dismissal immediately, including for a provisional row.
#[tauri::command]
pub fn dismiss_vault_suggestion(
    store: tauri::State<'_, LibraryStore>,
    suggestion: VaultSuggestion,
) -> Result<(), String> {
    store.upsert_vault_suggestion_decision(&suggestion, "dismissed")
}

/// Restore the most recently dismissed row to the pending inbox.
#[tauri::command]
pub fn undo_vault_suggestion_dismissal(
    store: tauri::State<'_, LibraryStore>,
    suggestion_id: String,
) -> Result<VaultSuggestionSnapshot, String> {
    let suggestion = store.get_vault_suggestion(&suggestion_id)?;
    store.set_vault_suggestion_state(&suggestion_id, "pending")?;
    store.get_vault_suggestions(&suggestion.vault_id)
}

/// Add a final or provisional suggestion through the normal paper-ingestion path.
#[tauri::command]
pub fn add_vault_suggestion(
    store: tauri::State<'_, LibraryStore>,
    pdf_downloads: tauri::State<'_, PdfDownloadManager>,
    reader_service: tauri::State<'_, ReaderService>,
    suggestion: VaultSuggestion,
) -> Result<LibrarySnapshot, String> {
    let paper = candidate_paper_draft(&suggestion);
    store.add_paper_to_vaults(&paper, std::slice::from_ref(&suggestion.vault_id))?;
    store.upsert_vault_suggestion_decision(&suggestion, "added")?;

    let mut queue_sources = Vec::new();
    for source in store
        .get_document_sources(&paper.id)?
        .into_iter()
        .filter(|source| source.status == "remote_available")
    {
        if reader_service.promote_discovery_cached_pdf(&source)? {
            pdf_downloads.queue_source(source.id);
        } else {
            queue_sources.push(source);
        }
    }
    pdf_downloads.queue_sources(queue_sources);
    store.get_library()
}

fn candidate_paper_draft(suggestion: &VaultSuggestion) -> PaperDraft {
    let candidate = &suggestion.candidate;
    PaperDraft {
        id: candidate.id.clone(),
        title: candidate.title.clone(),
        authors: candidate.authors.clone(),
        venue: candidate.venue.clone().unwrap_or_default(),
        year: candidate.year.unwrap_or_default(),
        citations: candidate.citation_count.unwrap_or_default(),
        tags: Vec::new(),
        status: "UNREAD".to_string(),
        abstract_text: candidate.abstract_text.clone(),
        sources: candidate
            .pdf_url
            .as_ref()
            .map(|pdf_url| {
                vec![PaperSourceDraft {
                    source_kind: "pdf".to_string(),
                    source_url: pdf_url.clone(),
                    landing_url: candidate.external_url.clone(),
                }]
            })
            .unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::discovery::{CandidateMatch, PaperCandidate};

    #[test]
    fn candidate_draft_preserves_reader_source() {
        let suggestion = VaultSuggestion {
            id: "s".to_string(),
            vault_id: "v".to_string(),
            run_id: "r".to_string(),
            paper_ref: "doi:10/example".to_string(),
            candidate: PaperCandidate {
                id: "paper".to_string(),
                source_provider: "test".to_string(),
                source_id: "paper".to_string(),
                title: "Title".to_string(),
                authors: vec!["Author".to_string()],
                abstract_text: None,
                year: Some(2026),
                publication_date: None,
                venue: Some("Venue".to_string()),
                citation_count: Some(3),
                doi: None,
                openalex_id: None,
                arxiv_id: None,
                external_url: Some("https://example.com".to_string()),
                pdf_url: Some("https://example.com/paper.pdf".to_string()),
                open_access: None,
                match_summary: CandidateMatch {
                    score: None,
                    reasons: Vec::new(),
                    matched_keywords: Vec::new(),
                    from_seed_paper_ids: Vec::new(),
                },
                already_in_library: false,
            },
            reason: "Related".to_string(),
            score: 1.0,
            state: "pending".to_string(),
            created_at: String::new(),
            updated_at: String::new(),
        };

        let draft = candidate_paper_draft(&suggestion);
        assert_eq!(draft.sources[0].source_url, "https://example.com/paper.pdf");
        assert_eq!(
            draft.sources[0].landing_url.as_deref(),
            Some("https://example.com")
        );
    }
}
