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
    /// An exact source-text passage created from a Reader MCP reference.
    ///
    /// PDF extraction gives us stable text offsets and a page, but not always
    /// trustworthy rectangles. Keeping that distinction avoids drawing a fake
    /// highlight while still letting the Reader navigate to the cited page.
    #[serde(rename = "sourcePassage")]
    SourcePassage {
        #[serde(rename = "sourceId")]
        source_id: String,
        #[serde(rename = "pageIndex")]
        page_index: Option<i32>,
        #[serde(rename = "startOffset")]
        start_offset: i64,
        #[serde(rename = "endOffset")]
        end_offset: i64,
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
            ThreadAnchor::SourcePassage { .. } => "source_passage",
        }
    }

    /// The quoted passage, when the anchor is a selection.
    pub fn selected_text(&self) -> Option<&str> {
        match self {
            ThreadAnchor::Document => None,
            ThreadAnchor::TextOffset { selected_text, .. }
            | ThreadAnchor::PdfRect { selected_text, .. }
            | ThreadAnchor::SourcePassage { selected_text, .. } => Some(selected_text),
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
    /// `user` for researcher-authored entries and `agent` for MCP writes.
    #[serde(default = "default_user_author")]
    pub author_kind: String,
    /// Stable caller identity supplied by the authenticated MCP grant.
    #[serde(default)]
    pub author_id: Option<String>,
    /// Research run that produced the entry, when one exists.
    #[serde(default)]
    pub run_id: Option<String>,
    pub created_at: String,
}

fn default_user_author() -> String {
    "user".to_string()
}

/// An entry to append. Internal to the backend; never crosses the IPC boundary.
#[derive(Debug, Clone)]
pub struct ChatEntryDraft {
    pub kind: String,
    pub body: String,
    pub model: Option<String>,
    pub context_summary: Option<ChatContextSummary>,
    pub pinned: bool,
    pub author_kind: String,
    pub author_id: Option<String>,
    pub run_id: Option<String>,
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
            author_kind: "user".to_string(),
            author_id: None,
            run_id: None,
        }
    }

    /// A note written through an authenticated agent connection.
    pub fn agent_note(body: String, caller: String, run_id: Option<String>) -> Self {
        Self {
            kind: ENTRY_NOTE.to_string(),
            body,
            model: None,
            context_summary: None,
            pinned: true,
            author_kind: "agent".to_string(),
            author_id: Some(caller),
            run_id,
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
            author_kind: "user".to_string(),
            author_id: None,
            run_id: None,
        }
    }

    /// An AI answer — unpinned by default; star the keepers.
    pub fn answer(body: String, model: String, context_summary: ChatContextSummary) -> Self {
        Self {
            kind: ENTRY_ANSWER.to_string(),
            body,
            model: Some(model.clone()),
            context_summary: Some(context_summary),
            pinned: false,
            author_kind: "agent".to_string(),
            author_id: Some(model.clone()),
            run_id: None,
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

/// Progress while the agent decides what evidence it needs (RFC 0078).
///
/// Phase 1 can take up to three round trips before the first word of prose. A
/// silent panel through that reads as hung rather than thinking.
#[derive(Debug, Clone, Serialize)]
#[serde(
    tag = "event",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ChatProgress {
    Deciding {
        turn_id: String,
        thread_id: Option<String>,
    },
    SearchingPaper {
        turn_id: String,
        thread_id: Option<String>,
        query: String,
    },
    SearchingLibrary {
        turn_id: String,
        thread_id: Option<String>,
        query: String,
    },
    SearchingWeb {
        turn_id: String,
        thread_id: Option<String>,
        query: String,
    },
    ReadingSource {
        turn_id: String,
        thread_id: Option<String>,
        title: String,
    },
    StartingDeepResearch {
        turn_id: String,
        thread_id: Option<String>,
        title: String,
    },
    Retrieved {
        turn_id: String,
        thread_id: Option<String>,
        count: usize,
    },
}

/// Event emitted when background title generation updates a thread title.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatThreadUpdated {
    pub thread_id: String,
    pub title: String,
}

/// What the model actually saw, rendered in the UI so context is inspectable.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatContextSummary {
    pub paper_title: String,
    pub included_chars: usize,
    pub truncated: bool,
    /// Context items that made it into the prompt (RFC 0077).
    ///
    /// These four are `serde(default)` because this struct is already
    /// serialized into `chat_entries.context_json`: every answer written before
    /// RFC 0077 must keep deserializing. Old rows read as zeros and
    /// `compacted: false`, which is accurate for them.
    #[serde(default)]
    pub context_items: usize,
    /// Items that did not fit the budget. What the UI needs to say so, rather
    /// than letting the paper-text fallback quietly hide the loss.
    #[serde(default)]
    pub dropped_items: usize,
    /// Items whose chunk no longer exists and could not be re-resolved.
    #[serde(default)]
    pub unresolved_items: usize,
    /// A compaction summary stood in for earlier turns.
    #[serde(default)]
    pub compacted: bool,
    /// The retrieval loop hit a bound and stopped short (RFC 0078). Reported
    /// so a capped turn does not read as an agent that decided it had enough.
    #[serde(default)]
    pub retrieval_capped: bool,
    /// Every passage the model was shown this turn, cited or not (RFC 0078).
    /// Powers the "what the agent read" drawer; `citations` stays the honest
    /// reference list.
    #[serde(default)]
    pub passages: Vec<crate::domain::context::PassageRef>,
    /// Resolves the `[C1]` markers in this answer back to places in the PDF.
    ///
    /// Stored with the answer rather than recomputed: handles are assigned per
    /// assembly, so the same chunk is `[C3]` in one turn and `[C1]` in the next.
    /// Reopening a thread must show the markers the model actually wrote.
    #[serde(default)]
    pub citations: Vec<crate::domain::context::ContextCitation>,
    /// Web evidence actually cited by the answer (RFC 0097).
    ///
    /// Kept separate from PDF citations because URLs have no page geometry.
    #[serde(default)]
    pub external_citations: Vec<crate::domain::context::ExternalCitation>,
    /// Whether this turn attempted a bounded web lookup and how it ended.
    #[serde(default)]
    pub web_lookup: WebLookupOutcome,
    /// Background Deep Research runs started by this turn.
    #[serde(default)]
    pub research_activities: Vec<ResearchActivity>,
    /// The queries retrieval ran this turn, oldest first (RFC 0079 R5.5).
    ///
    /// Stored, not just emitted as progress: a past answer with no references
    /// is unreadable without knowing whether anything was looked up.
    #[serde(default)]
    pub retrieval_queries: Vec<String>,
    /// False when the paper has no chunks to retrieve from (RFC 0079 R5.4).
    ///
    /// The difference between "nothing matched" and "this paper was never
    /// indexed" — the second is the reader's to fix, and used to be invisible.
    /// Defaults to `true` so pre-0079 answers do not read as unindexed.
    #[serde(default = "yes")]
    pub paper_indexed: bool,
}

/// `serde(default)` for a bool that must default to *true*.
fn yes() -> bool {
    true
}

/// Hand-written rather than derived for one field: `paper_indexed` must default
/// to true. A derived `false` would make every summary built from `..default()`
/// claim the paper is unindexed.
impl Default for ChatContextSummary {
    fn default() -> Self {
        Self {
            paper_title: String::new(),
            included_chars: 0,
            truncated: false,
            context_items: 0,
            dropped_items: 0,
            unresolved_items: 0,
            compacted: false,
            retrieval_capped: false,
            passages: Vec::new(),
            citations: Vec::new(),
            external_citations: Vec::new(),
            web_lookup: WebLookupOutcome::NotRequested,
            research_activities: Vec::new(),
            retrieval_queries: Vec::new(),
            paper_indexed: true,
        }
    }
}

/// Durable, user-safe outcome of one bounded chat web lookup (RFC 0101).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum WebLookupOutcome {
    #[default]
    NotRequested,
    Succeeded {
        source_count: usize,
    },
    NoEvidence,
    Unavailable {
        message: String,
    },
}

/// A durable link from a chat answer to an asynchronous Deep Research run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResearchActivity {
    pub search_id: String,
    pub run_id: String,
    pub title: String,
    /// Status when the activity was linked. The UI follows later updates by id.
    pub status: String,
}

/// Event pushed to the frontend over a Tauri channel while a streamed reply
/// is in flight. Variant tags are explicit, like the enums above.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "event")]
pub enum ChatStreamEvent {
    #[serde(rename = "progress")]
    Progress { progress: ChatProgress },
    #[serde(rename = "delta")]
    Delta { text: String },
    #[serde(rename = "done")]
    Done { thread: ChatThreadView },
    #[serde(rename = "error")]
    Error { message: String },
}

/// Event pushed over the annotation channel (RFC 0059 follow-up): the fast
/// annotation pass streams one `Intent` per marked passage, then `Done`. Kept
/// separate from `ChatStreamEvent` because annotation persists nothing and has
/// no thread to return.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "event")]
pub enum AnnotateEvent {
    #[serde(rename = "intent")]
    Intent {
        quote: String,
        color: String,
        label: Option<String>,
        note: Option<String>,
    },
    #[serde(rename = "done")]
    Done,
    #[serde(rename = "error")]
    Error { message: String },
}

/// One passage the AI proposes to highlight (RFC 0064 auto-highlight). Returned
/// as a list from the `auto_highlight` command; the frontend resolves each quote
/// to a locator and creates the AI highlight (reusing the RFC 0059 path).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HighlightIntentPayload {
    pub quote: String,
    pub color: String,
    pub label: Option<String>,
    pub note: Option<String>,
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
    fn source_passage_keeps_exact_offsets_and_optional_page() {
        let anchor = ThreadAnchor::SourcePassage {
            source_id: "pdf:paper:version-1".to_string(),
            page_index: Some(3),
            start_offset: 120,
            end_offset: 148,
            selected_text: "same words, specific location".to_string(),
        };
        let value = serde_json::to_value(&anchor).expect("anchor serializes");
        assert_eq!(value["kind"], "sourcePassage");
        assert_eq!(value["pageIndex"], 3);
        assert_eq!(value["startOffset"], 120);
        assert_eq!(
            serde_json::from_value::<ThreadAnchor>(value).expect("anchor parses"),
            anchor
        );
    }

    #[test]
    fn annotate_intent_event_serializes_with_event_tag() {
        let event = AnnotateEvent::Intent {
            quote: "scaled dot-product".to_string(),
            color: "yellow".to_string(),
            label: Some("key idea".to_string()),
            note: None,
        };
        let json = serde_json::to_value(&event).expect("event serializes");
        assert_eq!(json["event"], "intent");
        assert_eq!(json["quote"], "scaled dot-product");
        assert_eq!(json["color"], "yellow");
        assert_eq!(json["label"], "key idea");
        assert!(json["note"].is_null());
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
                    ..ChatContextSummary::default()
                },
            )
            .pinned
        );
    }

    #[test]
    fn old_context_summaries_default_new_research_fields() {
        let summary: ChatContextSummary =
            serde_json::from_str(r#"{"paperTitle":"Old","includedChars":10,"truncated":false}"#)
                .expect("old summary parses");

        assert!(summary.external_citations.is_empty());
        assert!(summary.research_activities.is_empty());
        assert_eq!(summary.web_lookup, WebLookupOutcome::NotRequested);
        assert!(summary.paper_indexed);
    }
}
