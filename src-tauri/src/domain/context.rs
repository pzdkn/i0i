//! Chat context: what the model is allowed to see (RFC 0077).
//!
//! Two kinds, with deliberately different lifetimes:
//!
//! - **Persistent** — [`ContextItem`] rows in `chat_context_items`, keyed by
//!   thread. They survive restarts and are removed only by an explicit delete
//!   or by compaction.
//! - **Ephemeral** — [`EphemeralContext`], a *parameter*, never a row. The
//!   current selection and page are dropped by never being written down.
//!
//! Three ids appear here and they are not interchangeable:
//!
//! | id | lives | job |
//! |---|---|---|
//! | citation handle (`C1`) | one assembly | what the model writes |
//! | `chunk_id` | until the next rechunk | lookup, and the key for rectangles |
//! | `paper_id` + source range | forever | survives rechunking |

use serde::{Deserialize, Serialize};

use crate::pdf_layout::NormRect;

/// A persistent context item: one chunk, or one compaction summary.
pub const CONTEXT_KIND_CHUNK: &str = "chunk";
pub const CONTEXT_KIND_SUMMARY: &str = "summary";

/// A row of `chat_context_items`.
///
/// Chunk items carry both a `chunk_id` (fast path) and a durable anchor
/// (`paper_id` + `source_start`/`source_end`) because `CHUNK_VERSION` bumps
/// re-mint chunk ids. Summary items carry `body` and the watermark instead.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextItem {
    pub id: String,
    pub thread_id: String,
    /// Insertion order, dense from 0. Emission order into the prompt.
    pub position: i32,
    pub kind: String,
    pub chunk_id: Option<String>,
    pub paper_id: Option<String>,
    pub source_start: Option<i64>,
    pub source_end: Option<i64>,
    /// Summary text. `None` for chunk items.
    pub body: Option<String>,
    /// Newest entry the summary covers; `get_context` replays entries after it.
    pub covers_through_entry_id: Option<String>,
    pub token_estimate: i32,
    pub created_at: String,
}

/// A context item to insert. `position` and `id` are assigned by the store.
#[derive(Debug, Clone)]
pub struct ContextItemDraft {
    pub kind: String,
    pub chunk_id: Option<String>,
    pub paper_id: Option<String>,
    pub source_start: Option<i64>,
    pub source_end: Option<i64>,
    pub body: Option<String>,
    pub covers_through_entry_id: Option<String>,
    pub token_estimate: i32,
}

/// Which key the caller happens to hold.
///
/// A caller looking at a search hit knows the chunk id; a caller looking at the
/// context list knows the item id. Neither should have to translate.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", content = "id", rename_all = "camelCase")]
pub enum ContextKey {
    Item(String),
    Chunk(String),
}

/// Context that lives for exactly one `get_context` call.
///
/// Never stored. "Dropped when the selection changes" costs nothing if it was
/// never written down, which also rules out every stale-selection bug.
///
/// `paper_id` is carried rather than derived from the thread because the anchor
/// ask path has no thread yet, and because context can span papers — a passage
/// from a different paper is labelled so the model does not conflate sources.
#[derive(Debug, Clone, Default)]
pub struct EphemeralContext {
    pub paper_id: String,
    pub selection: Option<String>,
}

/// Rectangles for one page, in reader space (normalized 0..1, origin top-left).
///
/// Same shape the reader already paints for highlights, so a citation jump
/// reuses that path rather than introducing a second geometry convention.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PageRects {
    pub page_index: i32,
    pub rects: Vec<NormRect>,
}

/// What a `[C1]` marker in an answer resolves to.
///
/// Persisted alongside the answer: without the map, reopening a thread would
/// show markers that point at nothing.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextCitation {
    /// `"C1"`, without the brackets.
    pub handle: String,
    pub item_id: String,
    pub paper_id: String,
    pub page_start: i32,
    pub heading_path: Option<String>,
    /// The chunk the rectangles came from. Lets the UI offer "add to context"
    /// on a citation the agent retrieved but nobody committed.
    pub chunk_id: Option<String>,
    /// A JSON array of `PageRects`. `"[]"` when the blocks carry no geometry —
    /// the citation then degrades to a page number rather than failing.
    pub rects_json: String,
}

/// One context item as the UI lists it: resolved, with its text.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextItemView {
    pub id: String,
    pub kind: String,
    pub chunk_id: Option<String>,
    pub paper_id: Option<String>,
    pub page_start: Option<i32>,
    pub heading_path: Option<String>,
    /// Chunk text, or the summary body.
    pub text: String,
    pub token_estimate: i32,
    /// The chunk no longer resolves — reported, never silently dropped.
    pub unresolved: bool,
}
