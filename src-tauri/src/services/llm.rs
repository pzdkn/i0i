//! OpenRouter chat-completions transport (OpenAI-compatible).
//!
//! M1 is non-streaming: build the request, POST it, map error statuses to
//! human-readable messages, and pull the assistant text out of the response.

use futures_util::StreamExt;
use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// One message in the OpenAI-compatible `messages` array.
///
/// `tool_calls` and `tool_call_id` exist for the agentic retrieval loop
/// (RFC 0078), which has to replay what the model asked for and what came back.
/// Both are skipped when `None`, so every pre-0078 call site emits a
/// byte-identical payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct WireMessage {
    pub role: String,
    pub content: String,
    /// Set on an `assistant` message replaying tool calls the model made.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<WireToolCall>>,
    /// Set on a `tool` message carrying one call's result.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

impl WireMessage {
    /// An ordinary `system` / `user` / `assistant` message.
    pub fn text(role: &str, content: String) -> Self {
        Self {
            role: role.to_string(),
            content,
            tool_calls: None,
            tool_call_id: None,
        }
    }

    /// The assistant turn that requested `calls`, replayed back to the model.
    pub fn tool_request(calls: Vec<WireToolCall>) -> Self {
        Self {
            role: "assistant".to_string(),
            content: String::new(),
            tool_calls: Some(calls),
            tool_call_id: None,
        }
    }

    /// One tool's result, answering the call with id `tool_call_id`.
    pub fn tool_result(tool_call_id: String, content: String) -> Self {
        Self {
            role: "tool".to_string(),
            content,
            tool_calls: None,
            tool_call_id: Some(tool_call_id),
        }
    }
}

/// A tool call as it goes back *out* on the wire, replaying what the model asked.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct WireToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String, // "function"
    pub function: WireToolCallFunction,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct WireToolCallFunction {
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct CompletionRequest {
    pub model: String,
    pub messages: Vec<WireMessage>,
    pub stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    /// Optional structured-output hint (e.g. JSON mode). Omitted from the wire
    /// payload when `None`, so the existing chat path is unchanged.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_format: Option<ResponseFormat>,
    /// Tool schemas the model may call (OpenAI-compatible `tools` array).
    /// Omitted from the wire payload when `None`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<Tool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<String>,
}

/// One OpenAI-compatible tool definition (`{"type":"function","function":{...}}`).
#[derive(Debug, Clone, Serialize)]
pub(crate) struct Tool {
    #[serde(rename = "type")]
    pub kind: String, // "function"
    pub function: ToolFunction,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct ToolFunction {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value, // JSON Schema
}

/// The `highlight` and `note` tools offered to the model during a chat that
/// supports agent-driven highlighting (RFC 0059 Phase 2). Wired into
/// `ask_at_anchor_streamed`'s request in `services/chat/service.rs`.
pub(crate) fn highlight_tools() -> Vec<Tool> {
    let colors = serde_json::json!(["yellow", "green", "blue", "red", "purple", "orange"]);
    vec![
        Tool {
            kind: "function".into(),
            function: ToolFunction {
                name: "highlight".into(),
                description:
                    "Highlight a verbatim passage from the paper in a color. Quote the exact text."
                        .into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "quote": {"type": "string", "description": "Exact verbatim text from the paper to highlight."},
                        "color": {"type": "string", "enum": colors},
                        "label": {"type": "string", "description": "Optional short category, e.g. 'experimental result'."}
                    },
                    "required": ["quote", "color"]
                }),
            },
        },
        Tool {
            kind: "function".into(),
            function: ToolFunction {
                name: "note".into(),
                description: "Highlight a verbatim passage and attach a short note to it.".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "quote": {"type": "string"},
                        "body": {"type": "string", "description": "The note text."},
                        "color": {"type": "string", "enum": colors}
                    },
                    "required": ["quote", "body"]
                }),
            },
        },
    ]
}

/// OpenAI-compatible `response_format` hint. `json_object` asks for a single
/// JSON object; `json_schema` (RFC 0064) additionally constrains it to a strict
/// schema so the reply is one parseable object.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct ResponseFormat {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub json_schema: Option<JsonSchemaFormat>,
}

/// The `json_schema` payload for structured outputs (RFC 0064).
#[derive(Debug, Clone, Serialize)]
pub(crate) struct JsonSchemaFormat {
    pub name: String,
    pub strict: bool,
    pub schema: serde_json::Value,
}

impl ResponseFormat {
    // Used by the research planner (RFC 0037 seams layer).
    #[allow(dead_code)]
    pub(crate) fn json_object() -> Self {
        Self {
            kind: "json_object".to_string(),
            json_schema: None,
        }
    }

    /// Strict structured output constrained to `schema` (RFC 0064).
    pub(crate) fn json_schema(name: &str, schema: serde_json::Value) -> Self {
        Self {
            kind: "json_schema".to_string(),
            json_schema: Some(JsonSchemaFormat {
                name: name.to_string(),
                strict: true,
                schema,
            }),
        }
    }
}

#[derive(Debug, Deserialize)]
struct CompletionResponse {
    choices: Vec<Choice>,
}

#[derive(Debug, Deserialize)]
struct Choice {
    message: WireMessage,
}

/// POST a non-streaming completion and return the assistant's text.
pub(crate) async fn complete(
    client: &Client,
    url: &str,
    api_key: &str,
    request: &CompletionRequest,
) -> Result<String, String> {
    let response = client
        .post(url)
        .header("Authorization", format!("Bearer {api_key}"))
        .header("X-Title", "i0i")
        .json(request)
        .send()
        .await
        .map_err(|error| format!("OpenRouter request failed: {error}"))?;

    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(describe_error_status(status, &body));
    }

    let body = response
        .text()
        .await
        .map_err(|error| format!("Failed to read OpenRouter response: {error}"))?;
    parse_completion(&body)
}

/// Pull the first choice's assistant text out of a completion response body.
fn parse_completion(body: &str) -> Result<String, String> {
    let response: CompletionResponse = serde_json::from_str(body)
        .map_err(|error| format!("Invalid OpenRouter response: {error}"))?;
    response
        .choices
        .into_iter()
        .next()
        .map(|choice| choice.message.content)
        .ok_or_else(|| "OpenRouter returned no choices.".to_string())
}

/// Map an HTTP error status (and body) into a user-facing message.
///
/// The key never appears in the message — only the status and a short body
/// snippet for the generic case.
fn describe_error_status(status: StatusCode, body: &str) -> String {
    match status {
        StatusCode::UNAUTHORIZED => {
            "OpenRouter rejected the API key (401). Check OPENROUTER_API_KEY.".to_string()
        }
        StatusCode::PAYMENT_REQUIRED => "OpenRouter account is out of credits (402).".to_string(),
        StatusCode::TOO_MANY_REQUESTS => {
            "OpenRouter rate limit reached (429). Wait a moment and try again.".to_string()
        }
        _ => {
            let snippet = body_snippet(body);
            if snippet.is_empty() {
                format!("OpenRouter request failed with {status}.")
            } else {
                format!("OpenRouter request failed with {status}: {snippet}")
            }
        }
    }
}

/// Trim a response body to a short, single-line snippet for error messages.
fn body_snippet(body: &str) -> String {
    body.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(200)
        .collect()
}

// --- Streaming (M2) ---

#[derive(Debug, Deserialize)]
struct StreamChunk {
    choices: Vec<StreamChoice>,
}

#[derive(Debug, Deserialize)]
struct StreamChoice {
    delta: StreamDelta,
}

#[derive(Debug, Deserialize)]
struct StreamDelta {
    content: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<ToolCallDelta>>,
}

/// One fragment of a streamed tool call, keyed by `index` so fragments for
/// the same call can be reassembled across chunks.
#[derive(Debug, Deserialize)]
struct ToolCallDelta {
    index: usize,
    /// Needed to answer the call: a `tool` message must name the id it responds
    /// to. Arrives on the first fragment only, so it is kept, not overwritten.
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    function: Option<FunctionDelta>,
}

#[derive(Debug, Deserialize)]
struct FunctionDelta {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    arguments: Option<String>,
}

/// A fully assembled tool call: fragments concatenated in arrival order.
#[derive(Debug, PartialEq)]
pub(crate) struct AssembledToolCall {
    /// Provider-assigned call id. Empty if the provider omitted it — the loop
    /// synthesizes one rather than failing, since a missing id only breaks the
    /// reply pairing, not the call itself.
    pub id: String,
    pub name: String,
    pub arguments: String,
}

/// One decoded server-sent event from the completion stream.
#[derive(Debug, PartialEq)]
pub(crate) enum SseEvent {
    Delta(String),
    Done,
}

/// Incremental decoder for OpenRouter's SSE response.
///
/// Network reads can split a line mid-bytes (even mid-codepoint), so bytes are
/// buffered and only decoded once a full `\n`-terminated line is available.
///
/// Tool-call fragments (when the model streams a `tool_calls` delta) are
/// accumulated by `index` as they arrive; call `finish` at stream end to get
/// the fully assembled calls in index order.
pub(crate) struct SseDecoder {
    buffer: Vec<u8>,
    /// index → (id, name, arguments)
    tool_calls: BTreeMap<usize, (String, String, String)>,
}

impl SseDecoder {
    pub(crate) fn new() -> Self {
        Self {
            buffer: Vec::new(),
            tool_calls: BTreeMap::new(),
        }
    }

    /// Feed a chunk of bytes; return the events from any now-complete lines.
    pub(crate) fn push(&mut self, chunk: &[u8]) -> Vec<SseEvent> {
        self.buffer.extend_from_slice(chunk);
        let mut events = Vec::new();

        while let Some(newline) = self.buffer.iter().position(|&byte| byte == b'\n') {
            let line: Vec<u8> = self.buffer.drain(..=newline).collect();
            // A full line up to '\n' is always valid UTF-8 even if an earlier
            // chunk boundary fell mid-codepoint, since '\n' is ASCII.
            if let Ok(text) = std::str::from_utf8(&line) {
                if let Some(event) = self.handle_sse_line(text.trim_end_matches('\n')) {
                    events.push(event);
                }
            }
        }

        events
    }

    /// Classify one complete SSE line (newline already stripped), applying
    /// any tool-call fragments to the running assembly state as a side effect.
    fn handle_sse_line(&mut self, line: &str) -> Option<SseEvent> {
        let line = line.trim_end_matches('\r');
        if line.is_empty() || line.starts_with(':') {
            // Blank separator or comment/keep-alive line.
            return None;
        }

        let data = line.strip_prefix("data:")?.trim_start();
        if data == "[DONE]" {
            return Some(SseEvent::Done);
        }

        let chunk: StreamChunk = serde_json::from_str(data).ok()?;
        let delta = chunk.choices.into_iter().next()?.delta;

        if let Some(fragments) = delta.tool_calls {
            for fragment in fragments {
                let entry = self
                    .tool_calls
                    .entry(fragment.index)
                    .or_insert_with(|| (String::new(), String::new(), String::new()));
                // The id arrives on the first fragment only; later fragments
                // carry arguments alone and must not blank it.
                if let Some(id) = fragment.id {
                    if !id.is_empty() {
                        entry.0 = id;
                    }
                }
                if let Some(function) = fragment.function {
                    if let Some(name) = function.name {
                        entry.1 = name;
                    }
                    if let Some(arguments) = function.arguments {
                        entry.2.push_str(&arguments);
                    }
                }
            }
        }

        let content = delta.content?;
        if content.is_empty() {
            return None;
        }
        Some(SseEvent::Delta(content))
    }

    /// Drain and return the tool calls assembled so far, ordered by index.
    /// Call this once the stream has ended (e.g. after `SseEvent::Done`).
    pub(crate) fn finish(&mut self) -> Vec<AssembledToolCall> {
        std::mem::take(&mut self.tool_calls)
            .into_iter()
            .map(|(_, (id, name, arguments))| AssembledToolCall {
                id,
                name,
                arguments,
            })
            .collect()
    }
}

/// The result of a streamed completion: the assembled assistant text plus any
/// tool calls the model requested during the stream.
#[derive(Debug, PartialEq)]
pub(crate) struct StreamOutcome {
    pub text: String,
    pub tool_calls: Vec<AssembledToolCall>,
}

/// POST a streaming completion, forwarding each content delta to `on_delta`,
/// and return the fully assembled assistant text plus any tool calls.
pub(crate) async fn complete_streamed<F>(
    client: &Client,
    url: &str,
    api_key: &str,
    request: &CompletionRequest,
    mut on_delta: F,
) -> Result<StreamOutcome, String>
where
    F: FnMut(String),
{
    let response = client
        .post(url)
        .header("Authorization", format!("Bearer {api_key}"))
        .header("X-Title", "i0i")
        .json(request)
        .send()
        .await
        .map_err(|error| format!("OpenRouter request failed: {error}"))?;

    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(describe_error_status(status, &body));
    }

    let mut stream = response.bytes_stream();
    let mut decoder = SseDecoder::new();
    let mut answer = String::new();

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|error| format!("OpenRouter stream error: {error}"))?;
        for event in decoder.push(&chunk) {
            match event {
                SseEvent::Delta(text) => {
                    answer.push_str(&text);
                    on_delta(text);
                }
                SseEvent::Done => {
                    return Ok(StreamOutcome {
                        text: answer,
                        tool_calls: decoder.finish(),
                    })
                }
            }
        }
    }

    Ok(StreamOutcome {
        text: answer,
        tool_calls: decoder.finish(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_request() -> CompletionRequest {
        CompletionRequest {
            model: "anthropic/claude-sonnet-4.5".to_string(),
            messages: Vec::new(),
            stream: false,
            max_tokens: None,
            response_format: None,
            tools: None,
            tool_choice: None,
        }
    }

    #[test]
    fn completion_request_omits_tools_when_none() {
        let value = serde_json::to_value(empty_request()).unwrap();
        assert!(value.get("tools").is_none());
        assert!(value.get("tool_choice").is_none());
    }

    #[test]
    fn completion_request_serializes_highlight_tools() {
        let request = CompletionRequest {
            tools: Some(highlight_tools()),
            ..empty_request()
        };
        let value = serde_json::to_value(request).unwrap();
        assert_eq!(value["tools"][0]["function"]["name"], "highlight");
        let colors = &value["tools"][0]["function"]["parameters"]["properties"]["color"]["enum"];
        let colors: Vec<String> = colors
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect();
        assert_eq!(
            colors,
            vec!["yellow", "green", "blue", "red", "purple", "orange"]
        );
        assert_eq!(value["tools"][1]["function"]["name"], "note");
    }

    #[test]
    fn completion_request_omits_response_format_when_none() {
        let value = serde_json::to_value(empty_request()).unwrap();
        assert!(value.get("response_format").is_none());
    }

    #[test]
    fn completion_request_serializes_json_object_response_format() {
        let request = CompletionRequest {
            response_format: Some(ResponseFormat::json_object()),
            ..empty_request()
        };
        let value = serde_json::to_value(request).unwrap();
        assert_eq!(value["response_format"]["type"], "json_object");
    }

    #[test]
    fn parses_assistant_text_from_response() {
        let body = r#"{
            "id": "gen-123",
            "model": "anthropic/claude-sonnet-4.5",
            "choices": [
                { "message": { "role": "assistant", "content": "Hello from the model." } }
            ]
        }"#;
        assert_eq!(parse_completion(body).unwrap(), "Hello from the model.");
    }

    #[test]
    fn empty_choices_is_an_error() {
        let body = r#"{ "id": "gen-1", "choices": [] }"#;
        assert!(parse_completion(body).is_err());
    }

    #[test]
    fn malformed_response_is_an_error_not_a_panic() {
        assert!(parse_completion("not json").is_err());
    }

    #[test]
    fn maps_auth_credit_and_rate_limit_statuses() {
        assert!(describe_error_status(StatusCode::UNAUTHORIZED, "").contains("401"));
        assert!(describe_error_status(StatusCode::UNAUTHORIZED, "")
            .to_lowercase()
            .contains("key"));
        assert!(describe_error_status(StatusCode::PAYMENT_REQUIRED, "").contains("402"));
        assert!(describe_error_status(StatusCode::PAYMENT_REQUIRED, "")
            .to_lowercase()
            .contains("credit"));
        assert!(describe_error_status(StatusCode::TOO_MANY_REQUESTS, "").contains("429"));
    }

    #[test]
    fn generic_status_includes_body_snippet_but_not_the_key() {
        let message = describe_error_status(StatusCode::INTERNAL_SERVER_ERROR, "upstream\n  boom");
        assert!(message.contains("500"));
        assert!(message.contains("upstream boom"));
    }

    fn delta_line(content: &str) -> Vec<u8> {
        format!("data: {{\"choices\":[{{\"delta\":{{\"content\":\"{content}\"}}}}]}}\n")
            .into_bytes()
    }

    /// Build an SSE `data:` line whose delta is the raw `tool_calls` JSON
    /// fragment given (already escaped as needed).
    fn tool_call_line(tool_calls_json: &str) -> Vec<u8> {
        format!("data: {{\"choices\":[{{\"delta\":{{\"tool_calls\":{tool_calls_json}}}}}]}}\n")
            .into_bytes()
    }

    #[test]
    fn an_ordinary_message_serializes_without_the_tool_fields() {
        // Every pre-RFC-0078 call site must emit a byte-identical payload:
        // annotate_streamed and the research planners share this transport, and
        // a stray `"tool_calls": null` is a wire change nothing asked for.
        let json = serde_json::to_string(&WireMessage::text("user", "hi".to_string()))
            .expect("serializes");
        assert_eq!(json, r#"{"role":"user","content":"hi"}"#);
    }

    #[test]
    fn tool_request_and_result_carry_their_pairing_id() {
        let request = WireMessage::tool_request(vec![WireToolCall {
            id: "call_1".to_string(),
            kind: "function".to_string(),
            function: WireToolCallFunction {
                name: "search_context".to_string(),
                arguments: r#"{"query":"scaling"}"#.to_string(),
            },
        }]);
        let json = serde_json::to_string(&request).expect("serializes");
        assert!(json.contains(r#""tool_calls""#));
        assert!(json.contains(r#""call_1""#));
        assert!(!json.contains("tool_call_id"));

        let result = WireMessage::tool_result("call_1".to_string(), "3 hits".to_string());
        let json = serde_json::to_string(&result).expect("serializes");
        assert!(json.contains(r#""tool_call_id":"call_1""#));
        assert!(!json.contains(r#""tool_calls""#));
    }

    #[test]
    fn decoder_parses_a_single_delta_line() {
        let mut decoder = SseDecoder::new();
        let events = decoder.push(&delta_line("Hello"));
        assert_eq!(events, vec![SseEvent::Delta("Hello".to_string())]);
    }

    #[test]
    fn decoder_buffers_a_line_split_across_chunks() {
        let mut decoder = SseDecoder::new();
        let line = delta_line("Hello");
        let split = line.len() / 2;

        assert!(decoder.push(&line[..split]).is_empty());
        assert_eq!(
            decoder.push(&line[split..]),
            vec![SseEvent::Delta("Hello".to_string())]
        );
    }

    #[test]
    fn decoder_assembles_multiple_deltas_in_order() {
        let mut decoder = SseDecoder::new();
        let mut bytes = delta_line("Hello");
        bytes.extend(delta_line(" world"));

        assert_eq!(
            decoder.push(&bytes),
            vec![
                SseEvent::Delta("Hello".to_string()),
                SseEvent::Delta(" world".to_string()),
            ]
        );
    }

    #[test]
    fn decoder_emits_done_on_done_sentinel() {
        let mut decoder = SseDecoder::new();
        assert_eq!(decoder.push(b"data: [DONE]\n"), vec![SseEvent::Done]);
    }

    #[test]
    fn decoder_ignores_comment_and_blank_lines() {
        let mut decoder = SseDecoder::new();
        assert!(decoder.push(b": OPENROUTER PROCESSING\n\n").is_empty());
    }

    #[test]
    fn decoder_handles_a_codepoint_split_across_chunks() {
        // "é" is two bytes; split the data line inside that codepoint.
        let mut decoder = SseDecoder::new();
        let line = delta_line("é");
        let cut = line
            .iter()
            .position(|&b| b == 0xC3)
            .expect("é byte present")
            + 1;

        let mut events = decoder.push(&line[..cut]);
        events.extend(decoder.push(&line[cut..]));

        assert_eq!(events, vec![SseEvent::Delta("é".to_string())]);
    }

    #[test]
    fn decoder_assembles_one_tool_call_split_across_chunks() {
        let mut decoder = SseDecoder::new();

        decoder.push(&tool_call_line(
            r#"[{"index":0,"id":"c1","function":{"name":"highlight","arguments":"{\"quote\":\"a"}}]"#,
        ));
        decoder.push(&tool_call_line(
            r#"[{"index":0,"function":{"arguments":"bc\",\"color\":\"red\"}"}}]"#,
        ));

        assert_eq!(
            decoder.finish(),
            vec![AssembledToolCall {
                // The id arrives on the first fragment only; the second carries
                // arguments alone and must not blank it (RFC 0078).
                id: "c1".to_string(),
                name: "highlight".to_string(),
                arguments: r#"{"quote":"abc","color":"red"}"#.to_string(),
            }]
        );
    }

    #[test]
    fn decoder_assembles_two_interleaved_tool_calls_independently() {
        let mut decoder = SseDecoder::new();

        decoder.push(&tool_call_line(
            r#"[{"index":0,"id":"c1","function":{"name":"highlight","arguments":"{\"quote\":\"a"}}]"#,
        ));
        decoder.push(&tool_call_line(
            r#"[{"index":1,"id":"c2","function":{"name":"note","arguments":"{\"tex"}}]"#,
        ));
        decoder.push(&tool_call_line(
            r#"[{"index":0,"function":{"arguments":"bc\"}"}}]"#,
        ));
        decoder.push(&tool_call_line(
            r#"[{"index":1,"function":{"arguments":"t\":\"hi\"}"}}]"#,
        ));

        assert_eq!(
            decoder.finish(),
            vec![
                AssembledToolCall {
                    id: "c1".to_string(),
                    name: "highlight".to_string(),
                    arguments: r#"{"quote":"abc"}"#.to_string(),
                },
                AssembledToolCall {
                    id: "c2".to_string(),
                    name: "note".to_string(),
                    arguments: r#"{"text":"hi"}"#.to_string(),
                },
            ]
        );
    }

    /// `complete_streamed` drives its transport over a live `reqwest::Client`
    /// with no seam for a fake server in this codebase (no mock-HTTP crate is
    /// wired in), so this proves the same assembly logic
    /// `complete_streamed` performs on stream end (concatenate content deltas
    /// via the decoder, call `finish()` for tool calls, package both into a
    /// `StreamOutcome`) at the decoder level, mixing content deltas and one
    /// tool-call fragment sequence in arrival order.
    #[test]
    fn decoder_assembles_text_and_tool_calls_from_a_mixed_stream() {
        let mut decoder = SseDecoder::new();
        let mut text = String::new();

        let mut bytes = delta_line("Hello");
        bytes.extend(tool_call_line(
            r#"[{"index":0,"id":"c1","function":{"name":"highlight","arguments":"{\"quote\":\"a"}}]"#,
        ));
        bytes.extend(delta_line(" world"));
        bytes.extend(tool_call_line(
            r#"[{"index":0,"function":{"arguments":"bc\"}"}}]"#,
        ));

        for event in decoder.push(&bytes) {
            if let SseEvent::Delta(chunk) = event {
                text.push_str(&chunk);
            }
        }

        let outcome = StreamOutcome {
            text,
            tool_calls: decoder.finish(),
        };

        assert_eq!(outcome.text, "Hello world");
        assert_eq!(outcome.tool_calls.len(), 1);
        assert_eq!(outcome.tool_calls[0].id, "c1");
        assert_eq!(outcome.tool_calls[0].name, "highlight");
        assert_eq!(outcome.tool_calls[0].arguments, r#"{"quote":"abc"}"#);
    }

    #[test]
    fn decoder_still_assembles_plain_content_and_finish_is_empty_without_tool_calls() {
        let mut decoder = SseDecoder::new();
        let mut bytes = delta_line("Hello");
        bytes.extend(delta_line(" world"));

        let events = decoder.push(&bytes);

        assert_eq!(
            events,
            vec![
                SseEvent::Delta("Hello".to_string()),
                SseEvent::Delta(" world".to_string()),
            ]
        );
        assert_eq!(decoder.finish(), Vec::<AssembledToolCall>::new());
    }
}
