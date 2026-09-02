//! Grouping PDF text fragments into lines and blocks (RFC 0075 R1).
//!
//! Pure geometry: takes positioned text fragments, returns blocks with spans,
//! normalized rectangles, and character offsets. No Pdfium types cross this
//! boundary, so the grouping heuristics — the part that will need tuning
//! against real papers — are testable from a `vec![]` of rectangles.
//!
//! What this deliberately does *not* attempt: multi-column detection, table
//! structure, reading order recovery, or formula extraction. Those are why
//! RFC 0025 concluded MinerU should eventually replace this. Here we need only
//! enough structure to give the chunker paragraph boundaries and give a
//! retrieved chunk a rectangle to highlight.

use serde::{Deserialize, Serialize};

/// A positioned run of text as the PDF reports it, in PDF page space:
/// origin at the bottom-left, y increasing upward.
#[derive(Debug, Clone)]
pub struct TextFragment {
    pub text: String,
    pub left: f32,
    pub right: f32,
    pub bottom: f32,
    pub top: f32,
    pub font_size: f32,
}

/// A rectangle in the reader's coordinate system: normalized 0..1 with the
/// origin at the *top* left, matching `PdfRect` in `src/lib/domain/reader.ts`
/// and every rect already stored in `highlights.rects_json`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct NormRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Clone)]
pub struct LayoutSpan {
    pub text: String,
    pub rect: NormRect,
    /// Character offsets within the owning block's text.
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone)]
pub struct LayoutBlock {
    /// `"heading"` or `"paragraph"` — the vocabulary `document_blocks.kind`
    /// already uses.
    pub kind: &'static str,
    pub text: String,
    pub rect: NormRect,
    pub spans: Vec<LayoutSpan>,
}

pub const KIND_HEADING: &str = "heading";
pub const KIND_PARAGRAPH: &str = "paragraph";

/// Two fragments belong to the same line when they overlap vertically by more
/// than this share of the shorter one. Generous, because sub- and superscripts
/// sit well off the baseline and still belong to their line.
const LINE_OVERLAP_RATIO: f32 = 0.5;

/// A vertical gap wider than this many line-heights starts a new block.
const PARAGRAPH_GAP_RATIO: f32 = 0.75;

/// A font-size change larger than this share starts a new block — which is what
/// separates a heading from the body text under it.
const FONT_CHANGE_RATIO: f32 = 0.15;

/// A line indented by more than this many font-sizes starts a new block. This
/// is the first-line indent convention that many papers use instead of a blank
/// line between paragraphs.
const INDENT_RATIO: f32 = 0.6;

/// A line starting this far *left* of the block starts a new block. Catches the
/// column wrap that would otherwise glue the bottom of one column to the top of
/// the next.
const OUTDENT_RATIO: f32 = 2.0;

/// A heading's font must exceed the page's body size by this share.
const HEADING_FONT_RATIO: f32 = 1.12;

/// Headings are short. Longer than this and a large font is a pull quote, a
/// title page, or a mis-measurement — not a section heading.
const HEADING_MAX_CHARS: usize = 120;
const HEADING_MAX_LINES: usize = 2;

/// Fragments closer than this many font-sizes apart join with no space, which
/// keeps a style change mid-word (an italic term, a ligature run) from being
/// split into two words.
const GLUE_GAP_RATIO: f32 = 0.25;

/// Group one page's fragments into blocks.
pub fn group_page(
    fragments: &[TextFragment],
    page_width: f32,
    page_height: f32,
) -> Vec<LayoutBlock> {
    let usable: Vec<&TextFragment> = fragments
        .iter()
        .filter(|fragment| !fragment.text.trim().is_empty())
        .filter(|fragment| fragment.right > fragment.left && fragment.top > fragment.bottom)
        .collect();
    if usable.is_empty() {
        return Vec::new();
    }

    let lines = group_lines(usable);
    let body_size = body_font_size(&lines);
    let median_height = median(&mut lines.iter().map(Line::height).collect::<Vec<_>>());

    group_blocks(lines, median_height)
        .into_iter()
        .map(|group| build_block(group, body_size, page_width, page_height))
        .collect()
}

struct Line {
    fragments: Vec<TextFragment>,
    top: f32,
    bottom: f32,
    left: f32,
    font_size: f32,
}

impl Line {
    fn height(&self) -> f32 {
        self.top - self.bottom
    }

    fn char_count(&self) -> usize {
        self.fragments
            .iter()
            .map(|fragment| fragment.text.trim().chars().count())
            .sum()
    }
}

fn group_lines(fragments: Vec<&TextFragment>) -> Vec<Line> {
    // Top-down, then left-to-right. This is where a two-column page goes wrong:
    // it interleaves the columns. Accepted for now — see the module note.
    let mut sorted: Vec<&TextFragment> = fragments;
    sorted.sort_by(|a, b| {
        b.top
            .partial_cmp(&a.top)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(
                a.left
                    .partial_cmp(&b.left)
                    .unwrap_or(std::cmp::Ordering::Equal),
            )
    });

    let mut lines: Vec<Line> = Vec::new();
    for fragment in sorted {
        let joined = lines.last_mut().filter(|line| {
            let overlap = line.top.min(fragment.top) - line.bottom.max(fragment.bottom);
            let shorter = line.height().min(fragment.top - fragment.bottom);
            shorter > 0.0 && overlap > LINE_OVERLAP_RATIO * shorter
        });

        match joined {
            Some(line) => {
                line.top = line.top.max(fragment.top);
                line.bottom = line.bottom.min(fragment.bottom);
                line.left = line.left.min(fragment.left);
                line.font_size = line.font_size.max(fragment.font_size);
                line.fragments.push(fragment.clone());
            }
            None => lines.push(Line {
                top: fragment.top,
                bottom: fragment.bottom,
                left: fragment.left,
                font_size: fragment.font_size,
                fragments: vec![fragment.clone()],
            }),
        }
    }

    for line in &mut lines {
        line.fragments.sort_by(|a, b| {
            a.left
                .partial_cmp(&b.left)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
    }
    lines
}

/// The page's body text size: the most common font size, weighted by how many
/// characters are set in it. Weighting matters — a page has one title and
/// hundreds of body words, and an unweighted mode can be swayed by a handful of
/// distinct heading sizes.
fn body_font_size(lines: &[Line]) -> f32 {
    let mut weights: Vec<(f32, usize)> = Vec::new();
    for line in lines {
        for fragment in &line.fragments {
            let chars = fragment.text.trim().chars().count();
            if chars == 0 {
                continue;
            }
            // Bucket to half a point; identical-looking text often differs in
            // the third decimal place.
            let bucket = (fragment.font_size * 2.0).round() / 2.0;
            match weights.iter_mut().find(|(size, _)| *size == bucket) {
                Some((_, weight)) => *weight += chars,
                None => weights.push((bucket, chars)),
            }
        }
    }

    weights
        .into_iter()
        .max_by_key(|(_, weight)| *weight)
        .map(|(size, _)| size)
        .unwrap_or(12.0)
}

fn group_blocks(lines: Vec<Line>, median_height: f32) -> Vec<Vec<Line>> {
    let mut blocks: Vec<Vec<Line>> = Vec::new();

    for line in lines {
        let starts_block = match blocks.last() {
            None => true,
            Some(current) => {
                let previous = current.last().expect("block is never empty");
                let block_left = current.iter().map(|l| l.left).fold(f32::INFINITY, f32::min);
                let gap = previous.bottom - line.top;
                let font_delta = (line.font_size - previous.font_size).abs();

                gap > PARAGRAPH_GAP_RATIO * median_height
                    || font_delta > FONT_CHANGE_RATIO * previous.font_size.max(1.0)
                    || line.left > block_left + INDENT_RATIO * line.font_size
                    || line.left < block_left - OUTDENT_RATIO * line.font_size
            }
        };

        if starts_block {
            blocks.push(vec![line]);
        } else {
            blocks
                .last_mut()
                .expect("just checked non-empty")
                .push(line);
        }
    }

    blocks
}

fn build_block(lines: Vec<Line>, body_size: f32, page_width: f32, page_height: f32) -> LayoutBlock {
    let mut text = String::new();
    let mut spans: Vec<LayoutSpan> = Vec::new();
    let mut previous_right: Option<f32> = None;

    for (line_index, line) in lines.iter().enumerate() {
        for (fragment_index, fragment) in line.fragments.iter().enumerate() {
            let trimmed = fragment.text.trim();
            if trimmed.is_empty() {
                continue;
            }

            if !text.is_empty() {
                let same_line = fragment_index > 0;
                let glued = same_line
                    && previous_right.is_some_and(|right| {
                        fragment.left - right < GLUE_GAP_RATIO * fragment.font_size.max(1.0)
                    });
                if !glued {
                    text.push(' ');
                }
            }
            let _ = line_index;

            let start = text.chars().count();
            text.push_str(trimmed);
            let end = text.chars().count();

            spans.push(LayoutSpan {
                text: trimmed.to_string(),
                rect: normalize(
                    fragment.left,
                    fragment.right,
                    fragment.bottom,
                    fragment.top,
                    page_width,
                    page_height,
                ),
                start,
                end,
            });
            previous_right = Some(fragment.right);
        }
        previous_right = None;
    }

    let left = lines.iter().map(|l| l.left).fold(f32::INFINITY, f32::min);
    let right = lines
        .iter()
        .flat_map(|l| l.fragments.iter().map(|f| f.right))
        .fold(f32::NEG_INFINITY, f32::max);
    let top = lines
        .iter()
        .map(|l| l.top)
        .fold(f32::NEG_INFINITY, f32::max);
    let bottom = lines.iter().map(|l| l.bottom).fold(f32::INFINITY, f32::min);

    let font_size = lines.iter().map(|l| l.font_size).fold(0.0_f32, f32::max);
    let chars: usize = lines.iter().map(Line::char_count).sum();
    let is_heading = font_size > body_size * HEADING_FONT_RATIO
        && chars <= HEADING_MAX_CHARS
        && lines.len() <= HEADING_MAX_LINES;

    LayoutBlock {
        kind: if is_heading {
            KIND_HEADING
        } else {
            KIND_PARAGRAPH
        },
        text,
        rect: normalize(left, right, bottom, top, page_width, page_height),
        spans,
    }
}

/// PDF space (origin bottom-left, y up) to reader space (origin top-left,
/// normalized 0..1). Clamped, because a fragment can sit fractionally outside
/// the page box and a negative rect would render as nothing.
fn normalize(
    left: f32,
    right: f32,
    bottom: f32,
    top: f32,
    page_width: f32,
    page_height: f32,
) -> NormRect {
    if page_width <= 0.0 || page_height <= 0.0 {
        return NormRect {
            x: 0.0,
            y: 0.0,
            width: 0.0,
            height: 0.0,
        };
    }

    let x = (left / page_width).clamp(0.0, 1.0) as f64;
    let x_end = (right / page_width).clamp(0.0, 1.0) as f64;
    // The flip: PDF measures y up from the bottom, the reader measures it down
    // from the top.
    let y = ((page_height - top) / page_height).clamp(0.0, 1.0) as f64;
    let y_end = ((page_height - bottom) / page_height).clamp(0.0, 1.0) as f64;

    NormRect {
        x,
        y,
        width: (x_end - x).max(0.0),
        height: (y_end - y).max(0.0),
    }
}

fn median(values: &mut Vec<f32>) -> f32 {
    if values.is_empty() {
        return 12.0;
    }
    values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    values[values.len() / 2]
}

#[cfg(test)]
mod tests {
    use super::*;

    const PAGE_W: f32 = 600.0;
    const PAGE_H: f32 = 800.0;

    /// A fragment on a text line whose baseline top is `top`, in PDF space.
    fn frag(text: &str, left: f32, top: f32, size: f32) -> TextFragment {
        TextFragment {
            text: text.to_string(),
            left,
            right: left + text.len() as f32 * size * 0.5,
            bottom: top - size,
            top,
            font_size: size,
        }
    }

    #[test]
    fn fragments_on_the_same_baseline_become_one_line_and_one_block() {
        let blocks = group_page(
            &[
                frag("hello", 72.0, 700.0, 10.0),
                frag("world", 130.0, 700.0, 10.0),
            ],
            PAGE_W,
            PAGE_H,
        );

        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].text, "hello world");
        assert_eq!(blocks[0].spans.len(), 2);
    }

    #[test]
    fn consecutive_lines_stay_in_one_block() {
        let blocks = group_page(
            &[
                frag("first line of the paragraph", 72.0, 700.0, 10.0),
                frag("second line of the paragraph", 72.0, 688.0, 10.0),
            ],
            PAGE_W,
            PAGE_H,
        );

        assert_eq!(blocks.len(), 1);
        assert!(blocks[0].text.contains("first line"));
        assert!(blocks[0].text.contains("second line"));
    }

    #[test]
    fn a_wide_vertical_gap_starts_a_new_block() {
        let blocks = group_page(
            &[
                frag("end of one paragraph", 72.0, 700.0, 10.0),
                frag("next line same para", 72.0, 688.0, 10.0),
                // A blank line's worth of space.
                frag("start of another paragraph", 72.0, 650.0, 10.0),
            ],
            PAGE_W,
            PAGE_H,
        );

        assert_eq!(blocks.len(), 2);
    }

    #[test]
    fn a_first_line_indent_starts_a_new_block() {
        let blocks = group_page(
            &[
                frag("body text at the margin", 72.0, 700.0, 10.0),
                frag("still at the margin here", 72.0, 688.0, 10.0),
                frag("indented new paragraph", 90.0, 676.0, 10.0),
            ],
            PAGE_W,
            PAGE_H,
        );

        assert_eq!(blocks.len(), 2);
        assert!(blocks[1].text.starts_with("indented"));
    }

    #[test]
    fn a_larger_font_on_a_short_line_is_a_heading() {
        let mut fragments = vec![frag("Methods", 72.0, 700.0, 16.0)];
        // Enough body text that the modal size is unambiguous.
        for index in 0..8 {
            fragments.push(frag(
                "a line of ordinary body text on the page",
                72.0,
                670.0 - index as f32 * 12.0,
                10.0,
            ));
        }

        let blocks = group_page(&fragments, PAGE_W, PAGE_H);

        assert_eq!(blocks[0].kind, KIND_HEADING);
        assert_eq!(blocks[0].text, "Methods");
        assert!(blocks[1..].iter().all(|b| b.kind == KIND_PARAGRAPH));
    }

    #[test]
    fn a_long_run_of_large_text_is_not_a_heading() {
        // A pull quote or a title page: big, but far too long to be a heading.
        let long = "x".repeat(HEADING_MAX_CHARS + 40);
        let mut fragments = vec![frag(&long, 72.0, 700.0, 16.0)];
        for index in 0..8 {
            fragments.push(frag(
                "ordinary body text here",
                72.0,
                600.0 - index as f32 * 12.0,
                10.0,
            ));
        }

        let blocks = group_page(&fragments, PAGE_W, PAGE_H);
        assert_eq!(blocks[0].kind, KIND_PARAGRAPH);
    }

    #[test]
    fn span_offsets_index_the_block_text() {
        let blocks = group_page(
            &[
                frag("alpha", 72.0, 700.0, 10.0),
                frag("beta", 140.0, 700.0, 10.0),
            ],
            PAGE_W,
            PAGE_H,
        );

        let block = &blocks[0];
        let chars: Vec<char> = block.text.chars().collect();
        for span in &block.spans {
            let slice: String = chars[span.start..span.end].iter().collect();
            assert_eq!(slice, span.text, "span offsets must slice the block text");
        }
    }

    #[test]
    fn adjacent_fragments_join_without_a_space() {
        // A style run splitting a word: "hyper" + "link" must not become
        // "hyper link".
        let first = frag("hyper", 72.0, 700.0, 10.0);
        let second = TextFragment {
            text: "link".to_string(),
            left: first.right + 0.5,
            right: first.right + 20.0,
            bottom: first.bottom,
            top: first.top,
            font_size: 10.0,
        };

        let blocks = group_page(&[first, second], PAGE_W, PAGE_H);
        assert_eq!(blocks[0].text, "hyperlink");
    }

    #[test]
    fn rectangles_are_normalized_top_down() {
        // A fragment at the top of the page must have a small y, because the
        // reader measures y downward from the top edge.
        let blocks = group_page(&[frag("top", 60.0, 790.0, 10.0)], PAGE_W, PAGE_H);
        let rect = blocks[0].rect;

        assert!(rect.y < 0.05, "expected near the top, got y={}", rect.y);
        assert!((rect.x - 0.1).abs() < 0.001);
        assert!(rect.width > 0.0 && rect.height > 0.0);
    }

    #[test]
    fn rectangles_stay_inside_the_page_box() {
        let outside = TextFragment {
            text: "bleeding off the edge".to_string(),
            left: -20.0,
            right: PAGE_W + 50.0,
            bottom: -10.0,
            top: PAGE_H + 10.0,
            font_size: 10.0,
        };

        let blocks = group_page(&[outside], PAGE_W, PAGE_H);
        let rect = blocks[0].rect;

        assert!(rect.x >= 0.0 && rect.y >= 0.0);
        assert!(rect.x + rect.width <= 1.0 + f64::EPSILON);
        assert!(rect.y + rect.height <= 1.0 + f64::EPSILON);
    }

    #[test]
    fn empty_and_degenerate_fragments_are_dropped() {
        let blocks = group_page(
            &[
                frag("   ", 72.0, 700.0, 10.0),
                TextFragment {
                    text: "zero size".to_string(),
                    left: 10.0,
                    right: 10.0,
                    bottom: 5.0,
                    top: 5.0,
                    font_size: 10.0,
                },
                frag("real", 72.0, 680.0, 10.0),
            ],
            PAGE_W,
            PAGE_H,
        );

        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].text, "real");
    }

    #[test]
    fn an_empty_page_produces_no_blocks() {
        assert!(group_page(&[], PAGE_W, PAGE_H).is_empty());
    }

    #[test]
    fn out_of_order_fragments_are_sorted_top_down() {
        let blocks = group_page(
            &[
                frag("third line here", 72.0, 660.0, 10.0),
                frag("first line here", 72.0, 700.0, 10.0),
                frag("second line here", 72.0, 680.0, 10.0),
            ],
            PAGE_W,
            PAGE_H,
        );

        let combined: String = blocks
            .iter()
            .map(|b| b.text.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        let first = combined.find("first").expect("first present");
        let second = combined.find("second").expect("second present");
        let third = combined.find("third").expect("third present");
        assert!(first < second && second < third);
    }
}
