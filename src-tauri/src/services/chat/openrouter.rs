//! OpenRouter chat-completions transport (OpenAI-compatible).
//!
//! M1 is non-streaming: build the request, POST it, map error statuses to
//! human-readable messages, and pull the assistant text out of the response.

use futures_util::StreamExt;
use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize};

/// One message in the OpenAI-compatible `messages` array.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct WireMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct CompletionRequest {
    pub model: String,
    pub messages: Vec<WireMessage>,
    pub stream: bool,
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
pub(super) async fn complete(
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
}

/// One decoded server-sent event from the completion stream.
#[derive(Debug, PartialEq)]
pub(super) enum SseEvent {
    Delta(String),
    Done,
}

/// Incremental decoder for OpenRouter's SSE response.
///
/// Network reads can split a line mid-bytes (even mid-codepoint), so bytes are
/// buffered and only decoded once a full `\n`-terminated line is available.
pub(super) struct SseDecoder {
    buffer: Vec<u8>,
}

impl SseDecoder {
    pub(super) fn new() -> Self {
        Self { buffer: Vec::new() }
    }

    /// Feed a chunk of bytes; return the events from any now-complete lines.
    pub(super) fn push(&mut self, chunk: &[u8]) -> Vec<SseEvent> {
        self.buffer.extend_from_slice(chunk);
        let mut events = Vec::new();

        while let Some(newline) = self.buffer.iter().position(|&byte| byte == b'\n') {
            let line: Vec<u8> = self.buffer.drain(..=newline).collect();
            // A full line up to '\n' is always valid UTF-8 even if an earlier
            // chunk boundary fell mid-codepoint, since '\n' is ASCII.
            if let Ok(text) = std::str::from_utf8(&line) {
                if let Some(event) = parse_sse_line(text.trim_end_matches('\n')) {
                    events.push(event);
                }
            }
        }

        events
    }
}

/// Classify one complete SSE line (newline already stripped).
fn parse_sse_line(line: &str) -> Option<SseEvent> {
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
    let content = chunk.choices.into_iter().next()?.delta.content?;
    if content.is_empty() {
        return None;
    }
    Some(SseEvent::Delta(content))
}

/// POST a streaming completion, forwarding each content delta to `on_delta`,
/// and return the fully assembled assistant text.
pub(super) async fn complete_streamed<F>(
    client: &Client,
    url: &str,
    api_key: &str,
    request: &CompletionRequest,
    mut on_delta: F,
) -> Result<String, String>
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
                SseEvent::Done => return Ok(answer),
            }
        }
    }

    Ok(answer)
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
