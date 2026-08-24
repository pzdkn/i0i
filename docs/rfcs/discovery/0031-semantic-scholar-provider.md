# RFC 0031: Semantic Scholar Discovery Provider

Status: Stale
Date: 2026-06-12  
Product: i0i  
Target: Tauri v2 + Svelte, macOS first

## Summary

Add Semantic Scholar as the third discovery provider alongside OpenAlex and arXiv.

Semantic Scholar's free Graph API exposes citation counts, an AI-generated `tldr` summary per paper, and an `influentialCitationCount` field — the number of citations that Semantic Scholar classifies as methodologically influential. These signals are not available from either existing provider. Coverage for CS/ML/NLP is better than OpenAlex, and responses are JSON, so no new parsing library is needed.

## Context

After RFC 0030, the provider stack looks like:

```text
DiscoverySearchRequest { provider: DiscoveryProviderChoice }
  -> search_papers() matches on provider
  -> DiscoveryService::new(providers.<choice>.clone()).search(request)
  -> normalized PaperCandidate[]
```

`DiscoveryProviderId::SemanticScholar` already exists in `provider.rs` as a stub variant. `DiscoveryProviderChoice` only has `OpenAlex` and `Arxiv` — the new `SemanticScholar` variant goes here. The `DiscoveryProviders` state struct in `mod.rs` gets a `semantic_scholar` field.

All provider config structs independently deserialize `app.conf.json` — the arXiv `AppConfig` only has an `arxiv` field in `ProvidersConfig`, tolerating the presence of other provider blocks because `serde` ignores unknown keys by default. The same pattern applies here: add `semantic_scholar` to `app.conf.json` and write a self-contained `SemanticScholarConfig::load()`.

## Goals

- Add a working Semantic Scholar provider adapter that outputs normalized `PaperCandidate` values.
- Expose `tldr` in the candidate abstract when no abstract is otherwise available.
- Expose `influentialCitationCount` as a second citation signal.
- Let the user select Semantic Scholar from the provider dropdown in the Discover UI.
- Keep the `DiscoveryProvider` trait and `DiscoveryService` unchanged.

## Non-Goals

- No multi-provider search in a single run.
- No Semantic Scholar-specific UI beyond what already exists (no "influential citations" column, no citation graph).
- No changes to the SQLite library store.
- No agentic or scheduled search.

## Semantic Scholar API Overview

Semantic Scholar exposes a public Graph API. A free optional API key (self-serve signup at `semanticscholar.org/product/api`) raises the unauthenticated rate limit of ~100 requests per 5 minutes.

```
GET https://api.semanticscholar.org/graph/v1/paper/search
```

Key query parameters:

| Parameter  | Description                                     | Example                  |
|------------|-------------------------------------------------|--------------------------|
| `query`    | Freeform text query                             | `sparse autoencoder`     |
| `limit`    | Number of results (max 100)                     | `25`                     |
| `offset`   | Pagination offset                               | `0`                      |
| `fields`   | Comma-separated field names to include          | `title,authors,year,...` |
| `year`     | Year range filter                               | `2020-2024` or `2020-`   |

The `fields` parameter limits what comes back and should always be set — omitting it returns only paper IDs. The year filter is a single string parameter, unlike OpenAlex's separate `from_publication_date` / `to_publication_date` filters.

Optional API key is passed as an `x-api-key` request header (not a query parameter).

A minimal paper object from the response looks like:

```json
{
  "paperId": "a23b4c...",
  "externalIds": { "DOI": "10.xxxx/...", "ArXiv": "2309.08600" },
  "title": "Sparse Autoencoders Find Highly Interpretable Features",
  "abstract": "We show that sparse autoencoders ...",
  "year": 2023,
  "publicationDate": "2023-09-15",
  "authors": [{ "authorId": "...", "name": "Hoagy Cunningham" }],
  "venue": "ICLR",
  "citationCount": 412,
  "influentialCitationCount": 38,
  "isOpenAccess": true,
  "openAccessPdf": { "url": "https://arxiv.org/pdf/2309.08600" },
  "tldr": { "model": "tldr@v2.0.0", "text": "Sparse autoencoders recover interpretable features." },
  "url": "https://www.semanticscholar.org/paper/..."
}
```

The `isOpenAccess` and `openAccessPdf.url` are flat on the paper object — simpler than OpenAlex's `best_oa_location` nesting.

### Fields to request

```
paperId,externalIds,title,abstract,year,publicationDate,authors,venue,
citationCount,influentialCitationCount,isOpenAccess,openAccessPdf,tldr,url
```

### Sort

Semantic Scholar's paper search has no explicit sort parameter. Results come back in relevance order only. There is no `newest` or `most_cited` sort on the search endpoint. Both `Newest` and `MostCited` sort variants fall back to relevance, similar to how `MostCited` falls back for arXiv.

### Year filter

The `year` query parameter accepts:
- `2023` — exact year
- `2020-2024` — range (inclusive)
- `2020-` — from year onwards
- `-2024` — up to year

This replaces the Lucene range syntax used by arXiv and the split date params used by OpenAlex.

### Open access filtering

All results in the discovery context are already filtered to open access via the provider contract. Semantic Scholar provides `isOpenAccess: true` on each paper, but there is no filter parameter to restrict search results to OA papers — open access is determined post-fetch from the `isOpenAccess` field. The provider should skip candidates where `isOpenAccess` is `false`.

## Provider Dispatch

Three changes to existing files:

**`src-tauri/src/domain/discovery.rs`** — add `SemanticScholar` variant:

```rust
pub enum DiscoveryProviderChoice {
    #[default]
    OpenAlex,
    Arxiv,
    SemanticScholar,
}
```

**`src-tauri/src/commands/discovery/mod.rs`** — extend `DiscoveryProviders` and the match:

```rust
pub struct DiscoveryProviders {
    pub openalex: OpenAlexProvider,
    pub arxiv: ArxivProvider,
    pub semantic_scholar: SemanticScholarProvider,
}

impl DiscoveryProviders {
    pub fn from_app_config() -> Result<Self, DiscoveryError> {
        Ok(Self {
            openalex: OpenAlexProvider::from_app_config()?,
            arxiv: ArxivProvider::from_app_config()?,
            semantic_scholar: SemanticScholarProvider::from_app_config()?,
        })
    }
}
```

The `search_papers` command gains one new match arm:

```rust
DiscoveryProviderChoice::SemanticScholar => {
    DiscoveryService::new(providers.semantic_scholar.clone())
        .search(request)
        .await
}
```

**`src-tauri/src/commands/discovery/providers/mod.rs`** — add `pub mod semantic_scholar`.

## New Files: Semantic Scholar Adapter

```text
src-tauri/src/commands/discovery/providers/semantic_scholar/
  mod.rs        re-exports SemanticScholarProvider
  config.rs     loads SemanticScholarConfig from app.conf.json
  remote.rs     JSON wire types for deserializing the API response
  search.rs     implements DiscoveryProvider for SemanticScholarProvider
  normalize.rs  converts SemanticScholarPaper -> PaperCandidate
```

### `mod.rs`

```rust
mod config;
mod normalize;
mod remote;
mod search;

pub use search::SemanticScholarProvider;
```

### `config.rs`

`SemanticScholarConfig::load()` reads from `app.conf.json` under `discovery.providers.semantic_scholar`.

The optional API key is stored as a string env-var name in config, resolved the same way OpenAlex resolves its key (env var first, `.env` file fallback). If the env-var name is empty or the var is unset, the provider runs unauthenticated rather than failing — Semantic Scholar allows anonymous requests, unlike OpenAlex.

```rust
pub(super) struct SemanticScholarConfig {
    pub(super) url: String,
    api_key_env: String,       // may be empty string
    max_result_limit: i32,
}
```

Key difference from OpenAlex: `resolve_api_key()` returns `Ok(String::new())` when the env-var is absent instead of returning an error.

### `remote.rs`

Semantic Scholar returns JSON, so `serde` deserialization with `serde_json` (already in `Cargo.toml`) is sufficient — no new dependency needed.

```rust
#[derive(Debug, Deserialize)]
pub(super) struct SemanticScholarResponse {
    pub total: Option<u32>,
    pub data: Vec<SemanticScholarPaper>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct SemanticScholarPaper {
    pub paper_id: String,
    pub external_ids: Option<ExternalIds>,
    pub title: Option<String>,
    #[serde(rename = "abstract")]
    pub abstract_text: Option<String>,
    pub year: Option<i32>,
    pub publication_date: Option<String>,
    pub authors: Option<Vec<Author>>,
    pub venue: Option<String>,
    pub citation_count: Option<i32>,
    pub influential_citation_count: Option<i32>,
    pub is_open_access: Option<bool>,
    pub open_access_pdf: Option<OpenAccessPdf>,
    pub tldr: Option<Tldr>,
    pub url: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(super) struct ExternalIds {
    #[serde(rename = "DOI")]
    pub doi: Option<String>,
    #[serde(rename = "ArXiv")]
    pub arxiv: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct Author {
    pub name: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct OpenAccessPdf {
    pub url: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct Tldr {
    pub text: Option<String>,
}
```

Note: `ExternalIds` uses `PascalCase` field names from the wire format (`DOI`, `ArXiv`) — these must match the API response exactly.

### `search.rs`

`SemanticScholarProvider` structure:

```rust
#[derive(Clone)]
pub struct SemanticScholarProvider {
    client: Client,
    config: SemanticScholarConfig,
}
```

The `search()` implementation:

1. Calls `ss_query_params()` to build query parameters.
2. Builds the URL via `build_url()`.
3. Makes a GET request. If `api_key` is non-empty, adds header `x-api-key: {key}`. Always adds `User-Agent: ioi/0.1 local Tauri Discovery`.
4. Deserializes with `serde_json` into `SemanticScholarResponse`.
5. Filters papers where `is_open_access` is `false`.
6. Maps remaining papers through `normalize_paper()`.

Sort mapping:

| `DiscoverySort` | Semantic Scholar sort | Notes                                    |
|-----------------|-----------------------|------------------------------------------|
| Relevance       | (default)             | No explicit sort param needed            |
| Newest          | (default)             | No newest sort on search endpoint        |
| MostCited       | (default)             | No citation sort on search endpoint      |

All three variants produce the same request — relevance is the only available order. The frontend disables the sort selector (or marks all options equivalent) when Semantic Scholar is selected.

Year filter construction — build the `year` param string:

```rust
fn ss_year_filter(year_from: Option<i32>, year_to: Option<i32>) -> Option<String> {
    match (year_from, year_to) {
        (None, None)         => None,
        (Some(f), None)      => Some(format!("{f}-")),
        (None, Some(t))      => Some(format!("-{t}")),
        (Some(f), Some(t))   => Some(format!("{f}-{t}")),
    }
}
```

`api_key()` returns `Ok(String::new())` when no key is configured. The `search()` implementation only sets the `x-api-key` header when the returned string is non-empty.

### `normalize.rs`

Key normalization steps:

```text
paper_id
  -> source_id = paper_id
  -> id = "semantic_scholar:{paper_id}"

external_ids.ArXiv (if present)
  -> arxiv_id

external_ids.DOI (if present)
  -> doi

title
  -> falls back to "Untitled Semantic Scholar paper"

abstract_text (if None) and tldr.text (if Some)
  -> use tldr.text as abstract when abstract is missing

authors[].name
  -> collect non-None values into Vec<String>

year, publication_date
  -> passed through directly

venue
  -> passed through as-is (journal or conference name from S2)

citation_count, influential_citation_count
  -> both passed through (influential_citation_count stored in a new field — see below)

is_open_access, open_access_pdf.url
  -> open_access: Some(OpenAccessSummary { is_open_access: true, status: Some("open_access".to_string()) })
  -> pdf_url: open_access_pdf.url

url
  -> external_url (Semantic Scholar landing page)

source_provider = "semantic_scholar"
```

`influential_citation_count` does not fit in the existing `PaperCandidate` schema. Two options:

**Option A** — ignore it at the domain level for now; surface it only in the match reasons string:
```text
reasons: ["38 influential citations (Semantic Scholar)."]
```

**Option B** — add `influential_citation_count: Option<i32>` to `PaperCandidate`.

Option A is simpler and avoids a domain change. Option B is the right long-term answer if we ever want to sort or filter by it. **This RFC takes Option A** to keep the diff small. A follow-up RFC can promote it to a first-class field.

The `CandidateMatch` for Semantic Scholar results:

```text
score: None  (S2 search gives no numeric relevance score)
reasons: formatted influential-citation string if influential_citation_count > 0
matched_keywords: tokenized query words
from_seed_paper_ids: []
```

## `PaperCandidate` domain — no changes

`PaperCandidate` is unchanged. Semantic Scholar maps cleanly onto the existing fields. The `tldr` text goes into `abstract_text` when no abstract is present. `influentialCitationCount` appears in match reasons text (Option A above).

## Config File Changes

Add `semantic_scholar` block to `app.conf.json`:

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
      },
      "semantic_scholar": {
        "url": "https://api.semanticscholar.org/graph/v1/paper/search",
        "api_key": "SEMANTIC_SCHOLAR_API_KEY"
      }
    },
    "search": {
      "max_result_limit": 30
    }
  }
}
```

`SEMANTIC_SCHOLAR_API_KEY` is the name of the env var. If the var is absent or empty, the provider runs unauthenticated — this is a soft failure, not a hard one.

## Dependency Changes

None. `serde_json` and `reqwest` are already in `Cargo.toml`.

## Frontend Changes

### `src/lib/domain/discover.ts`

Add `"semantic_scholar"` to `DiscoveryProviderChoice`:

```ts
export type DiscoveryProviderChoice = "open_alex" | "arxiv" | "semantic_scholar";
```

### `src/lib/state/library-cache.svelte.ts`

Update `candidateSignalSummary` to handle `"semantic_scholar"` in the provider label:

```ts
const providerLabel =
  candidate.sourceProvider === "arxiv" ? "arXiv"
  : candidate.sourceProvider === "semantic_scholar" ? "Semantic Scholar"
  : "OpenAlex";
```

### `src/lib/features/discover/DiscoverSeedBar.svelte`

Add a third option to the provider `<select>`:

```html
<option value="semantic_scholar">Semantic Scholar</option>
```

When `semantic_scholar` is selected:
- Disable the sort selector entirely (all options produce the same result) or grey out `newest` and `most_cited` with a note that S2 search is relevance-only.
- Show `"Semantic Scholar"` in the status chip.

The `onProviderChange` handler should reset `sortBy` to `"relevance"` when switching to Semantic Scholar, same as the arXiv reset for `"most_cited"`.

## Capability Differences Visible to the User

| Feature                    | OpenAlex              | arXiv                      | Semantic Scholar                   |
|----------------------------|-----------------------|----------------------------|------------------------------------|
| Citation counts            | Yes                   | No                         | Yes                                |
| Influential citations      | No                    | No                         | Yes (in match reasons)             |
| AI summary (tldr)          | No                    | No                         | Yes (used as abstract fallback)    |
| Relevance score            | Yes                   | No                         | No                                 |
| Sort by most cited         | Works                 | Disabled                   | Disabled                           |
| Sort by newest             | Works                 | Works                      | Disabled                           |
| Venue                      | Journal/conf name     | arXiv category (cs.LG)     | Journal/conf name                  |
| PDF URL                    | Variable (OA metadata)| Always available           | When OA PDF is indexed             |
| Coverage                   | Broad, all fields     | CS/ML/math/physics         | CS/ML/NLP/medicine — strong in ML  |
| API key required           | Yes (hard fail)       | No                         | No (optional, raises rate limit)   |

## Files Affected

Rust:

- `src-tauri/app.conf.json` — add `semantic_scholar` provider block
- `src-tauri/src/domain/discovery.rs` — add `SemanticScholar` to `DiscoveryProviderChoice`
- `src-tauri/src/commands/discovery/mod.rs` — add `semantic_scholar` field to `DiscoveryProviders`, add match arm
- `src-tauri/src/commands/discovery/providers/mod.rs` — add `pub mod semantic_scholar`
- `src-tauri/src/commands/discovery/providers/semantic_scholar/mod.rs` — new
- `src-tauri/src/commands/discovery/providers/semantic_scholar/config.rs` — new
- `src-tauri/src/commands/discovery/providers/semantic_scholar/remote.rs` — new
- `src-tauri/src/commands/discovery/providers/semantic_scholar/search.rs` — new
- `src-tauri/src/commands/discovery/providers/semantic_scholar/normalize.rs` — new

Frontend:

- `src/lib/domain/discover.ts` — add `"semantic_scholar"` to `DiscoveryProviderChoice`
- `src/lib/state/library-cache.svelte.ts` — handle `"semantic_scholar"` in provider label
- `src/lib/features/discover/DiscoverSeedBar.svelte` — add third provider option, disable sort when S2 selected

## Risks

- **`ExternalIds` field casing**: The Semantic Scholar API returns `DOI` and `ArXiv` in PascalCase within `externalIds`. The `serde` rename attributes in `remote.rs` must match the wire names exactly — a test against a fixture is essential.
- **Rate limiting without a key**: 100 requests per 5 minutes unauthenticated is enough for normal use but not for rapid repeated runs. The error path in `search.rs` should surface 429 responses clearly and suggest adding an API key.
- **Open access filtering post-fetch**: Unlike OpenAlex (server-side `is_oa:true` filter) and arXiv (always OA by definition), Semantic Scholar has no request-time OA filter. Papers where `isOpenAccess: false` must be filtered out in the provider before returning candidates. This means `result_limit` is a request count, not a result count — the caller may get fewer candidates than requested.
- **`tldr` as abstract fallback**: Many older or niche papers have no `tldr`. The normalizer must handle `None` gracefully and not treat an absent tldr as a failure.
- **Venue field inconsistency**: Semantic Scholar's `venue` can be an empty string `""` or a short abbreviation. Apply the same `filter(|v| !v.is_empty())` guard used in OpenAlex normalization.
- **`influentialCitationCount` not in domain type**: Option A (reasons text) loses the signal if we later want to sort by it. Track this as a known shortcut in the commit message so it can be promoted cleanly.

## Validation Plan

```bash
cargo fmt --check
cargo clippy
cargo test
pnpm check
pnpm build
```

Unit tests in `remote.rs`:

- Full paper object deserializes correctly including nested `externalIds`, `tldr`, `openAccessPdf`.
- Paper with all optional fields absent deserializes without panic.
- `ExternalIds` reads `DOI` and `ArXiv` from PascalCase wire names.

Unit tests in `normalize.rs`:

- `tldr.text` is used as `abstract_text` when `abstract` is `None`.
- `tldr.text` is NOT used when `abstract` is already present.
- Paper with `isOpenAccess: false` is excluded from the result set.
- `influential_citation_count > 0` produces a reasons entry.
- `influential_citation_count == 0` or `None` produces no reasons entry.
- `source_id` and `id` derive correctly from `paperId`.
- Authors with `name: None` are silently skipped.
- Empty venue string is treated as `None`.

Unit tests in `search.rs`:

- `ss_year_filter` produces correct strings for all four cases (both, from-only, to-only, neither).
- `ss_query_params` includes `fields` and `limit` in the output.
- All three `DiscoverySort` variants produce a request with no `sort` parameter.

Integration (manual):

- Select Semantic Scholar in Discover UI.
- Run a query; verify candidates appear with title, abstract (or tldr), authors, venue, year, citation count.
- Verify a paper with a known arXiv ID shows the arXiv PDF URL.
- Verify "sort by newest" and "sort by most cited" are disabled.
- Verify status chip shows "Semantic Scholar".
- Switch back to OpenAlex and arXiv — verify both still work correctly.
- Run without `SEMANTIC_SCHOLAR_API_KEY` set — verify the provider works unauthenticated and does not error.

## Open Questions

- Should the sort selector be fully hidden when Semantic Scholar is active, or shown but greyed out? Greyed out is more transparent — the user can see what they're giving up. Prefer greyed-out with a tooltip ("Semantic Scholar search is relevance-only").
- Should `influential_citation_count` be promoted to `PaperCandidate` immediately, or deferred to a follow-up RFC? This RFC defers it (Option A). If the signal proves useful in the UI, RFC 0032 can promote it.
- Should the post-fetch OA filter log a debug message when papers are dropped? Useful for diagnosing low result counts. Add a `tracing::debug!` call in `search.rs` when papers are filtered out.
