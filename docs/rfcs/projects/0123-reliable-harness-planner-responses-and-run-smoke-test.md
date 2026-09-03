# RFC 0123: Reliable Harness Planner Responses and Run Smoke Test

- Status: Implemented and verified
- Date: 2026-09-03
- Area: Projects / Research Harness / OpenRouter
- Parent: RFC 0108
- Builds on: RFC 0111, RFC 0122

## Summary

Make a Project Research Run survive valid OpenRouter response variants and add
one test seam that exercises the complete Harness pipeline below the Tauri UI.
Failures must identify the failed stage and must not be presented as malformed
Harness configuration when an external model response caused them.

This RFC intentionally does not redesign the Harness settings or Research UI.
That usability work should follow separately after the basic Run path is
reliable.

## Observed Failure

On 2026-09-03, a manual Run using configuration v3 failed during its first
planning call with:

```text
Invalid OpenRouter response: invalid type: null, expected a string at line 9 column 334
```

The persisted Run stopped before issuing a discovery query. The active planner
preference was `deepseek/deepseek-v4-flash`. A later live request to the same
model returned ordinary string content, so the failure is intermittent rather
than a permanently invalid model selection.

The UI rendered the provider response failure next to `configuration v3`. That
layout makes the failure appear to be a configuration-validation problem even
though configuration validation had already succeeded.

## Diagnosis

`services/llm.rs` uses the same `WireMessage` type for outbound requests and
inbound non-streaming responses. Its `content` field is a required `String`.
The OpenAI-compatible response envelope can contain an assistant message whose
`content` is `null`, for example when a provider returns reasoning, refusal, or
another non-text completion shape. Serde therefore rejects the entire HTTP 200
response before the Research planner receives it.

The planner's existing repair retry cannot help. `complete_json()` retries only
after it receives text that is not parseable as a JSON object; the transport
fails first while decoding the envelope.

The exact serde error and current DTO establish the failed boundary. The saved
response body is not retained, so this diagnosis cannot determine which
alternate field accompanied `content: null` in the original response. The fix
must preserve a bounded, sanitized description of that shape in future errors.

## Decision

### Separate request and response DTOs

Keep outbound `WireMessage.content` as `String`. Introduce a small inbound
completion message whose `content` is `Option<String>` and which captures only
the response metadata needed to classify an absent answer. Do not weaken every
outbound call site to accept nullable content.

The non-streaming completion parser returns text only when the first choice has
non-empty textual content. Otherwise it returns a stable error that includes:

- the selected model;
- the choice finish reason, when present;
- whether reasoning, refusal, or tool calls were present; and
- no provider reasoning text, secrets, or unbounded response body.

Null content is not converted to an empty string. An empty string would merely
move the failure into the JSON parser and hide the actual provider contract
problem.

### Retry at the correct boundary

The Research planner may retry one completion when the transport reports a
retryable missing-text response. The retry explicitly requests a final JSON
answer. Selecting a non-reasoning planner remains the portable way to avoid
reasoning-only responses; the shared transport does not add provider-specific
reasoning flags. HTTP authentication, payment, rate-limit, and
malformed-envelope errors are not retried by this layer.

If the second response still has no textual answer, the Run fails honestly at
the `planning` stage. It remains possible to start another Run without editing
or resaving Harness configuration.

### Stage-aware Run failure

Persist and display the phase that failed (`planning`, `searching`, `ranking`,
or `reconciling`). A planner transport failure is a Run failure, not a Harness
configuration failure. The compact user-facing message should be actionable;
the detailed provider diagnostics remain available in Run Activity.

## Test Strategy

### Unit regression tests

Add response fixtures for:

1. ordinary string content;
2. `content: null` with reasoning metadata;
3. `content: null` with refusal metadata;
4. no choices; and
5. malformed JSON.

The null-content fixtures must deserialize successfully and then produce the
new classified error, rather than the current serde type error.

### Planner transport test

Use a local HTTP test server to return a null-content response followed by a
valid JSON plan. Verify exactly one retry and a valid `Query` result. A second
null-content response must fail with the classified planning error.

### Harness pipeline integration test

Add a manager-level test with temporary SQLite storage, a fake completion
server, and a fake candidate source. It must execute:

```text
manual Harness Run
  -> linked Search Run
  -> plan
  -> discover
  -> rank
  -> reconcile
  -> finalize checkpoint
```

The assertion covers linked IDs, terminal status, Activity ordering, usage,
candidate decisions, resulting State, and checkpoint completeness. This is the
missing layer between isolated loop/store tests and the desktop UI.

### Opt-in live smoke test

Provide one ignored test or diagnostic command that uses the configured
OpenRouter key and planner model to run a minimal plan request. It reports the
sanitized response shape and does not require Obscura. This catches provider
contract drift without making ordinary test runs network-dependent.

### Desktop smoke test

The manual acceptance pass starts one Run from the Project Research UI and
observes at least one planning and one discovery Activity event before a
terminal result. It also verifies that a forced null-content failure is shown
as a planning/provider failure and that **Run now** remains available afterward.

## Current Coverage Audit

The repository currently has substantial component coverage:

- 24 focused tests for the shared LLM transport;
- 11 focused tests for planner JSON parsing and normalization;
- 16 focused tests for the bounded Research loop using `FakePlanner` and fake
  candidate sources; and
- 116 `LibraryStore` tests, including Harness lifecycle, reconciliation,
  checkpoints, scheduling, and recovery.

All of those focused suites passed during this diagnosis. They do not reproduce
the production failure because no transport test includes nullable completion
content, the loop tests replace OpenRouter with a fake planner, and the store
tests do not execute the manager's network pipeline.

There is currently no automated end-to-end test that launches a Project Run
through the Tauri command, OpenRouter-compatible transport, discovery,
reconciliation, persistence, and Svelte UI. Existing ignored live tests cover
CORE, general discovery, embeddings, query expansion, and metadata extraction,
but not a Research Harness Run.

## Acceptance Criteria

1. A successful OpenRouter envelope with `message.content: null` no longer
   fails Serde deserialization.
2. Missing textual output is classified without leaking response bodies or
   silently becoming an empty planner answer.
3. One retry can recover a missing-text planner response; repeated absence
   fails with a stage-aware message.
4. A failed provider response does not imply that Harness configuration is
   invalid and does not prevent a later manual Run.
5. A manager-level integration test completes the Harness pipeline with local
   deterministic dependencies.
6. An opt-in live planner smoke test is documented and runnable.
7. Rust tests, frontend type checking, and the production frontend build pass.

## Implementation Record

Implemented on 2026-09-03:

- inbound completion messages accept nullable content and produce a sanitized,
  classified missing-text error;
- the Research planner retries that error exactly once with a concise repair
  instruction;
- Run failures identify the active phase instead of presenting provider errors
  as invalid Harness configuration;
- a deterministic manager test exercises planning through checkpoint
  finalization with temporary SQLite storage; and
- an ignored live planner smoke test can be run with:

```bash
I0I_LIVE_PLANNER_MODEL=anthropic/claude-sonnet-4.5 \
  cargo test --lib --no-default-features \
  services::research::planner::tests::live_planner_returns_queries \
  -- --ignored --nocapture
```

Verification completed with 539 Rust library tests passing (6 ignored), the
live Sonnet planner smoke test passing, `pnpm check` reporting no errors or
warnings, and the production frontend build succeeding.
