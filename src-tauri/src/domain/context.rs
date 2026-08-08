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
    /// `ORIGIN_USER` or `ORIGIN_AGENT` (RFC 0078).
    pub origin: String,
    pub token_estimate: i32,
    pub created_at: String,
}

/// Who added a context item. The agent may drop only its own additions.
pub const ORIGIN_USER: &str = "user";
pub const ORIGIN_AGENT: &str = "agent";

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
    pub origin: String,
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
    /// The opening of the passage, flattened and clipped. A page number alone
    /// is cryptic — "p7 · Method" tells you where it is, not what it said.
    /// Taken from the text rather than generated: a summary would cost a model
    /// call per reference, on a path already fighting for latency.
    pub preview: String,
    /// A JSON array of `PageRects`. `"[]"` when the blocks carry no geometry —
    /// the citation then degrades to a page number rather than failing.
    pub rects_json: String,
}

/// Characters of a passage shown in a reference row. About one line at the
/// panel's width — enough to recognize the passage, short enough not to
/// compete with the answer.
pub const PREVIEW_CHARS: usize = 140;

/// Flatten and clip a passage for display.
pub fn preview_of(text: &str) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    match flat.char_indices().nth(PREVIEW_CHARS) {
        Some((index, _)) => format!("{}…", flat[..index].trim_end()),
        None => flat,
    }
}

/// A passage the model was shown this turn (RFC 0078).
///
/// Distinct from [`ContextCitation`], which is only what the answer *cited*.
/// References must stay honest — what was offered is not evidence — but you
/// should still be able to open a drawer and see everything the agent read.
/// Deliberately light: no text, no rectangles.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PassageRef {
    pub handle: String,
    pub paper_id: String,
    pub page_start: i32,
    pub heading_path: Option<String>,
    pub chunk_id: Option<String>,
    pub preview: String,
    /// The answer cited this one.
    pub cited: bool,
}

impl PassageRef {
    /// Every passage starts uncited; `retain_cited` decides after the answer.
    pub fn from_citation(citation: &ContextCitation) -> Self {
        Self {
            handle: citation.handle.clone(),
            paper_id: citation.paper_id.clone(),
            page_start: citation.page_start,
            heading_path: citation.heading_path.clone(),
            chunk_id: citation.chunk_id.clone(),
            preview: citation.preview.clone(),
            cited: false,
        }
    }
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
    /// `"user"` or `"agent"` — the panel tags the agent's additions.
    pub origin: String,
    /// The chunk no longer resolves — reported, never silently dropped.
    pub unresolved: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_preview_is_flattened_to_one_line() {
        assert_eq!(preview_of("  We divide\n  by sqrt(d_k).  "), "We divide by sqrt(d_k).");
    }

    #[test]
    fn a_long_preview_is_clipped_without_a_trailing_space() {
        let preview = preview_of(&"word ".repeat(200));
        assert!(preview.ends_with("…"));
        assert!(!preview.contains(" …"), "clipped mid-space leaves a gap: {preview}");
        assert!(preview.chars().count() <= PREVIEW_CHARS + 1);
    }

    #[test]
    fn a_multibyte_preview_does_not_split_a_codepoint() {
        // Slicing on a byte index inside a codepoint would panic.
        let preview = preview_of(&"é".repeat(PREVIEW_CHARS + 40));
        assert!(preview.ends_with('…'));
    }
}
