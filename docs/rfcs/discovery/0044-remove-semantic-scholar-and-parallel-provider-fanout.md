# RFC 0044: Remove Semantic Scholar And Parallel Provider Fan-Out

Status: Draft
Date: 2026-07-08
Product: i0i
Target: Tauri v2 + Svelte, macOS first
Builds on: RFC 0043 (Multi-Provider Scout Search Quality)
Supersedes: the Semantic Scholar provider-set decision in RFC 0043

## Summary

Make Discover fast again after RFC 0043 by removing Semantic Scholar and
running selected provider searches concurrently.

The decision is:

- Delete Semantic Scholar from the product and codebase for now.
- Supported discovery providers are OpenAlex and arXiv.
- Default search uses OpenAlex + arXiv.
- Provider fan-out runs concurrently for shallow search, deep search, and
  Improve Search.
- Keep RFC 0043's merge, dedupe, partial-failure, and deterministic-ranking
  model.

Plain English version: Scout should still search multiple places, but it should
not wait for one provider before starting the next one.

## Problem

RFC 0043 improved recall by making search multi-provider, but the first
implementation dispatches providers sequentially:

```text
OpenAlex
then arXiv
then Semantic Scholar
```

That makes shallow search feel slow. Shallow search is the user's fast path, so
latency matters more there than provider completeness.

Semantic Scholar also has unreliable API-key/rate-limit behavior for our current
usage. It adds operational uncertainty and does not justify being in the
default provider set right now.

## Goals

- Restore acceptable shallow-search latency.
- Make deep and improve search use concurrent provider fan-out too.
- Remove Semantic Scholar instead of hiding it behind UI conditionals.
- Keep OpenAlex + arXiv multi-provider recall.
- Preserve partial failure behavior: one provider can fail while the other
  still returns results.
- Keep stale saved/frontend state from crashing if it still mentions Semantic
  Scholar.

## Non-Goals

- No streaming shallow results in this RFC.
- No provider cancellation/racing UI.
- No new providers.
- No LLM or embedding reranking changes.
- No redesign of Discover result cards.
- No changes to PDF acquisition or source fallback.

## Decisions

1. **Remove Semantic Scholar completely.** Delete provider code, config, enum
   variant, bridge type, UI checkbox, and tests tied only to Semantic Scholar.
2. **Keep OpenAlex + arXiv.** They are the supported provider set for now.
3. **Fan out providers concurrently.** Selected providers should start at the
   same time inside the backend orchestrator.
4. **Use the same orchestrator everywhere.** Shallow search, Deep search, and
   Improve Search all go through the same provider fan-out path.
5. **Sanitize stale provider values.** If old local state or stored search
   constraints mention `semantic_scholar`, drop it. If nothing valid remains,
   fall back to OpenAlex + arXiv.

## Execution Model

### Shallow Search

```text
literal query
  -> selected providers: OpenAlex + arXiv
  -> start both provider calls concurrently
  -> await both
  -> merge successful results
  -> record provider errors
  -> dedupe
  -> rank
  -> return results
```

Expected latency becomes roughly:

```text
max(openalex latency, arxiv latency)
```

instead of:

```text
openalex latency + arxiv latency
```

### Deep / Improve Search

Deep and Improve Search still have extra work:

- LLM query planning,
- possibly multiple expanded queries,
- assessment/refinement loops.

But each provider batch inside that loop should fan out concurrently:

```text
Scout query 1
  -> OpenAlex + arXiv concurrently
Scout query 2
  -> OpenAlex + arXiv concurrently
...
merge / dedupe / assess / rank
```

Deep search can still be slower than shallow search, but it should not multiply
latency by calling providers sequentially.

## Backend Design

### Provider Set

Supported providers:

```text
OpenAlex
arXiv
```

Remove:

```text
Semantic Scholar
```

Affected backend areas:

```text
src-tauri/src/commands/discovery/providers/semantic_scholar/
src-tauri/src/commands/discovery/providers/mod.rs
src-tauri/src/commands/discovery/mod.rs
src-tauri/src/commands/discovery/orchestrator.rs
src-tauri/src/commands/discovery/provider.rs
src-tauri/src/domain/discovery.rs
src-tauri/src/services/research/source.rs
src-tauri/src/services/research/planner.rs
src-tauri/src/services/research/agent.rs
src-tauri/src/storage/library_store.rs
```

### Concurrent Fan-Out

The orchestrator should create one async task/future per selected provider and
await them together.

Sketch:

```rust
let openalex = self.openalex.search(&openalex_request);
let arxiv = self.arxiv.search(&arxiv_request);
let results = futures::future::join_all(vec![openalex, arxiv]).await;
```

Exact implementation can use `tokio::join!`, `join_all`, or a small typed
helper. Keep it simple and readable.

### Partial Failure

The existing RFC 0043 behavior stays:

```text
OpenAlex succeeds + arXiv fails
  -> return OpenAlex results
  -> include provider_error:arxiv in filters/provenance

OpenAlex fails + arXiv succeeds
  -> return arXiv results
  -> include provider_error:openalex in filters/provenance

OpenAlex fails + arXiv fails
  -> return error
```

### Stale Provider Values

Old workspace/search state might still contain:

```text
semantic_scholar
```

The backend should sanitize provider lists:

```text
["open_alex", "semantic_scholar"] -> ["open_alex"]
["semantic_scholar"] -> ["open_alex", "arxiv"]
[] -> legacy provider fallback or OpenAlex + arXiv depending call path
```

Do not crash or fail deserialization because of old state if we can avoid it.

## Frontend Design

Discover settings should show:

```text
Providers
[x] OpenAlex
[x] arXiv
```

Remove:

```text
Semantic Scholar checkbox
```

Defaults:

```text
providers: ["open_alex", "arxiv"]
provider label: "All providers" means OpenAlex + arXiv
```

The frontend should also sanitize stale provider arrays in workspace state:

```text
drop semantic_scholar
fallback to OpenAlex + arXiv if empty
```

## Documentation Updates

- Mark RFC 0044 as superseding the Semantic Scholar part of RFC 0043.
- Update RFC 0043 implementation notes or add a short correction note saying:
  - Semantic Scholar was removed after practical API reliability issues.
  - Provider fan-out now runs concurrently.
- RFC 0031 can stay as historical context, but it should not be treated as an
  active implementation target.

## Validation

Backend tests:

```text
selected provider list drops semantic_scholar
empty/stale provider list falls back safely
orchestrator dispatches OpenAlex and arXiv concurrently
one provider failure still returns successful provider results
all provider failures return an error
deep search uses the same concurrent orchestrator
provider budget counts OpenAlex + arXiv fan-out
```

Frontend checks:

```text
Semantic Scholar no longer appears in settings
default providers are OpenAlex + arXiv
"All providers" means OpenAlex + arXiv
stale semantic_scholar workspace values are removed
```

Commands:

```bash
cargo fmt --check
cargo test discovery
cargo test research
cargo test
cargo check
pnpm check
git diff --check
```

Manual smoke:

```text
1. Run shallow search with OpenAlex + arXiv enabled.
2. Confirm runtime feels close to the slower provider, not the sum.
3. Toggle arXiv off and confirm only OpenAlex runs.
4. Toggle OpenAlex off and confirm only arXiv runs.
5. Run Deep search and confirm provider fan-out is also concurrent.
6. Confirm no Semantic Scholar UI remains.
```

## Implementation Order

1. Add RFC 0044.
2. Remove Semantic Scholar frontend provider option and defaults.
3. Remove Semantic Scholar backend enum/config/module wiring.
4. Add provider-list sanitization for stale values.
5. Parallelize provider dispatch in `DiscoveryOrchestrator`.
6. Ensure deep/improve search still uses that orchestrator.
7. Update tests.
8. Run validation.

## Future Work

- Reintroduce Semantic Scholar only if API reliability is solved.
- Add provider timing metrics to search traces.
- Stream provider results incrementally if shallow search still feels slow.
- Add provider-specific timeout settings.
