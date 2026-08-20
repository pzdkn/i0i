//! The `Planner` seam: the LLM-backed plan, reflect, and rank primitives.
//!
//! The real implementation talks to OpenRouter and parses JSON tolerantly; the
//! loop depends on the trait so tests can substitute a fake. Candidates are
//! never invented: the planner ranks by index into a provided candidate list,
//! so it can only reorder or score real provider results.

use async_trait::async_trait;
use reqwest::Client;
use serde::Deserialize;

use crate::domain::discovery::{DiscoveryProviderChoice, PaperCandidate};
use crate::domain::research::{RankedCandidate, SearchConstraints};
use crate::domain::vault_suggestion::VaultSuggestionQueryPath;
use crate::services::llm::{self, CompletionRequest, WireMessage};
use crate::services::research::budget::BudgetRemaining;
use crate::services::research::error::ResearchError;

/// One planned provider query.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Query {
    #[serde(
        default,
        deserialize_with = "crate::domain::discovery::deserialize_provider_lenient"
    )]
    pub provider: DiscoveryProviderChoice,
    pub text: String,
}

/// One pool entry as `reflect` sees it (RFC 0088 R6.3).
///
/// Title, year, venue and — when the reranker is available — the semantic score
/// that already accounts for the abstract. Roughly twenty tokens.
#[derive(Debug, Clone, PartialEq)]
pub struct PoolEntry {
    pub title: String,
    pub year: Option<i32>,
    pub venue: Option<String>,
    pub semantic_score: Option<f64>,
}

/// A candidate shown to `reflect` in full, abstract included.
#[derive(Debug, Clone, PartialEq)]
pub struct CandidateDetail {
    pub title: String,
    pub year: Option<i32>,
    pub abstract_text: String,
}

/// Computed coverage, not narrated (RFC 0088 R6.3).
///
/// A score distribution is a harder signal than a model's impression of a title
/// list, and it costs nothing to produce — the reranker has already read the
/// abstracts.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct CoverageStats {
    pub total: usize,
    /// Candidates below `SEMANTIC_FLOOR` — present but probably off-topic.
    pub below_floor: usize,
    pub mean_score: Option<f64>,
    pub year_min: Option<i32>,
    pub year_max: Option<i32>,
}

/// What `reflect` is shown of the pool.
///
/// Three tiers, because the three questions differ: coverage is answerable from
/// titles, keep-or-drop needs the abstract, and "how good is this pool overall"
/// is arithmetic. RFC 0088 §6.3 argues the whole case; the short version is that
/// `candidate_embed_text` already embeds title + abstract, so sending abstracts
/// to the model pays it to re-derive a signal we computed locally for free.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PoolSummary {
    pub entries: Vec<PoolEntry>,
    /// Abstracts for uncertain candidates, bounded by the research loop.
    pub detailed: Vec<CandidateDetail>,
    pub coverage: CoverageStats,
}

/// The planner's read on the pool after a round (RFC 0088 R1.1).
///
/// Replaces `Assessment`: one call now answers "is this enough", "what is
/// missing" and "what would close the gap", which were previously two calls
/// with the gaps passed between them as loose strings.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Reflection {
    /// Whether another round is warranted. `false` => converged.
    pub should_continue: bool,
    #[serde(default)]
    pub gaps: Vec<String>,
    #[serde(default)]
    pub next_queries: Vec<Query>,
}

/// Room for a JSON object holding a handful of queries, and no more.
const PLANNER_MAX_TOKENS: u32 = 1_024;

#[async_trait]
pub trait Planner: Send + Sync {
    /// Expand a research goal into the first round of provider queries.
    async fn plan_queries(
        &self,
        goal: &str,
        constraints: &SearchConstraints,
    ) -> Result<Vec<Query>, ResearchError>;

    /// RFC 0088 R1.1/R6.1: one call that decides whether to continue, names the
    /// gaps, and writes the queries that would close them. Replaces the
    /// `assess` + `refine_queries` pair, which cost two round trips to reach the
    /// same place and left the continue decision implicit.
    async fn reflect(
        &self,
        goal: &str,
        pool: &PoolSummary,
        remaining: &BudgetRemaining,
    ) -> Result<Reflection, ResearchError>;

    /// Rank retrieved candidates without creating new candidate identities.
    async fn rank(
        &self,
        goal: &str,
        candidates: &[PaperCandidate],
    ) -> Result<Vec<RankedCandidate>, ResearchError>;
}

// --- JSON DTOs for parsing planner responses ---

#[derive(Debug, Deserialize)]
struct QueriesResponse {
    #[serde(default)]
    queries: Vec<Query>,
}

#[derive(Debug, Deserialize)]
struct SuggestionQueriesResponse {
    #[serde(default)]
    queries: Vec<SuggestionQueryItem>,
}

#[derive(Debug, Deserialize)]
struct SuggestionQueryItem {
    intent: String,
    query: String,
}

#[derive(Debug, Deserialize)]
struct RankResponse {
    #[serde(default)]
    ranked: Vec<RankedItem>,
}

#[derive(Debug, Deserialize)]
struct RankedItem {
    index: usize,
    #[serde(default)]
    score: Option<f64>,
    #[serde(default)]
    rationale: Option<String>,
}

/// Extract a single JSON object from possibly-noisy model output: tolerate
/// surrounding prose and ```json code fences by slicing the first `{` to the
/// last `}`. This is the primary path (no reliance on provider JSON mode).
pub fn extract_json_object(raw: &str) -> Result<serde_json::Value, ResearchError> {
    let start = raw.find('{');
    let end = raw.rfind('}');
    match (start, end) {
        (Some(s), Some(e)) if e >= s => serde_json::from_str(&raw[s..=e])
            .map_err(|err| ResearchError::new(format!("JSON parse failed: {err}"))),
        _ => Err(ResearchError::new("No JSON object found in model output")),
    }
}

/// Map ranked items back onto the real candidates by index, dropping any
/// out-of-range indices the model might emit (it cannot invent papers). Ranks
/// are assigned 1..N in the model's order.
fn ranked_from_indices(
    candidates: &[PaperCandidate],
    items: Vec<RankedItem>,
) -> Vec<RankedCandidate> {
    items
        .into_iter()
        .filter_map(|item| {
            candidates.get(item.index).map(|candidate| RankedCandidate {
                candidate: candidate.clone(),
                rank: 0,
                score: item.score,
                rationale: item.rationale,
                rank_signals_json: None,
                provider_hits_json: None,
            })
        })
        .enumerate()
        .map(|(i, mut rc)| {
            rc.rank = (i + 1) as i32;
            rc
        })
        .collect()
}

/// Real planner backed by OpenRouter.
pub struct OpenRouterPlanner {
    client: Client,
    url: String,
    api_key: String,
    model: String,
}

impl OpenRouterPlanner {
    pub fn new(client: Client, url: String, api_key: String, model: String) -> Self {
        Self {
            client,
            url,
            api_key,
            model,
        }
    }

    /// One JSON round-trip: send system+user, parse tolerantly, and on parse
    /// failure re-ask once with the parser error appended.
    async fn complete_json(
        &self,
        system: &str,
        user: &str,
    ) -> Result<serde_json::Value, ResearchError> {
        let first = self.send(system, user).await?;
        match extract_json_object(&first) {
            Ok(value) => Ok(value),
            Err(parse_err) => {
                let retry_user = format!(
                    "{user}\n\nYour previous reply could not be parsed as JSON \
                     ({parse_err}). Reply with a single valid JSON object only."
                );
                let second = self.send(system, &retry_user).await?;
                extract_json_object(&second)
            }
        }
    }

    async fn send(&self, system: &str, user: &str) -> Result<String, ResearchError> {
        let request = CompletionRequest {
            model: self.model.clone(),
            messages: vec![
                WireMessage::text("system", system.to_string()),
                WireMessage::text("user", user.to_string()),
            ],
            stream: false,
            // A planner reply is a small JSON object of queries. Unset,
            // OpenRouter reserves the model's full completion ceiling and can
            // 402 a request that would have cost a fraction of a cent.
            max_tokens: Some(PLANNER_MAX_TOKENS),
            response_format: None,
            tools: None,
            tool_choice: None,
        };
        llm::complete(&self.client, &self.url, &self.api_key, &request)
            .await
            .map_err(ResearchError::new)
    }

    /// Propose a fixed number of distinct, provider-neutral vault search paths.
    pub async fn plan_vault_suggestion_queries(
        &self,
        profile: &str,
        count: u8,
    ) -> Result<Vec<VaultSuggestionQueryPath>, ResearchError> {
        let user =
            format!("Vault profile:\n{profile}\n\nPropose exactly {count} distinct query paths.");
        let value = self.complete_json(SUGGESTION_PLAN_SYSTEM, &user).await?;
        let parsed: SuggestionQueriesResponse = serde_json::from_value(value)
            .map_err(|error| ResearchError::new(format!("suggestion query shape: {error}")))?;
        normalize_suggestion_queries(parsed.queries, count)
    }
}

const PLAN_SYSTEM: &str = "You are a scholarly search planner. Expand the user's research \
goal into focused provider queries (synonyms, key methods, datasets). Reply with a single \
JSON object: {\"queries\":[{\"provider\":\"open_alex\"|\"arxiv\",\"text\":\"...\"}]}. No prose.";

const SUGGESTION_PLAN_SYSTEM: &str = "You prepare a small search agenda for a research vault. \
Each path must cover a distinct method, application, or open question from the vault rather than \
paraphrasing one broad topic. Write concise provider-neutral scholarly queries. Reply with one JSON \
object: {\"queries\":[{\"intent\":\"short human-readable angle\",\"query\":\"search query\"}]}. \
Return exactly the requested count. No prose.";

const REFLECT_SYSTEM: &str = "You review a paper search in progress. Decide whether the \
pool answers the goal, name what is still missing, and write the queries that would close \
those gaps. Reply with a single JSON object: {\"should_continue\":true|false,\
\"gaps\":[\"...\"],\"next_queries\":[{\"provider\":\"open_alex\"|\"arxiv\",\"text\":\"...\"}]}. \
Set should_continue=false when coverage is sufficient, and leave next_queries empty then. \
Judge coverage from the titles and the score summary; the detailed entries are the \
borderline cases. No prose.";

const RANK_SYSTEM: &str = "You rank candidate papers by fit to the goal. You are given a \
numbered list; reply with a single JSON object {\"ranked\":[{\"index\":N,\"score\":0..1,\
\"rationale\":\"one line\"}]} referencing only the given indices, best first. Never invent \
papers. No prose.";

#[async_trait]
impl Planner for OpenRouterPlanner {
    async fn plan_queries(
        &self,
        goal: &str,
        constraints: &SearchConstraints,
    ) -> Result<Vec<Query>, ResearchError> {
        let providers: Vec<&str> = constraints.providers.iter().map(provider_name).collect();
        let user = format!(
            "Goal: {goal}\nAllowed providers: {}\nPropose 3-5 queries.",
            if providers.is_empty() {
                "open_alex, arxiv".to_string()
            } else {
                providers.join(", ")
            }
        );
        let value = self.complete_json(PLAN_SYSTEM, &user).await?;
        let parsed: QueriesResponse = serde_json::from_value(value)
            .map_err(|e| ResearchError::new(format!("plan_queries shape: {e}")))?;
        Ok(parsed.queries)
    }

    async fn reflect(
        &self,
        goal: &str,
        pool: &PoolSummary,
        remaining: &BudgetRemaining,
    ) -> Result<Reflection, ResearchError> {
        let user = render_reflect_prompt(goal, pool, remaining);
        let value = self.complete_json(REFLECT_SYSTEM, &user).await?;
        serde_json::from_value(value).map_err(|e| ResearchError::new(format!("reflect shape: {e}")))
    }

    async fn rank(
        &self,
        goal: &str,
        candidates: &[PaperCandidate],
    ) -> Result<Vec<RankedCandidate>, ResearchError> {
        if candidates.is_empty() {
            return Ok(Vec::new());
        }
        let listing = candidates
            .iter()
            .enumerate()
            .map(|(i, c)| {
                format!(
                    "{i}. {} ({}) — {}",
                    c.title,
                    c.year.map(|y| y.to_string()).unwrap_or_default(),
                    c.abstract_text
                        .as_deref()
                        .unwrap_or("")
                        .chars()
                        .take(280)
                        .collect::<String>()
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        let user = format!("Goal: {goal}\nCandidates:\n{listing}");
        let value = self.complete_json(RANK_SYSTEM, &user).await?;
        let parsed: RankResponse = serde_json::from_value(value)
            .map_err(|e| ResearchError::new(format!("rank shape: {e}")))?;
        Ok(ranked_from_indices(candidates, parsed.ranked))
    }
}

fn provider_name(choice: &DiscoveryProviderChoice) -> &'static str {
    match choice {
        DiscoveryProviderChoice::OpenAlex => "open_alex",
        DiscoveryProviderChoice::Arxiv => "arxiv",
        DiscoveryProviderChoice::EuropePmc => "europe_pmc",
        DiscoveryProviderChoice::Core => "core",
    }
}

fn normalize_suggestion_queries(
    items: Vec<SuggestionQueryItem>,
    count: u8,
) -> Result<Vec<VaultSuggestionQueryPath>, ResearchError> {
    let mut seen = std::collections::HashSet::new();
    let queries: Vec<VaultSuggestionQueryPath> = items
        .into_iter()
        .filter_map(|item| {
            let query = item.query.trim().to_string();
            let intent = item.intent.trim().to_string();
            if query.is_empty() || intent.is_empty() || !seen.insert(query.to_lowercase()) {
                return None;
            }
            Some((intent, query))
        })
        .take(count as usize)
        .enumerate()
        .map(|(index, (intent, query))| VaultSuggestionQueryPath {
            id: format!("path-{}", index + 1),
            intent,
            query,
        })
        .collect();
    if queries.len() != count as usize {
        return Err(ResearchError::new(format!(
            "Planner returned {} distinct queries; expected {count}",
            queries.len()
        )));
    }
    Ok(queries)
}

/// Render the three tiers of `PoolSummary` into one prompt (RFC 0088 R6.3).
///
/// Kept as a free function so it can be asserted on directly: the shape of what
/// `reflect` sees is a design decision, and a test that pins it is cheaper than
/// re-reading the prompt string every time the loop changes.
fn render_reflect_prompt(goal: &str, pool: &PoolSummary, remaining: &BudgetRemaining) -> String {
    let mut out = format!("Goal: {goal}\n");

    let coverage = &pool.coverage;
    out.push_str(&format!("Pool: {} papers", coverage.total));
    if let Some(mean) = coverage.mean_score {
        out.push_str(&format!(", mean relevance {mean:.2}"));
    }
    if coverage.below_floor > 0 {
        out.push_str(&format!(
            ", {} below the relevance floor",
            coverage.below_floor
        ));
    }
    if let (Some(min), Some(max)) = (coverage.year_min, coverage.year_max) {
        out.push_str(&format!(", years {min}-{max}"));
    }
    out.push('\n');

    out.push_str(&format!(
        "Budget left: {} rounds, {} provider queries\n",
        remaining.rounds, remaining.provider_queries
    ));

    out.push_str("Found:\n");
    for entry in pool.entries.iter().take(POOL_ENTRY_CAP) {
        out.push_str("- ");
        out.push_str(&entry.title);
        if let Some(year) = entry.year {
            out.push_str(&format!(" ({year})"));
        }
        if let Some(venue) = entry.venue.as_deref().filter(|v| !v.is_empty()) {
            out.push_str(&format!(" · {venue}"));
        }
        if let Some(score) = entry.semantic_score {
            out.push_str(&format!(" · {score:.2}"));
        }
        out.push('\n');
    }
    if pool.entries.len() > POOL_ENTRY_CAP {
        out.push_str(&format!(
            "… and {} more\n",
            pool.entries.len() - POOL_ENTRY_CAP
        ));
    }

    if !pool.detailed.is_empty() {
        out.push_str("Borderline (abstracts):\n");
        for detail in &pool.detailed {
            out.push_str(&format!(
                "- {}{}: {}\n",
                detail.title,
                detail.year.map(|y| format!(" ({y})")).unwrap_or_default(),
                detail
                    .abstract_text
                    .chars()
                    .take(DETAIL_ABSTRACT_CHARS)
                    .collect::<String>()
            ));
        }
    }

    out
}

/// Titles shown to `reflect`. Beyond this the list stops informing a coverage
/// judgement and starts costing tokens.
const POOL_ENTRY_CAP: usize = 60;

/// Abstract characters per borderline candidate. ~250 tokens at 1000 chars.
const DETAIL_ABSTRACT_CHARS: usize = 1_000;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_clean_json_object() {
        let value = extract_json_object(r#"{"should_continue":false,"gaps":[],"next_queries":[]}"#)
            .unwrap();
        assert_eq!(value["should_continue"], false);
    }

    #[test]
    fn parses_fenced_json() {
        let raw = "```json\n{\"should_continue\":true,\"gaps\":[\"long-context\"],\"next_queries\":[]}\n```";
        let value = extract_json_object(raw).unwrap();
        assert_eq!(value["gaps"][0], "long-context");
    }

    #[test]
    fn parses_prose_wrapped_json() {
        let raw = "Sure! Here is the result:\n{\"queries\": []}\nHope that helps.";
        let value = extract_json_object(raw).unwrap();
        assert!(value["queries"].is_array());
    }

    #[test]
    fn malformed_output_errors() {
        assert!(extract_json_object("no json at all").is_err());
    }

    #[test]
    fn suggestion_queries_are_trimmed_deduplicated_and_bounded() {
        let items = vec![
            SuggestionQueryItem {
                intent: " Methods ".to_string(),
                query: " sparse attention ".to_string(),
            },
            SuggestionQueryItem {
                intent: "Duplicate".to_string(),
                query: "SPARSE ATTENTION".to_string(),
            },
            SuggestionQueryItem {
                intent: "Inference".to_string(),
                query: "efficient transformer inference".to_string(),
            },
        ];

        let queries = normalize_suggestion_queries(items, 2).expect("two distinct queries");

        assert_eq!(queries.len(), 2);
        assert_eq!(queries[0].intent, "Methods");
        assert_eq!(queries[0].query, "sparse attention");
        assert_eq!(queries[1].id, "path-2");
    }

    #[test]
    fn suggestion_query_plan_requires_the_requested_count() {
        let items = vec![SuggestionQueryItem {
            intent: "Only angle".to_string(),
            query: "one query".to_string(),
        }];

        assert!(normalize_suggestion_queries(items, 3).is_err());
    }

    #[test]
    fn queries_response_deserializes_provider_and_text() {
        let value = extract_json_object(
            r#"{"queries":[{"provider":"arxiv","text":"sparse autoencoder"},{"provider":"open_alex","text":"dictionary learning features"}]}"#,
        )
        .unwrap();
        let parsed: QueriesResponse = serde_json::from_value(value).unwrap();
        assert_eq!(parsed.queries.len(), 2);
        assert_eq!(parsed.queries[0].provider, DiscoveryProviderChoice::Arxiv);
        assert_eq!(parsed.queries[0].text, "sparse autoencoder");
        assert_eq!(
            parsed.queries[1].provider,
            DiscoveryProviderChoice::OpenAlex
        );
    }

    #[test]
    fn stale_semantic_scholar_provider_falls_back_to_default() {
        // RFC 0044: a removed/stale provider must not crash deserialization.
        let value = extract_json_object(
            r#"{"queries":[{"provider":"semantic_scholar","text":"legacy state"}]}"#,
        )
        .unwrap();
        let parsed: QueriesResponse = serde_json::from_value(value).unwrap();
        assert_eq!(parsed.queries.len(), 1);
        assert_eq!(
            parsed.queries[0].provider,
            DiscoveryProviderChoice::OpenAlex
        );
    }

    #[test]
    fn reflection_requires_an_explicit_continue_decision() {
        let missing_decision = serde_json::json!({
            "gaps": ["missing benchmarks"],
            "next_queries": []
        });
        assert!(serde_json::from_value::<Reflection>(missing_decision).is_err());

        let reflection: Reflection = serde_json::from_value(serde_json::json!({
            "should_continue": true,
            "gaps": ["missing benchmarks"],
            "next_queries": [{
                "provider": "arxiv",
                "text": "benchmark sparse autoencoders"
            }]
        }))
        .unwrap();
        assert!(reflection.should_continue);
        assert_eq!(
            reflection.next_queries[0].provider,
            DiscoveryProviderChoice::Arxiv
        );
    }

    #[test]
    fn reflection_prompt_contains_coverage_budget_and_bounded_detail() {
        let summary = PoolSummary {
            entries: vec![PoolEntry {
                title: "Sparse Autoencoders".to_string(),
                year: Some(2024),
                venue: Some("ICML".to_string()),
                semantic_score: Some(0.81),
            }],
            detailed: vec![CandidateDetail {
                title: "Borderline Paper".to_string(),
                year: Some(2023),
                abstract_text: "An uncertain but relevant abstract.".to_string(),
            }],
            coverage: CoverageStats {
                total: 1,
                below_floor: 0,
                mean_score: Some(0.81),
                year_min: Some(2024),
                year_max: Some(2024),
            },
        };
        let prompt = render_reflect_prompt(
            "mechanistic interpretability",
            &summary,
            &BudgetRemaining {
                rounds: 2,
                provider_queries: 5,
            },
        );

        assert!(prompt.contains("Goal: mechanistic interpretability"));
        assert!(prompt.contains("mean relevance 0.81"));
        assert!(prompt.contains("Budget left: 2 rounds, 5 provider queries"));
        assert!(prompt.contains("Sparse Autoencoders (2024) · ICML · 0.81"));
        assert!(prompt.contains("An uncertain but relevant abstract."));
    }

    #[test]
    fn ranking_maps_indices_and_drops_out_of_range() {
        use crate::domain::discovery::{CandidateMatch, PaperCandidate};
        let candidate = |t: &str| PaperCandidate {
            id: t.to_string(),
            source_provider: "openalex".to_string(),
            source_id: t.to_string(),
            title: t.to_string(),
            authors: Vec::new(),
            abstract_text: None,
            year: None,
            publication_date: None,
            venue: None,
            citation_count: None,
            doi: None,
            openalex_id: None,
            arxiv_id: None,
            external_url: None,
            pdf_url: None,
            open_access: None,
            match_summary: CandidateMatch {
                score: None,
                reasons: Vec::new(),
                matched_keywords: Vec::new(),
                from_seed_paper_ids: Vec::new(),
            },
            already_in_library: false,
        };
        let candidates = vec![candidate("A"), candidate("B")];
        let response = RankResponse {
            ranked: vec![
                RankedItem {
                    index: 1,
                    score: Some(0.9),
                    rationale: Some("best".into()),
                },
                RankedItem {
                    index: 5,
                    score: Some(0.1),
                    rationale: None,
                }, // out of range -> dropped
                RankedItem {
                    index: 0,
                    score: Some(0.5),
                    rationale: None,
                },
            ],
        };
        let ranked = ranked_from_indices(&candidates, response.ranked);
        assert_eq!(ranked.len(), 2);
        assert_eq!(ranked[0].candidate.title, "B");
        assert_eq!(ranked[0].rank, 1);
        assert_eq!(ranked[1].candidate.title, "A");
        assert_eq!(ranked[1].rank, 2);
    }
}
