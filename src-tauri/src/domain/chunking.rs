//! Splitting an extracted document into retrieval-sized chunks (RFC 0075).
//!
//! A pure function over blocks — no database, no model, no PDF. That is the
//! point: the part of the retrieval stack most likely to need tuning is the
//! part that can be tested with a `vec![]` of synthetic blocks.
//!
//! Chunks are derived data. Nothing here is canonical; everything is
//! regenerable from `document_blocks`, and `CHUNK_VERSION` is what tells a
//! later launch that the stored chunks came from an older rule set.

use crate::domain::library::{DocumentBlock, DocumentChunk};

/// Names the strategy in `document_chunks.chunker`, so a future chunker can
/// coexist with this one rather than having to replace it in place.
pub const CHUNKER: &str = "structural";

/// Bump when the chunking *rules* change. The startup sweep re-chunks any
/// extraction sitting below the current value (RFC 0075 R6).
///
/// 2: consecutive headings merge instead of each becoming a chunk.
pub const CHUNK_VERSION: i32 = 2;

/// Tokens are estimated, never counted. A real tokenizer is not worth a
/// dependency for a bound this soft — every threshold below is a preference,
/// not a correctness constraint.
pub const CHARS_PER_TOKEN: usize = 4;

/// Close a chunk once it reaches this. Chunks land at or just above it.
const TARGET_TOKENS: usize = 450;

/// Below this, a page break is not worth breaking on — see `should_close`.
const MIN_TOKENS: usize = 150;

/// Hard ceiling. BGE-small truncates input at 512 tokens, so a chunk above this
/// would be embedded with its tail silently discarded: the text stays readable
/// and searchable while its vector quietly describes only part of it. The
/// ceiling is a property of the model, which is why it is recorded in
/// `CHUNK_VERSION` — a model with a longer context makes this a re-chunk.
const MAX_TOKENS: usize = 500;

/// Separator between blocks, matching the canonical `source_text` join.
const BLOCK_JOIN: &str = "\n\n";

pub fn estimate_tokens(text: &str) -> usize {
    tokens_for_chars(text.chars().count())
}

fn tokens_for_chars(chars: usize) -> usize {
    chars.div_ceil(CHARS_PER_TOKEN)
}

/// Split `blocks` — one extraction's worth, any order — into chunks.
///
/// Blocks are sorted by reading order internally, so callers do not have to
/// care whether the extractor emitted them sorted.
pub fn chunk_blocks(blocks: &[DocumentBlock]) -> Vec<DocumentChunk> {
    let mut ordered: Vec<&DocumentBlock> = blocks
        .iter()
        .filter(|block| {
            block
                .text
                .as_deref()
                .is_some_and(|text| !text.trim().is_empty())
        })
        .collect();
    ordered.sort_by_key(|block| (block.reading_order, block.block_index));

    let mut builder = ChunkBuilder::default();
    for block in ordered {
        for piece in split_oversized(block) {
            builder.push(block, piece);
        }
    }
    builder.finish()
}

/// One contiguous run of a block's text, with offsets into `source_text`.
///
/// Usually the whole block. A block that alone exceeds `MAX_TOKENS` is cut into
/// several — see `split_oversized`.
struct Piece {
    text: String,
    source_start: i64,
    source_end: i64,
    is_heading: bool,
}

#[derive(Default)]
struct ChunkBuilder {
    chunks: Vec<DocumentChunk>,
    /// Pieces accumulated for the chunk currently being built.
    pending: Vec<(BlockRef, Piece)>,
    /// Characters, not tokens — including the `BLOCK_JOIN` separators that will
    /// appear in the joined text. Summing per-piece token estimates instead
    /// would drift from the chunk's own `token_estimate` by the separators plus
    /// a rounding error per piece, and let a chunk cross `MAX_TOKENS`.
    pending_chars: usize,
    /// The most recent heading seen, which is what the *next* chunk inherits.
    current_heading: Option<String>,
    /// The heading in force when the pending chunk opened. Snapshotted at open
    /// time so a heading appearing mid-chunk cannot retroactively relabel it.
    pending_heading: Option<String>,
    /// Whether the pending chunk has any non-heading text yet. Guards the
    /// heading boundary — see `should_close`.
    pending_has_body: bool,
}

/// The identity fields a chunk copies from its blocks.
#[derive(Clone)]
struct BlockRef {
    id: String,
    paper_id: String,
    source_id: String,
    extraction_id: String,
    page_index: i32,
}

impl ChunkBuilder {
    fn push(&mut self, block: &DocumentBlock, piece: Piece) {
        let piece_chars = piece.text.chars().count();

        if self.should_close(block, &piece, piece_chars) {
            self.close();
        }

        // A heading updates the running heading *before* the chunk it opens
        // snapshots it, which is what makes the heading label the section it
        // introduces rather than the one it follows.
        if piece.is_heading {
            self.current_heading = Some(piece.text.trim().to_string());
        }
        // Snapshot the label while the chunk is still all headings, so a run of
        // "4 Experiments" then "4.1 Setup" is labelled by the *nearest* heading
        // — the more specific one, and the one the body actually sits under.
        // Once body text has arrived the label is fixed, which is what stops a
        // heading appearing mid-chunk from relabelling text above it.
        if !self.pending_has_body {
            self.pending_heading = self.current_heading.clone();
        }

        self.pending_chars += self.join_cost() + piece_chars;
        self.pending_has_body |= !piece.is_heading;
        self.pending.push((
            BlockRef {
                id: block.id.clone(),
                paper_id: block.paper_id.clone(),
                source_id: block.source_id.clone(),
                extraction_id: block.extraction_id.clone(),
                page_index: block.page_index,
            },
            piece,
        ));

        if self.pending_tokens() >= TARGET_TOKENS {
            self.close();
        }
    }

    fn pending_tokens(&self) -> usize {
        tokens_for_chars(self.pending_chars)
    }

    /// Characters the next `BLOCK_JOIN` will add, or zero when the chunk is
    /// empty and the piece needs no separator in front of it.
    fn join_cost(&self) -> usize {
        if self.pending.is_empty() {
            0
        } else {
            BLOCK_JOIN.chars().count()
        }
    }

    /// Whether the pending chunk should be closed *before* absorbing `piece`.
    fn should_close(&self, block: &DocumentBlock, piece: &Piece, piece_chars: usize) -> bool {
        let Some((last, _)) = self.pending.last() else {
            return false;
        };

        // A heading opens a new chunk — left at the tail of the previous one it
        // would describe text it does not introduce, which is backwards for
        // retrieval, where a heading is the strongest topical signal a chunk
        // carries.
        //
        // Unless the pending chunk is *only* headings. Consecutive headings are
        // everywhere in a paper — "4 Experiments" then "4.1 Setup", and a title
        // page where title, authors, and affiliation are all large type — and
        // closing between them emits chunks like "A Appendix": four tokens of
        // pure noise that still cost an embedding and still compete for a slot
        // in the results. Measured on the reference vault, this rule is the
        // difference between 259 sub-20-token chunks and a handful.
        if piece.is_heading {
            return self.pending_has_body;
        }

        if tokens_for_chars(self.pending_chars + self.join_cost() + piece_chars) > MAX_TOKENS {
            return true;
        }

        // A page break is a *soft* boundary. Hard-breaking there would be tidier
        // — no chunk would ever span pages — but a paragraph continuing across a
        // page break is the most common structure in a paper, and breaking it
        // produces a stub chunk at every page top. Prefer the page boundary only
        // when the chunk is already worth closing.
        block.page_index != last.page_index && self.pending_tokens() >= MIN_TOKENS
    }

    fn close(&mut self) {
        if self.pending.is_empty() {
            return;
        }

        let pieces = std::mem::take(&mut self.pending);
        self.pending_chars = 0;
        self.pending_has_body = false;

        let first = &pieces[0].0;
        let text = pieces
            .iter()
            .map(|(_, piece)| piece.text.as_str())
            .collect::<Vec<_>>()
            .join(BLOCK_JOIN);

        // Distinct block ids in reading order. A block split across chunks
        // contributes to each, and a block cut into several pieces inside one
        // chunk must not be listed twice — (chunk_id, block_id) is a primary key.
        let mut block_ids: Vec<String> = Vec::with_capacity(pieces.len());
        for (block, _) in &pieces {
            if block_ids.last() != Some(&block.id) {
                block_ids.push(block.id.clone());
            }
        }

        let chunk_index = self.chunks.len() as i32;
        self.chunks.push(DocumentChunk {
            id: format!("{}:chunk:{chunk_index}", first.extraction_id),
            paper_id: first.paper_id.clone(),
            source_id: first.source_id.clone(),
            extraction_id: first.extraction_id.clone(),
            chunk_index,
            chunker: CHUNKER.to_string(),
            chunk_version: CHUNK_VERSION,
            page_start: pieces
                .iter()
                .map(|(block, _)| block.page_index)
                .min()
                .unwrap_or(0),
            page_end: pieces
                .iter()
                .map(|(block, _)| block.page_index)
                .max()
                .unwrap_or(0),
            heading_path: self.pending_heading.clone(),
            token_estimate: estimate_tokens(&text) as i32,
            text,
            source_start: pieces[0].1.source_start,
            source_end: pieces[pieces.len() - 1].1.source_end,
            block_ids,
        });
    }

    fn finish(mut self) -> Vec<DocumentChunk> {
        self.close();
        self.chunks
    }
}

/// Cut a block into pieces that each fit under `MAX_TOKENS`.
///
/// "Never split a block" is the rule, and this is its one exception: a block
/// that alone exceeds the cap has to be split, or it becomes a chunk whose tail
/// the embedder silently discards. It happens for a page with no detectable
/// paragraph structure, and for every block extracted before RFC 0075 R1 (those
/// are page-sized by construction).
///
/// Cuts land at sentence boundaries where possible, falling back to whitespace,
/// so a piece is never severed mid-word.
fn split_oversized(block: &DocumentBlock) -> Vec<Piece> {
    let text = block.text.as_deref().unwrap_or("");
    let is_heading = block.kind == "heading";
    // Blocks written before the offset invariant existed may carry no offsets;
    // a zero-width range is honest about that rather than inventing one.
    let start = block.source_start.unwrap_or(0);

    if estimate_tokens(text) <= MAX_TOKENS {
        return vec![Piece {
            source_start: start,
            source_end: block
                .source_end
                .unwrap_or(start + text.chars().count() as i64),
            text: text.to_string(),
            is_heading,
        }];
    }

    let max_chars = MAX_TOKENS * CHARS_PER_TOKEN;
    let chars: Vec<char> = text.chars().collect();
    let mut pieces = Vec::new();
    let mut cursor = 0usize;

    while cursor < chars.len() {
        let remaining = chars.len() - cursor;
        let take = if remaining <= max_chars {
            remaining
        } else {
            // Prefer the last sentence end in the window; then the last space.
            // `.max(1)` keeps the cursor advancing even in pathological text
            // with no break at all, which is what makes this loop terminate.
            let window = &chars[cursor..cursor + max_chars];
            find_last(window, |c| matches!(c, '.' | '!' | '?'))
                .or_else(|| find_last(window, |c| c.is_whitespace()))
                .unwrap_or(max_chars)
                .max(1)
        };

        let piece: String = chars[cursor..cursor + take].iter().collect();
        let trimmed = piece.trim();
        if !trimmed.is_empty() {
            pieces.push(Piece {
                text: trimmed.to_string(),
                source_start: start + cursor as i64,
                source_end: start + (cursor + take) as i64,
                is_heading,
            });
        }
        cursor += take;
    }

    pieces
}

/// Index just past the last char in `window` satisfying `predicate`.
fn find_last(window: &[char], predicate: impl Fn(char) -> bool) -> Option<usize> {
    window
        .iter()
        .rposition(|c| predicate(*c))
        .map(|index| index + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block(index: i32, page: i32, kind: &str, text: &str) -> DocumentBlock {
        DocumentBlock {
            id: format!("ex:block:{index}"),
            paper_id: "paper".to_string(),
            source_id: "source".to_string(),
            extraction_id: "ex".to_string(),
            page_index: page,
            block_index: index,
            reading_order: index,
            kind: kind.to_string(),
            text: Some(text.to_string()),
            asset_id: None,
            source_start: Some(0),
            source_end: Some(text.chars().count() as i64),
            bbox_json: None,
        }
    }

    /// `n` tokens' worth of text, as the estimator counts them.
    fn words(tokens: usize) -> String {
        vec!["abc"; tokens].join(" ")
    }

    #[test]
    fn small_document_becomes_one_chunk() {
        let chunks = chunk_blocks(&[
            block(0, 0, "paragraph", "alpha beta"),
            block(1, 0, "paragraph", "gamma delta"),
        ]);

        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].text, "alpha beta\n\ngamma delta");
        assert_eq!(chunks[0].chunk_index, 0);
        assert_eq!(chunks[0].chunker, CHUNKER);
        assert_eq!(chunks[0].block_ids, vec!["ex:block:0", "ex:block:1"]);
    }

    #[test]
    fn empty_blocks_are_dropped() {
        let chunks = chunk_blocks(&[
            block(0, 0, "paragraph", "   "),
            block(1, 0, "paragraph", "real text"),
        ]);

        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].text, "real text");
        assert_eq!(chunks[0].block_ids, vec!["ex:block:1"]);
    }

    #[test]
    fn heading_opens_the_next_chunk_rather_than_closing_the_last() {
        let chunks = chunk_blocks(&[
            block(0, 0, "paragraph", "body of the first section"),
            block(1, 0, "heading", "Methods"),
            block(2, 0, "paragraph", "body of the second section"),
        ]);

        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].text, "body of the first section");
        assert_eq!(chunks[0].heading_path, None);
        assert!(chunks[1].text.starts_with("Methods"));
        assert_eq!(chunks[1].heading_path.as_deref(), Some("Methods"));
    }

    /// Consecutive headings must not each become their own chunk.
    ///
    /// Found by running against the reference vault: 259 of 1,032 chunks came
    /// back under 20 tokens — "4 Experiments", "A Appendix", and title-page
    /// lines where the title, authors, and affiliation are all large type. Each
    /// cost an embedding and competed for a slot in the results while carrying
    /// no retrievable content.
    #[test]
    fn consecutive_headings_do_not_each_become_a_chunk() {
        let chunks = chunk_blocks(&[
            block(0, 0, "heading", "4 Experiments"),
            block(1, 0, "heading", "4.1 Setup"),
            block(2, 0, "paragraph", "We evaluate on four benchmarks."),
        ]);

        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].text.starts_with("4 Experiments"));
        assert!(chunks[0].text.contains("4.1 Setup"));
        // The *nearest* heading labels the chunk.
        assert_eq!(chunks[0].heading_path.as_deref(), Some("4.1 Setup"));
    }

    #[test]
    fn a_title_page_of_headings_becomes_one_chunk() {
        let chunks = chunk_blocks(&[
            block(0, 0, "heading", "Attention Is All You Need"),
            block(1, 0, "heading", "Vaswani, Shazeer, Parmar"),
            block(2, 0, "heading", "Google Brain"),
        ]);

        assert_eq!(chunks.len(), 1, "a title page is one chunk, not three");
    }

    /// The guard must not defeat the rule it guards: once a chunk has body
    /// text, the next heading still opens a new chunk.
    #[test]
    fn a_heading_after_body_still_opens_a_new_chunk() {
        let chunks = chunk_blocks(&[
            block(0, 0, "heading", "3 Method"),
            block(1, 0, "paragraph", "We describe the architecture."),
            block(2, 0, "heading", "4 Experiments"),
            block(3, 0, "paragraph", "We evaluate on four benchmarks."),
        ]);

        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].heading_path.as_deref(), Some("3 Method"));
        assert_eq!(chunks[1].heading_path.as_deref(), Some("4 Experiments"));
    }

    #[test]
    fn heading_labels_every_chunk_beneath_it() {
        let chunks = chunk_blocks(&[
            block(0, 0, "heading", "Results"),
            block(1, 0, "paragraph", &words(300)),
            block(2, 0, "paragraph", &words(300)),
        ]);

        assert!(chunks.len() >= 2);
        for chunk in &chunks {
            assert_eq!(chunk.heading_path.as_deref(), Some("Results"));
        }
    }

    #[test]
    fn a_page_break_below_the_minimum_does_not_close_a_chunk() {
        // A paragraph continuing across a page break is the common case; a stub
        // chunk at the page top is the bug this rule exists to avoid.
        let chunks = chunk_blocks(&[
            block(0, 0, "paragraph", &words(40)),
            block(1, 1, "paragraph", &words(40)),
        ]);

        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].page_start, 0);
        assert_eq!(chunks[0].page_end, 1);
    }

    #[test]
    fn a_page_break_above_the_minimum_does_close_a_chunk() {
        let chunks = chunk_blocks(&[
            block(0, 0, "paragraph", &words(200)),
            block(1, 1, "paragraph", &words(40)),
        ]);

        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].page_end, 0);
        assert_eq!(chunks[1].page_start, 1);
    }

    #[test]
    fn chunks_stay_within_the_cap_and_blocks_stay_whole() {
        let blocks: Vec<DocumentBlock> = (0..12)
            .map(|index| block(index, 0, "paragraph", &words(100)))
            .collect();
        let chunks = chunk_blocks(&blocks);

        assert!(chunks.len() > 1);
        for chunk in &chunks {
            assert!(
                chunk.token_estimate as usize <= MAX_TOKENS,
                "chunk {} is {} tokens",
                chunk.chunk_index,
                chunk.token_estimate
            );
        }

        // Every block landed in exactly one chunk, and none was split.
        let mut seen: Vec<&String> = chunks.iter().flat_map(|c| &c.block_ids).collect();
        seen.sort();
        seen.dedup();
        assert_eq!(seen.len(), 12);
    }

    #[test]
    fn chunk_indexes_are_dense_and_ids_unique() {
        let blocks: Vec<DocumentBlock> = (0..20)
            .map(|index| block(index, index / 3, "paragraph", &words(120)))
            .collect();
        let chunks = chunk_blocks(&blocks);

        for (position, chunk) in chunks.iter().enumerate() {
            assert_eq!(chunk.chunk_index, position as i32);
            assert_eq!(chunk.id, format!("ex:chunk:{position}"));
        }
    }

    #[test]
    fn an_oversized_block_is_split_at_a_sentence_boundary() {
        // The pre-RFC-0075 shape: one page-sized block, far above the cap.
        let sentence = "This is a sentence of some length. ";
        let huge = sentence.repeat(200);
        let chunks = chunk_blocks(&[block(0, 0, "paragraph", &huge)]);

        assert!(chunks.len() > 1);
        for chunk in &chunks {
            assert!(chunk.token_estimate as usize <= MAX_TOKENS);
            // Never severed mid-word.
            assert!(!chunk.text.starts_with(char::is_whitespace));
            assert_eq!(chunk.text.trim(), chunk.text);
        }
        // Each piece still points back at the one block it came from.
        for chunk in &chunks {
            assert_eq!(chunk.block_ids, vec!["ex:block:0"]);
        }
    }

    #[test]
    fn an_oversized_block_without_any_break_still_terminates() {
        let unbroken = "x".repeat(MAX_TOKENS * CHARS_PER_TOKEN * 3);
        let chunks = chunk_blocks(&[block(0, 0, "paragraph", &unbroken)]);

        assert!(chunks.len() >= 3);
        let recovered: String = chunks.iter().map(|c| c.text.as_str()).collect();
        assert_eq!(recovered.chars().count(), unbroken.chars().count());
    }

    #[test]
    fn offsets_track_the_source_text_range() {
        let mut first = block(0, 0, "paragraph", "alpha");
        first.source_start = Some(0);
        first.source_end = Some(5);
        let mut second = block(1, 0, "paragraph", "beta");
        second.source_start = Some(7); // 5 + len("\n\n")
        second.source_end = Some(11);

        let chunks = chunk_blocks(&[first, second]);

        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].source_start, 0);
        assert_eq!(chunks[0].source_end, 11);
    }

    #[test]
    fn blocks_are_sorted_by_reading_order_before_chunking() {
        let chunks = chunk_blocks(&[
            block(2, 0, "paragraph", "third"),
            block(0, 0, "paragraph", "first"),
            block(1, 0, "paragraph", "second"),
        ]);

        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].text, "first\n\nsecond\n\nthird");
    }
}
