//! Chat domain types: anchored threads of entries, plus pins (RFC 0033).
//!
//! A conversation is a `ChatThread` attached to a `ThreadAnchor` (a text
//! selection, or the whole document). A thread holds an ordered list of
//! `ChatEntry` items, each a note you wrote or a question/answer with the AI.
//! Any entry can be pinned; notes are pinned by default.
//!
//! `ChatScope` keeps paper-scope and (later) vault-scope on one surface: the
//! store persists `scope_kind` + `scope_id` columns rather than a paper foreign
//! key, so a new scope is a new enum variant rather than a schema change.

use serde::{Deserialize, Serialize};

/// Entry kind discriminators (storage + role mapping).
pub const ENTRY_NOTE: &str = "note";
pub const ENTRY_QUESTION: &str = "question";
pub const ENTRY_ANSWER: &str = "answer";

/// What a thread is scoped to.
///
/// Serialized with an internal `kind` tag, e.g. `{ "kind": "paper", "paperId": "x" }`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum ChatScope {
    #[serde(rename = "paper")]
    Paper {
        #[serde(rename = "paperId")]
        paper_id: String,
    },
    // Vault { vault_id }   // follow-up RFC, additive
}

impl ChatScope {
    /// Storage discriminator column value.
    pub fn kind(&self) -> &'static str {
        match self {
            ChatScope::Paper { .. } => "paper",
        }
    }

    /// Storage id column value (the paper id for paper scope).
    pub fn id(&self) -> &str {
        match self {
            ChatScope::Paper { paper_id } => paper_id,
        }
    }
}

/// Where a thread is attached in the document.
///
/// Variant tags and field names are explicit so the wire contract does not
/// depend on serde's case-conversion behavior for tagged enums.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum ThreadAnchor {
    /// The whole paper.
    #[serde(rename = "document")]
    Document,
    /// A reader text selection.
    #[serde(rename = "textOffset")]
    TextOffset {
        #[serde(rename = "sourceId")]
        source_id: String,
        #[serde(rename = "startOffset")]
        start_offset: i64,
        #[serde(rename = "endOffset")]
        end_offset: i64,
        #[serde(rename = "selectedText")]
        selected_text: String,
    },
    /// A PDF rectangle selection.
    #[serde(rename = "pdfRect")]
    PdfRect {
        #[serde(rename = "sourceId")]
        source_id: String,
        #[serde(rename = "pageIndex")]
        page_index: i32,
        #[serde(rename = "rectsJson")]
        rects_json: String,
        #[serde(rename = "selectedText")]
        selected_text: String,
    },
}

impl ThreadAnchor {
    /// Storage `anchor_kind` value (snake_case, shared with paper-note anchors).
    pub fn storage_kind(&self) -> &'static str {
        match self {
            ThreadAnchor::Document => "document",
            ThreadAnchor::TextOffset { .. } => "text_offset",
            ThreadAnchor::PdfRect { .. } => "pdf_rect",
        }
    }

    /// The quoted passage, when the anchor is a selection.
    pub fn selected_text(&self) -> Option<&str> {
        match self {
            ThreadAnchor::Document => None,
            ThreadAnchor::TextOffset { selected_text, .. }
            | ThreadAnchor::PdfRect { selected_text, .. } => Some(selected_text),
        }
    }

    /// Default thread title: the quoted passage, or "Whole paper".
    pub fn default_title(&self) -> String {
        match self.selected_text() {
            Some(text) if !text.trim().is_empty() => text.trim().to_string(),
            _ => "Whole paper".to_string(),
        }
    }
}

/// A thread: an anchor plus identity and title.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatThread {
    pub id: String,
    pub anchor: ThreadAnchor,
    pub title: String,
    pub created_at: String,
    pub updated_at: String,
}

/// A thread plus per-thread counts, for the Threads list.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatThreadSummary {
    pub id: String,
    pub anchor: ThreadAnchor,
    pub title: String,
    pub entry_count: i64,
    pub pinned_count: i64,
    pub updated_at: String,
}

/// One entry in a thread.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatEntry {
    pub id: String,
    pub thread_id: String,
    pub kind: String, // "note" | "question" | "answer"
    pub body: String,
    pub model: Option<String>,                       // answers only
    pub context_summary: Option<ChatContextSummary>, // answers only
    pub pinned: bool,
    pub created_at: String,
}

/// An entry to append. Internal to the backend; never crosses the IPC boundary.
#[derive(Debug, Clone)]
pub struct ChatEntryDraft {
    pub kind: String,
    pub body: String,
    pub model: Option<String>,
    pub context_summary: Option<ChatContextSummary>,
    pub pinned: bool,
}

impl ChatEntryDraft {
    /// A self-authored note — pinned by default (writing one is curation).
    pub fn note(body: String) -> Self {
        Self {
            kind: ENTRY_NOTE.to_string(),
            body,
            model: None,
            context_summary: None,
            pinned: true,
        }
    }

    /// A question to the AI — unpinned by default.
    pub fn question(body: String) -> Self {
        Self {
            kind: ENTRY_QUESTION.to_string(),
            body,
            model: None,
            context_summary: None,
            pinned: false,
        }
    }

    /// An AI answer — unpinned by default; star the keepers.
    pub fn answer(body: String, model: String, context_summary: ChatContextSummary) -> Self {
        Self {
            kind: ENTRY_ANSWER.to_string(),
            body,
            model: Some(model),
            context_summary: Some(context_summary),
            pinned: false,
        }
    }
}

/// A thread plus its entries (oldest-first).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatThreadView {
    pub thread: ChatThread,
    pub entries: Vec<ChatEntry>,
}

/// A pinned entry with the thread context needed to render and link it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PinnedHighlight {
    pub entry: ChatEntry,
    pub thread_title: String,
    pub anchor: ThreadAnchor,
}

/// What the model actually saw, rendered in the UI so context is inspectable.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatContextSummary {
    pub paper_title: String,
    pub included_chars: usize,
    pub truncated: bool,
}

/// Event pushed to the frontend over a Tauri channel while a streamed reply
/// is in flight. Variant tags are explicit, like the enums above.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "event")]
pub enum ChatStreamEvent {
    #[serde(rename = "delta")]
    Delta { text: String },
    #[serde(rename = "done")]
    Done { thread: ChatThreadView },
    #[serde(rename = "error")]
    Error { message: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paper_scope_serializes_with_kind_tag() {
        let scope = ChatScope::Paper {
            paper_id: "vaswani2017".to_string(),
        };
        let json = serde_json::to_value(&scope).expect("scope serializes");
        assert_eq!(json["kind"], "paper");
        assert_eq!(json["paperId"], "vaswani2017");
    }

    #[test]
    fn paper_scope_round_trips_from_camel_case() {
        let scope: ChatScope =
            serde_json::from_str(r#"{ "kind": "paper", "paperId": "abc" }"#).expect("scope parses");
        assert_eq!(
            scope,
            ChatScope::Paper {
                paper_id: "abc".to_string()
            }
        );
    }

    #[test]
    fn scope_exposes_storage_kind_and_id() {
        let scope = ChatScope::Paper {
            paper_id: "abc".to_string(),
        };
        assert_eq!(scope.kind(), "paper");
        assert_eq!(scope.id(), "abc");
    }

    #[test]
    fn document_anchor_serializes_to_kind_only() {
        let json = serde_json::to_value(ThreadAnchor::Document).expect("anchor serializes");
        assert_eq!(json["kind"], "document");
    }

    #[test]
    fn text_offset_anchor_round_trips_with_camel_case_fields() {
        let anchor = ThreadAnchor::TextOffset {
            source_id: "src-1".to_string(),
            start_offset: 4,
            end_offset: 16,
            selected_text: "scaled dot-product".to_string(),
        };
        let json = serde_json::to_value(&anchor).expect("anchor serializes");
        assert_eq!(json["kind"], "textOffset");
        assert_eq!(json["sourceId"], "src-1");
        assert_eq!(json["startOffset"], 4);
        assert_eq!(json["selectedText"], "scaled dot-product");

        let back: ThreadAnchor = serde_json::from_value(json).expect("anchor parses");
        assert_eq!(back, anchor);
    }

    #[test]
    fn anchor_default_title_uses_passage_or_whole_paper() {
        assert_eq!(ThreadAnchor::Document.default_title(), "Whole paper");
        let anchor = ThreadAnchor::PdfRect {
            source_id: "s".to_string(),
            page_index: 2,
            rects_json: "[]".to_string(),
            selected_text: "  multi-head  ".to_string(),
        };
        assert_eq!(anchor.default_title(), "multi-head");
        assert_eq!(anchor.storage_kind(), "pdf_rect");
    }

    #[test]
    fn entry_drafts_set_pin_default_by_kind() {
        assert!(ChatEntryDraft::note("n".to_string()).pinned);
        assert!(!ChatEntryDraft::question("q".to_string()).pinned);
        assert!(
            !ChatEntryDraft::answer(
                "a".to_string(),
                "m".to_string(),
                ChatContextSummary {
                    paper_title: "t".to_string(),
                    included_chars: 1,
                    truncated: false,
                },
            )
            .pinned
        );
    }
}
