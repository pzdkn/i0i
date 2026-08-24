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

use crate::domain::chat::{ChatEntry, ResearchActivity, WebLookupOutcome, ENTRY_ANSWER};
use crate::domain::context::ExternalCitation;
use crate::domain::context::ORIGIN_AGENT;
use crate::domain::library::DocumentChunk;
use crate::services::llm::{
    self as openrouter, CompletionRequest, Tool, ToolFunction, WireMessage, WireToolCall,
    WireToolCallFunction,
};
use crate::services::search::{SearchMode, SearchRequest};

use super::context_manager::ContextManager;
use super::research_tools::ResearchToolbox;

/// Round trips the model may spend deciding.
///
/// One. Every iteration is a full round trip the reader waits through before
/// the first word of prose, and refining a query a second time is worth far
/// less than answering sooner. The agent still *decides* — it just gets one
/// look, and can issue more than one query in it.
const MAX_ITERATIONS: usize = 1;

/// Tool calls honoured in that one round. Two queries cover a question with two
/// parts; more is flailing, and each one is a sequential search.
const MAX_CALLS_PER_ITERATION: usize = 2;

/// Ceiling on the deciding turn's output. It should emit tool calls and nothing
/// else, so a low cap trims generation time off the critical path.
const DECIDE_MAX_TOKENS: u32 = 512;

/// Chunks the loop may pull into one turn, across every search.
///
/// Deliberately tight. Every passage competes with the paper text for the same
/// assembly budget, and a long reference list is worse than a short one: it
/// spreads the reader's attention over material the answer did not need.
///
/// RFC 0079 R5.1a: this is now the budget for the *whole* turn — the baseline
/// search and the agent's refinements draw from it in that order — rather than
/// a bound the loop alone enforced.
const MAX_CHUNKS_PER_TURN: usize = 6;

/// Chunks the unconditional baseline search may claim (RFC 0079 R5.1).
///
/// Less than the whole budget on purpose: the agent must be left room to
/// refine. Four good passages from the reader's own words, two more if the
/// agent finds better wording.
const BASELINE_CHUNKS: usize = 4;

/// Hits one `search_context` call returns. Three good ones beat five, and the
/// agent can always search again with better wording.
const SEARCH_LIMIT: usize = 3;

/// External pages one chat turn may read. Web lookup stays a small evidence
/// operation; broader work belongs to asynchronous Deep Research.
const WEB_SOURCE_LIMIT: usize = 3;

/// Preview length per hit. Enough to judge relevance, far short of the full
/// text — which arrives once, in the final assembly, if the passage is used.
const PREVIEW_CHARS: usize = 300;

/// Turns of history the deciding model sees. Enough to resolve "why?" against
/// what was just said; short enough that resending it three times is cheap.
pub const RECENT_TURNS: usize = 4;

/// What phase 1 produced.
#[derive(Debug)]
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
    /// Whether the paper has any chunks to retrieve at all (RFC 0079 R5.4).
    ///
    /// Zero passages means two very different things — "nothing matched" and
    /// "this paper was never indexed" — and only one of them is the reader's
    /// to fix. Defaults to `true` so a failure to check never accuses the
    /// library of being unindexed.
    pub paper_indexed: bool,
    /// External evidence read during the routing phase, numbered for assembly.
    pub external_citations: Vec<ExternalCitation>,
    /// Typed result propagated to the final answer phase and persisted audit.
    pub web_lookup: WebLookupOutcome,
    /// Background research launched by this turn. At most one is accepted.
    pub research_activities: Vec<ResearchActivity>,
}

impl Default for RetrievalOutcome {
    fn default() -> Self {
        Self {
            chunks: Vec::new(),
            capped: false,
            queries: Vec::new(),
            paper_indexed: true,
            external_citations: Vec::new(),
            web_lookup: WebLookupOutcome::NotRequested,
            research_activities: Vec::new(),
        }
    }
}

/// Progress emitted while the loop runs, so the panel is not dead through up to
/// three round trips.
pub enum LoopEvent {
    /// Phase 1 has started. Emitted before the first round trip so the panel
    /// says something immediately — deciding *not* to search still takes a
    /// round trip, and silence through it reads as a hang.
    Deciding,
    SearchingPaper {
        query: String,
    },
    SearchingLibrary {
        query: String,
    },
    SearchingWeb {
        query: String,
    },
    ReadingSource {
        title: String,
    },
    StartingDeepResearch {
        title: String,
    },
    Retrieved {
        count: usize,
    },
}

pub struct LoopRequest<'a> {
    pub paper_id: &'a str,
    /// `None` on the anchor path, where the thread does not exist yet — the
    /// write tools are withheld rather than offered and then failed.
    pub thread_id: Option<&'a str>,
    pub paper_title: &'a str,
    pub question: &'a str,
    pub selection: Option<&'a str>,
    /// Model for the deciding round. The cheap one: choosing a search query is
    /// a far lighter task than writing the answer, and this round sits on the
    /// critical path before the first token of prose.
    pub decide_model: &'a str,
    /// The last few turns, oldest first. Without them "why?" reaches the
    /// deciding model with no antecedent, and the prompt's claim that
    /// follow-ups need no lookup becomes something it cannot act on.
    ///
    /// A tail, not the whole thread: these messages are resent on every
    /// iteration, so history is the part that multiplies.
    pub recent: &'a [ChatEntry],
    pub url: &'a str,
    pub api_key: &'a str,
}

/// Run phase 1. Never fails the turn: a transport error, a malformed tool call,
/// or a refused write all degrade to "answer with what we have".
pub async fn run<F>(
    client: &Client,
    context: &ContextManager,
    research: &dyn ResearchToolbox,
    request: LoopRequest<'_>,
    mut on_event: F,
) -> RetrievalOutcome
where
    F: FnMut(LoopEvent),
{
    let mut outcome = RetrievalOutcome::default();

    // RFC 0079 R5.1: retrieval is the floor, not the agent's call. Search runs
    // before the model gets a vote — it is local (BM25 in SQLite, and a query
    // embedding against an in-process model), so it costs milliseconds, while
    // the round trip that used to decide whether to run it costs hundreds. An
    // ask can no longer reach the answer model with nothing to cite.
    baseline_search(context, &request, &mut outcome, &mut on_event).await;

    // An explicit command is not a classification problem. Honor it before
    // asking the optional planner, which may otherwise decline every tool.
    if explicit_web_search_request(request.question) {
        search_web(research, request.question, &mut outcome, &mut on_event).await;
        on_event(LoopEvent::Retrieved {
            count: outcome.chunks.len() + outcome.external_citations.len(),
        });
        return outcome;
    }

    on_event(LoopEvent::Deciding);

    let mut messages = vec![WireMessage::text(
        "system",
        system_prompt(&request, context, &outcome.chunks),
    )];
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
                model: request.decide_model.to_string(),
                messages: messages.clone(),
                stream: true,
                max_tokens: Some(DECIDE_MAX_TOKENS),
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
            if iteration == 0 {
                // The single most useful line when references do not appear:
                // it separates "the agent chose not to look" from "the loop
                // never ran" from "the model cited a number we did not mint".
                eprintln!("[chat] agent declined to search");
            }
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
            let result = run_tool(
                context,
                research,
                &request,
                call,
                &mut outcome,
                &mut on_event,
            )
            .await;
            messages.push(WireMessage::tool_result(call.id.clone(), result));
        }

        // Deliberately *not* flagged as capped. With a single round, stopping
        // after it is the design rather than a limit hit, and a notice on every
        // searching turn would teach you to ignore the notice. `capped` now
        // means only: we refused work the agent asked for.
    }

    eprintln!(
        "[chat] retrieval done queries={:?} chunks={} capped={}",
        outcome.queries,
        outcome.chunks.len(),
        outcome.capped
    );
    outcome
}

/// What the baseline searches for: the reader's words, and the passage they
/// are looking at when there is one.
///
/// The selection leads because an anchored ask is *about* that passage — "why
/// scale?" on its own is not a query, and the two together are.
fn baseline_query(request: &LoopRequest<'_>) -> String {
    match request.selection.map(str::trim) {
        Some(selection) if !selection.is_empty() => {
            format!("{selection} {}", request.question)
        }
        _ => request.question.to_string(),
    }
}

/// The unconditional first search (RFC 0079 R5.1).
///
/// Queries with the reader's own words, plus the passage they have selected
/// when there is one — the two things we know the question is about before any
/// model has looked at it. Silent about its own failures for the same reason
/// the loop is: retrieval failing is not the ask failing.
///
/// Feeds the same `outcome` the loop then adds to, so `search`'s existing
/// dedup covers the overlap: a chunk the agent re-finds is already held and is
/// neither pushed twice nor counted against the budget twice.
async fn baseline_search<F>(
    context: &ContextManager,
    request: &LoopRequest<'_>,
    outcome: &mut RetrievalOutcome,
    on_event: &mut F,
) where
    F: FnMut(LoopEvent),
{
    match context.paper_has_chunks(request.paper_id) {
        Ok(true) => {}
        Ok(false) => {
            // Not a failure to report as one: an unindexed paper is a state the
            // reader can act on, and R5.4 surfaces it as such.
            outcome.paper_indexed = false;
            eprintln!("[chat] baseline skipped — paper has no chunks (not indexed)");
            return;
        }
        Err(error) => {
            eprintln!("[chat] baseline index check failed: {error}");
            return;
        }
    }

    let query = baseline_query(request);

    outcome.queries.push(query.clone());
    on_event(LoopEvent::SearchingPaper {
        query: query.clone(),
    });

    let response = context
        .search(SearchRequest {
            query,
            paper_ids: vec![request.paper_id.to_string()],
            vault_ids: Vec::new(),
            mode: SearchMode::Hybrid,
            limit: Some(BASELINE_CHUNKS),
        })
        .await;

    match response {
        Ok(response) => {
            for hit in response.hits.into_iter().take(BASELINE_CHUNKS) {
                outcome.chunks.push(hit.chunk);
            }
            on_event(LoopEvent::Retrieved {
                count: outcome.chunks.len(),
            });
        }
        Err(error) => eprintln!("[chat] baseline search failed: {error}"),
    }
}

/// Execute one call. Every failure comes back as a tool *result*, not an error:
/// a turn that dies because the model mistyped an id is worse than one that
/// answers slightly less well.
async fn run_tool<F>(
    context: &ContextManager,
    research: &dyn ResearchToolbox,
    request: &LoopRequest<'_>,
    call: &openrouter::AssembledToolCall,
    outcome: &mut RetrievalOutcome,
    on_event: &mut F,
) -> String
where
    F: FnMut(LoopEvent),
{
    match call.name.as_str() {
        "search_paper" => {
            let Ok(args) = serde_json::from_str::<SearchArgs>(&call.arguments) else {
                return "Could not parse arguments. Expected {\"query\": \"...\"}.".to_string();
            };
            search_chunks(
                context,
                request,
                &args.query,
                SearchScope::Paper,
                outcome,
                on_event,
            )
            .await
        }
        "search_library" => {
            let Ok(args) = serde_json::from_str::<SearchArgs>(&call.arguments) else {
                return "Could not parse arguments. Expected {\"query\": \"...\"}.".to_string();
            };
            search_chunks(
                context,
                request,
                &args.query,
                SearchScope::Library,
                outcome,
                on_event,
            )
            .await
        }
        "search_web" => {
            let Ok(args) = serde_json::from_str::<SearchArgs>(&call.arguments) else {
                return "Could not parse arguments. Expected {\"query\": \"...\"}.".to_string();
            };
            search_web(research, &args.query, outcome, on_event).await
        }
        "start_deep_research" => {
            let Ok(args) = serde_json::from_str::<DeepResearchArgs>(&call.arguments) else {
                return "Could not parse arguments. Expected {\"title\": \"...\", \"goal\": \"...\"}."
                    .to_string();
            };
            start_deep_research(research, request, args, outcome, on_event)
        }
        "add_context" => {
            let Ok(args) = serde_json::from_str::<ChunkArgs>(&call.arguments) else {
                return "Could not parse arguments. Expected {\"chunk_id\": \"...\"}.".to_string();
            };
            let Some(thread_id) = request.thread_id else {
                return "This conversation has no history yet, so nothing can be kept.".to_string();
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

#[derive(Clone, Copy)]
enum SearchScope {
    Paper,
    Library,
}

async fn search_chunks<F>(
    context: &ContextManager,
    request: &LoopRequest<'_>,
    query: &str,
    scope: SearchScope,
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
    match scope {
        SearchScope::Paper => on_event(LoopEvent::SearchingPaper {
            query: query.to_string(),
        }),
        SearchScope::Library => on_event(LoopEvent::SearchingLibrary {
            query: query.to_string(),
        }),
    }

    let response = context
        .search(SearchRequest {
            query: query.to_string(),
            paper_ids: match scope {
                SearchScope::Paper => vec![request.paper_id.to_string()],
                SearchScope::Library => Vec::new(),
            },
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

/// Run one bounded external lookup and assign stable `W…` handles.
async fn search_web<F>(
    research: &dyn ResearchToolbox,
    query: &str,
    outcome: &mut RetrievalOutcome,
    on_event: &mut F,
) -> String
where
    F: FnMut(LoopEvent),
{
    if !outcome.external_citations.is_empty() {
        outcome.capped = true;
        return "The web-search budget for this turn is spent.".to_string();
    }

    outcome.queries.push(query.to_string());
    on_event(LoopEvent::SearchingWeb {
        query: query.to_string(),
    });
    let evidence = match research.search_web(query, WEB_SOURCE_LIMIT).await {
        Ok(evidence) => evidence,
        Err(error) => {
            let message = web_unavailable_message(&error);
            outcome.web_lookup = WebLookupOutcome::Unavailable {
                message: message.clone(),
            };
            return message;
        }
    };

    let evidence = evidence.into_iter().take(WEB_SOURCE_LIMIT);
    let mut lines = Vec::new();
    for source in evidence {
        let handle = format!("W{}", outcome.external_citations.len() + 1);
        on_event(LoopEvent::ReadingSource {
            title: source.title.clone(),
        });
        lines.push(format!(
            "[{handle}] {} :: {}",
            source.title,
            preview(&source.excerpt)
        ));
        outcome.external_citations.push(ExternalCitation {
            handle,
            url: source.url,
            title: source.title,
            publisher: source.publisher,
            retrieved_at: source.retrieved_at,
            excerpt: source.excerpt,
        });
    }

    if lines.is_empty() {
        outcome.web_lookup = WebLookupOutcome::NoEvidence;
        "No external sources matched. Say that the lookup found no usable evidence.".to_string()
    } else {
        outcome.web_lookup = WebLookupOutcome::Succeeded {
            source_count: outcome.external_citations.len(),
        };
        format!(
            "{} external source(s). Cite only the [W…] handles below.\n{}",
            lines.len(),
            lines.join("\n")
        )
    }
}

fn explicit_web_search_request(question: &str) -> bool {
    let question = question.to_ascii_lowercase();
    [
        "search the web",
        "search web",
        "search online",
        "look this up online",
        "look it up online",
        "look online",
        "find current sources",
        "check the internet",
        "search the internet",
    ]
    .iter()
    .any(|phrase| question.contains(phrase))
}

fn web_unavailable_message(error: &str) -> String {
    crate::shared::log::warn("chat", format!("web lookup unavailable: {error}"));
    if error.to_ascii_lowercase().contains("browser")
        || error.to_ascii_lowercase().contains("obscura")
    {
        "Web search is temporarily unavailable because the browser could not start. Retry after the browser is ready."
            .to_string()
    } else {
        "Web search was temporarily unavailable for this turn. Retry in a moment.".to_string()
    }
}

/// Launch at most one background run, and only for an explicit user request.
fn start_deep_research<F>(
    research: &dyn ResearchToolbox,
    request: &LoopRequest<'_>,
    args: DeepResearchArgs,
    outcome: &mut RetrievalOutcome,
    on_event: &mut F,
) -> String
where
    F: FnMut(LoopEvent),
{
    if !explicit_deep_research_request(request.question) {
        return "Do not start research yet. Suggest it and ask the reader to confirm explicitly."
            .to_string();
    }
    if !outcome.research_activities.is_empty() {
        outcome.capped = true;
        return "One Deep Research run is already linked to this turn.".to_string();
    }

    let title = args.title.trim();
    let goal = args.goal.trim();
    if goal.is_empty() {
        return "Could not start Deep Research: the goal is empty.".to_string();
    }
    let title = if title.is_empty() { goal } else { title };
    on_event(LoopEvent::StartingDeepResearch {
        title: title.to_string(),
    });
    match research.start_deep_research(title, goal) {
        Ok(activity) => {
            let result = format!(
                "Deep Research started: search_id={}, run_id={}. It is background activity, not evidence for this answer.",
                activity.search_id, activity.run_id
            );
            outcome.research_activities.push(activity);
            result
        }
        Err(error) => format!("Could not start Deep Research: {error}"),
    }
}

fn explicit_deep_research_request(question: &str) -> bool {
    let question = question.to_ascii_lowercase();
    [
        "deep research",
        "start research",
        "run research",
        "search the literature",
        "literature search",
        "find papers",
    ]
    .iter()
    .any(|phrase| question.contains(phrase))
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
fn system_prompt(
    request: &LoopRequest<'_>,
    context: &ContextManager,
    held: &[DocumentChunk],
) -> String {
    let mut prompt = format!(
        "You are preparing to answer a question about the paper \"{}\".\n\
         \n\
         Your only job right now is to fetch evidence or start explicitly \
         requested background research. Do not answer — reply \
         with tool calls, or with nothing at all.\n\
         \n\
         A search on the reader's own words has already run, and what it \
         found is listed below. Your job is to improve on it, not to repeat \
         it: search again only when those passages plainly miss what the \
         question turns on — a section they do not reach, a term the reader \
         did not use, the second half of a two-part question. If they already \
         cover it, reply with nothing at all. That is the common case and it \
         is the right answer.\n\
         \n\
         Read as little as possible. One precise query beats three broad ones, \
         and every passage competes for room with the paper text, so one you \
         do not end up citing cost the answer something.\n\
         \n\
         You get one round, so make it count: issue one query, or two if the \
         question genuinely has two parts. Use search_library for relevant work \
         already in the reader's library. Use search_web only for current facts, \
         comparisons, broader context, or an explicit request for outside \
         evidence. Start Deep Research only when the reader explicitly asks to \
         run broad research; otherwise suggest it in the final answer and wait \
         for confirmation. Keep a passage with add_context only if later turns \
         will need it.\n",
        request.paper_title,
    );

    // RFC 0079 R5.1: what the baseline already put in front of the answer
    // model. Without this the agent searches blind and re-fetches what it
    // already has.
    if held.is_empty() {
        prompt.push_str(
            "\nThe first search found nothing usable. If the question needs \
             evidence, try different wording.\n",
        );
    } else {
        prompt.push_str("\nAlready retrieved for this turn:\n");
        for chunk in held {
            prompt.push_str(&format!(
                "- id={} p{}{} :: {}\n",
                chunk.id,
                chunk.page_start + 1,
                chunk
                    .heading_path
                    .as_deref()
                    .map(|heading| format!(" · {heading}"))
                    .unwrap_or_default(),
                preview(&chunk.text),
            ));
        }
    }

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
    let search_parameters = serde_json::json!({
        "type": "object",
        "properties": {
            "query": {
                "type": "string",
                "description": "What to look for, in your own words. Keep it narrow and specific."
            }
        },
        "required": ["query"]
    });
    let mut tools = vec![Tool {
        kind: "function".into(),
        function: ToolFunction {
            name: "search_paper".into(),
            description: "Find passages in this paper by wording and meaning. \
                 Use it when the question turns on a specific claim, number, \
                 method, or term you have not already been shown. Do not use it \
                 for general questions about what the paper is about."
                .into(),
            parameters: search_parameters.clone(),
        },
    }, Tool {
        kind: "function".into(),
        function: ToolFunction {
            name: "search_library".into(),
            description: "Find passages across papers already saved in the user's library. Use for comparisons or cross-paper connections that do not require current web evidence.".into(),
            parameters: search_parameters.clone(),
        },
    }, Tool {
        kind: "function".into(),
        function: ToolFunction {
            name: "search_web".into(),
            description: "Run one bounded external lookup and read at most three sources. Use for current facts, broader context, or comparisons requiring evidence outside the library.".into(),
            parameters: search_parameters,
        },
    }, Tool {
        kind: "function".into(),
        function: ToolFunction {
            name: "start_deep_research".into(),
            description: "Start a persisted background literature search. Call only when the reader explicitly asks to start broad research; never use its unfinished run as evidence for the current answer.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "title": { "type": "string", "description": "Short activity title." },
                    "goal": { "type": "string", "description": "Self-contained research goal." }
                },
                "required": ["title", "goal"]
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
struct DeepResearchArgs {
    title: String,
    goal: String,
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
    use super::super::research_tools::{ResearchToolbox, WebEvidence};
    use super::*;
    use crate::domain::context::ORIGIN_USER;
    use crate::services::search::SearchService;
    use crate::storage::library_store::LibraryStore;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::{SystemTime, UNIX_EPOCH};

    static SEQUENCE: AtomicUsize = AtomicUsize::new(0);

    struct FakeResearch;
    struct EmptyResearch;
    struct FailingResearch;

    #[derive(Deserialize)]
    struct EvalCase {
        name: String,
        question: String,
        expected_tool: String,
        expected_boundary: String,
    }

    #[async_trait::async_trait]
    impl ResearchToolbox for FakeResearch {
        async fn search_web(&self, query: &str, limit: usize) -> Result<Vec<WebEvidence>, String> {
            Ok(vec![WebEvidence {
                url: "https://example.test/evidence".to_string(),
                title: format!("Evidence for {query}"),
                publisher: Some("Example".to_string()),
                retrieved_at: "2026-08-24T00:00:00Z".to_string(),
                excerpt: "A bounded external fact.".to_string(),
            }]
            .into_iter()
            .take(limit)
            .collect())
        }

        fn start_deep_research(
            &self,
            title: &str,
            _goal: &str,
        ) -> Result<ResearchActivity, String> {
            Ok(ResearchActivity {
                search_id: "search-1".to_string(),
                run_id: "run-1".to_string(),
                title: title.to_string(),
                status: "queued".to_string(),
            })
        }
    }

    #[async_trait::async_trait]
    impl ResearchToolbox for FailingResearch {
        async fn search_web(
            &self,
            _query: &str,
            _limit: usize,
        ) -> Result<Vec<WebEvidence>, String> {
            Err("browser_process_unavailable: Obscura did not become healthy".to_string())
        }

        fn start_deep_research(
            &self,
            _title: &str,
            _goal: &str,
        ) -> Result<ResearchActivity, String> {
            unreachable!("this test requests web search")
        }
    }

    #[async_trait::async_trait]
    impl ResearchToolbox for EmptyResearch {
        async fn search_web(
            &self,
            _query: &str,
            _limit: usize,
        ) -> Result<Vec<WebEvidence>, String> {
            Ok(Vec::new())
        }

        fn start_deep_research(
            &self,
            _title: &str,
            _goal: &str,
        ) -> Result<ResearchActivity, String> {
            unreachable!("this test requests web search")
        }
    }

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
            decide_model: "unused",
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
            &FakeResearch,
            &request(thread_id),
            call,
            outcome,
            &mut |_| {},
        )
        .await
    }

    /// RFC 0079 R5.4: an unindexed paper is a state the reader can fix, so it
    /// must be distinguishable from "the search found nothing". Without chunks
    /// the baseline reports it and spends no search.
    #[tokio::test]
    async fn the_baseline_reports_an_unindexed_paper_instead_of_searching_it() {
        let fixture = fixture();
        let mut outcome = RetrievalOutcome::default();

        baseline_search(&fixture.manager, &request(None), &mut outcome, &mut |_| {}).await;

        assert!(!outcome.paper_indexed);
        assert!(outcome.chunks.is_empty());
        assert!(
            outcome.queries.is_empty(),
            "no query is worth running against an empty index"
        );
    }

    /// R5.1: the anchored ask searches for the passage *and* the question. "why
    /// scale?" alone retrieves nothing useful; with the selection it is a real
    /// query.
    #[tokio::test]
    async fn an_anchored_ask_searches_the_selection_with_the_question() {
        let fixture = fixture();
        let mut outcome = RetrievalOutcome::default();
        let mut request = request(None);
        request.selection = Some("  dot-product attention  ");

        baseline_search(&fixture.manager, &request, &mut outcome, &mut |_| {}).await;

        // The paper is unindexed in this fixture, so the query never runs — but
        // when it does, this is the shape it takes.
        assert_eq!(
            baseline_query(&request),
            "dot-product attention why scale?",
            "selection first, trimmed, then the question"
        );
    }

    #[tokio::test]
    async fn malformed_arguments_answer_the_call_instead_of_killing_the_turn() {
        let fixture = fixture();
        let mut outcome = RetrievalOutcome::default();

        for (name, expected) in [
            ("search_paper", "query"),
            ("search_library", "query"),
            ("search_web", "query"),
            ("start_deep_research", "title"),
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

    #[test]
    fn one_round_keeps_the_wait_in_front_of_the_answer_bounded() {
        // The reader waits through every iteration before the first word of
        // prose. This is the constant that decides how long that is, so it is
        // pinned rather than left to drift back up.
        assert_eq!(MAX_ITERATIONS, 1);
        assert!(MAX_CALLS_PER_ITERATION <= 2);
        assert!(SEARCH_LIMIT * MAX_CALLS_PER_ITERATION <= MAX_CHUNKS_PER_TURN);
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
            &call("search_paper", r#"{"query":"scaling"}"#),
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
        assert_eq!(
            names,
            vec![
                "search_paper",
                "search_library",
                "search_web",
                "start_deep_research"
            ]
        );

        let names: Vec<String> = context_tools(true)
            .into_iter()
            .map(|tool| tool.function.name)
            .collect();
        assert_eq!(
            names,
            vec![
                "search_paper",
                "search_library",
                "search_web",
                "start_deep_research",
                "add_context",
                "drop_context"
            ]
        );
    }

    #[tokio::test]
    async fn web_search_is_bounded_and_assigns_external_handles() {
        let fixture = fixture();
        let mut outcome = RetrievalOutcome::default();

        let result = run_one(
            &fixture,
            Some("t"),
            &call("search_web", r#"{"query":"current comparison"}"#),
            &mut outcome,
        )
        .await;

        assert!(result.contains("[W1]"), "{result}");
        assert_eq!(outcome.external_citations.len(), 1);
        assert_eq!(outcome.external_citations[0].handle, "W1");
    }

    #[test]
    fn explicit_web_commands_are_detected_conservatively() {
        assert!(explicit_web_search_request(
            "Please search the web for newer DINO results"
        ));
        assert!(explicit_web_search_request("Look this up online"));
        assert!(!explicit_web_search_request(
            "What does the current paper conclude?"
        ));
    }

    #[tokio::test]
    async fn explicit_web_request_bypasses_a_planner_that_could_decline_tools() {
        let fixture = fixture();
        let mut request = request(None);
        request.question = "Search the web for current comparisons";

        // `url` is deliberately invalid. Reaching the planner would fail; the
        // explicit command must invoke the toolbox directly instead.
        let outcome = run(
            &reqwest::Client::new(),
            &fixture.manager,
            &FakeResearch,
            request,
            |_| {},
        )
        .await;

        assert_eq!(outcome.external_citations.len(), 1);
        assert_eq!(
            outcome.web_lookup,
            WebLookupOutcome::Succeeded { source_count: 1 }
        );
    }

    #[tokio::test]
    async fn browser_failure_becomes_a_typed_unavailable_outcome() {
        let fixture = fixture();
        let mut request = request(None);
        request.question = "Search online for current comparisons";

        let outcome = run(
            &reqwest::Client::new(),
            &fixture.manager,
            &FailingResearch,
            request,
            |_| {},
        )
        .await;

        assert!(outcome.external_citations.is_empty());
        assert!(matches!(
            outcome.web_lookup,
            WebLookupOutcome::Unavailable { ref message }
                if message.contains("browser could not start")
        ));
    }

    #[tokio::test]
    async fn empty_web_results_are_distinct_from_browser_failure() {
        let fixture = fixture();
        let mut request = request(None);
        request.question = "Search online for current comparisons";

        let outcome = run(
            &reqwest::Client::new(),
            &fixture.manager,
            &EmptyResearch,
            request,
            |_| {},
        )
        .await;

        assert!(outcome.external_citations.is_empty());
        assert_eq!(outcome.web_lookup, WebLookupOutcome::NoEvidence);
    }

    #[test]
    fn deep_research_requires_an_explicit_request() {
        assert!(explicit_deep_research_request(
            "Run deep research on sparse attention"
        ));
        assert!(explicit_deep_research_request(
            "Find papers about this method"
        ));
        assert!(!explicit_deep_research_request(
            "How does this compare with sparse attention?"
        ));
    }

    #[tokio::test]
    async fn explicit_deep_research_starts_one_linked_background_run() {
        let fixture = fixture();
        let mut outcome = RetrievalOutcome::default();
        let mut request = request(Some("t"));
        request.question = "Run Deep Research about sparse attention";

        let result = run_tool(
            &fixture.manager,
            &FakeResearch,
            &request,
            &call(
                "start_deep_research",
                r#"{"title":"Sparse attention","goal":"Find recent sparse attention papers"}"#,
            ),
            &mut outcome,
            &mut |_| {},
        )
        .await;

        assert!(result.contains("background activity"), "{result}");
        assert_eq!(outcome.research_activities.len(), 1);
        assert_eq!(outcome.research_activities[0].run_id, "run-1");
    }

    #[test]
    fn routing_prompt_preserves_epistemic_and_latency_bounds() {
        let fixture = fixture();
        let prompt = system_prompt(&request(Some("t")), &fixture.manager, &[]);

        assert!(prompt.contains("search_library"));
        assert!(prompt.contains("search_web"));
        assert!(prompt.contains("explicitly asks"));
        assert_eq!(MAX_ITERATIONS, 1);
        assert_eq!(WEB_SOURCE_LIMIT, 3);
    }

    #[test]
    fn fixed_evaluation_suite_covers_every_tool_and_epistemic_boundary() {
        let cases: Vec<EvalCase> = serde_json::from_str(include_str!(
            "../../../tests/fixtures/chat_research_connected_prompts.json"
        ))
        .expect("evaluation fixture parses");
        let tools = context_tools(true)
            .into_iter()
            .map(|tool| tool.function.name)
            .collect::<Vec<_>>();

        for required in [
            "search_paper",
            "search_library",
            "search_web",
            "start_deep_research",
        ] {
            assert!(
                cases.iter().any(|case| case.expected_tool == required),
                "missing evaluation case for {required}"
            );
            assert!(tools.iter().any(|tool| tool == required));
        }
        for boundary in [
            "evidence",
            "external",
            "inference",
            "hypothesis",
            "activity",
        ] {
            assert!(
                cases.iter().any(|case| case.expected_boundary == boundary),
                "missing evaluation boundary {boundary}"
            );
        }
        assert!(cases
            .iter()
            .all(|case| !case.name.is_empty() && !case.question.is_empty()));
    }
}
