//! Research capabilities available to the bounded chat routing round.
//!
//! This is deliberately a small interface over existing application services,
//! not another agent framework. The chat loop decides whether to call a tool;
//! this module performs the side effect and returns typed evidence or a durable
//! Deep Research link.

use async_trait::async_trait;
use futures_util::future::join_all;

use crate::commands::discovery::browser::BrowserDiscoverySource;
use crate::domain::chat::ResearchActivity;
use crate::domain::research::{Depth, SearchConstraints, SearchDraft};
use crate::services::research::manager::SearchManager;
use crate::services::source_acquisition::SourceAcquisitionService;
use crate::storage::library_store::LibraryStore;

/// Unnumbered web evidence returned by a search backend.
///
/// The chat assembler assigns `W1`, `W2`, ... only after the final bounded set
/// is known, just as local citation handles are assigned during assembly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebEvidence {
    pub url: String,
    pub title: String,
    pub publisher: Option<String>,
    pub retrieved_at: String,
    pub excerpt: String,
}

/// Tool-side research operations used by the existing two-phase chat loop.
#[async_trait]
pub trait ResearchToolbox: Send + Sync {
    /// Search and read at most `limit` external sources for one query.
    async fn search_web(&self, query: &str, limit: usize) -> Result<Vec<WebEvidence>, String>;

    /// Create and enqueue one persisted Deep Research run, returning immediately.
    fn start_deep_research(&self, title: &str, goal: &str) -> Result<ResearchActivity, String>;
}

/// Production toolbox backed by the existing search store and SearchManager.
#[derive(Clone)]
pub struct AppResearchToolbox {
    store: LibraryStore,
    search_manager: SearchManager,
    browser_discovery: BrowserDiscoverySource,
    source_acquisition: SourceAcquisitionService,
}

impl AppResearchToolbox {
    pub fn new(
        store: LibraryStore,
        search_manager: SearchManager,
        browser_discovery: BrowserDiscoverySource,
        source_acquisition: SourceAcquisitionService,
    ) -> Self {
        Self {
            store,
            search_manager,
            browser_discovery,
            source_acquisition,
        }
    }
}

#[async_trait]
impl ResearchToolbox for AppResearchToolbox {
    async fn search_web(&self, query: &str, limit: usize) -> Result<Vec<WebEvidence>, String> {
        let limit = limit.clamp(1, 3);
        let candidates = self
            .browser_discovery
            .discover(query, limit, &|_| {})
            .await
            .map_err(|error| error.to_string())?;

        // Read the bounded result set concurrently. Search snippets are useful
        // for ranking, but they are not evidence; only pages Obscura actually
        // opened become W-citations in the answer.
        let reads = candidates.into_iter().take(limit).filter_map(|candidate| {
            let url = candidate.external_url.clone()?;
            let acquisition = self.source_acquisition.clone();
            Some(async move {
                let page = acquisition.inspect_browser_page(&url).await.ok()?;
                let excerpt = clip_evidence(page.snapshot.text.as_deref().unwrap_or_default());
                if excerpt.is_empty() {
                    return None;
                }
                Some(WebEvidence {
                    publisher: url::Url::parse(&page.snapshot.final_url)
                        .ok()
                        .and_then(|parsed| parsed.host_str().map(str::to_string)),
                    retrieved_at: retrieval_timestamp(),
                    title: page
                        .snapshot
                        .title
                        .filter(|title| !title.trim().is_empty())
                        .unwrap_or(candidate.title),
                    url: page.snapshot.final_url,
                    excerpt,
                })
            })
        });
        let evidence = join_all(reads)
            .await
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();
        if evidence.is_empty() {
            Err("Web search found candidates, but none could be read as evidence.".to_string())
        } else {
            Ok(evidence)
        }
    }

    fn start_deep_research(&self, title: &str, goal: &str) -> Result<ResearchActivity, String> {
        let draft = SearchDraft {
            title: clipped_title(title, goal),
            goal: goal.trim().to_string(),
            constraints: SearchConstraints {
                year_from: None,
                year_to: None,
                // RFC 0098: browsers discover candidates; providers resolve
                // identities internally and are no longer a user-facing lane.
                providers: Vec::new(),
                open_access: false,
                target_count: 20,
                venues: Vec::new(),
                authors: Vec::new(),
                fields_of_study: Vec::new(),
                seed_paper_ids: Vec::new(),
            },
            strategy: Depth::Standard.budget(),
            schedule: None,
        };
        let search = self.store.create_search(&draft)?;
        let run_id = self
            .search_manager
            .run_search(search.id.clone(), Some("deep".to_string()))?;
        Ok(ResearchActivity {
            search_id: search.id,
            run_id,
            title: search.title,
            status: "queued".to_string(),
        })
    }
}

/// Keep activity titles useful in narrow panels and safe at UTF-8 boundaries.
fn clipped_title(title: &str, fallback: &str) -> String {
    let value = if title.trim().is_empty() {
        fallback.trim()
    } else {
        title.trim()
    };
    value.chars().take(60).collect()
}

/// Bound external text before it enters the tool transcript and final prompt.
fn clip_evidence(text: &str) -> String {
    const MAX_CHARS: usize = 4_000;
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    match text.char_indices().nth(MAX_CHARS) {
        Some((index, _)) => format!("{}…", text[..index].trim_end()),
        None => text,
    }
}

fn retrieval_timestamp() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn activity_titles_use_the_goal_and_clip_by_characters() {
        assert_eq!(
            clipped_title("", "  sparse attention  "),
            "sparse attention"
        );
        assert_eq!(
            clipped_title(&"é".repeat(80), "fallback").chars().count(),
            60
        );
    }

    #[test]
    fn external_evidence_is_flattened_and_clipped_on_character_boundaries() {
        let evidence = clip_evidence(&format!("{}\n trailing", "é".repeat(4_100)));
        assert_eq!(evidence.chars().count(), 4_001);
        assert!(evidence.ends_with('…'));
        assert!(!evidence.contains('\n'));
    }
}
