//! ContextManager — what the model is allowed to see (RFC 0077).
//!
//! The agent's working memory. ChatAgent asks this for a prompt and never calls
//! [`SearchService`] itself; this is the only door to retrieval.
//!
//! Two kinds of context, with different lifetimes:
//!
//! - **Persistent** — `chat_context_items` rows, keyed by thread. Survive
//!   restarts, removed only by [`ContextManager::delete_context`] or compaction.
//! - **Ephemeral** — the [`EphemeralContext`] parameter. The current selection
//!   and page are dropped by never being written down, so a selection change
//!   needs no invalidation.
//!
//! Only [`ContextManager::compact_context`] calls a model.
//! [`ContextManager::get_context`] is pure assembly: if it could trigger
//! compaction, some asks would silently gain a round-trip before the first
//! token.

use std::sync::Arc;

use crate::domain::chat::{ChatContextSummary, ChatEntry};
use crate::domain::chunking::{estimate_tokens, CHARS_PER_TOKEN};
use crate::domain::context::{
    ContextCitation, ContextItem, ContextItemDraft, ContextItemView, ContextKey, EphemeralContext,
    CONTEXT_KIND_CHUNK, CONTEXT_KIND_SUMMARY,
};
use crate::domain::library::DocumentChunk;
use crate::services::chat::context::build_context;
use crate::services::search::{SearchRequest, SearchResponse, SearchService};
use crate::storage::library_store::LibraryStore;

/// Hits pulled into ephemeral context before an answer (RFC 0077 R1).
///
/// Small on purpose: this is pre-answer retrieval, not a research pass, and
/// every chunk here competes with the paper text for the same budget.
pub const RETRIEVAL_LIMIT: usize = 5;

/// Facts about the paper the thread is about, supplied by the caller.
///
/// Passed in rather than fetched so ContextManager does not depend on
/// ReaderService — the ask path has already loaded the document by this point.
pub struct PaperFacts<'a> {
    pub title: &'a str,
    pub authors: &'a [String],
    pub venue: &'a str,
    pub year: i32,
    /// Canonical extracted text, used for the row-4 fallback.
    pub source_text: &'a str,
}

pub struct ContextRequest<'a> {
    /// `None` on the anchor ask path, where the thread does not exist yet — that
    /// call gets ephemeral context only.
    pub thread_id: Option<&'a str>,
    pub paper: PaperFacts<'a>,
    /// The thread's entries, oldest first. Filtered by the compaction watermark.
    pub entries: &'a [ChatEntry],
    pub ephemeral: &'a EphemeralContext,
    /// Chunks retrieved for this turn only (RFC 0077 R1). Never persisted.
    pub retrieved: &'a [DocumentChunk],
}

/// The assembled prompt, provider-neutral.
///
/// Deliberately not `Vec<WireMessage>`: the wire format is OpenRouter's, and
/// binding context assembly to one provider is the mistake SearchService was
/// designed to avoid. `chat/service.rs` converts.
pub struct AssembledContext {
    pub system_prompt: String,
    /// Entries to replay, after the compaction watermark.
    pub entries: Vec<ChatEntry>,
    /// Carries the citation map, so the answer that quotes `[C1]` is stored
    /// with the thing that resolves it.
    pub summary: ChatContextSummary,
}

#[derive(Clone)]
pub struct ContextManager {
    store: LibraryStore,
    search: Arc<SearchService>,
    /// The same cap `build_context` has always used, shared between context
    /// items and the paper-text fallback rather than added on top of it.
    max_context_chars: usize,
}

impl ContextManager {
    pub fn new(store: LibraryStore, search: Arc<SearchService>, max_context_chars: usize) -> Self {
        Self {
            store,
            search,
            max_context_chars,
        }
    }

    /// Find candidate passages. Commits nothing — `add_context` does that.
    ///
    /// A thin pass-through, so SearchService stays caller-agnostic and
    /// ContextManager gets no privileged ranking.
    pub async fn search(&self, request: SearchRequest) -> Result<SearchResponse, String> {
        self.search.search(request).await
    }

    /// Commit a chunk to persistent context.
    ///
    /// Takes only a chunk id: the durable anchor and token estimate are read off
    /// the chunk, so no caller has to know that layout. Adding a chunk already
    /// in context is a no-op returning the existing item.
    pub fn add_context(&self, thread_id: &str, chunk_id: &str) -> Result<ContextItem, String> {
        let existing = self.store.context_items(thread_id)?;
        if let Some(item) = existing
            .iter()
            .find(|item| item.chunk_id.as_deref() == Some(chunk_id))
        {
            return Ok(item.clone());
        }

        let chunk = self
            .store
            .chunks_by_ids(&[chunk_id.to_string()])?
            .into_iter()
            .next()
            .ok_or_else(|| format!("No chunk {chunk_id}"))?;

        self.store.insert_context_item(
            thread_id,
            &ContextItemDraft {
                kind: CONTEXT_KIND_CHUNK.to_string(),
                chunk_id: Some(chunk.id.clone()),
                paper_id: Some(chunk.paper_id.clone()),
                source_start: Some(chunk.source_start),
                source_end: Some(chunk.source_end),
                body: None,
                covers_through_entry_id: None,
                token_estimate: chunk.token_estimate,
            },
        )
    }

    /// Drop one item, by item id or by chunk id.
    pub fn delete_context(&self, thread_id: &str, key: &ContextKey) -> Result<bool, String> {
        self.store.delete_context_item(thread_id, key)
    }

    /// The thread's persistent context, resolved, for display.
    pub fn list_context(&self, thread_id: &str) -> Result<Vec<ContextItemView>, String> {
        let items = self.store.context_items(thread_id)?;
        items
            .iter()
            .map(|item| {
                let resolved = self.resolve(item)?;
                Ok(match resolved {
                    Resolved::Summary(body) => ContextItemView {
                        id: item.id.clone(),
                        kind: item.kind.clone(),
                        chunk_id: None,
                        paper_id: None,
                        page_start: None,
                        heading_path: None,
                        text: body,
                        token_estimate: item.token_estimate,
                        unresolved: false,
                    },
                    Resolved::Chunks(chunks) => ContextItemView {
                        id: item.id.clone(),
                        kind: item.kind.clone(),
                        chunk_id: item.chunk_id.clone(),
                        paper_id: item.paper_id.clone(),
                        page_start: chunks.first().map(|chunk| chunk.page_start),
                        heading_path: chunks
                            .first()
                            .and_then(|chunk| chunk.heading_path.clone()),
                        text: join_chunk_text(&chunks),
                        token_estimate: item.token_estimate,
                        unresolved: false,
                    },
                    Resolved::Unresolved => ContextItemView {
                        id: item.id.clone(),
                        kind: item.kind.clone(),
                        chunk_id: item.chunk_id.clone(),
                        paper_id: item.paper_id.clone(),
                        page_start: None,
                        heading_path: None,
                        text: String::new(),
                        token_estimate: item.token_estimate,
                        unresolved: true,
                    },
                })
            })
            .collect()
    }

    /// Assemble the prompt under a token budget. No model call, no network.
    ///
    /// Fill order, stopping when the budget is spent:
    ///
    /// | # | item | note |
    /// |---|---|---|
    /// | 0 | ephemeral selection | **outside** the budget, as today |
    /// | 1 | compaction summaries | the only record of what was dropped |
    /// | 2 | entries after the watermark | the live conversation |
    /// | 3 | persistent chunks | selected newest-first, emitted in position order |
    /// | 4 | retrieved chunks (this turn only) | |
    /// | 5 | paper head text | today's behaviour, with what is left |
    ///
    /// Row 5 is what keeps a thread with no context items byte-identical to the
    /// pre-RFC prompt — which is every thread that exists today.
    pub fn get_context(&self, request: ContextRequest<'_>) -> Result<AssembledContext, String> {
        let items = match request.thread_id {
            Some(thread_id) => self.store.context_items(thread_id)?,
            // The anchor path: no thread exists yet, so there is nothing
            // persistent to read. Ephemeral only, by construction.
            None => Vec::new(),
        };

        let mut assembly = Assembly::new(self.budget_tokens());
        let mut unresolved = 0usize;
        let mut watermark: Option<String> = None;

        // Row 1 — compaction summaries, and the watermark they set.
        for item in items.iter().filter(|item| item.kind == CONTEXT_KIND_SUMMARY) {
            if let Some(entry_id) = &item.covers_through_entry_id {
                watermark = Some(entry_id.clone());
            }
            if let Resolved::Summary(body) = self.resolve(item)? {
                assembly.push_summary(body);
            }
        }

        // Row 3 — persistent chunks. Selected newest-first under budget
        // pressure; emitted in position order so the prompt reads in the order
        // the context was built.
        let chunk_items: Vec<&ContextItem> = items
            .iter()
            .filter(|item| item.kind == CONTEXT_KIND_CHUNK)
            .collect();
        for item in chunk_items.iter().rev() {
            match self.resolve(item)? {
                Resolved::Chunks(chunks) => {
                    assembly.push_item(item, &chunks);
                }
                Resolved::Unresolved => unresolved += 1,
                Resolved::Summary(_) => {}
            }
        }
        assembly.restore_emission_order();

        // Row 4 — this turn's retrieval. Ephemeral: never written down, so it
        // is re-selected for every turn.
        for chunk in request.retrieved {
            if items
                .iter()
                .any(|item| item.chunk_id.as_deref() == Some(chunk.id.as_str()))
            {
                continue; // already present as a persistent item
            }
            assembly.push_retrieved(chunk);
        }

        // Row 2 — entries after the watermark. Not charged against the budget,
        // matching today: the thread's turns have always been sent in full.
        let entries = entries_after(request.entries, watermark.as_deref());

        // Row 5 — paper text with what is left. When nothing above spent any
        // budget this is the full allowance, which is what makes an untouched
        // thread byte-identical to the pre-RFC prompt.
        let paper_chars = chars_for_tokens(assembly.remaining());
        let bundle = build_context(
            request.paper.title,
            request.paper.authors,
            request.paper.venue,
            request.paper.year,
            request.paper.source_text,
            paper_chars,
            request.ephemeral.selection.as_deref(),
        );

        let system_prompt = match assembly.passages_block(&request.ephemeral.paper_id) {
            // No citable context: today's prompt exactly, down to the byte.
            None => bundle.system_prompt,
            Some(passages) => format!("{}\n\n{passages}", bundle.system_prompt),
        };

        // Rectangles last, and only for what actually made it into the prompt.
        // Resolving them for dropped passages would be work nobody can click.
        let mut citations = assembly.citations;
        for citation in &mut citations {
            let Some(chunk_id) = &citation.chunk_id else {
                continue;
            };
            let rects = self.store.chunk_rects(chunk_id)?;
            citation.rects_json =
                serde_json::to_string(&rects).map_err(|error| error.to_string())?;
        }

        Ok(AssembledContext {
            system_prompt,
            entries,
            summary: ChatContextSummary {
                citations,
                paper_title: request.paper.title.to_string(),
                included_chars: bundle.summary.included_chars,
                truncated: bundle.summary.truncated,
                context_items: assembly.included,
                dropped_items: assembly.dropped,
                unresolved_items: unresolved,
                compacted: watermark.is_some(),
            },
        })
    }

    /// Replace the thread's chunk items and pre-watermark turns with a summary.
    ///
    /// The only method that calls a model. Non-destructive to `chat_entries`:
    /// it writes a watermark, and `get_context` replays only what came after.
    /// The thread view still shows every entry.
    ///
    /// `summarize` receives the text to compress and returns the summary, so
    /// the model call stays in `chat/service.rs` where the provider lives.
    pub async fn compact_context<F, Fut>(
        &self,
        thread_id: &str,
        entries: &[ChatEntry],
        summarize: F,
    ) -> Result<ContextItem, String>
    where
        F: FnOnce(String) -> Fut,
        Fut: std::future::Future<Output = Result<String, String>>,
    {
        let items = self.store.context_items(thread_id)?;
        // Every current item, chunks *and* earlier summaries. The new summary
        // is built from `material`, which already contains the old one, so
        // keeping it would emit the same content twice and charge for it twice
        // — and a second compaction would make three.
        //
        // Captured before the model call: anything added while it runs survives.
        let superseded: Vec<String> = items.iter().map(|item| item.id.clone()).collect();

        let mut material = String::new();
        for item in &items {
            match self.resolve(item)? {
                Resolved::Summary(body) => material.push_str(&body),
                Resolved::Chunks(chunks) => material.push_str(&join_chunk_text(&chunks)),
                Resolved::Unresolved => continue,
            }
            material.push_str("\n\n");
        }
        for entry in entries {
            material.push_str(&entry.body);
            material.push_str("\n\n");
        }
        if material.trim().is_empty() {
            return Err("Nothing to compact yet.".to_string());
        }

        // A failed summarization writes nothing — the thread is untouched, the
        // same invariant `ask_at_anchor_streamed` already holds.
        let body = summarize(material).await?;
        let body = body.trim().to_string();
        if body.is_empty() {
            return Err("The summary came back empty; context is unchanged.".to_string());
        }

        let token_estimate = estimate_tokens(&body) as i32;
        self.store.compact_context_items(
            thread_id,
            &ContextItemDraft {
                kind: CONTEXT_KIND_SUMMARY.to_string(),
                chunk_id: None,
                paper_id: None,
                source_start: None,
                source_end: None,
                body: Some(body),
                covers_through_entry_id: entries.last().map(|entry| entry.id.clone()),
                token_estimate,
            },
            &superseded,
        )
    }

    /// Tokens available to context items, from the configured character cap.
    fn budget_tokens(&self) -> usize {
        self.max_context_chars / CHARS_PER_TOKEN
    }

    /// Resolve one item to its text, trying the fast path before the durable one.
    fn resolve(&self, item: &ContextItem) -> Result<Resolved, String> {
        if item.kind == CONTEXT_KIND_SUMMARY {
            return Ok(Resolved::Summary(item.body.clone().unwrap_or_default()));
        }

        if let Some(chunk_id) = &item.chunk_id {
            let found = self.store.chunks_by_ids(std::slice::from_ref(chunk_id))?;
            if !found.is_empty() {
                return Ok(Resolved::Chunks(found));
            }
        }

        // The chunk id died in a rechunk. The character range into the
        // extraction still names the same passage.
        let (Some(paper_id), Some(start), Some(end)) =
            (&item.paper_id, item.source_start, item.source_end)
        else {
            return Ok(Resolved::Unresolved);
        };
        let chunks = self.store.chunks_overlapping(paper_id, start, end)?;
        if chunks.is_empty() {
            return Ok(Resolved::Unresolved);
        }
        Ok(Resolved::Chunks(chunks))
    }
}

enum Resolved {
    Summary(String),
    Chunks(Vec<DocumentChunk>),
    Unresolved,
}

/// Accumulates the passages block under a token budget.
///
/// Handles are assigned in *emission* order, so `[C1]` is the first passage the
/// model reads. Selection happens newest-first, which is why
/// `restore_emission_order` exists: the two orders are genuinely different.
struct Assembly {
    budget: usize,
    spent: usize,
    passages: Vec<Passage>,
    citations: Vec<ContextCitation>,
    included: usize,
    dropped: usize,
}

/// Emission tiers. Separate from the within-tier index so ordering never has
/// to do arithmetic near an integer boundary.
const SUMMARY_TIER: i8 = 0;
const PERSISTENT_TIER: i8 = 1;
const RETRIEVED_TIER: i8 = 2;

struct Passage {
    /// Sort key: `(tier, index)`. Summaries lead, then persistent items in
    /// `position` order, then this turn's retrieval.
    order: (i8, i64),
    item_id: String,
    paper_id: String,
    page_start: i32,
    heading_path: Option<String>,
    text: String,
    /// The chunk to draw rectangles from. `None` when re-resolution produced
    /// several chunks — the citation then points at the first one.
    chunk_id: Option<String>,
}

impl Assembly {
    fn new(budget: usize) -> Self {
        Self {
            budget,
            spent: 0,
            passages: Vec::new(),
            citations: Vec::new(),
            included: 0,
            dropped: 0,
        }
    }

    fn remaining(&self) -> usize {
        self.budget.saturating_sub(self.spent)
    }

    /// A summary is charged but never citable — it has no place in the PDF.
    fn push_summary(&mut self, body: String) {
        let cost = estimate_tokens(&body);
        if cost > self.remaining() {
            self.dropped += 1;
            return;
        }
        self.spent += cost;
        self.included += 1;
        self.passages.push(Passage {
            order: (SUMMARY_TIER, 0),
            item_id: String::new(),
            paper_id: String::new(),
            page_start: 0,
            heading_path: Some("Summary of earlier context".to_string()),
            text: body,
            chunk_id: None,
        });
    }

    fn push_item(&mut self, item: &ContextItem, chunks: &[DocumentChunk]) {
        let Some(first) = chunks.first() else {
            return;
        };
        let text = join_chunk_text(chunks);
        let cost = estimate_tokens(&text);
        if cost > self.remaining() {
            self.dropped += 1;
            return;
        }
        self.spent += cost;
        self.included += 1;
        self.passages.push(Passage {
            order: (PERSISTENT_TIER, item.position as i64),
            item_id: item.id.clone(),
            paper_id: first.paper_id.clone(),
            page_start: first.page_start,
            heading_path: first.heading_path.clone(),
            text,
            chunk_id: Some(first.id.clone()),
        });
    }

    /// This turn's retrieval. `item_id` is empty: there is no persistent row to
    /// delete, which is exactly what makes it ephemeral.
    fn push_retrieved(&mut self, chunk: &DocumentChunk) {
        let cost = estimate_tokens(&chunk.text);
        if cost > self.remaining() {
            self.dropped += 1;
            return;
        }
        self.spent += cost;
        self.included += 1;
        self.passages.push(Passage {
            // A separate tier after every persistent item, in retrieval rank
            // order. Arithmetic near i64::MAX would overflow once six passages
            // had been pushed — a panic in debug, and a negative sort key that
            // puts retrieval *first* in release.
            order: (RETRIEVED_TIER, self.passages.len() as i64),
            item_id: String::new(),
            paper_id: chunk.paper_id.clone(),
            page_start: chunk.page_start,
            heading_path: chunk.heading_path.clone(),
            text: chunk.text.clone(),
            chunk_id: Some(chunk.id.clone()),
        });
    }

    fn restore_emission_order(&mut self) {
        self.passages.sort_by_key(|passage| passage.order);
    }

    /// Render the passages, assigning `[C1]`… and building the citation map.
    ///
    /// `None` when there is nothing citable, which is the case the
    /// byte-identical-prompt guarantee depends on.
    fn passages_block(&mut self, current_paper_id: &str) -> Option<String> {
        if self.passages.is_empty() {
            return None;
        }

        let mut block = String::from(
            "Context passages. Cite the ones you use by their marker, like [C1].\n\
             Do not invent markers.\n",
        );
        for (index, passage) in self.passages.iter().enumerate() {
            let handle = format!("C{}", index + 1);
            // Pages are 0-based in storage and 1-based to a reader.
            let mut location = match &passage.heading_path {
                Some(heading) => format!("p{} · {heading}", passage.page_start + 1),
                None => format!("p{}", passage.page_start + 1),
            };
            // Context can span papers. An unlabelled passage from elsewhere
            // would read as part of the paper under discussion.
            if !passage.paper_id.is_empty() && passage.paper_id != current_paper_id {
                location.push_str(&format!(" · from {}", passage.paper_id));
            }
            block.push_str(&format!("\n[{handle}] ({location})\n{}\n", passage.text));

            if !passage.paper_id.is_empty() {
                self.citations.push(ContextCitation {
                    handle,
                    item_id: passage.item_id.clone(),
                    paper_id: passage.paper_id.clone(),
                    page_start: passage.page_start,
                    heading_path: passage.heading_path.clone(),
                    chunk_id: passage.chunk_id.clone(),
                    rects_json: "[]".to_string(),
                });
            }
        }
        Some(block)
    }
}

/// Entries after the compaction watermark. Everything before it is represented
/// by the summary — but the rows themselves are untouched, so the thread view
/// still shows the whole conversation.
fn entries_after(entries: &[ChatEntry], watermark: Option<&str>) -> Vec<ChatEntry> {
    let Some(watermark) = watermark else {
        return entries.to_vec();
    };
    match entries.iter().position(|entry| entry.id == watermark) {
        Some(index) => entries[index + 1..].to_vec(),
        // The watermark entry was deleted. Replaying everything is the safe
        // reading: the summary is redundant, not wrong.
        None => entries.to_vec(),
    }
}

fn join_chunk_text(chunks: &[DocumentChunk]) -> String {
    chunks
        .iter()
        .map(|chunk| chunk.text.as_str())
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// Characters a token budget buys, for the row-5 paper-text fallback.
fn chars_for_tokens(tokens: usize) -> usize {
    tokens.saturating_mul(CHARS_PER_TOKEN)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::chat::ENTRY_ANSWER;
    use crate::domain::library::PaperDraft;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static SEQUENCE: AtomicUsize = AtomicUsize::new(0);

    struct Fixture {
        manager: ContextManager,
        store: LibraryStore,
        dir: std::path::PathBuf,
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    fn fixture(max_context_chars: usize) -> Fixture {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "i0i-context-test-{}-{nanos}-{sequence}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let store = LibraryStore::for_test(dir.join("library.sqlite"));
        store.init().expect("schema");
        let search = Arc::new(SearchService::new(store.clone(), None));
        Fixture {
            manager: ContextManager::new(store.clone(), search, max_context_chars),
            store,
            dir,
        }
    }

    /// A paper with one extraction and `blocks` worth of text, plus a thread.
    fn seeded(fixture: &Fixture, blocks: &[&str]) -> (String, Vec<DocumentChunk>) {
        let paper_id = "vaswani2017";
        let draft = PaperDraft {
            id: paper_id.to_string(),
            title: "Attention Is All You Need".to_string(),
            authors: vec!["A. Vaswani".to_string()],
            venue: "NeurIPS".to_string(),
            year: 2017,
            citations: 0,
            tags: Vec::new(),
            status: "unread".to_string(),
            abstract_text: None,
            sources: vec![crate::domain::library::PaperSourceDraft {
                source_kind: "pdf".to_string(),
                source_url: "https://example.test/context.pdf".to_string(),
                landing_url: None,
            }],
        };
        fixture
            .store
            .add_paper_to_vaults(&draft, &["attention".to_string()])
            .expect("paper");
        let source = fixture
            .store
            .get_document_sources(paper_id)
            .expect("sources")
            .remove(0);
        fixture
            .store
            .set_document_source_cached(&source.id, "/tmp/context.pdf")
            .expect("cached");
        let extraction = fixture
            .store
            .start_document_extraction(
                &source.id,
                "pdfium_basic",
                "0.2.0",
                &format!("pdfium_basic:{}", source.id),
                false,
            )
            .expect("extraction");

        let mut offset = 0_i64;
        let rows: Vec<crate::domain::library::DocumentBlock> = blocks
            .iter()
            .enumerate()
            .map(|(index, text)| {
                if index > 0 {
                    offset += 2;
                }
                let start = offset;
                offset += text.chars().count() as i64;
                crate::domain::library::DocumentBlock {
                    id: format!("{}:block:0:{index}", extraction.id),
                    paper_id: paper_id.to_string(),
                    source_id: source.id.clone(),
                    extraction_id: extraction.id.clone(),
                    page_index: 0,
                    block_index: index as i32,
                    reading_order: index as i32,
                    kind: "paragraph".to_string(),
                    text: Some((*text).to_string()),
                    asset_id: None,
                    source_start: Some(start),
                    source_end: Some(offset),
                    bbox_json: None,
                }
            })
            .collect();
        let page = crate::domain::library::DocumentPage {
            id: format!("{}:page:0", extraction.id),
            paper_id: paper_id.to_string(),
            source_id: source.id.clone(),
            extraction_id: extraction.id.clone(),
            page_index: 0,
            width: 612.0,
            height: 792.0,
        };
        fixture
            .store
            .finish_document_extraction(&extraction.id, &[page], &rows, &[])
            .expect("finish");

        let chunks = fixture
            .store
            .chunks_for_extraction(&extraction.id)
            .expect("chunks");
        (paper_id.to_string(), chunks)
    }

    fn thread_for(fixture: &Fixture, paper_id: &str) -> String {
        fixture
            .store
            .add_note_at_anchor(
                "paper",
                paper_id,
                &crate::domain::chat::ThreadAnchor::Document,
                "n",
            )
            .expect("thread")
            .thread
            .id
    }

    fn facts<'a>(source_text: &'a str, authors: &'a [String]) -> PaperFacts<'a> {
        PaperFacts {
            title: "Attention Is All You Need",
            authors,
            venue: "NeurIPS",
            year: 2017,
            source_text,
        }
    }

    #[test]
    fn a_thread_with_no_context_gets_todays_prompt_byte_for_byte() {
        let fixture = fixture(32_000);
        let authors = vec!["A. Vaswani".to_string()];
        let source_text = "The body of the paper.";
        let ephemeral = EphemeralContext {
            paper_id: "vaswani2017".to_string(),
            selection: Some("scaled dot-product".to_string()),
        };

        let assembled = fixture
            .manager
            .get_context(ContextRequest {
                thread_id: None,
                paper: facts(source_text, &authors),
                entries: &[],
                ephemeral: &ephemeral,
                retrieved: &[],
            })
            .expect("assembles");

        // This is the regression that matters: every thread that exists today
        // has no context items, and none of them may change.
        let today = build_context(
            "Attention Is All You Need",
            &authors,
            "NeurIPS",
            2017,
            source_text,
            32_000,
            Some("scaled dot-product"),
        );
        assert_eq!(assembled.system_prompt, today.system_prompt);
        assert!(assembled.summary.citations.is_empty());
        assert_eq!(assembled.summary.context_items, 0);
        assert!(!assembled.summary.compacted);
    }

    #[test]
    fn added_chunks_appear_as_numbered_citable_passages() {
        let fixture = fixture(32_000);
        // Two blocks long enough not to merge into one chunk, so emission
        // order is actually exercised.
        let (paper_id, chunks) = seeded(
            &fixture,
            &[&"First passage. ".repeat(200), &"Second passage. ".repeat(200)],
        );
        assert!(chunks.len() >= 2, "the fixture must produce several chunks");
        let thread_id = thread_for(&fixture, &paper_id);
        for chunk in &chunks {
            fixture
                .manager
                .add_context(&thread_id, &chunk.id)
                .expect("adds");
        }

        let authors = vec!["A. Vaswani".to_string()];
        let assembled = fixture
            .manager
            .get_context(ContextRequest {
                thread_id: Some(&thread_id),
                paper: facts("body", &authors),
                entries: &[],
                ephemeral: &EphemeralContext {
                    paper_id: paper_id.clone(),
                    ..Default::default()
                },
                retrieved: &[],
            })
            .expect("assembles");

        assert!(assembled.system_prompt.contains("[C1]"));
        assert_eq!(assembled.summary.citations.len(), chunks.len());
        assert_eq!(assembled.summary.citations[0].handle, "C1");
        assert_eq!(assembled.summary.citations[0].paper_id, paper_id);
        assert_eq!(assembled.summary.context_items, assembled.summary.citations.len());
        // Handles are assigned in emission order, so C1 is the first passage
        // the model reads — not the most recently added. Selection under budget
        // pressure runs newest-first; these two orders are different and the
        // prompt must use the reading one.
        for (index, citation) in assembled.summary.citations.iter().enumerate() {
            assert_eq!(citation.handle, format!("C{}", index + 1));
        }
        let first = assembled
            .system_prompt
            .find("First passage")
            .expect("first chunk is in the prompt");
        let second = assembled
            .system_prompt
            .find("Second passage")
            .expect("second chunk is in the prompt");
        assert!(first < second, "passages are emitted in position order");
    }

    #[test]
    fn adding_the_same_chunk_twice_is_one_item() {
        let fixture = fixture(32_000);
        let (paper_id, chunks) = seeded(&fixture, &["Only passage."]);
        let thread_id = thread_for(&fixture, &paper_id);

        let first = fixture
            .manager
            .add_context(&thread_id, &chunks[0].id)
            .expect("adds");
        let second = fixture
            .manager
            .add_context(&thread_id, &chunks[0].id)
            .expect("adds again");

        assert_eq!(first.id, second.id);
        assert_eq!(fixture.store.context_items(&thread_id).unwrap().len(), 1);
    }

    #[test]
    fn items_over_the_budget_are_dropped_and_counted() {
        // 40 chars ≈ 10 tokens: room for nothing but the smallest passage.
        let fixture = fixture(40);
        let (paper_id, chunks) = seeded(
            &fixture,
            &["A passage long enough to exceed a ten token budget on its own."],
        );
        let thread_id = thread_for(&fixture, &paper_id);
        fixture
            .manager
            .add_context(&thread_id, &chunks[0].id)
            .expect("adds");

        let authors = vec!["A. Vaswani".to_string()];
        let assembled = fixture
            .manager
            .get_context(ContextRequest {
                thread_id: Some(&thread_id),
                paper: facts("body", &authors),
                entries: &[],
                ephemeral: &EphemeralContext {
                    paper_id,
                    ..Default::default()
                },
                retrieved: &[],
            })
            .expect("assembles");

        assert_eq!(assembled.summary.dropped_items, 1);
        assert_eq!(assembled.summary.context_items, 0);
        // Dropped, not silently absorbed: no marker the model could cite.
        assert!(!assembled.system_prompt.contains("[C1]"));
    }

    #[test]
    fn an_unresolvable_item_is_reported_not_dropped_in_silence() {
        let fixture = fixture(32_000);
        let (paper_id, chunks) = seeded(&fixture, &["A passage."]);
        let thread_id = thread_for(&fixture, &paper_id);
        fixture
            .manager
            .add_context(&thread_id, &chunks[0].id)
            .expect("adds");

        // Break both paths: the chunk id and the durable anchor.
        let conn = rusqlite::Connection::open(fixture.dir.join("library.sqlite")).expect("conn");
        conn.execute(
            "update chat_context_items set chunk_id = 'gone', paper_id = 'nobody'",
            [],
        )
        .expect("break");
        drop(conn);

        let authors = vec!["A. Vaswani".to_string()];
        let assembled = fixture
            .manager
            .get_context(ContextRequest {
                thread_id: Some(&thread_id),
                paper: facts("body", &authors),
                entries: &[],
                ephemeral: &EphemeralContext {
                    paper_id,
                    ..Default::default()
                },
                retrieved: &[],
            })
            .expect("assembles");

        assert_eq!(assembled.summary.unresolved_items, 1);
        assert_eq!(assembled.summary.context_items, 0);
    }

    #[test]
    fn a_rechunked_paper_still_resolves_through_the_durable_anchor() {
        let fixture = fixture(32_000);
        let (paper_id, chunks) = seeded(&fixture, &["A durable passage worth keeping."]);
        let thread_id = thread_for(&fixture, &paper_id);
        fixture
            .manager
            .add_context(&thread_id, &chunks[0].id)
            .expect("adds");

        let extraction_id = chunks[0].extraction_id.clone();
        fixture
            .store
            .rechunk_extraction(&extraction_id)
            .expect("rechunk");

        let listed = fixture.manager.list_context(&thread_id).expect("lists");
        assert_eq!(listed.len(), 1);
        assert!(
            !listed[0].unresolved,
            "the chunk id is gone but the character range still names the passage"
        );
        assert!(listed[0].text.contains("durable passage"));
    }

    #[test]
    fn entries_before_the_watermark_are_replaced_by_the_summary() {
        let entries = vec![
            entry("e1", "first question"),
            entry("e2", "first answer"),
            entry("e3", "second question"),
        ];

        assert_eq!(entries_after(&entries, None).len(), 3);
        let kept = entries_after(&entries, Some("e2"));
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].id, "e3");
    }

    #[test]
    fn a_deleted_watermark_entry_replays_everything_rather_than_nothing() {
        let entries = vec![entry("e1", "a"), entry("e2", "b")];
        // The safe reading: the summary is redundant, not wrong. Returning an
        // empty replay would silently erase the conversation from the prompt.
        assert_eq!(entries_after(&entries, Some("deleted")).len(), 2);
    }

    fn entry(id: &str, body: &str) -> ChatEntry {
        ChatEntry {
            id: id.to_string(),
            thread_id: "t".to_string(),
            kind: ENTRY_ANSWER.to_string(),
            body: body.to_string(),
            model: None,
            context_summary: None,
            pinned: false,
            created_at: "2026-08-08T00:00:00Z".to_string(),
        }
    }

    #[tokio::test]
    async fn compaction_replaces_chunk_items_and_sets_the_watermark() {
        let fixture = fixture(32_000);
        let (paper_id, chunks) = seeded(&fixture, &["First passage.", "Second passage."]);
        let thread_id = thread_for(&fixture, &paper_id);
        for chunk in &chunks {
            fixture
                .manager
                .add_context(&thread_id, &chunk.id)
                .expect("adds");
        }
        let entries = vec![entry("e1", "what did they find"), entry("e2", "they found x")];

        let item = fixture
            .manager
            .compact_context(&thread_id, &entries, |material| async move {
                assert!(material.contains("First passage"));
                assert!(material.contains("they found x"));
                Ok("They found x, using passages one and two.".to_string())
            })
            .await
            .expect("compacts");

        assert_eq!(item.kind, "summary");
        assert_eq!(item.covers_through_entry_id.as_deref(), Some("e2"));
        let items = fixture.store.context_items(&thread_id).expect("items");
        assert_eq!(items.len(), 1, "chunk items were superseded");
    }

    #[tokio::test]
    async fn compacting_twice_leaves_one_summary_not_two() {
        let fixture = fixture(32_000);
        let (paper_id, chunks) = seeded(&fixture, &["First passage.", "Second passage."]);
        let thread_id = thread_for(&fixture, &paper_id);
        for chunk in &chunks {
            fixture
                .manager
                .add_context(&thread_id, &chunk.id)
                .expect("adds");
        }

        for round in 0..2 {
            fixture
                .manager
                .compact_context(&thread_id, &[entry("e1", "a turn")], move |_| async move {
                    Ok(format!("summary round {round}"))
                })
                .await
                .expect("compacts");
        }

        // The second summary is built from material that already contains the
        // first, so keeping both would emit the same content twice and charge
        // the budget twice — and a third compaction would make three.
        let items = fixture.store.context_items(&thread_id).expect("items");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].body.as_deref(), Some("summary round 1"));
    }

    #[test]
    fn retrieved_passages_are_emitted_after_persistent_ones() {
        let fixture = fixture(32_000);
        let (paper_id, chunks) = seeded(&fixture, &[&"Committed passage. ".repeat(200)]);
        let thread_id = thread_for(&fixture, &paper_id);
        fixture
            .manager
            .add_context(&thread_id, &chunks[0].id)
            .expect("adds");

        // More than the six that used to overflow the old sort key.
        let (_, other) = (0, chunks.clone());
        let retrieved: Vec<DocumentChunk> = other
            .iter()
            .cycle()
            .take(8)
            .enumerate()
            .map(|(index, chunk)| DocumentChunk {
                id: format!("retrieved-{index}"),
                text: format!("Retrieved passage {index}."),
                ..chunk.clone()
            })
            .collect();

        let authors = vec!["A. Vaswani".to_string()];
        let assembled = fixture
            .manager
            .get_context(ContextRequest {
                thread_id: Some(&thread_id),
                paper: facts("body", &authors),
                entries: &[],
                ephemeral: &EphemeralContext {
                    paper_id,
                    ..Default::default()
                },
                retrieved: &retrieved,
            })
            .expect("assembles");

        let committed = assembled
            .system_prompt
            .find("Committed passage")
            .expect("the committed passage is in the prompt");
        let first_retrieved = assembled
            .system_prompt
            .find("Retrieved passage 0")
            .expect("retrieval is in the prompt");
        assert!(committed < first_retrieved);
    }

    #[tokio::test]
    async fn a_failed_summarization_leaves_the_thread_untouched() {
        let fixture = fixture(32_000);
        let (paper_id, chunks) = seeded(&fixture, &["First passage."]);
        let thread_id = thread_for(&fixture, &paper_id);
        fixture
            .manager
            .add_context(&thread_id, &chunks[0].id)
            .expect("adds");

        let error = fixture
            .manager
            .compact_context(&thread_id, &[], |_| async {
                Err("provider is down".to_string())
            })
            .await
            .expect_err("propagates");
        assert!(error.contains("provider is down"));

        let items = fixture.store.context_items(&thread_id).expect("items");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].kind, "chunk", "nothing was written or removed");
    }

    #[tokio::test]
    async fn an_empty_summary_is_rejected_rather_than_stored() {
        let fixture = fixture(32_000);
        let (paper_id, chunks) = seeded(&fixture, &["First passage."]);
        let thread_id = thread_for(&fixture, &paper_id);
        fixture
            .manager
            .add_context(&thread_id, &chunks[0].id)
            .expect("adds");

        let error = fixture
            .manager
            .compact_context(&thread_id, &[], |_| async { Ok("   ".to_string()) })
            .await
            .expect_err("rejects");
        assert!(error.contains("empty"));
        assert_eq!(fixture.store.context_items(&thread_id).unwrap().len(), 1);
    }
}
