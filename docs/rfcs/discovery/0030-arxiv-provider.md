# RFC 0030: arXiv Discovery Provider

Status: Stale
Date: 2026-06-12  
Product: i0i  
Target: Tauri v2 + Svelte, macOS first

## Summary

Add arXiv as the second discovery provider alongside OpenAlex.

arXiv is the dominant open-access preprint repository for CS, ML, math, physics, and adjacent fields — the exact papers i0i's primary users care most about. Many important ML results appear on arXiv before (or instead of) a peer-reviewed venue, so discovery without arXiv has a meaningful gap.

This RFC defines the full backend adapter, the small provider-dispatch change needed to route requests to the right provider, and the minimal frontend addition of a provider selector.

## Context

The existing provider stack after RFC 0020 is:

```text
DiscoverySearchRequest
  -> DiscoveryService<OpenAlexProvider>
  -> OpenAlexProvider::search()
  -> normalized PaperCandidate[]
```

`DiscoveryProviderId` already has an `Arxiv` variant defined in `provider.rs`, but no adapter exists. The `DiscoverySearchRequest` domain type has no `provider` field yet — the service is hardcoded to OpenAlex via `AppDiscoveryService = DiscoveryService<OpenAlexProvider>` in `mod.rs`.

## Goals

- Add a working arXiv provider adapter that outputs normalized `PaperCandidate` values.
- Let the user switch between OpenAlex and arXiv in the Discover UI.
- Keep the provider contract (`DiscoveryProvider` trait) unchanged.
- Keep `DiscoveryService` unchanged.
- Add as little glue as possible to dispatch the request to the right provider.

## Non-Goals

- No multi-provider search in a single run (that is a later slice).
- No arXiv category browser or faceted search UI.
- No full Atom XML validation beyond what deserialization covers.
- No changes to the SQLite library store.
- No agentic or scheduled search.

## arXiv API Overview

arXiv exposes a public Atom XML search API. No API key or authentication is required.

```
GET https://export.arxiv.org/api/query
```

Key query parameters:

| Parameter       | Description                                          | Example                          |
|-----------------|------------------------------------------------------|----------------------------------|
| `search_query`  | Freeform or field-prefixed query                     | `all:sparse+autoencoder`         |
| `start`         | Result offset (0-based)                              | `0`                              |
| `max_results`   | Number of results requested                          | `25`                             |
| `sortBy`        | `relevance`, `lastUpdatedDate`, `submittedDate`      | `relevance`                      |
| `sortOrder`     | `ascending` or `descending`                          | `descending`                     |

Field prefixes for `search_query`:

| Prefix | Scope               |
|--------|---------------------|
| `all`  | All fields          |
| `ti`   | Title               |
| `au`   | Author              |
| `abs`  | Abstract            |
| `cat`  | arXiv category      |

Example request for "sparse autoencoder", last 25 results, sorted by submission date:

```
https://export.arxiv.org/api/query?search_query=all:sparse+autoencoder&start=0&max_results=25&sortBy=submittedDate&sortOrder=descending
```

The response is an Atom XML feed. Each `<entry>` maps to one paper.

A minimal parsed entry looks like:

```xml
<entry>
  <id>http://arxiv.org/abs/2309.08600v2</id>
  <title>Sparse Autoencoders Find Highly Interpretable Features in Language Models</title>
  <summary>We show that...</summary>
  <published>2023-09-15T17:48:05Z</published>
  <updated>2023-10-01T12:00:00Z</updated>
  <author><name>Hoagy Cunningham</name></author>
  <author><name>Aidan Beren</name></author>
  <category term="cs.LG" scheme="http://arxiv.org/schemas/atom"/>
  <link rel="alternate" href="https://arxiv.org/abs/2309.08600v2" type="text/html"/>
  <link title="pdf" href="https://arxiv.org/pdf/2309.08600v2" rel="related" type="application/pdf"/>
  <arxiv:primary_category term="cs.LG" xmlns:arxiv="http://arxiv.org/schemas/atom"/>
  <arxiv:doi xmlns:arxiv="http://arxiv.org/schemas/atom">10.xxxx/xxxxx</arxiv:doi>
</entry>
```

Key extraction rules for normalization:

- **arXiv ID**: strip `http://arxiv.org/abs/` from `<id>`, then strip the version suffix (e.g., `v2`). Result: `2309.08600`.
- **PDF URL**: `https://arxiv.org/pdf/{arxiv_id}` — constructed from the ID, not from the link element, to avoid version pinning.
- **Landing URL**: `https://arxiv.org/abs/{arxiv_id}` — also constructed from the ID.
- **Year**: parse the 4-digit year from `<published>`.
- **Publication date**: truncate `<published>` to `YYYY-MM-DD`.
- **Venue**: use the `term` attribute of `<arxiv:primary_category>` (e.g., `cs.LG`). arXiv papers do not have a journal venue.
- **Citation count**: not available from the arXiv API — populate as `None`.
- **Open access**: all arXiv papers are open access — always `is_open_access: true`, status `"green"`.

## Provider Dispatch

The current service type alias is hardcoded:

```rust
// src-tauri/src/commands/discovery/mod.rs
pub type AppDiscoveryService = DiscoveryService<OpenAlexProvider>;
```

To route requests to the right provider without changing `DiscoveryService`, introduce a `ProviderDispatch` enum that implements `DiscoveryProvider` and delegates to the concrete adapter chosen by the request.

### Changes to `DiscoverySearchRequest`

Add a `provider` field:

```rust
// src-tauri/src/domain/discovery.rs
pub struct DiscoverySearchRequest {
    pub query: String,
    pub year_from: Option<i32>,
    pub year_to: Option<i32>,
    pub result_limit: i32,
    pub sort_by: DiscoverySort,
    pub open_access_only: bool,
    pub provider: DiscoveryProviderChoice,   // <-- new
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiscoveryProviderChoice {
    OpenAlex,
    Arxiv,
}

impl Default for DiscoveryProviderChoice {
    fn default() -> Self {
        Self::OpenAlex
    }
}
```

`DiscoveryProviderChoice` mirrors `DiscoveryProviderId` but is a serializable domain value the frontend passes in. The dispatch layer converts it to the right concrete provider.

### `DiscoveryProviders` state and `mod.rs`

Both providers are initialized **once at app startup** and stored in Tauri's managed state. This preserves the `reqwest::Client` connection pool inside each provider across multiple search calls, which is the correct `reqwest` usage pattern.

Introduce `DiscoveryProviders` as the new state type:

```rust
// src-tauri/src/commands/discovery/mod.rs

/// Both provider adapters, initialized once and shared across all search calls.
///
/// Each provider holds its own `reqwest::Client`, which maintains a connection
/// pool. Storing providers here instead of constructing them per-request lets
/// that pool survive between searches.
pub struct DiscoveryProviders {
    pub openalex: OpenAlexProvider,
    pub arxiv: ArxivProvider,
}

impl DiscoveryProviders {
    pub fn from_app_config() -> Result<Self, DiscoveryError> {
        Ok(Self {
            openalex: OpenAlexProvider::from_app_config()?,
            arxiv: ArxivProvider::from_app_config()?,
        })
    }
}
```

The command borrows the right provider from state based on the request:

```rust
#[tauri::command]
pub async fn search_papers(
    providers: tauri::State<'_, DiscoveryProviders>,
    request: DiscoverySearchRequest,
) -> Result<DiscoverySearchResponse, String> {
    match request.provider {
        DiscoveryProviderChoice::OpenAlex => {
            DiscoveryService::new(&providers.openalex)
                .search(request)
                .await
        }
        DiscoveryProviderChoice::Arxiv => {
            DiscoveryService::new(&providers.arxiv)
                .search(request)
                .await
        }
    }
    .map_err(|e| e.to_string())
}
```

`DiscoveryService` is a thin wrapper created per-call — only the provider reference is borrowed from state, not cloned. The connection pools inside each provider live for the full app session.

This requires `DiscoveryService` to accept a reference: `DiscoveryService::new(&providers.openalex)`. The generic bound on `DiscoveryService<P>` should accommodate `P: DiscoveryProvider` where `P` can be a reference type. If the current bound does not allow this, changing it to `P: AsRef<dyn DiscoveryProvider>` or accepting `&'a impl DiscoveryProvider` are both valid options — the exact lifetime choice belongs to implementation.

**Registration in `lib.rs`:**

```rust
.manage(
    DiscoveryProviders::from_app_config()
        .expect("Failed to initialize discovery providers")
)
```

The `DiscoveryProviders` type replaces the old `AppDiscoveryService` type alias, which can be removed.

## New Files: arXiv Adapter

```text
src-tauri/src/commands/discovery/providers/arxiv/
  mod.rs        re-exports ArxivProvider
  config.rs     loads ArxivConfig from app.conf.json
  remote.rs     Atom XML wire types (deserialized via quick-xml + serde)
  search.rs     implements DiscoveryProvider for ArxivProvider
  normalize.rs  converts ArxivEntry -> PaperCandidate
```

### `mod.rs`

```rust
mod config;
mod normalize;
mod remote;
mod search;

pub use search::ArxivProvider;
```

### `config.rs`

arXiv requires no API key. The config only stores the base URL.

`ArxivConfig::load()` reads from `app.conf.json` under `discovery.providers.arxiv`.

`api_key()` on `ArxivProvider` returns `Ok(String::new())` and `search()` never passes it to the request.

### `remote.rs`

Atom XML wire types for deserialization with `quick-xml` and its `serde` feature:

```rust
#[derive(Debug, Deserialize)]
struct ArxivFeed {
    #[serde(rename = "entry", default)]
    entries: Vec<ArxivEntry>,
}

#[derive(Debug, Deserialize)]
struct ArxivEntry {
    id: String,
    title: String,
    summary: Option<String>,
    published: Option<String>,
    updated: Option<String>,
    #[serde(rename = "author", default)]
    authors: Vec<ArxivAuthor>,
    #[serde(rename = "link", default)]
    links: Vec<ArxivLink>,
    #[serde(rename = "primary_category")]
    primary_category: Option<ArxivCategory>,
    doi: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ArxivAuthor {
    name: String,
}

#[derive(Debug, Deserialize)]
struct ArxivLink {
    #[serde(rename = "@rel")]
    rel: Option<String>,
    #[serde(rename = "@href")]
    href: Option<String>,
    #[serde(rename = "@title")]
    title: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ArxivCategory {
    #[serde(rename = "@term")]
    term: Option<String>,
}
```

Note on `quick-xml`: XML namespaces (e.g., `arxiv:primary_category`, `arxiv:doi`) require namespace-aware deserialization or stripping. The implementation should handle this robustly — either by using `quick-xml`'s `NsReader` or by treating the prefixed elements as plain local names after namespace stripping.

### `search.rs`

The `search()` implementation:

1. Calls `arxiv_query_params()` to build the query string.
2. Builds the URL via `build_url()`.
3. Makes a GET request with a `User-Agent` header (`ioi/0.1 local Tauri Discovery`).
4. Reads the response body as UTF-8 text.
5. Deserializes with `quick_xml::de::from_str::<ArxivFeed>()`.
6. Maps entries through `normalize_entry()`.

arXiv sort mapping:

| `DiscoverySort` | arXiv `sortBy`    | arXiv `sortOrder` |
|-----------------|-------------------|-------------------|
| Relevance       | `relevance`       | `descending`      |
| Newest          | `submittedDate`   | `descending`      |
| MostCited       | `relevance`       | `descending`      |

`MostCited` maps to relevance because arXiv has no citation-count sort. The normalizer will set `citation_count: None` for all arXiv results, so the UI should not surface "sort by most cited" as a meaningful option for arXiv — the frontend can disable or grey it out when arXiv is selected.

arXiv `open_access_only` filter: all arXiv results are open access by definition, so when `open_access_only` is `true`, no additional filter parameter is needed. When `open_access_only` is `false`, the results are unchanged — still all open access. The normalizer always sets `is_open_access: true`.

Year filters are applied by date range using `submittedDate` in the query:

```
search_query=all:sparse+autoencoder+AND+submittedDate:[20230101+TO+20261231]
```

This embeds the date range into the query string because arXiv has no separate filter parameter for year range.

### `normalize.rs`

Key normalization steps:

```text
<id> field
  -> strip "http://arxiv.org/abs/" prefix
  -> strip version suffix (e.g., "v2")
  -> result: "2309.08600"
  -> source_id = "2309.08600"
  -> id = "arxiv:2309.08600"
  -> pdf_url = "https://arxiv.org/pdf/2309.08600"
  -> external_url = "https://arxiv.org/abs/2309.08600"

<published> field
  -> parse year (first 4 characters)
  -> parse date (first 10 characters, "YYYY-MM-DD")

<arxiv:primary_category term="cs.LG"/>
  -> venue = "cs.LG"

<author><name>...</name></author>
  -> authors: Vec<String>

<summary>
  -> abstract_text

open_access always:
  -> is_open_access: true, status: "green"

citation_count: None (arXiv does not provide this)
```

The `CandidateMatch` for arXiv results:

```text
score: None  (arXiv gives no relevance score)
reasons: []  (empty — RFC 0022 already deprecates sentence-style reasons)
matched_keywords: tokenized query words
from_seed_paper_ids: []
```

## Config File Changes

Add arXiv to `app.conf.json`:

```json
{
  "discovery": {
    "providers": {
      "openalex": {
        "url": "https://api.openalex.org/works",
        "api_key": "OPENALEX_API_KEY"
      },
      "arxiv": {
        "url": "https://export.arxiv.org/api/query"
      }
    },
    "search": {
      "max_result_limit": 30
    }
  }
}
```

`ArxivConfig` reads `discovery.providers.arxiv.url`. No `api_key` field is needed.

The config struct in `config.rs` mirrors the OpenAlex pattern but with no key field:

```rust
pub(super) struct ArxivConfig {
    pub(super) url: String,
    max_result_limit: i32,
}
```

The `AppConfig` deserializer must be updated to include the `arxiv` field alongside `openalex` under `providers`.

## Dependency Change

Add `quick-xml` with serde support to `src-tauri/Cargo.toml`:

```toml
quick-xml = { version = "0.37", features = ["serialize"] }
```

`quick-xml` is the standard choice for XML deserialization in Rust. The `serialize` feature enables `serde::Deserialize` on Atom feed structs, matching the pattern already used for OpenAlex JSON.

## Frontend Changes

### Domain type update

Add `provider` to `DiscoverySearchRequest` in `src/lib/domain/discover.ts`:

```ts
export type DiscoveryProviderChoice = "open_alex" | "arxiv";

export type DiscoverySearchRequest = {
  query: string;
  yearFrom?: number;
  yearTo?: number;
  resultLimit: number;
  sortBy: DiscoverySort;
  openAccessOnly: boolean;
  provider: DiscoveryProviderChoice;
};
```

Default request should include `provider: "open_alex"`.

### Provider selector in `DiscoverSeedBar.svelte`

Add a small provider toggle to the search controls row, alongside the existing sort and limit controls:

```text
[ query input ] [Run]
[ year from ] [ year to ] [ limit 25 ] [ relevance ] [x] open access  [ OpenAlex ▼ ]
```

The selector shows `OpenAlex` and `arXiv`. Selecting `arXiv` should:
- Switch the provider in the local workspace state.
- Grey out or disable the "Most cited" sort option, since arXiv returns no citation counts.
- Keep all other controls active.

The chip-style status bar (RFC 0022) should also update to show the active provider.

## Capability Differences Visible to the User

| Feature                 | OpenAlex         | arXiv                     |
|-------------------------|------------------|---------------------------|
| Citation counts         | Yes              | No (always empty)         |
| Relevance score         | Yes              | No (score hidden)         |
| "Sort by most cited"    | Works            | Disabled (falls back to relevance) |
| Open access filter      | Meaningful       | Meaningless (all are OA) |
| Venue                   | Journal/conf name | arXiv category (e.g. cs.LG) |
| Coverage                | Broad, all fields | CS/ML/math/physics/quant  |
| PDF URL                 | Variable (from OA metadata) | Always available |

## Files Affected

Rust:

- `src-tauri/Cargo.toml` — add `quick-xml`
- `src-tauri/app.conf.json` — add `arxiv` provider block
- `src-tauri/src/domain/discovery.rs` — add `provider` field and `DiscoveryProviderChoice` enum
- `src-tauri/src/commands/discovery/mod.rs` — add `DiscoveryProviders` state struct, update `search_papers` command, remove `AppDiscoveryService` alias
- `src-tauri/src/commands/discovery/providers/mod.rs` — declare `pub mod arxiv`
- `src-tauri/src/commands/discovery/providers/arxiv/mod.rs` — new
- `src-tauri/src/commands/discovery/providers/arxiv/config.rs` — new
- `src-tauri/src/commands/discovery/providers/arxiv/remote.rs` — new
- `src-tauri/src/commands/discovery/providers/arxiv/search.rs` — new
- `src-tauri/src/commands/discovery/providers/arxiv/normalize.rs` — new
- `src-tauri/src/commands/discovery/providers/openalex/config.rs` — extend `AppConfig` to include `arxiv` provider block

Frontend:

- `src/lib/domain/discover.ts` — add `provider` field and `DiscoveryProviderChoice` type
- `src/lib/features/discover/DiscoverSeedBar.svelte` — add provider selector
- `src/lib/features/discover/DiscoverView.svelte` — pass `provider` in the request, handle "most cited" disabling for arXiv

## Risks

- **XML namespace handling**: `quick-xml`'s serde support can be tricky with XML namespaces. The `arxiv:` prefix on `primary_category` and `doi` elements requires testing against real API responses. If namespace stripping is necessary, that logic belongs in `remote.rs`.
- **arXiv rate limiting**: arXiv requests 3 seconds between automated requests. For single-user desktop use this is not normally an issue, but repeated fast runs could trigger 429 responses. The error path in `search.rs` should surface rate-limit errors clearly.
- **Year filter in query string**: Embedding date ranges in the `search_query` field is the only supported way in the arXiv API. If the query also uses Boolean operators, the date filter must be added with `AND`. This combination needs a test.
- **"Most cited" sort in arXiv**: must be greyed out in the UI, not silently remapped, to avoid confusing the user.
- **Version suffix in arXiv IDs**: the `<id>` field includes version suffixes (e.g., `v2`). The normalizer must strip these to derive a stable canonical ID and consistent PDF URL.

## Validation Plan

```bash
cargo fmt --check
cargo clippy
cargo test
pnpm check
pnpm build
```

Unit tests in `normalize.rs`:

- arXiv ID extraction strips prefix and version suffix correctly.
- Year and date parsing from `<published>` field.
- Author list extraction.
- PDF URL construction from arXiv ID.
- Year filter is embedded in `search_query` string when `year_from`/`year_to` are set.
- `open_access_only: true` adds no arXiv-specific filter (arXiv is always OA).

Unit test in `search.rs`:

- `arxiv_query_params` produces correct parameter values for all three `DiscoverySort` variants.
- `MostCited` maps to `sortBy=relevance` (no silent behavior difference).

Integration (manual):

- Select arXiv provider in Discover UI.
- Enter a query and click Run.
- Verify candidates appear with title, authors, abstract, year, arXiv category as venue.
- Verify PDF URL opens the correct arXiv PDF page.
- Verify "sort by most cited" is disabled when arXiv is selected.
- Verify the status chip shows `arXiv` not `OpenAlex`.
- Switch back to OpenAlex and confirm OpenAlex search still works.
- Verify two Discover tabs can each hold a different provider with independent results.

## Open Questions

- Should `open_access_only` be hidden entirely in the UI when arXiv is selected, or shown but greyed out with a note? Greyed out with a tooltip ("All arXiv papers are open access") is probably clearest.
- Should the year filter for arXiv use `submittedDate` (when first posted) or `lastUpdatedDate`? `submittedDate` is the more semantically correct choice for "published in year X" — use it.
- Should the arXiv provider hard-fail if `OPENALEX_API_KEY` is missing, or succeed (arXiv needs no key)? Answer: arXiv should succeed with no key. The missing-key error should only fire for OpenAlex.
