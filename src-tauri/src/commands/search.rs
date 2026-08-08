//! Commands for searching within papers we hold (RFC 0076).
//!
//! This is where "current paper" and "current vault" become concrete. The
//! service takes an explicit scope and knows nothing about what is open — the
//! caller passes the ids, because "current" means something different to the
//! reader, the vault view, global search, and the agent.

use crate::services::search::{SearchRequest, SearchResponse, SearchService};
use crate::storage::library_store::LibraryStore;

/// Search chunks across an explicit scope.
///
/// `paperIds` and `vaultIds` are AND-ed, each empty meaning unconstrained:
/// omit both for global search, pass a vault for vault-wide, pass a paper and
/// its vault for "this PDF in this vault". A scope that resolves to nothing
/// returns no hits with `scope.emptyReason` set — it is not an error.
#[tauri::command]
pub async fn search_chunks(
    search: tauri::State<'_, SearchService>,
    request: SearchRequest,
) -> Result<SearchResponse, String> {
    search.search(request).await
}

/// Embedding coverage for one paper, so a caller can show that semantic search
/// is still catching up rather than silently returning weaker results.
#[tauri::command]
pub fn chunk_embedding_coverage(
    store: tauri::State<'_, LibraryStore>,
    paper_id: String,
) -> Result<crate::domain::library::EmbeddingCoverage, String> {
    store.embedding_coverage(
        &paper_id,
        crate::services::embedding::MODEL_NAME,
        crate::services::embedding::MODEL_VERSION,
    )
}
