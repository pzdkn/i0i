//! Build the model context (system prompt + inspectable summary) for a paper.
//!
//! v1 context is the extracted full text under a character budget — no
//! chunking or retrieval. When the text exceeds the budget we keep the head,
//! drop the tail, and record that it was truncated so the UI can say so.

use crate::domain::chat::ChatContextSummary;

/// The assembled system message plus the summary shown to the user.
pub(super) struct ContextBundle {
    pub system_prompt: String,
    pub summary: ChatContextSummary,
}

/// Assemble the system prompt and context summary from paper metadata + text.
///
/// When `anchor_text` is set (a thread anchored to a passage), it is
/// foregrounded in the prompt and always included — even if the paper body is
/// truncated to the budget — so a passage-scoped question never loses its
/// passage.
pub(super) fn build_context(
    title: &str,
    authors: &[String],
    venue: &str,
    year: i32,
    source_text: &str,
    max_context_chars: usize,
    anchor_text: Option<&str>,
) -> ContextBundle {
    let (context_text, truncated) = truncate_to_chars(source_text, max_context_chars);
    let included_chars = context_text.chars().count();
    let truncation_note = if truncated {
        format!(" (truncated to first {included_chars} characters)")
    } else {
        String::new()
    };

    let focus_block = match anchor_text.map(str::trim).filter(|text| !text.is_empty()) {
        Some(passage) => format!(
            "The reader is focused on this passage; weight it heavily but you may\n\
             use the rest of the paper:\n\
             > {passage}\n\
             \n"
        ),
        None => String::new(),
    };

    let system_prompt = format!(
        "You are a research assistant inside i0i. Answer questions about the\n\
         paper below. Ground every claim in the paper text. If the paper does\n\
         not contain the answer, say so explicitly.\n\
         \n\
         Be brief. Two or three sentences answers most questions; a short list\n\
         answers the rest. No preamble, no restating the question, no summary\n\
         of what you just said. The reader has the paper open next to you —\n\
         they want the answer, not an essay.\n\
         \n\
         Title: {title}\n\
         Authors: {authors}\n\
         Venue: {venue} {year}\n\
         \n\
         {focus_block}Paper text{truncation_note}:\n\
         {context_text}",
        authors = authors.join(", "),
    );

    ContextBundle {
        system_prompt,
        summary: ChatContextSummary {
            paper_title: title.to_string(),
            included_chars,
            truncated,
            // Filled in by ContextManager, which is the only thing that knows
            // about context items. Zero here is honest: build_context has none.
            ..ChatContextSummary::default()
        },
    }
}

/// Keep the first `max_chars` characters; report whether anything was dropped.
///
/// Counting in `char`s (not bytes) keeps multibyte text from being split mid
/// codepoint.
fn truncate_to_chars(text: &str, max_chars: usize) -> (String, bool) {
    let mut included = String::new();
    for (index, character) in text.chars().enumerate() {
        if index >= max_chars {
            return (included, true);
        }
        included.push(character);
    }
    (included, false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn authors() -> Vec<String> {
        vec!["A. Vaswani".to_string(), "N. Shazeer".to_string()]
    }

    #[test]
    fn context_under_budget_is_not_truncated() {
        let bundle = build_context(
            "Attention Is All You Need",
            &authors(),
            "NeurIPS",
            2017,
            "Short body text.",
            1000,
            None,
        );

        assert!(!bundle.summary.truncated);
        assert_eq!(
            bundle.summary.included_chars,
            "Short body text.".chars().count()
        );
        assert_eq!(bundle.summary.paper_title, "Attention Is All You Need");
        assert!(bundle.system_prompt.contains("Short body text."));
        assert!(bundle.system_prompt.contains("Attention Is All You Need"));
        assert!(bundle.system_prompt.contains("A. Vaswani"));
        assert!(bundle.system_prompt.contains("NeurIPS"));
        assert!(!bundle.system_prompt.contains("truncated"));
        assert!(!bundle.system_prompt.contains("focused on this passage"));
    }

    #[test]
    fn context_over_budget_truncates_and_notes_it() {
        let body = "abcdefghijklmnopqrstuvwxyz"; // 26 chars
        let bundle = build_context("T", &authors(), "V", 2020, body, 10, None);

        assert!(bundle.summary.truncated);
        assert_eq!(bundle.summary.included_chars, 10);
        assert!(bundle.system_prompt.contains("abcdefghij"));
        assert!(!bundle.system_prompt.contains("klmnop"));
        assert!(bundle
            .system_prompt
            .contains("truncated to first 10 characters"));
    }

    #[test]
    fn anchor_passage_is_foregrounded_even_when_paper_truncated() {
        // Budget of 5 truncates the body, but the anchor passage must survive.
        let bundle = build_context(
            "T",
            &authors(),
            "V",
            2020,
            "abcdefghijklmnop",
            5,
            Some("the focused passage"),
        );

        assert!(bundle.summary.truncated);
        assert!(bundle.system_prompt.contains("focused on this passage"));
        assert!(bundle.system_prompt.contains("the focused passage"));
        // The passage is included even though the body was cut to 5 chars.
        assert!(bundle.system_prompt.contains("abcde"));
        assert!(!bundle.system_prompt.contains("fghij"));
    }

    #[test]
    fn truncation_counts_characters_not_bytes_without_panicking() {
        // Each "é" is two bytes; a byte-based cut at 3 would split a codepoint.
        let body = "ééééé"; // 5 chars, 10 bytes
        let (included, truncated) = truncate_to_chars(body, 3);

        assert_eq!(included, "ééé");
        assert!(truncated);
        assert_eq!(included.chars().count(), 3);
    }

    #[test]
    fn truncation_at_exact_length_is_not_truncated() {
        let (included, truncated) = truncate_to_chars("abcde", 5);
        assert_eq!(included, "abcde");
        assert!(!truncated);
    }
}
