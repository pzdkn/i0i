//! The highlight primitive (RFC 0058): a standalone, colored mark on a passage.
//! Notes and chat threads attach to a highlight; the locator lives here, lifted
//! out of the thread's embedded anchor.

use serde::{Deserialize, Serialize};

/// Fixed, named palette. Stored as the name string (never hex) so UI, storage,
/// and the future agent tool share one vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum HighlightColor {
    #[default]
    Yellow,
    Green,
    Blue,
    Red,
    Purple,
    Orange,
}

/// WHERE a highlight sits on a rendered source. Payloads mirror the legacy
/// `ThreadAnchor` selection variants so migration is a field lift. Offsets are
/// browser-space (RFC 0056); the backend never resolves them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum Locator {
    #[serde(rename = "textOffset")]
    TextOffset {
        #[serde(rename = "sourceId")]
        source_id: String,
        #[serde(rename = "startOffset")]
        start_offset: i64,
        #[serde(rename = "endOffset")]
        end_offset: i64,
    },
    #[serde(rename = "pdfRect")]
    PdfRect {
        #[serde(rename = "sourceId")]
        source_id: String,
        #[serde(rename = "pageIndex")]
        page_index: i32,
        #[serde(rename = "rectsJson")]
        rects_json: String,
    },
}

impl Locator {
    pub fn source_id(&self) -> &str {
        match self {
            Locator::TextOffset { source_id, .. } | Locator::PdfRect { source_id, .. } => source_id,
        }
    }

    /// Storage discriminator, shared with the legacy `anchor_kind` values.
    pub fn kind_str(&self) -> &'static str {
        match self {
            Locator::TextOffset { .. } => "text_offset",
            Locator::PdfRect { .. } => "pdf_rect",
        }
    }
}

/// Who created a highlight. The agent variant carries the model id.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum HighlightAuthor {
    User,
    Agent {
        #[serde(rename = "model")]
        model: String,
    },
}

impl HighlightAuthor {
    pub fn kind_str(&self) -> &'static str {
        match self {
            HighlightAuthor::User => "user",
            HighlightAuthor::Agent { .. } => "agent",
        }
    }
    pub fn model(&self) -> Option<&str> {
        match self {
            HighlightAuthor::User => None,
            HighlightAuthor::Agent { model } => Some(model),
        }
    }
}

/// The primitive, as returned across IPC.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Highlight {
    pub id: String,
    pub paper_id: String,
    pub source_id: String,
    pub locator: Locator,
    pub excerpt: String,
    /// The color mark, if any. `None` = an annotated passage with no color
    /// highlight (note-only or conversation-only) — RFC 0061.
    pub color: Option<HighlightColor>,
    /// The passage's note text, if any (RFC 0061) — distinct from its
    /// conversation thread.
    pub note: Option<String>,
    pub label: Option<String>,
    pub author: HighlightAuthor,
    pub created_at: String,
    pub updated_at: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_serdes_as_lowercase_name() {
        assert_eq!(serde_json::to_string(&HighlightColor::Yellow).unwrap(), "\"yellow\"");
        assert_eq!(
            serde_json::from_str::<HighlightColor>("\"red\"").unwrap(),
            HighlightColor::Red
        );
        assert_eq!(HighlightColor::default(), HighlightColor::Yellow);
    }

    #[test]
    fn locator_round_trips_with_kind_tag_and_exposes_source() {
        let loc = Locator::TextOffset {
            source_id: "src-1".into(),
            start_offset: 10,
            end_offset: 40,
        };
        let json = serde_json::to_string(&loc).unwrap();
        assert!(json.contains("\"kind\":\"textOffset\""));
        assert_eq!(loc.source_id(), "src-1");
        let back: Locator = serde_json::from_str(&json).unwrap();
        assert_eq!(back, loc);
    }

    #[test]
    fn author_serdes_user_and_agent() {
        assert_eq!(
            serde_json::to_string(&HighlightAuthor::User).unwrap(),
            "{\"kind\":\"user\"}"
        );
        let agent = HighlightAuthor::Agent { model: "x".into() };
        let json = serde_json::to_string(&agent).unwrap();
        assert!(json.contains("\"kind\":\"agent\""));
        assert!(json.contains("\"model\":\"x\""));
        let back: HighlightAuthor = serde_json::from_str(&json).unwrap();
        assert_eq!(back, agent);
    }
}
