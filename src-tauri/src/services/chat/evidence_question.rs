//! Bounded, non-persistent answers over evidence already held by i0i.
//!
//! This is the question-answering core shared by the MCP `reader_ask` and
//! `vault_ask` tools. It retrieves only within paper IDs authorized by the
//! caller, asks the configured chat model once, and accepts only citations to
//! passages included in that request. It never creates chat threads or writes
//! Research State.

use std::collections::BTreeSet;
use std::sync::Arc;

use async_trait::async_trait;
use reqwest::Client;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::config::ChatConfig;
use crate::domain::library::{DocumentChunk, Paper};
use crate::services::llm::{self as openrouter, CompletionRequest, ResponseFormat, WireMessage};
use crate::services::search::{SearchMode, SearchRequest, SearchService};
use crate::storage::library_store::LibraryStore;

pub(crate) const MAX_QUESTION_PASSAGES: usize = 8;
pub(crate) const MAX_QUESTION_SOURCE_CHARS: usize = 12_000;

/// One exact source span considered by the delegated answer model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct EvidenceQuestionPassage {
    pub passage_ref: Option<String>,
    pub paper_id: String,
    pub paper_title: String,
    pub source_id: String,
    pub extraction_id: Option<String>,
    pub chunk_id: Option<String>,
    /// Zero-based page index, matching persisted document chunks.
    pub page_start: Option<i32>,
    /// Zero-based inclusive page index, matching persisted document chunks.
    pub page_end: Option<i32>,
    pub source_start: i64,
    pub source_end: i64,
    pub text: String,
    pub evidence_kind: String,
}

/// Metadata-only match returned separately from source evidence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct EvidenceMetadataMatch {
    pub paper_id: String,
    pub title: String,
    pub reason: String,
}

/// Honest description of what material was available to answer a question.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct EvidenceQuestionCoverage {
    pub kind: String,
    pub scoped_paper_count: usize,
    pub full_text_paper_ids: Vec<String>,
    pub abstract_paper_ids: Vec<String>,
    pub unavailable_paper_ids: Vec<String>,
}

/// Retrieved evidence before the single delegated model call.
#[derive(Debug, Clone)]
pub(crate) struct EvidenceQuestionContext {
    pub passages: Vec<EvidenceQuestionPassage>,
    pub metadata_matches: Vec<EvidenceMetadataMatch>,
    pub coverage: EvidenceQuestionCoverage,
}

/// Validated model interpretation and the source passages it actually cited.
#[derive(Debug, Clone)]
pub(crate) struct EvidenceQuestionAnswer {
    pub answer: String,
    pub cited_passages: Vec<EvidenceQuestionPassage>,
    pub insufficient_evidence: bool,
    pub model: Option<String>,
    pub llm_calls: u32,
}

#[async_trait]
pub(crate) trait EvidenceAnswerModel: Send + Sync {
    /// Stable model identifier reported in question-tool usage.
    fn model_id(&self) -> &str;

    /// Validate local configuration before a Run spends a model-call budget.
    fn ensure_ready(&self) -> Result<(), String>;

    /// Return one structured evidence answer as JSON.
    async fn complete(&self, system: &str, user: &str) -> Result<String, String>;
}

#[derive(Clone)]
struct OpenRouterEvidenceModel {
    client: Client,
    config: ChatConfig,
}

impl OpenRouterEvidenceModel {
    /// Build the configured OpenRouter client used for one-shot evidence answers.
    fn new(config: ChatConfig) -> Self {
        let client = Client::builder()
            .user_agent(concat!(
                env!("CARGO_PKG_NAME"),
                "/",
                env!("CARGO_PKG_VERSION"),
                " evidence-question"
            ))
            .build()
            .expect("evidence question HTTP client should build");
        Self { client, config }
    }
}

#[async_trait]
impl EvidenceAnswerModel for OpenRouterEvidenceModel {
    fn model_id(&self) -> &str {
        &self.config.model
    }

    fn ensure_ready(&self) -> Result<(), String> {
        self.config.resolve_api_key().map(|_| ())
    }

    async fn complete(&self, system: &str, user: &str) -> Result<String, String> {
        let api_key = self.config.resolve_api_key()?;
        let request = CompletionRequest {
            model: self.config.model.clone(),
            messages: vec![
                WireMessage::text("system", system.to_string()),
                WireMessage::text("user", user.to_string()),
            ],
            stream: false,
            max_tokens: Some(self.config.max_answer_tokens),
            response_format: Some(ResponseFormat::json_schema(
                "i0i_evidence_answer",
                serde_json::json!({
                    "type": "object",
                    "properties": {
                        "answer": {"type": "string"},
                        "citedPassages": {
                            "type": "array",
                            "items": {"type": "string", "pattern": "^E[1-8]$"}
                        },
                        "insufficientEvidence": {"type": "boolean"}
                    },
                    "required": ["answer", "citedPassages", "insufficientEvidence"],
                    "additionalProperties": false
                }),
            )),
            tools: None,
            tool_choice: None,
        };
        openrouter::complete(&self.client, &self.config.url, &api_key, &request).await
    }
}

/// Retrieves scoped evidence and asks one configured model without side effects.
#[derive(Clone)]
pub(crate) struct EvidenceQuestionService {
    store: LibraryStore,
    search: SearchService,
    model: Arc<dyn EvidenceAnswerModel>,
}

impl EvidenceQuestionService {
    /// Build the production service from existing retrieval and chat configuration.
    pub(crate) fn new(store: LibraryStore, search: SearchService, config: ChatConfig) -> Self {
        Self {
            store,
            search,
            model: Arc::new(OpenRouterEvidenceModel::new(config)),
        }
    }

    #[cfg(test)]
    /// Build the service with a deterministic model fixture.
    pub(crate) fn with_model(
        store: LibraryStore,
        search: SearchService,
        model: Arc<dyn EvidenceAnswerModel>,
    ) -> Self {
        Self {
            store,
            search,
            model,
        }
    }

    /// Retrieve bounded full-text passages, with abstracts as an explicit fallback.
    pub(crate) async fn collect(
        &self,
        question: &str,
        paper_ids: &[String],
    ) -> Result<EvidenceQuestionContext, String> {
        let question = question.trim();
        if question.is_empty() {
            return Err("Question must not be empty".to_string());
        }
        let terms = query_terms(question);
        let retrieval_query = if terms.is_empty() {
            question.to_string()
        } else {
            terms.iter().cloned().collect::<Vec<_>>().join(" ")
        };
        let snapshot = self.store.get_library()?;
        let papers: Vec<&Paper> = paper_ids
            .iter()
            .filter_map(|id| snapshot.papers.iter().find(|paper| paper.id == *id))
            .collect();
        let response = self
            .search
            .search(SearchRequest {
                query: retrieval_query,
                paper_ids: paper_ids.to_vec(),
                vault_ids: Vec::new(),
                mode: SearchMode::Hybrid,
                limit: Some(MAX_QUESTION_PASSAGES),
            })
            .await?;

        let mut passages: Vec<EvidenceQuestionPassage> = response
            .hits
            .into_iter()
            .filter_map(|hit| {
                let paper = papers.iter().find(|paper| paper.id == hit.chunk.paper_id)?;
                Some(passage_from_chunk(&hit.chunk, &paper.title))
            })
            .collect();
        let full_text_paper_ids: BTreeSet<String> = passages
            .iter()
            .map(|passage| passage.paper_id.clone())
            .collect();

        let mut metadata_matches = Vec::new();
        for paper in &papers {
            let score = metadata_score(paper, &terms);
            if score > 0 || (papers.len() == 1 && passages.is_empty()) {
                metadata_matches.push(EvidenceMetadataMatch {
                    paper_id: paper.id.clone(),
                    title: paper.title.clone(),
                    reason: if score > 0 {
                        "Title or abstract matches the question".to_string()
                    } else {
                        "Only this paper was requested".to_string()
                    },
                });
            }
            if passages.len() >= MAX_QUESTION_PASSAGES
                || full_text_paper_ids.contains(&paper.id)
                || score == 0 && papers.len() > 1
            {
                continue;
            }
            if let Some(abstract_text) = paper.abstract_text.as_deref() {
                passages.push(passage_from_abstract(paper, abstract_text));
            }
        }
        metadata_matches.truncate(MAX_QUESTION_PASSAGES);
        bound_passages(&mut passages);

        let full_text: BTreeSet<String> = passages
            .iter()
            .filter(|passage| passage.evidence_kind == "full_text")
            .map(|passage| passage.paper_id.clone())
            .collect();
        let abstracts: BTreeSet<String> = passages
            .iter()
            .filter(|passage| passage.evidence_kind == "abstract")
            .map(|passage| passage.paper_id.clone())
            .collect();
        let mut unavailable = Vec::new();
        for paper in &papers {
            if !full_text.contains(&paper.id)
                && !abstracts.contains(&paper.id)
                && paper.abstract_text.is_none()
                && !self.store.paper_has_chunks(&paper.id)?
            {
                unavailable.push(paper.id.clone());
            }
        }
        let kind = match (full_text.is_empty(), abstracts.is_empty()) {
            (false, true) => "full_text",
            (false, false) => "mixed",
            (true, false) => "abstract_only",
            (true, true) => "none",
        };
        Ok(EvidenceQuestionContext {
            passages,
            metadata_matches,
            coverage: EvidenceQuestionCoverage {
                kind: kind.to_string(),
                scoped_paper_count: papers.len(),
                full_text_paper_ids: full_text.into_iter().collect(),
                abstract_paper_ids: abstracts.into_iter().collect(),
                unavailable_paper_ids: unavailable,
            },
        })
    }

    /// Check credentials before the caller charges a managed Run.
    pub(crate) fn ensure_ready(&self) -> Result<(), String> {
        self.model.ensure_ready()
    }

    /// Ask once over a caller-prepared bounded context and validate every citation.
    pub(crate) async fn answer(
        &self,
        question: &str,
        passages: &[EvidenceQuestionPassage],
    ) -> Result<EvidenceQuestionAnswer, String> {
        if passages.is_empty() {
            return Ok(EvidenceQuestionAnswer {
                answer: "The available source material is insufficient to answer this question."
                    .to_string(),
                cited_passages: Vec::new(),
                insufficient_evidence: true,
                model: None,
                llm_calls: 0,
            });
        }
        validate_passage_bounds(passages)?;
        let system = "Answer the question only from the supplied source passages. Distinguish an abstract from full text. Do not invent facts or citation labels. Return the required JSON object; citedPassages contains the E-labels that directly support the answer. Set insufficientEvidence to true when the passages cannot answer the question.";
        let user = question_prompt(question, passages);
        let raw = self.model.complete(system, &user).await?;
        parse_answer(&raw, passages, self.model.model_id())
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WireEvidenceAnswer {
    answer: String,
    cited_passages: Vec<String>,
    insufficient_evidence: bool,
}

fn parse_answer(
    raw: &str,
    passages: &[EvidenceQuestionPassage],
    model: &str,
) -> Result<EvidenceQuestionAnswer, String> {
    // Validate both the structured citation list and any labels printed in the
    // prose. This prevents a plausible answer from displaying a fabricated ref.
    let parsed: WireEvidenceAnswer =
        serde_json::from_str(raw).map_err(|error| format!("Invalid evidence answer: {error}"))?;
    if parsed.answer.trim().is_empty() {
        return Err("Evidence answer must not be empty".to_string());
    }
    let mut seen: BTreeSet<usize> = BTreeSet::new();
    let mut cited_passages = Vec::new();
    for label in &parsed.cited_passages {
        let index = passage_label_index(label, passages.len())?;
        if seen.insert(index) {
            cited_passages.push(passages[index - 1].clone());
        }
    }

    let inline_pattern =
        regex::Regex::new(r"\[E\d+\]").expect("the fixed evidence-label expression must compile");
    let inline_labels = inline_pattern
        .find_iter(&parsed.answer)
        .map(|matched| &matched.as_str()[1..matched.as_str().len() - 1]);
    for label in inline_labels {
        passage_label_index(label, passages.len())?;
        if !parsed
            .cited_passages
            .iter()
            .any(|citation| citation == label)
        {
            return Err(format!(
                "Evidence answer used {label} in its text without listing it in citedPassages"
            ));
        }
    }
    if !parsed.insufficient_evidence && cited_passages.is_empty() {
        return Err("Evidence answer claimed sufficient evidence without a citation".to_string());
    }
    Ok(EvidenceQuestionAnswer {
        answer: parsed.answer.trim().to_string(),
        cited_passages,
        insufficient_evidence: parsed.insufficient_evidence,
        model: Some(model.to_string()),
        llm_calls: 1,
    })
}

/// Resolve an `E1`-style model label into a one-based passage index.
fn passage_label_index(label: &str, passage_count: usize) -> Result<usize, String> {
    label
        .strip_prefix('E')
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|value| *value > 0 && *value <= passage_count)
        .ok_or_else(|| format!("Evidence answer cited unknown passage {label}"))
}

/// Convert a persisted full-text chunk into question evidence.
fn passage_from_chunk(chunk: &DocumentChunk, paper_title: &str) -> EvidenceQuestionPassage {
    EvidenceQuestionPassage {
        passage_ref: None,
        paper_id: chunk.paper_id.clone(),
        paper_title: paper_title.to_string(),
        source_id: chunk.source_id.clone(),
        extraction_id: Some(chunk.extraction_id.clone()),
        chunk_id: Some(chunk.id.clone()),
        page_start: Some(chunk.page_start),
        page_end: Some(chunk.page_end),
        source_start: chunk.source_start,
        source_end: chunk.source_end,
        text: chunk.text.clone(),
        evidence_kind: "full_text".to_string(),
    }
}

/// Represent a saved abstract as explicitly classified, location-free evidence.
fn passage_from_abstract(paper: &Paper, abstract_text: &str) -> EvidenceQuestionPassage {
    let digest = format!("{:x}", Sha256::digest(abstract_text.as_bytes()));
    EvidenceQuestionPassage {
        passage_ref: None,
        paper_id: paper.id.clone(),
        paper_title: paper.title.clone(),
        source_id: format!("metadata:{}:{}", paper.id, &digest[..12]),
        extraction_id: None,
        chunk_id: None,
        page_start: None,
        page_end: None,
        source_start: 0,
        source_end: abstract_text.chars().count() as i64,
        text: abstract_text.to_string(),
        evidence_kind: "abstract".to_string(),
    }
}

/// Enforce the model-context limits while retaining the highest-ranked passages.
fn bound_passages(passages: &mut Vec<EvidenceQuestionPassage>) {
    passages.truncate(MAX_QUESTION_PASSAGES);
    let mut remaining = MAX_QUESTION_SOURCE_CHARS;
    passages.retain_mut(|passage| {
        if remaining == 0 {
            return false;
        }
        let original_chars = passage.text.chars().count();
        if original_chars > remaining {
            passage.text = passage.text.chars().take(remaining).collect();
            passage.source_end = passage.source_start + remaining as i64;
        }
        remaining = remaining.saturating_sub(original_chars.min(remaining));
        !passage.text.is_empty()
    });
}

/// Reject caller-supplied evidence that exceeds the question contract.
fn validate_passage_bounds(passages: &[EvidenceQuestionPassage]) -> Result<(), String> {
    let chars: usize = passages
        .iter()
        .map(|passage| passage.text.chars().count())
        .sum();
    if passages.len() > MAX_QUESTION_PASSAGES || chars > MAX_QUESTION_SOURCE_CHARS {
        return Err("Evidence question context exceeds its passage or character limit".to_string());
    }
    Ok(())
}

/// Render the question and numbered source passages for the answer model.
fn question_prompt(question: &str, passages: &[EvidenceQuestionPassage]) -> String {
    let mut prompt = format!("Question:\n{}\n\nSource passages:\n", question.trim());
    for (index, passage) in passages.iter().enumerate() {
        let location = passage
            .page_start
            .map(|page| format!("page {}", page + 1))
            .unwrap_or_else(|| passage.evidence_kind.clone());
        prompt.push_str(&format!(
            "\n[E{}] {} · {} · {}\n{}\n",
            index + 1,
            passage.paper_title,
            passage.paper_id,
            location,
            passage.text,
        ));
    }
    prompt
}

/// Extract useful retrieval terms from a natural-language question.
fn query_terms(question: &str) -> BTreeSet<String> {
    const STOP_WORDS: &[&str] = &[
        "about", "could", "does", "from", "have", "into", "paper", "papers", "reported", "that",
        "their", "these", "this", "under", "what", "when", "where", "which", "with",
    ];
    question
        .split(|character: char| !character.is_alphanumeric())
        .map(str::to_lowercase)
        .filter(|term| term.chars().count() >= 3 && !STOP_WORDS.contains(&term.as_str()))
        .collect()
}

/// Count question terms present in a paper's title or abstract.
fn metadata_score(paper: &Paper, terms: &BTreeSet<String>) -> usize {
    let haystack = format!(
        "{} {}",
        paper.title,
        paper.abstract_text.as_deref().unwrap_or_default()
    )
    .to_lowercase();
    terms.iter().filter(|term| haystack.contains(*term)).count()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn passage(reference: &str) -> EvidenceQuestionPassage {
        EvidenceQuestionPassage {
            passage_ref: Some(reference.to_string()),
            paper_id: "paper:one".to_string(),
            paper_title: "Fixture".to_string(),
            source_id: "source:one".to_string(),
            extraction_id: Some("extraction:one".to_string()),
            chunk_id: Some("chunk:one".to_string()),
            page_start: Some(0),
            page_end: Some(0),
            source_start: 0,
            source_end: 8,
            text: "Evidence".to_string(),
            evidence_kind: "full_text".to_string(),
        }
    }

    #[test]
    fn invented_citation_labels_are_rejected() {
        let error = parse_answer(
            r#"{"answer":"Unsupported","citedPassages":["E2"],"insufficientEvidence":false}"#,
            &[passage("passage:one")],
            "fixture-model",
        )
        .expect_err("unknown citation must fail");
        assert!(error.contains("unknown passage E2"));
    }

    #[test]
    fn invented_inline_citation_labels_are_rejected() {
        let error = parse_answer(
            r#"{"answer":"Unsupported [E2]","citedPassages":["E1"],"insufficientEvidence":false}"#,
            &[passage("passage:one")],
            "fixture-model",
        )
        .expect_err("unknown inline citation must fail");
        assert!(error.contains("unknown passage E2"));
    }

    #[test]
    fn inline_citations_must_appear_in_structured_citations() {
        let error = parse_answer(
            r#"{"answer":"Supported [E1]","citedPassages":[],"insufficientEvidence":false}"#,
            &[passage("passage:one")],
            "fixture-model",
        )
        .expect_err("unregistered inline citation must fail");
        assert!(error.contains("without listing it in citedPassages"));
    }

    #[test]
    fn insufficient_answers_may_honestly_have_no_citation() {
        let answer = parse_answer(
            r#"{"answer":"The evidence does not establish this.","citedPassages":[],"insufficientEvidence":true}"#,
            &[passage("passage:one")],
            "fixture-model",
        )
        .expect("honest insufficiency");
        assert!(answer.insufficient_evidence);
        assert!(answer.cited_passages.is_empty());
    }

    #[tokio::test]
    #[ignore = "explicit live OpenRouter evidence-question check"]
    async fn live_question_records_model_passage_and_answer() {
        let path = std::env::temp_dir().join(format!(
            "i0i-live-evidence-question-{}.sqlite",
            uuid::Uuid::new_v4().simple()
        ));
        let store = LibraryStore::for_test(path);
        store.init().expect("initialize live question store");
        let service = EvidenceQuestionService::new(
            store.clone(),
            SearchService::new(store, None),
            ChatConfig::load().expect("load chat config"),
        );
        let evidence = passage("passage:live-fixture");
        let answer = service
            .answer(
                "What does this fixture establish?",
                std::slice::from_ref(&evidence),
            )
            .await
            .expect("complete live evidence question");
        eprintln!(
            "{}",
            serde_json::json!({
                "model": answer.model,
                "passages": [evidence],
                "answer": answer.answer,
                "citedPassageRefs": answer.cited_passages
                    .iter()
                    .filter_map(|passage| passage.passage_ref.clone())
                    .collect::<Vec<_>>(),
            })
        );
    }
}
