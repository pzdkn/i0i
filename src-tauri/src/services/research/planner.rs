//! The `Planner` seam: the LLM-backed primitives (plan/refine/assess/rank).
//!
//! Real impl talks to OpenRouter and parses JSON tolerantly; the loop depends on
//! the trait so tests can substitute a fake. Candidates are never invented — the
//! planner ranks by *index* into a provided candidate list, so it can only
//! reorder/score real provider results.

use async_trait::async_trait;
use reqwest::Client;
use serde::Deserialize;

use crate::domain::discovery::{DiscoveryProviderChoice, PaperCandidate};
use crate::domain::research::{RankedCandidate, SearchConstraints};
use crate::services::llm::{self, CompletionRequest, WireMessage};
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

/// The planner's read on coverage after an iteration.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Assessment {
    /// Whether another query round is warranted. `false` => coverage sufficient.
    pub refine: bool,
    #[serde(default)]
    pub gaps: Vec<String>,
}

#[async_trait]
pub trait Planner: Send + Sync {
    async fn plan_queries(
        &self,
        goal: &str,
        constraints: &SearchConstraints,
    ) -> Result<Vec<Query>, ResearchError>;

    async fn refine_queries(
        &self,
        goal: &str,
        gaps: &[String],
        pool_titles: &[String],
    ) -> Result<Vec<Query>, ResearchError>;

    async fn assess(&self, goal: &str, pool_titles: &[String])
        -> Result<Assessment, ResearchError>;

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
            max_tokens: None,
            response_format: None,
            tools: None,
            tool_choice: None,
        };
        llm::complete(&self.client, &self.url, &self.api_key, &request)
            .await
            .map_err(ResearchError::new)
    }
}

const PLAN_SYSTEM: &str = "You are a scholarly search planner. Expand the user's research \
goal into focused provider queries (synonyms, key methods, datasets). Reply with a single \
JSON object: {\"queries\":[{\"provider\":\"open_alex\"|\"arxiv\",\"text\":\"...\"}]}. No prose.";

const ASSESS_SYSTEM: &str = "You judge whether a paper search has enough coverage for the \
goal. Reply with a single JSON object: {\"refine\":true|false,\"gaps\":[\"...\"]}. Set \
refine=false when coverage is sufficient. No prose.";

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

    async fn refine_queries(
        &self,
        goal: &str,
        gaps: &[String],
        pool_titles: &[String],
    ) -> Result<Vec<Query>, ResearchError> {
        let user = format!(
            "Goal: {goal}\nCoverage gaps: {}\nAlready found ({} papers): {}\n\
             Propose 3-5 NEW queries targeting the gaps.",
            gaps.join("; "),
            pool_titles.len(),
            preview_titles(pool_titles),
        );
        let value = self.complete_json(PLAN_SYSTEM, &user).await?;
        let parsed: QueriesResponse = serde_json::from_value(value)
            .map_err(|e| ResearchError::new(format!("refine_queries shape: {e}")))?;
        Ok(parsed.queries)
    }

    async fn assess(
        &self,
        goal: &str,
        pool_titles: &[String],
    ) -> Result<Assessment, ResearchError> {
        let user = format!(
            "Goal: {goal}\nFound {} papers: {}\nIs coverage sufficient?",
            pool_titles.len(),
            preview_titles(pool_titles),
        );
        let value = self.complete_json(ASSESS_SYSTEM, &user).await?;
        serde_json::from_value(value).map_err(|e| ResearchError::new(format!("assess shape: {e}")))
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

fn preview_titles(titles: &[String]) -> String {
    titles
        .iter()
        .take(40)
        .map(|t| t.as_str())
        .collect::<Vec<_>>()
        .join(" | ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_clean_json_object() {
        let value = extract_json_object(r#"{"refine": false, "gaps": []}"#).unwrap();
        assert_eq!(value["refine"], false);
    }

    #[test]
    fn parses_fenced_json() {
        let raw = "```json\n{\"refine\": true, \"gaps\": [\"long-context\"]}\n```";
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
