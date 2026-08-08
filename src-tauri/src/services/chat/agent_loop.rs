//! Phase 1 of a turn: the agent decides what evidence it needs (RFC 0078).
//!
//! RFC 0077 searched on *every* ask whether the question needed it or not. Here
//! the model gets tools and decides for itself — search, look, search again,
//! keep the good one — and only then does the answer stream.
//!
//! The split matters. RFC 0077 R1 kept the answer path free of tool-call
//! generation so replies stream clean; that property survives because every
//! tool call happens **before** phase 2 starts. Phase 2 sends `tools: None`.
//!
//! Cost is why the loop runs on a *reduced* prompt — metadata and a passage
//! list, no paper body. Full text is paid for once, in the final assembly.

use reqwest::Client;
use serde::Deserialize;

use crate::domain::chat::{ChatEntry, ENTRY_ANSWER};
use crate::domain::context::ORIGIN_AGENT;
use crate::domain::library::DocumentChunk;
use crate::services::llm::{
    self as openrouter, CompletionRequest, Tool, ToolFunction, WireMessage, WireToolCall,
    WireToolCallFunction,
};
use crate::services::search::{SearchMode, SearchRequest};

use super::context_manager::ContextManager;

/// Round trips the model may spend deciding. Three is enough to search, read
/// the previews, and refine once. Beyond that the turn stops feeling like an
/// answer and starts feeling like a hang.
const MAX_ITERATIONS: usize = 3;

/// Tool calls honoured per iteration. A model that asks for twelve searches at
/// once is not refining, it is flailing.
const MAX_CALLS_PER_ITERATION: usize = 4;

/// Chunks the loop may pull into one turn, across every search.
///
/// Deliberately tight. Every passage competes with the paper text for the same
/// assembly budget, and a long reference list is worse than a short one: it
/// spreads the reader's attention over material the answer did not need.
const MAX_CHUNKS_PER_TURN: usize = 6;

/// Hits one `search_context` call returns. Three good ones beat five, and the
/// agent can always search again with better wording.
const SEARCH_LIMIT: usize = 3;

/// Preview length per hit. Enough to judge relevance, far short of the full
/// text — which arrives once, in the final assembly, if the passage is used.
const PREVIEW_CHARS: usize = 300;

/// Turns of history the deciding model sees. Enough to resolve "why?" against
/// what was just said; short enough that resending it three times is cheap.
pub const RECENT_TURNS: usize = 4;

/// What phase 1 produced.
#[derive(Debug, Default)]
pub struct RetrievalOutcome {
    /// Passages found this turn. Ephemeral: they go into the assembly and are
    /// not persisted unless something called `add_context`.
    pub chunks: Vec<DocumentChunk>,
    /// A bound was reached. Reported, never silent — a loop that quietly
    /// stopped short reads as an agent that decided it had enough.
    pub capped: bool,
    /// Queries the agent ran, oldest first. Drives the progress line, and makes
    /// over-searching visible.
    pub queries: Vec<String>,
}

/// Progress emitted while the loop runs, so the panel is not dead through up to
/// three round trips.
pub enum LoopEvent {
    /// Phase 1 has started. Emitted before the first round trip so the panel
    /// says something immediately — deciding *not* to search still takes a
    /// round trip, and silence through it reads as a hang.
    Deciding,
    Searching { query: String },
    Retrieved { count: usize },
}

pub struct LoopRequest<'a> {
    pub paper_id: &'a str,
    /// `None` on the anchor path, where the thread does not exist yet — the
    /// write tools are withheld rather than offered and then failed.
    pub thread_id: Option<&'a str>,
    pub paper_title: &'a str,
    pub question: &'a str,
    pub selection: Option<&'a str>,
    /// The last few turns, oldest first. Without them "why?" reaches the
    /// deciding model with no antecedent, and the prompt's claim that
    /// follow-ups need no lookup becomes something it cannot act on.
    ///
    /// A tail, not the whole thread: these messages are resent on every
    /// iteration, so history is the part that multiplies.
    pub recent: &'a [ChatEntry],
    pub model: &'a str,
    pub url: &'a str,
    pub api_key: &'a str,
}

/// Run phase 1. Never fails the turn: a transport error, a malformed tool call,
/// or a refused write all degrade to "answer with what we have".
pub async fn run<F>(
    client: &Client,
    context: &ContextManager,
    request: LoopRequest<'_>,
    mut on_event: F,
) -> RetrievalOutcome
where
    F: FnMut(LoopEvent),
{
    on_event(LoopEvent::Deciding);

    let mut outcome = RetrievalOutcome::default();
    let mut messages = vec![WireMessage::text("system", system_prompt(&request, context))];
    for entry in request.recent {
        let role = if entry.kind == ENTRY_ANSWER {
            "assistant"
        } else {
            "user"
        };
        messages.push(WireMessage::text(role, entry.body.clone()));
    }
    messages.push(WireMessage::text("user", request.question.to_string()));

    for iteration in 0..MAX_ITERATIONS {
        let completion = openrouter::complete_streamed(
            client,
            request.url,
            request.api_key,
            &CompletionRequest {
                model: request.model.to_string(),
                messages: messages.clone(),
                stream: true,
                max_tokens: None,
                response_format: None,
                tools: Some(context_tools(request.thread_id.is_some())),
                tool_choice: None,
            },
            |_| {},
        )
        .await;

        let completion = match completion {
            Ok(completion) => completion,
            Err(error) => {
                // Retrieval failing is not the ask failing (RFC 0077).
                eprintln!("[chat] retrieval loop iteration {iteration} failed: {error}");
                break;
            }
        };

        if completion.tool_calls.is_empty() {
            break;
        }

        let honoured = completion.tool_calls.len().min(MAX_CALLS_PER_ITERATION);
        if completion.tool_calls.len() > honoured {
            outcome.capped = true;
        }
        let calls = &completion.tool_calls[..honoured];

        messages.push(WireMessage::tool_request(
            calls
                .iter()
                .map(|call| WireToolCall {
                    id: call.id.clone(),
                    kind: "function".to_string(),
                    function: WireToolCallFunction {
                        name: call.name.clone(),
                        arguments: call.arguments.clone(),
                    },
                })
                .collect(),
        ));

        for call in calls {
            let result = run_tool(context, &request, call, &mut outcome, &mut on_event).await;
            messages.push(WireMessage::tool_result(call.id.clone(), result));
        }

        // Only a cap if the agent was still *gathering* when the loop ended. A
        // final iteration that only wrote context was not cut off mid-search,
        // and a notice on that turn would teach you to distrust the notice.
        let still_searching = calls.iter().any(|call| call.name == "search_context");
        if iteration + 1 == MAX_ITERATIONS && still_searching {
            outcome.capped = true;
        }
    }

    outcome
}

/// Execute one call. Every failure comes back as a tool *result*, not an error:
/// a turn that dies because the model mistyped an id is worse than one that
/// answers slightly less well.
async fn run_tool<F>(
    context: &ContextManager,
    request: &LoopRequest<'_>,
    call: &openrouter::AssembledToolCall,
    outcome: &mut RetrievalOutcome,
    on_event: &mut F,
) -> String
where
    F: FnMut(LoopEvent),
{
    match call.name.as_str() {
        "search_context" => {
            let Ok(args) = serde_json::from_str::<SearchArgs>(&call.arguments) else {
                return "Could not parse arguments. Expected {\"query\": \"...\"}.".to_string();
            };
            search(context, request, &args.query, outcome, on_event).await
        }
        "add_context" => {
            let Ok(args) = serde_json::from_str::<ChunkArgs>(&call.arguments) else {
                return "Could not parse arguments. Expected {\"chunk_id\": \"...\"}.".to_string();
            };
            let Some(thread_id) = request.thread_id else {
                return "This conversation has no history yet, so nothing can be kept."
                    .to_string();
            };
            match context.add_context(thread_id, &args.chunk_id, ORIGIN_AGENT) {
                Ok(item) => format!("Kept as {}.", item.id),
                Err(error) => format!("Could not keep it: {error}"),
            }
        }
        "drop_context" => {
            let Ok(args) = serde_json::from_str::<ItemArgs>(&call.arguments) else {
                return "Could not parse arguments. Expected {\"item_id\": \"...\"}.".to_string();
            };
            let Some(thread_id) = request.thread_id else {
                return "This conversation has no kept context yet.".to_string();
            };
            context
                .agent_delete_context(thread_id, &args.item_id)
                .unwrap_or_else(|error| format!("Could not drop it: {error}"))
        }
        other => format!("No tool named {other}."),
    }
}

async fn search<F>(
    context: &ContextManager,
    request: &LoopRequest<'_>,
    query: &str,
    outcome: &mut RetrievalOutcome,
    on_event: &mut F,
) -> String
where
    F: FnMut(LoopEvent),
{
    if outcome.chunks.len() >= MAX_CHUNKS_PER_TURN {
        outcome.capped = true;
        return "Retrieval budget for this turn is spent. Answer with what you have.".to_string();
    }

    outcome.queries.push(query.to_string());
    on_event(LoopEvent::Searching {
        query: query.to_string(),
    });

    let response = context
        .search(SearchRequest {
            query: query.to_string(),
            paper_ids: vec![request.paper_id.to_string()],
            vault_ids: Vec::new(),
            mode: SearchMode::Hybrid,
            limit: Some(SEARCH_LIMIT),
        })
        .await;

    let response = match response {
        Ok(response) => response,
        Err(error) => return format!("Search failed: {error}"),
    };
    if response.hits.is_empty() {
        return "No passages matched. Try different wording, or answer without one.".to_string();
    }

    let room = MAX_CHUNKS_PER_TURN - outcome.chunks.len();
    // Counted against what we would actually have stored: surplus hits that are
    // duplicates of passages already held cost nothing and cap nothing.
    let fresh = response
        .hits
        .iter()
        .filter(|hit| !outcome.chunks.iter().any(|held| held.id == hit.chunk.id))
        .count();
    if fresh > room {
        outcome.capped = true;
    }

    let mut lines = Vec::new();
    for hit in response.hits.into_iter().take(room) {
        let chunk = hit.chunk;
        if outcome.chunks.iter().any(|held| held.id == chunk.id) {
            continue;
        }
        lines.push(format!(
            "- id={} p{}{} :: {}",
            chunk.id,
            chunk.page_start + 1,
            chunk
                .heading_path
                .as_deref()
                .map(|heading| format!(" · {heading}"))
                .unwrap_or_default(),
            preview(&chunk.text),
        ));
        outcome.chunks.push(chunk);
    }

    on_event(LoopEvent::Retrieved {
        count: outcome.chunks.len(),
    });

    if lines.is_empty() {
        return "Those passages are already in front of you.".to_string();
    }
    // Deliberately no [C…] handles here. Handles are minted once, at final
    // assembly; showing provisional ones mid-loop would let the model form an
    // intention about a number that later renumbers, and cite the wrong
    // passage. The model sees chunk ids while deciding, markers while writing.
    format!(
        "{} passage(s). Their full text will appear in your context with \
         [C…] markers — cite the marker shown there, not these ids.\n{}",
        lines.len(),
        lines.join("\n")
    )
}

fn preview(text: &str) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    match flat.char_indices().nth(PREVIEW_CHARS) {
        Some((index, _)) => format!("{}…", &flat[..index]),
        None => flat,
    }
}

/// The reduced prompt: enough to decide, not enough to answer from.
///
/// Deliberately excludes the paper body. Including it would multiply the
/// largest part of the prompt by the iteration count for no gain — the model is
/// choosing what to look at here, not writing.
fn system_prompt(request: &LoopRequest<'_>, context: &ContextManager) -> String {
    let mut prompt = format!(
        "You are preparing to answer a question about the paper \"{}\".\n\
         \n\
         Decide whether you need to look anything up. Many questions — what the \
         paper is about, what a term means, follow-ups to what was just said — \
         need no lookup at all. If you do not need one, reply with no tool calls \
         and no text.\n\
         \n\
         If you do, call search_context — and read as little as possible. \
         Prefer one precise search over three broad ones, and stop as soon as \
         you have what the question needs. Every passage you pull competes for \
         room with the paper itself, so a passage you do not end up citing is a \
         passage that cost the answer something.\n\
         \n\
         You may search up to {MAX_ITERATIONS} times. Keep a passage with \
         add_context only if later turns in this conversation will need it.\n",
        request.paper_title,
    );

    if let Some(selection) = request.selection {
        prompt.push_str(&format!(
            "\nThe reader is looking at this passage:\n> {selection}\n"
        ));
    }

    if let Some(thread_id) = request.thread_id {
        if let Ok(items) = context.list_context(thread_id) {
            if !items.is_empty() {
                prompt.push_str("\nAlready in context:\n");
                for item in items {
                    prompt.push_str(&format!(
                        "- {} ({}){}\n",
                        item.id,
                        item.origin,
                        item.page_start
                            .map(|page| format!(" p{}", page + 1))
                            .unwrap_or_default(),
                    ));
                }
            }
        }
    }

    prompt
}

fn context_tools(can_write: bool) -> Vec<Tool> {
    let mut tools = vec![Tool {
        kind: "function".into(),
        function: ToolFunction {
            name: "search_context".into(),
            description: "Find passages in this paper by wording and meaning. \
                 Use it when the question turns on a specific claim, number, \
                 method, or term you have not already been shown. Do not use it \
                 for general questions about what the paper is about."
                .into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "What to look for, in your own words. Be specific — a narrow query returns the passage you need instead of five you do not."
                    }
                },
                "required": ["query"]
            }),
        },
    }];

    if can_write {
        tools.push(Tool {
            kind: "function".into(),
            function: ToolFunction {
                name: "add_context".into(),
                description: "Keep a passage so later turns in this conversation \
                     still have it. Only for passages the conversation will keep \
                     returning to — everything you retrieve is available for this \
                     turn regardless."
                    .into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "chunk_id": { "type": "string" },
                        "why": {
                            "type": "string",
                            "description": "One line on why it is worth keeping."
                        }
                    },
                    "required": ["chunk_id"]
                }),
            },
        });
        tools.push(Tool {
            kind: "function".into(),
            function: ToolFunction {
                name: "drop_context".into(),
                description: "Remove a passage YOU kept earlier. Passages the \
                     reader kept cannot be removed — say why one looks unhelpful \
                     instead."
                    .into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": { "item_id": { "type": "string" } },
                    "required": ["item_id"]
                }),
            },
        });
    }

    tools
}

#[derive(Deserialize)]
struct SearchArgs {
    query: String,
}

#[derive(Deserialize)]
struct ChunkArgs {
    chunk_id: String,
}

#[derive(Deserialize)]
struct ItemArgs {
    item_id: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::context::ORIGIN_USER;
    use crate::services::search::SearchService;
    use crate::storage::library_store::LibraryStore;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
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

    /// The tool branches below never touch the network — `model`, `url`, and
    /// `api_key` are unused on them — so a store is all the fixture needs.
    fn fixture() -> Fixture {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "i0i-agent-loop-test-{}-{nanos}-{sequence}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let store = LibraryStore::for_test(dir.join("library.sqlite"));
        store.init().expect("schema");
        let search = Arc::new(SearchService::new(store.clone(), None));
        Fixture {
            manager: ContextManager::new(store.clone(), search, 32_000),
            store,
            dir,
        }
    }

    fn request<'a>(thread_id: Option<&'a str>) -> LoopRequest<'a> {
        LoopRequest {
            paper_id: "vaswani2017",
            thread_id,
            paper_title: "Attention Is All You Need",
            question: "why scale?",
            selection: None,
            recent: &[],
            model: "unused",
            url: "unused",
            api_key: "unused",
        }
    }

    fn call(name: &str, arguments: &str) -> openrouter::AssembledToolCall {
        openrouter::AssembledToolCall {
            id: "call_1".to_string(),
            name: name.to_string(),
            arguments: arguments.to_string(),
        }
    }

    async fn run_one(
        fixture: &Fixture,
        thread_id: Option<&str>,
        call: &openrouter::AssembledToolCall,
        outcome: &mut RetrievalOutcome,
    ) -> String {
        run_tool(
            &fixture.manager,
            &request(thread_id),
            call,
            outcome,
            &mut |_| {},
        )
        .await
    }

    #[tokio::test]
    async fn malformed_arguments_answer_the_call_instead_of_killing_the_turn() {
        let fixture = fixture();
        let mut outcome = RetrievalOutcome::default();

        for (name, expected) in [
            ("search_context", "query"),
            ("add_context", "chunk_id"),
            ("drop_context", "item_id"),
        ] {
            let result = run_one(
                &fixture,
                Some("thread-1"),
                &call(name, "{ not json"),
                &mut outcome,
            )
            .await;
            assert!(
                result.contains("Could not parse") && result.contains(expected),
                "{name}: {result}"
            );
        }
        assert!(outcome.chunks.is_empty());
    }

    #[tokio::test]
    async fn an_unknown_tool_name_answers_rather_than_failing() {
        let fixture = fixture();
        let mut outcome = RetrievalOutcome::default();
        let result = run_one(&fixture, Some("t"), &call("teleport", "{}"), &mut outcome).await;
        assert!(result.contains("No tool named teleport"));
    }

    #[tokio::test]
    async fn the_write_tools_decline_politely_when_there_is_no_thread() {
        let fixture = fixture();
        let mut outcome = RetrievalOutcome::default();

        let result = run_one(
            &fixture,
            None,
            &call("add_context", r#"{"chunk_id":"c1"}"#),
            &mut outcome,
        )
        .await;
        assert!(result.contains("no history yet"));

        let result = run_one(
            &fixture,
            None,
            &call("drop_context", r#"{"item_id":"i1"}"#),
            &mut outcome,
        )
        .await;
        assert!(result.contains("no kept context"));
    }

    #[tokio::test]
    async fn dropping_a_passage_the_reader_kept_is_refused() {
        let fixture = fixture();
        let (thread_id, item_id) = seeded_user_item(&fixture);
        let mut outcome = RetrievalOutcome::default();

        let result = run_one(
            &fixture,
            Some(&thread_id),
            &call("drop_context", &format!(r#"{{"item_id":"{item_id}"}}"#)),
            &mut outcome,
        )
        .await;

        assert!(result.contains("Refused"), "{result}");
        assert_eq!(fixture.store.context_items(&thread_id).unwrap().len(), 1);
    }

    #[tokio::test]
    async fn a_spent_chunk_budget_declines_further_searches_and_reports_it() {
        let fixture = fixture();
        let mut outcome = RetrievalOutcome::default();
        // Pretend the turn already pulled its allowance.
        outcome.chunks = (0..MAX_CHUNKS_PER_TURN)
            .map(|index| stub_chunk(&format!("chunk-{index}")))
            .collect();

        let result = run_one(
            &fixture,
            Some("t"),
            &call("search_context", r#"{"query":"scaling"}"#),
            &mut outcome,
        )
        .await;

        assert!(result.contains("budget for this turn is spent"), "{result}");
        assert!(outcome.capped, "a refused search must be reported");
        assert_eq!(outcome.chunks.len(), MAX_CHUNKS_PER_TURN);
        // A declined search is not a search that ran.
        assert!(outcome.queries.is_empty());
    }

    fn stub_chunk(id: &str) -> DocumentChunk {
        DocumentChunk {
            id: id.to_string(),
            paper_id: "vaswani2017".to_string(),
            source_id: "s".to_string(),
            extraction_id: "e".to_string(),
            chunk_index: 0,
            chunker: "structural".to_string(),
            chunk_version: 2,
            page_start: 0,
            page_end: 0,
            heading_path: None,
            text: "body".to_string(),
            token_estimate: 1,
            source_start: 0,
            source_end: 4,
            block_ids: Vec::new(),
        }
    }

    /// A thread holding one passage the *user* kept.
    fn seeded_user_item(fixture: &Fixture) -> (String, String) {
        let draft = crate::domain::library::PaperDraft {
            id: "vaswani2017".to_string(),
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
                source_url: "https://example.test/a.pdf".to_string(),
                landing_url: None,
            }],
        };
        fixture
            .store
            .add_paper_to_vaults(&draft, &["attention".to_string()])
            .expect("paper");
        let source = fixture
            .store
            .get_document_sources("vaswani2017")
            .expect("sources")
            .remove(0);
        fixture
            .store
            .set_document_source_cached(&source.id, "/tmp/a.pdf")
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
        let block = crate::domain::library::DocumentBlock {
            id: format!("{}:block:0:0", extraction.id),
            paper_id: "vaswani2017".to_string(),
            source_id: source.id.clone(),
            extraction_id: extraction.id.clone(),
            page_index: 0,
            block_index: 0,
            reading_order: 0,
            kind: "paragraph".to_string(),
            text: Some("A kept passage.".to_string()),
            asset_id: None,
            source_start: Some(0),
            source_end: Some(15),
            bbox_json: None,
        };
        let page = crate::domain::library::DocumentPage {
            id: format!("{}:page:0", extraction.id),
            paper_id: "vaswani2017".to_string(),
            source_id: source.id.clone(),
            extraction_id: extraction.id.clone(),
            page_index: 0,
            width: 612.0,
            height: 792.0,
        };
        fixture
            .store
            .finish_document_extraction(&extraction.id, &[page], &[block], &[])
            .expect("finish");

        let chunk = fixture
            .store
            .chunks_for_extraction(&extraction.id)
            .expect("chunks")
            .remove(0);
        let thread_id = fixture
            .store
            .add_note_at_anchor(
                "paper",
                "vaswani2017",
                &crate::domain::chat::ThreadAnchor::Document,
                "n",
            )
            .expect("thread")
            .thread
            .id;
        let item = fixture
            .manager
            .add_context(&thread_id, &chunk.id, ORIGIN_USER)
            .expect("adds");
        (thread_id, item.id)
    }

    #[test]
    fn a_preview_is_flattened_and_bounded() {
        let text = format!("word {}", "x".repeat(1_000));
        let preview = preview(&text);
        assert!(preview.chars().count() <= PREVIEW_CHARS + 1);
        assert!(preview.ends_with('…'));
    }

    #[test]
    fn a_short_preview_is_not_truncated_or_marked() {
        assert_eq!(preview("  a short\n  passage  "), "a short passage");
    }

    #[test]
    fn a_multibyte_preview_does_not_split_a_codepoint() {
        // Slicing on a byte index inside a codepoint would panic.
        let text = "é".repeat(PREVIEW_CHARS + 50);
        let preview = preview(&text);
        assert!(preview.ends_with('…'));
    }

    #[test]
    fn the_write_tools_are_withheld_when_there_is_no_thread() {
        let names: Vec<String> = context_tools(false)
            .into_iter()
            .map(|tool| tool.function.name)
            .collect();
        // Offering a tool that must then fail teaches the model nothing useful.
        assert_eq!(names, vec!["search_context"]);

        let names: Vec<String> = context_tools(true)
            .into_iter()
            .map(|tool| tool.function.name)
            .collect();
        assert_eq!(names, vec!["search_context", "add_context", "drop_context"]);
    }
}
