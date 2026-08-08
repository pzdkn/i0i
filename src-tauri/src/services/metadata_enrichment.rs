//! Background metadata enrichment for locally imported PDFs.
//!
//! Importing a PDF must stay fast: the command copies the file, creates a
//! fallback paper row, and returns. This service runs after that path and tries
//! to replace filename metadata with evidence-backed scholarly metadata.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use pdfium_render::prelude::*;
use regex::Regex;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::Semaphore;

use crate::commands::discovery::provider::DiscoveryProvider;
use crate::commands::discovery::providers::{arxiv::ArxivProvider, openalex::OpenAlexProvider};
use crate::domain::discovery::{
    DiscoveryProviderChoice, DiscoverySearchRequest, DiscoverySort, PaperCandidate,
};
use crate::domain::library::{MetadataCandidate, Paper};
use crate::pdf_extraction::PdfExtractionConfig;
use crate::services::chat::config::ChatConfig;
use crate::services::llm::{self, CompletionRequest, ResponseFormat, WireMessage};
use crate::shared::env::read_dotenv_value;
use crate::storage::library_store::LibraryStore;

const MAX_CONCURRENT_METADATA_JOBS: usize = 1;
const FIRST_PAGE_TEXT_LIMIT: usize = 5;
// Token cost control: the LLM only ever sees capped page-1 text, never the
// whole PDF (RFC 0049).
const LLM_EVIDENCE_CHAR_LIMIT: usize = 6_000;
const LLM_METADATA_MAX_TOKENS: u32 = 500;
const PROVIDER_RESULT_LIMIT: i32 = 3;
const BIBLIOGRAPHIC_QUERY_CHAR_LIMIT: usize = 1_500;
// RFC 0049/0050 confidence tiers: page-1 identifier match, verified title
// (optionally corroborated by page-1 authors), single source, unverified.
const CONFIDENCE_PAGE1_ID: f64 = 0.98;
const CONFIDENCE_VERIFIED_PROVIDER_AUTHORS: f64 = 0.95;
const CONFIDENCE_VERIFIED_PROVIDER: f64 = 0.90;
const CONFIDENCE_VERIFIED_SINGLE_AUTHORS: f64 = 0.85;
const CONFIDENCE_VERIFIED_SINGLE: f64 = 0.80;
const CONFIDENCE_UNVERIFIED_CAP: f64 = 0.35;
const AUTO_APPLY_CONFIDENCE: f64 = 0.90;
// RFC 0050: candidates below the floor never reach the review list; the best
// one is offered as an editable draft instead.
const DISPLAY_CONFIDENCE_FLOOR: f64 = 0.60;
const REVIEW_CANDIDATE_LIMIT: usize = 3;
const SEARCH_AUTHOR_LIMIT: usize = 3;
const CROSSREF_FIELDED_ROWS: i32 = 5;

type MetadataResult<T> = Result<T, String>;

#[derive(Clone)]
pub struct MetadataEnrichmentService {
    app: AppHandle,
    store: LibraryStore,
    pdfium_config: PdfExtractionConfig,
    openalex: OpenAlexProvider,
    arxiv: ArxivProvider,
    client: Client,
    llm_config: Option<ChatConfig>,
    semaphore: Arc<Semaphore>,
    queued_or_active: Arc<Mutex<HashSet<String>>>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PaperMetadataUpdated {
    pub paper_id: String,
    pub status: String,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MetadataAutofillProgress {
    pub paper_id: String,
    pub status: String,
    pub stage: String,
    pub message: String,
    pub candidates: Vec<MetadataCandidate>,
}

#[derive(Debug, Default)]
struct PdfEvidence {
    /// Title from the PDF's embedded metadata dictionary, if plausible.
    embedded_title: Option<String>,
    embedded_authors: Vec<String>,
    /// The contiguous run of largest-font text on page 1 — almost always the
    /// paper title (RFC 0049).
    largest_font_title: Option<String>,
    page1_text: String,
    /// Identifiers found on page 1 count as the paper's own identity;
    /// identifiers from later pages are hints only (they may be citations).
    page1_doi: Option<String>,
    later_doi: Option<String>,
    page1_arxiv_id: Option<String>,
    later_arxiv_id: Option<String>,
}

impl PdfEvidence {
    fn best_doi(&self) -> Option<&String> {
        self.page1_doi.as_ref().or(self.later_doi.as_ref())
    }

    fn best_arxiv_id(&self) -> Option<&String> {
        self.page1_arxiv_id
            .as_ref()
            .or(self.later_arxiv_id.as_ref())
    }
}

/// RFC 0049 verification gate: every candidate is checked against the PDF's
/// own page-1 evidence before it may auto-apply or rank highly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Verification {
    Page1IdMatch,
    TitleOnPage1,
    TitleSimilar,
    Unverified,
}

/// How many of a candidate's leading authors appear on page 1 (RFC 0050).
/// A wrong candidate essentially never has both its title and its authors on
/// page 1, so this raises true positives without raising false ones.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AuthorMatch {
    Strong,
    Weak,
    None,
}

/// RFC 0050: the LLM extraction (plus local evidence) becomes the query plan
/// driving fielded provider searches, not just another candidate.
#[derive(Debug, Default)]
struct SearchPlan {
    title: Option<String>,
    author_last_names: Vec<String>,
}

fn build_search_plan(evidence: &PdfEvidence, llm: Option<&MetadataCandidate>) -> SearchPlan {
    let title = [
        llm.map(|candidate| candidate.title.clone()),
        evidence.largest_font_title.clone(),
        evidence.embedded_title.clone(),
    ]
    .into_iter()
    .flatten()
    .map(|title| clean_metadata_text(&title))
    .find(|title| plausible_title(title));

    let authors = llm
        .map(|candidate| candidate.authors.as_slice())
        .filter(|authors| !authors.is_empty())
        .unwrap_or(evidence.embedded_authors.as_slice());
    let author_last_names = authors
        .iter()
        .take(SEARCH_AUTHOR_LIMIT)
        .filter_map(|author| author_last_name(author))
        .collect();

    SearchPlan {
        title,
        author_last_names,
    }
}

fn author_last_name(author: &str) -> Option<String> {
    normalize_title(author)
        .split_whitespace()
        .last()
        .filter(|name| name.len() >= 2)
        .map(str::to_string)
}

impl MetadataEnrichmentService {
    pub fn new(
        app: AppHandle,
        store: LibraryStore,
        pdfium_config: PdfExtractionConfig,
        openalex: OpenAlexProvider,
        arxiv: ArxivProvider,
    ) -> Self {
        Self {
            app,
            store,
            pdfium_config,
            openalex,
            arxiv,
            client: Client::new(),
            llm_config: ChatConfig::load().ok(),
            semaphore: Arc::new(Semaphore::new(MAX_CONCURRENT_METADATA_JOBS)),
            queued_or_active: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    pub fn queue_paper(&self, paper_id: String) {
        if !self.mark_queued(&paper_id) {
            return;
        }

        metadata_log(format!("queued paper_id={paper_id}"));
        self.emit_progress(
            &paper_id,
            "running",
            "queued",
            "Metadata autofill queued.",
            Vec::new(),
        );
        let service = self.clone();
        tauri::async_runtime::spawn(async move {
            let paper_id_for_cleanup = paper_id.clone();
            let result = service.enrich_paper(paper_id).await;
            service.mark_finished(&paper_id_for_cleanup);

            if let Err(error) = result {
                metadata_log(format!(
                    "failed paper_id={paper_id_for_cleanup} error={error}"
                ));
                service.emit_progress(
                    &paper_id_for_cleanup,
                    "failed",
                    "failed",
                    &error,
                    Vec::new(),
                );
                service.emit_update(&paper_id_for_cleanup, "failed", Some(error));
            }
        });
    }

    async fn enrich_paper(&self, paper_id: String) -> MetadataResult<()> {
        let _permit = self
            .semaphore
            .clone()
            .acquire_owned()
            .await
            .map_err(|error| error.to_string())?;
        let current_paper = self
            .store
            .get_paper(&paper_id)?
            .ok_or_else(|| format!("Paper disappeared before metadata enrichment: {paper_id}"))?;
        let source = self.store.resolve_cached_pdf_source(&paper_id, None)?;
        let local_path = source
            .local_path
            .as_deref()
            .ok_or_else(|| format!("Cached PDF source has no local path: {}", source.id))?
            .to_string();

        metadata_log(format!(
            "start paper_id={paper_id} source_id={} path={local_path}",
            source.id
        ));
        self.emit_progress(
            &paper_id,
            "running",
            "extracting",
            "Extracting PDF clues...",
            Vec::new(),
        );

        let app = self.app.clone();
        let config = self.pdfium_config.clone();
        let evidence = tokio::task::spawn_blocking(move || {
            extract_pdf_evidence(Some(&app), &config, Path::new(&local_path))
        })
        .await
        .map_err(|error| error.to_string())??;

        let candidates = self
            .metadata_candidates_from_evidence(&paper_id, &evidence, &current_paper)
            .await?;

        let exact_match = candidates
            .iter()
            .find(|candidate| candidate.confidence >= AUTO_APPLY_CONFIDENCE)
            .cloned();

        let Some(candidate) = exact_match else {
            // RFC 0050 display floor: junk never reaches the review list. When
            // nothing clears the floor the best guess ships as an editable
            // draft instead of a misleading Apply card.
            let review: Vec<MetadataCandidate> = candidates
                .iter()
                .filter(|candidate| candidate.confidence >= DISPLAY_CONFIDENCE_FLOOR)
                .take(REVIEW_CANDIDATE_LIMIT)
                .cloned()
                .collect();
            if review.is_empty() {
                metadata_log(format!(
                    "no_match paper_id={paper_id} draft_available={}",
                    !candidates.is_empty()
                ));
                self.emit_progress(
                    &paper_id,
                    "no_match",
                    "no_match",
                    if candidates.is_empty() {
                        "No reliable metadata match found."
                    } else {
                        "No verified metadata match. Best guess available as an editable draft."
                    },
                    candidates.into_iter().take(1).collect(),
                );
            } else {
                metadata_log(format!(
                    "review paper_id={paper_id} candidates={}",
                    review.len()
                ));
                self.emit_progress(
                    &paper_id,
                    "needs_review",
                    "candidate_review",
                    "Review metadata candidates before applying.",
                    review,
                );
            }
            self.emit_update(&paper_id, "skipped", None);
            return Ok(());
        };

        match self
            .store
            .apply_paper_metadata_enrichment(&paper_id, &candidate.clone().into())?
        {
            Some(_) => {
                metadata_log(format!("ready paper_id={paper_id}"));
                self.emit_progress(
                    &paper_id,
                    "applied",
                    "applied",
                    "Exact metadata match applied.",
                    vec![candidate],
                );
                self.emit_update(&paper_id, "ready", None);
            }
            None => {
                metadata_log(format!("skip paper_id={paper_id} not eligible"));
                self.emit_update(&paper_id, "skipped", None);
            }
        }

        Ok(())
    }

    async fn metadata_candidates_from_evidence(
        &self,
        paper_id: &str,
        evidence: &PdfEvidence,
        current_paper: &Paper,
    ) -> MetadataResult<Vec<MetadataCandidate>> {
        let mut candidates = Vec::new();
        let doi = evidence
            .best_doi()
            .cloned()
            .or_else(|| extract_doi(&current_paper.title));
        let arxiv_id = evidence
            .best_arxiv_id()
            .cloned()
            .or_else(|| extract_arxiv_id(&current_paper.title))
            .or_else(|| extract_bare_arxiv_id(&current_paper.title));
        let has_page1_id = evidence.page1_doi.is_some() || evidence.page1_arxiv_id.is_some();

        // Stage: identifiers -> providers. Confidence is NOT assigned here;
        // the verification gate scores every candidate at the end.
        if let Some(doi) = &doi {
            self.emit_progress(
                paper_id,
                "running",
                "crossref",
                &format!("Found DOI {doi}. Checking Crossref..."),
                Vec::new(),
            );
            match self.lookup_crossref_by_doi(doi).await {
                Ok(Some(candidate)) => candidates.push(candidate),
                Ok(None) => {}
                Err(error) => {
                    metadata_log(format!("Crossref DOI lookup failed doi={doi}: {error}"))
                }
            }

            self.emit_progress(
                paper_id,
                "running",
                "openalex",
                &format!("Checking OpenAlex for DOI {doi}..."),
                Vec::new(),
            );
            match self.lookup_openalex_by_doi(doi).await {
                Ok(Some(candidate)) => {
                    candidates.push(metadata_candidate_from_paper_candidate(
                        candidate,
                        vec!["openalex".to_string()],
                        0.0,
                        vec![format!("DOI lookup: {}", normalize_doi(doi))],
                    ));
                }
                Ok(None) => {}
                Err(error) => {
                    metadata_log(format!("OpenAlex DOI lookup failed doi={doi}: {error}"))
                }
            }
        }

        if let Some(arxiv_id) = &arxiv_id {
            self.emit_progress(
                paper_id,
                "running",
                "arxiv",
                &format!("Found arXiv id {arxiv_id}. Checking arXiv..."),
                Vec::new(),
            );
            match self.lookup_arxiv_by_id(arxiv_id).await {
                Ok(Some(candidate)) => {
                    candidates.push(metadata_candidate_from_paper_candidate(
                        candidate,
                        vec!["arxiv".to_string()],
                        0.0,
                        vec![format!("arXiv lookup: {}", normalize_arxiv_id(arxiv_id))],
                    ));
                }
                Ok(None) => {}
                Err(error) => metadata_log(format!("arXiv lookup failed id={arxiv_id}: {error}")),
            }
        }

        // Stage: LLM search-field extraction, EARLY (RFC 0049). Runs whenever
        // page 1 carries no identifier — turning messy page-1 text into search
        // fields is what the model is good at. Hallucination is handled by the
        // verification gate, not by ordering.
        let mut llm_candidate: Option<MetadataCandidate> = None;
        if !has_page1_id {
            match self
                .metadata_candidate_from_llm(paper_id, evidence, current_paper)
                .await
            {
                Ok(Some(candidate)) => {
                    candidates.extend(
                        self.validated_candidates_from_llm(paper_id, &candidate)
                            .await,
                    );
                    candidates.push(candidate.clone());
                    llm_candidate = Some(candidate);
                }
                Ok(None) => {}
                Err(error) => metadata_log(format!(
                    "LLM metadata extraction failed paper_id={paper_id}: {error}"
                )),
            }
        }

        // Stage: fielded provider fan-out driven by the search plan (RFC
        // 0050). The LLM extraction becomes the query — title plus author
        // last names — instead of being just another candidate.
        let plan = build_search_plan(evidence, llm_candidate.as_ref());
        if !has_page1_id {
            if let Some(title) = plan.title.clone() {
                // arXiv title(+author) search: recent preprints often exist
                // only on arXiv, which id-only lookup never reached.
                self.emit_progress(
                    paper_id,
                    "running",
                    "arxiv",
                    &format!("Searching arXiv for: {title}"),
                    Vec::new(),
                );
                match self
                    .lookup_arxiv_by_title(&title, &plan.author_last_names)
                    .await
                {
                    Ok(found) => candidates.extend(found),
                    Err(error) => {
                        metadata_log(format!("arXiv title search failed title={title}: {error}"))
                    }
                }

                self.emit_progress(
                    paper_id,
                    "running",
                    "crossref",
                    &format!("Searching Crossref for: {title}"),
                    Vec::new(),
                );
                match self
                    .lookup_crossref_fielded(&title, &plan.author_last_names)
                    .await
                {
                    Ok(found) => candidates.extend(found),
                    Err(error) => metadata_log(format!("Crossref fielded lookup failed: {error}")),
                }
            }

            // Fallback: raw-blob bibliographic matching, only when the fielded
            // fan-out produced nothing provider-backed.
            if !candidates.iter().any(is_provider_backed) {
                self.emit_progress(
                    paper_id,
                    "running",
                    "crossref",
                    "Matching page-1 text against Crossref...",
                    Vec::new(),
                );
                match self
                    .lookup_crossref_bibliographic(&evidence.page1_text)
                    .await
                {
                    Ok(found) => candidates.extend(found),
                    Err(error) => {
                        metadata_log(format!("Crossref bibliographic lookup failed: {error}"))
                    }
                }
            }
        }

        // Stage: OpenAlex title search over the strong title candidates only
        // (largest page-1 font, embedded metadata, LLM title). Every returned
        // result is scored by the gate — score-all, keep-best (RFC 0050).
        let llm_title = llm_candidate
            .as_ref()
            .map(|candidate| candidate.title.as_str());
        for title in title_queries(evidence, llm_title, current_paper) {
            self.emit_progress(
                paper_id,
                "running",
                "openalex",
                &format!("Checking OpenAlex title match: {title}"),
                Vec::new(),
            );
            match self.lookup_openalex_by_title(&title).await {
                Ok(found) => {
                    candidates.extend(found.into_iter().map(|candidate| {
                        metadata_candidate_from_paper_candidate(
                            candidate,
                            vec!["openalex".to_string()],
                            0.0,
                            vec![format!("title search: {title}")],
                        )
                    }));
                }
                Err(error) => metadata_log(format!(
                    "OpenAlex title lookup failed title={title}: {error}"
                )),
            }
        }

        if candidates.is_empty() {
            candidates.push(metadata_candidate_from_local_clues(evidence, current_paper));
        }

        // Stage: verification gate. Merge equivalent candidates first so
        // provider corroboration is visible, then score each against page-1
        // evidence.
        self.emit_progress(
            paper_id,
            "running",
            "verifying",
            &format!(
                "Verifying {} candidate(s) against the PDF...",
                candidates.len()
            ),
            Vec::new(),
        );
        let mut merged = merge_metadata_candidates(candidates);
        for candidate in &mut merged {
            score_candidate(candidate, evidence);
        }
        merged.sort_by(|left, right| {
            right
                .confidence
                .partial_cmp(&left.confidence)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        Ok(merged)
    }

    /// arXiv fielded title(+author) search with a free-text fallback —
    /// fielded Lucene queries are brittle around quoting (RFC 0050).
    async fn lookup_arxiv_by_title(
        &self,
        title: &str,
        author_last_names: &[String],
    ) -> MetadataResult<Vec<MetadataCandidate>> {
        let quoted = title.replace('"', " ");
        let mut request = provider_request(
            &format!("ti:\"{}\"", quoted.trim()),
            DiscoveryProviderChoice::Arxiv,
        );
        request.authors = author_last_names.to_vec();
        let mut found = self
            .arxiv
            .search(&request)
            .await
            .map_err(|error| error.to_string())?
            .candidates;
        if found.is_empty() {
            let request = provider_request(title, DiscoveryProviderChoice::Arxiv);
            found = self
                .arxiv
                .search(&request)
                .await
                .map_err(|error| error.to_string())?
                .candidates;
        }
        Ok(found
            .into_iter()
            .map(|candidate| {
                metadata_candidate_from_paper_candidate(
                    candidate,
                    vec!["arxiv".to_string()],
                    0.0,
                    vec![format!("arXiv title search: {title}")],
                )
            })
            .collect())
    }

    /// Crossref fielded search: `query.title` + `query.author` is far more
    /// precise than the raw-blob bibliographic query (RFC 0050).
    async fn lookup_crossref_fielded(
        &self,
        title: &str,
        author_last_names: &[String],
    ) -> MetadataResult<Vec<MetadataCandidate>> {
        let mut params = vec![
            ("query.title", title.to_string()),
            ("rows", CROSSREF_FIELDED_ROWS.to_string()),
            (
                "select",
                "DOI,title,author,issued,container-title,abstract".to_string(),
            ),
        ];
        if !author_last_names.is_empty() {
            params.push(("query.author", author_last_names.join(" ")));
        }
        self.crossref_list(params, "Crossref title+author match")
            .await
    }

    async fn lookup_crossref_bibliographic(
        &self,
        page1_text: &str,
    ) -> MetadataResult<Vec<MetadataCandidate>> {
        let blob = bibliographic_query_blob(page1_text);
        if blob.len() < 40 {
            return Ok(Vec::new());
        }

        let params = vec![
            ("query.bibliographic", blob),
            ("rows", "3".to_string()),
            (
                "select",
                "DOI,title,author,issued,container-title,abstract".to_string(),
            ),
        ];
        self.crossref_list(params, "Crossref bibliographic match over page-1 text")
            .await
    }

    async fn crossref_list(
        &self,
        mut params: Vec<(&str, String)>,
        evidence_note: &str,
    ) -> MetadataResult<Vec<MetadataCandidate>> {
        if let Some(mailto) = crossref_mailto() {
            params.push(("mailto", mailto));
        }
        let url = reqwest::Url::parse_with_params("https://api.crossref.org/works", &params)
            .map_err(|error| format!("Invalid Crossref query: {error}"))?;

        let response = self
            .client
            .get(url)
            .header("User-Agent", crossref_user_agent())
            .send()
            .await
            .map_err(|error| format!("Crossref request failed: {error}"))?;
        if !response.status().is_success() {
            let status = response.status();
            return Err(format!("Crossref list query failed with {status}"));
        }

        let payload = response
            .json::<CrossrefListResponse>()
            .await
            .map_err(|error| format!("Invalid Crossref list response: {error}"))?;

        Ok(payload
            .message
            .items
            .into_iter()
            .filter_map(|work| {
                crossref_work_to_metadata_candidate(work, 0.0, vec![evidence_note.to_string()])
            })
            .collect())
    }

    async fn metadata_candidate_from_llm(
        &self,
        paper_id: &str,
        evidence: &PdfEvidence,
        current_paper: &Paper,
    ) -> MetadataResult<Option<MetadataCandidate>> {
        self.emit_progress(
            paper_id,
            "running",
            "llm_extracting",
            "Preparing model metadata extraction...",
            Vec::new(),
        );

        if evidence.page1_text.trim().is_empty() {
            metadata_log(format!(
                "skip LLM fallback paper_id={paper_id} no extracted text"
            ));
            self.emit_progress(
                paper_id,
                "running",
                "llm_unavailable",
                "No visible PDF text for model metadata extraction; using local clues.",
                Vec::new(),
            );
            return Ok(None);
        }

        let Some(config) = &self.llm_config else {
            metadata_log(format!(
                "skip LLM fallback paper_id={paper_id} no chat config"
            ));
            self.emit_progress(
                paper_id,
                "running",
                "llm_unavailable",
                "Model metadata extraction is unavailable; using local clues.",
                Vec::new(),
            );
            return Ok(None);
        };

        let api_key = match config.resolve_api_key() {
            Ok(api_key) => api_key,
            Err(error) => {
                metadata_log(format!(
                    "skip LLM fallback paper_id={paper_id} missing key: {error}"
                ));
                self.emit_progress(
                    paper_id,
                    "running",
                    "llm_unavailable",
                    "OpenRouter key is missing; using local clues.",
                    Vec::new(),
                );
                return Ok(None);
            }
        };

        self.emit_progress(
            paper_id,
            "running",
            "llm_extracting",
            "Asking the model to read visible PDF metadata...",
            Vec::new(),
        );

        let request = CompletionRequest {
            model: config.model.clone(),
            messages: vec![
                WireMessage::text("system", LLM_METADATA_SYSTEM_PROMPT.to_string()),
                WireMessage::text("user", llm_metadata_user_prompt(evidence, current_paper)),
            ],
            stream: false,
            max_tokens: Some(LLM_METADATA_MAX_TOKENS),
            response_format: Some(ResponseFormat::json_object()),
            tools: None,
            tool_choice: None,
        };

        let raw = llm::complete(&self.client, &config.url, &api_key, &request).await?;
        let value = extract_json_object_value(&raw)?;
        let parsed: LlmMetadataResponse = serde_json::from_value(value)
            .map_err(|error| format!("LLM metadata JSON shape was invalid: {error}"))?;

        Ok(metadata_candidate_from_llm_response(parsed))
    }

    async fn validated_candidates_from_llm(
        &self,
        paper_id: &str,
        candidate: &MetadataCandidate,
    ) -> Vec<MetadataCandidate> {
        let mut candidates = Vec::new();

        if let Some(doi) = &candidate.doi {
            self.emit_progress(
                paper_id,
                "running",
                "llm_validating",
                &format!("Validating model DOI {doi}..."),
                Vec::new(),
            );
            match self.lookup_crossref_by_doi(doi).await {
                Ok(Some(candidate)) => candidates.push(candidate),
                Ok(None) => {}
                Err(error) => {
                    metadata_log(format!("Crossref LLM DOI lookup failed doi={doi}: {error}"))
                }
            }
            match self.lookup_openalex_by_doi(doi).await {
                Ok(Some(candidate)) => candidates.push(metadata_candidate_from_paper_candidate(
                    candidate,
                    vec!["openalex".to_string()],
                    0.98,
                    vec![format!(
                        "LLM DOI validated by OpenAlex: {}",
                        normalize_doi(doi)
                    )],
                )),
                Ok(None) => {}
                Err(error) => {
                    metadata_log(format!("OpenAlex LLM DOI lookup failed doi={doi}: {error}"))
                }
            }
        }

        if let Some(arxiv_id) = &candidate.arxiv_id {
            self.emit_progress(
                paper_id,
                "running",
                "llm_validating",
                &format!("Validating model arXiv id {arxiv_id}..."),
                Vec::new(),
            );
            match self.lookup_arxiv_by_id(arxiv_id).await {
                Ok(Some(candidate)) => candidates.push(metadata_candidate_from_paper_candidate(
                    candidate,
                    vec!["arxiv".to_string()],
                    0.98,
                    vec![format!(
                        "LLM arXiv id validated by arXiv: {}",
                        normalize_arxiv_id(arxiv_id)
                    )],
                )),
                Ok(None) => {}
                Err(error) => {
                    metadata_log(format!("arXiv LLM lookup failed id={arxiv_id}: {error}"))
                }
            }
        }

        candidates
    }

    async fn lookup_crossref_by_doi(&self, doi: &str) -> MetadataResult<Option<MetadataCandidate>> {
        let normalized = normalize_doi(doi);
        let mut url = format!("https://api.crossref.org/works/{normalized}");
        if let Some(mailto) = crossref_mailto() {
            url.push_str("?mailto=");
            url.push_str(&mailto);
        }

        let response = self
            .client
            .get(url)
            .header("User-Agent", crossref_user_agent())
            .send()
            .await
            .map_err(|error| format!("Crossref request failed: {error}"))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(format!("Crossref request failed with {status}: {body}"));
        }

        let payload = response
            .json::<CrossrefWorksResponse>()
            .await
            .map_err(|error| format!("Invalid Crossref response: {error}"))?;

        Ok(crossref_work_to_metadata_candidate(
            payload.message,
            0.0,
            vec![format!("Crossref DOI lookup: {normalized}")],
        ))
    }

    async fn lookup_openalex_by_doi(&self, doi: &str) -> MetadataResult<Option<PaperCandidate>> {
        let result = self
            .openalex
            .search(&provider_request(doi, DiscoveryProviderChoice::OpenAlex))
            .await
            .map_err(|error| error.to_string())?;
        let normalized = normalize_doi(doi);
        Ok(result.candidates.into_iter().find(|candidate| {
            candidate.doi.as_deref().map(normalize_doi) == Some(normalized.clone())
        }))
    }

    /// Returns every top result; the verification gate is the single arbiter
    /// (score-all, RFC 0050) instead of a local first-match similarity filter.
    async fn lookup_openalex_by_title(&self, title: &str) -> MetadataResult<Vec<PaperCandidate>> {
        let result = self
            .openalex
            .search(&provider_request(title, DiscoveryProviderChoice::OpenAlex))
            .await
            .map_err(|error| error.to_string())?;

        Ok(result.candidates)
    }

    async fn lookup_arxiv_by_id(&self, arxiv_id: &str) -> MetadataResult<Option<PaperCandidate>> {
        let result = self
            .arxiv
            .search(&provider_request(
                &format!("id:{arxiv_id}"),
                DiscoveryProviderChoice::Arxiv,
            ))
            .await
            .map_err(|error| error.to_string())?;
        let normalized = normalize_arxiv_id(arxiv_id);
        Ok(result.candidates.into_iter().find(|candidate| {
            candidate.arxiv_id.as_deref().map(normalize_arxiv_id) == Some(normalized.clone())
        }))
    }

    fn emit_update(&self, paper_id: &str, status: &str, error: Option<String>) {
        let payload = PaperMetadataUpdated {
            paper_id: paper_id.to_string(),
            status: status.to_string(),
            error,
        };
        let _ = self.app.emit("paper_metadata_updated", payload);
    }

    fn emit_progress(
        &self,
        paper_id: &str,
        status: &str,
        stage: &str,
        message: &str,
        candidates: Vec<MetadataCandidate>,
    ) {
        let payload = MetadataAutofillProgress {
            paper_id: paper_id.to_string(),
            status: status.to_string(),
            stage: stage.to_string(),
            message: message.to_string(),
            candidates,
        };
        let _ = self.app.emit("metadata_autofill_progress", payload);
    }

    fn mark_queued(&self, paper_id: &str) -> bool {
        let mut queued_or_active = self
            .queued_or_active
            .lock()
            .expect("metadata enrichment queue lock should not be poisoned");
        queued_or_active.insert(paper_id.to_string())
    }

    fn mark_finished(&self, paper_id: &str) {
        let mut queued_or_active = self
            .queued_or_active
            .lock()
            .expect("metadata enrichment queue lock should not be poisoned");
        queued_or_active.remove(paper_id);
    }
}

fn extract_pdf_evidence(
    app: Option<&AppHandle>,
    config: &PdfExtractionConfig,
    path: &Path,
) -> MetadataResult<PdfEvidence> {
    let pdfium = bind_pdfium(app, config)?;
    let document = pdfium
        .load_pdf_from_file(path, None)
        .map_err(|error| format!("Pdfium could not open {}: {error}", path.display()))?;
    let embedded_title = document
        .metadata()
        .get(PdfDocumentMetadataTagType::Title)
        .map(|tag| clean_metadata_text(tag.value()))
        .filter(|title| plausible_title(title));
    let embedded_authors = document
        .metadata()
        .get(PdfDocumentMetadataTagType::Author)
        .map(|tag| split_authors(tag.value()))
        .unwrap_or_default();

    let mut page_texts = Vec::new();
    let mut largest_font_title = None;
    for (index, page) in document
        .pages()
        .iter()
        .take(FIRST_PAGE_TEXT_LIMIT)
        .enumerate()
    {
        let Ok(text) = page.text() else {
            continue;
        };
        if index == 0 {
            largest_font_title = largest_font_run(&text).filter(|title| plausible_title(title));
        }
        page_texts.push(text.all());
    }

    let page1_text = page_texts.first().cloned().unwrap_or_default();
    let later_text = page_texts.get(1..).unwrap_or_default().join("\n\n");
    let page1_doi = extract_doi(&page1_text);
    let later_doi = extract_doi(&later_text);
    let page1_arxiv_id = extract_arxiv_id(&page1_text);
    let later_arxiv_id = extract_arxiv_id(&later_text);

    Ok(PdfEvidence {
        embedded_title,
        embedded_authors,
        largest_font_title,
        page1_text,
        page1_doi,
        later_doi,
        page1_arxiv_id,
        later_arxiv_id,
    })
}

/// The contiguous run of characters at (close to) the page's largest font
/// size. On scholarly PDFs this is the title far more reliably than any
/// line-based heuristic over the extraction-ordered text dump.
fn largest_font_run(text: &PdfPageText) -> Option<String> {
    let chars: Vec<(Option<char>, f32)> = text
        .chars()
        .iter()
        .map(|character| (character.unicode_char(), character.scaled_font_size().value))
        .collect();

    let max_size = chars
        .iter()
        .filter(|(character, _)| character.map(|c| c.is_alphabetic()).unwrap_or(false))
        .map(|(_, size)| *size)
        .fold(0.0_f32, f32::max);
    if max_size <= 0.0 {
        return None;
    }

    // Small-caps titles mix two sizes (large initials + smaller capitals,
    // e.g. 17.2pt/13.8pt) while body text sits far lower (~10pt). A 0.75
    // threshold keeps both caps tiers in one run without pulling in body text.
    let threshold = max_size * 0.75;
    let mut runs: Vec<String> = Vec::new();
    let mut current = String::new();
    for (character, size) in chars {
        // Whitespace, control glyphs (soft hyphens, ligature markers), and
        // untranslatable chars often report size 0 — they bridge a run, they
        // never break it. Only a sub-threshold visible glyph ends the run.
        let is_soft = character
            .map(|c| c.is_whitespace() || c.is_control())
            .unwrap_or(true);
        if is_soft {
            if !current.is_empty() {
                current.push(' ');
            }
            continue;
        }

        if size >= threshold {
            current.push(character.expect("soft chars are handled above"));
        } else if !current.trim().is_empty() {
            runs.push(current.clone());
            current.clear();
        } else {
            current.clear();
        }
    }
    if !current.trim().is_empty() {
        runs.push(current);
    }

    runs.into_iter()
        .map(|run| clean_metadata_text(&run))
        .filter(|run| (15..=300).contains(&run.len()))
        .max_by_key(String::len)
}

fn bind_pdfium(app: Option<&AppHandle>, config: &PdfExtractionConfig) -> MetadataResult<Pdfium> {
    let mut errors = Vec::new();
    for candidate in pdfium_library_candidates(app, config) {
        match bind_pdfium_library(&candidate) {
            Ok(pdfium) => return Ok(pdfium),
            Err(error) => errors.push(format!("{} ({error})", candidate.display())),
        }
    }

    match Pdfium::bind_to_system_library() {
        Ok(bindings) => Ok(Pdfium::new(bindings)),
        Err(error) => {
            errors.push(format!("system library ({error})"));
            Err(format!(
                "Pdfium library not found for metadata enrichment. Tried: {}",
                errors.join("; ")
            ))
        }
    }
}

fn pdfium_library_candidates(
    app: Option<&AppHandle>,
    config: &PdfExtractionConfig,
) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(path) = &config.pdfium_library_path {
        candidates.push(path.clone());
    }

    if let Some(resource_dir) = app.and_then(|app| app.path().resource_dir().ok()) {
        candidates.push(resource_dir.join("libpdfium.dylib"));
        candidates.push(resource_dir.join("pdfium").join("libpdfium.dylib"));
    }

    if let Ok(cwd) = std::env::current_dir() {
        candidates.push(cwd.join("resources/pdfium/libpdfium.dylib"));
        if let Some(repo_root) = cwd.parent() {
            candidates.push(repo_root.join("src-tauri/resources/pdfium/libpdfium.dylib"));
        }
    }

    candidates
}

fn bind_pdfium_library(path: &Path) -> MetadataResult<Pdfium> {
    let library_path = if path.is_dir() {
        Pdfium::pdfium_platform_library_name_at_path(path)
    } else {
        path.to_path_buf()
    };

    if !library_path.exists() {
        return Err("library path does not exist".to_string());
    }

    match Pdfium::bind_to_library(library_path) {
        Ok(bindings) => Ok(Pdfium::new(bindings)),
        Err(PdfiumError::PdfiumLibraryBindingsAlreadyInitialized) => Ok(Pdfium::default()),
        Err(error) => Err(error.to_string()),
    }
}

fn provider_request(query: &str, provider: DiscoveryProviderChoice) -> DiscoverySearchRequest {
    DiscoverySearchRequest {
        query: query.to_string(),
        year_from: None,
        year_to: None,
        result_limit: PROVIDER_RESULT_LIMIT,
        sort_by: DiscoverySort::Relevance,
        provider,
        providers: vec![provider],
        open_access: false,
        only_viewable: false,
        venues: Vec::new(),
        authors: Vec::new(),
        fields_of_study: Vec::new(),
    }
}

fn metadata_candidate_from_paper_candidate(
    candidate: PaperCandidate,
    providers: Vec<String>,
    confidence: f64,
    evidence: Vec<String>,
) -> MetadataCandidate {
    let id = candidate
        .doi
        .as_deref()
        .map(normalize_doi)
        .or_else(|| candidate.arxiv_id.as_deref().map(normalize_arxiv_id))
        .unwrap_or_else(|| normalize_title(&candidate.title));

    MetadataCandidate {
        id,
        title: candidate.title,
        authors: candidate.authors,
        venue: candidate.venue,
        year: candidate.year,
        doi: candidate.doi,
        arxiv_id: candidate.arxiv_id,
        abstract_text: candidate.abstract_text,
        providers,
        confidence,
        evidence,
    }
}

fn metadata_candidate_from_local_clues(
    evidence: &PdfEvidence,
    current_paper: &Paper,
) -> MetadataCandidate {
    let title = title_queries(evidence, None, current_paper)
        .into_iter()
        .next()
        .unwrap_or_else(|| "Imported PDF".to_string());

    let evidence_note = if evidence.largest_font_title.as_deref() == Some(title.as_str()) {
        "Largest-font page-1 text"
    } else if evidence.embedded_title.as_deref() == Some(title.as_str()) {
        "PDF embedded title"
    } else if current_paper.title == title {
        "Current imported title fallback"
    } else {
        "Local PDF fallback"
    };

    let confidence = if evidence.largest_font_title.is_some()
        || evidence.embedded_title.is_some()
        || !evidence.embedded_authors.is_empty()
    {
        0.35
    } else {
        0.25
    };

    MetadataCandidate {
        id: normalize_title(&title),
        title,
        authors: evidence.embedded_authors.clone(),
        venue: None,
        year: None,
        doi: evidence.best_doi().cloned(),
        arxiv_id: evidence.best_arxiv_id().cloned(),
        abstract_text: None,
        providers: vec!["local_pdf".to_string()],
        confidence,
        evidence: vec![evidence_note.to_string()],
    }
}

fn merge_metadata_candidates(candidates: Vec<MetadataCandidate>) -> Vec<MetadataCandidate> {
    let mut merged: Vec<MetadataCandidate> = Vec::new();
    for candidate in candidates {
        if let Some(existing) = merged
            .iter_mut()
            .find(|existing| same_candidate(existing, &candidate))
        {
            existing.confidence = existing.confidence.max(candidate.confidence);
            for provider in candidate.providers {
                if !existing.providers.contains(&provider) {
                    existing.providers.push(provider);
                }
            }
            for evidence in candidate.evidence {
                if !existing.evidence.contains(&evidence) {
                    existing.evidence.push(evidence);
                }
            }
            if existing.abstract_text.is_none() {
                existing.abstract_text = candidate.abstract_text;
            }
            if existing.venue.is_none() {
                existing.venue = candidate.venue;
            }
            if existing.year.is_none() {
                existing.year = candidate.year;
            }
            // Backfill identifiers so preprint/published versions of the same
            // paper accrue corroboration instead of splitting (RFC 0050).
            if existing.doi.is_none() {
                existing.doi = candidate.doi;
            }
            if existing.arxiv_id.is_none() {
                existing.arxiv_id = candidate.arxiv_id;
            }
        } else {
            merged.push(candidate);
        }
    }

    merged.sort_by(|left, right| {
        right
            .confidence
            .partial_cmp(&left.confidence)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    merged
}

fn same_candidate(left: &MetadataCandidate, right: &MetadataCandidate) -> bool {
    match (&left.doi, &right.doi) {
        (Some(left_doi), Some(right_doi))
            if normalize_doi(left_doi) == normalize_doi(right_doi) =>
        {
            return true
        }
        _ => {}
    }
    match (&left.arxiv_id, &right.arxiv_id) {
        (Some(left_id), Some(right_id))
            if normalize_arxiv_id(left_id) == normalize_arxiv_id(right_id) =>
        {
            return true
        }
        _ => {}
    }
    titles_similar(&left.title, &right.title)
}

const LLM_METADATA_SYSTEM_PROMPT: &str = "You extract scholarly PDF metadata from visible text. \
Reply with one JSON object only. Use null for unknown fields. Do not invent details. \
Schema: {\"title\":string|null,\"authors\":string[],\"venue\":string|null,\"year\":number|null,\
\"doi\":string|null,\"arxivId\":string|null,\"abstract\":string|null,\"confidence\":number,\
\"evidence\":string[]}. Confidence is your confidence that the fields appear in the text.";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LlmMetadataResponse {
    title: Option<String>,
    #[serde(default)]
    authors: Vec<String>,
    venue: Option<String>,
    year: Option<i32>,
    doi: Option<String>,
    #[serde(default, alias = "arxiv_id")]
    arxiv_id: Option<String>,
    #[serde(rename = "abstract")]
    abstract_text: Option<String>,
    confidence: Option<f64>,
    #[serde(default)]
    evidence: Vec<String>,
}

fn llm_metadata_user_prompt(evidence: &PdfEvidence, current_paper: &Paper) -> String {
    format!(
        "Current imported title: {}\nPDF embedded title: {}\nPDF embedded authors: {}\n\
Largest-font text on page 1: {}\n\n\
Visible text from PDF page 1, possibly noisy:\n{}",
        current_paper.title,
        evidence.embedded_title.as_deref().unwrap_or("unknown"),
        if evidence.embedded_authors.is_empty() {
            "unknown".to_string()
        } else {
            evidence.embedded_authors.join(", ")
        },
        evidence.largest_font_title.as_deref().unwrap_or("unknown"),
        evidence_text_for_llm(&evidence.page1_text)
    )
}

fn evidence_text_for_llm(text: &str) -> String {
    text.chars().take(LLM_EVIDENCE_CHAR_LIMIT).collect()
}

fn metadata_candidate_from_llm_response(
    response: LlmMetadataResponse,
) -> Option<MetadataCandidate> {
    let title = response
        .title
        .as_deref()
        .map(clean_metadata_text)
        .filter(|title| plausible_title(title))?;
    let authors = response
        .authors
        .into_iter()
        .map(|author| clean_metadata_text(&author))
        .filter(|author| !author.is_empty())
        .collect::<Vec<_>>();
    let confidence = llm_candidate_confidence(response.confidence);
    let mut evidence = response
        .evidence
        .into_iter()
        .map(|item| clean_metadata_text(&item))
        .filter(|item| !item.is_empty())
        .collect::<Vec<_>>();
    if evidence.is_empty() {
        evidence.push("LLM extracted metadata from PDF text".to_string());
    }

    Some(MetadataCandidate {
        id: response
            .doi
            .as_deref()
            .map(normalize_doi)
            .or_else(|| response.arxiv_id.as_deref().map(normalize_arxiv_id))
            .unwrap_or_else(|| normalize_title(&title)),
        title,
        authors,
        venue: clean_optional_metadata_text(response.venue),
        year: response.year.filter(|year| (1400..=2200).contains(year)),
        doi: response.doi.as_deref().map(normalize_doi),
        arxiv_id: response.arxiv_id.as_deref().map(normalize_arxiv_id),
        abstract_text: clean_optional_metadata_text(response.abstract_text),
        providers: vec!["llm_metadata".to_string()],
        confidence,
        evidence,
    })
}

fn llm_candidate_confidence(confidence: Option<f64>) -> f64 {
    let confidence = confidence.unwrap_or(0.65);
    if confidence.is_finite() {
        confidence.clamp(0.55, 0.74)
    } else {
        0.65
    }
}

fn extract_json_object_value(raw: &str) -> MetadataResult<serde_json::Value> {
    let start = raw.find('{');
    let end = raw.rfind('}');
    match (start, end) {
        (Some(start), Some(end)) if end >= start => serde_json::from_str(&raw[start..=end])
            .map_err(|error| format!("LLM metadata JSON parse failed: {error}")),
        _ => Err("LLM metadata response did not contain a JSON object.".to_string()),
    }
}

#[derive(Debug, Deserialize)]
struct CrossrefWorksResponse {
    message: CrossrefWork,
}

#[derive(Debug, Deserialize)]
struct CrossrefListResponse {
    message: CrossrefListMessage,
}

#[derive(Debug, Deserialize)]
struct CrossrefListMessage {
    #[serde(default)]
    items: Vec<CrossrefWork>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
struct CrossrefWork {
    #[serde(rename = "DOI")]
    doi: Option<String>,
    title: Option<Vec<String>>,
    author: Option<Vec<CrossrefAuthor>>,
    container_title: Option<Vec<String>>,
    published_print: Option<CrossrefDate>,
    published_online: Option<CrossrefDate>,
    published: Option<CrossrefDate>,
    issued: Option<CrossrefDate>,
    #[serde(rename = "abstract")]
    abstract_text: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CrossrefAuthor {
    given: Option<String>,
    family: Option<String>,
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
struct CrossrefDate {
    date_parts: Option<Vec<Vec<i32>>>,
}

fn crossref_work_to_metadata_candidate(
    work: CrossrefWork,
    confidence: f64,
    evidence: Vec<String>,
) -> Option<MetadataCandidate> {
    let title = work
        .title
        .as_ref()
        .and_then(|titles| titles.first())
        .map(|title| clean_metadata_text(title))
        .filter(|title| plausible_title(title))?;
    let doi = work.doi.map(|doi| normalize_doi(&doi));
    let year = work
        .published_print
        .as_ref()
        .or(work.published_online.as_ref())
        .or(work.published.as_ref())
        .or(work.issued.as_ref())
        .and_then(crossref_year);
    let venue = work
        .container_title
        .as_ref()
        .and_then(|titles| titles.first())
        .map(|venue| clean_metadata_text(venue))
        .filter(|venue| !venue.is_empty());

    Some(MetadataCandidate {
        id: doi.clone().unwrap_or_else(|| normalize_title(&title)),
        title,
        authors: work
            .author
            .unwrap_or_default()
            .into_iter()
            .filter_map(crossref_author_name)
            .collect(),
        venue,
        year,
        doi,
        arxiv_id: None,
        abstract_text: work.abstract_text.map(|text| clean_metadata_text(&text)),
        providers: vec!["crossref".to_string()],
        confidence,
        evidence,
    })
}

fn crossref_year(date: &CrossrefDate) -> Option<i32> {
    date.date_parts
        .as_ref()
        .and_then(|parts| parts.first())
        .and_then(|parts| parts.first())
        .copied()
}

fn crossref_author_name(author: CrossrefAuthor) -> Option<String> {
    if let Some(name) = author.name {
        let cleaned = clean_metadata_text(&name);
        return (!cleaned.is_empty()).then_some(cleaned);
    }

    let name = [author.given, author.family]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" ");
    let cleaned = clean_metadata_text(&name);
    (!cleaned.is_empty()).then_some(cleaned)
}

fn crossref_user_agent() -> String {
    match crossref_mailto() {
        Some(mailto) => format!("i0i/0.1.0 (mailto:{mailto})"),
        None => "i0i/0.1.0".to_string(),
    }
}

fn crossref_mailto() -> Option<String> {
    std::env::var("I0I_CROSSREF_MAILTO")
        .ok()
        .or_else(|| read_dotenv_value("I0I_CROSSREF_MAILTO"))
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

/// Strong title candidates only: largest page-1 font run, embedded PDF
/// metadata, the LLM-extracted title, and the imported (filename) title.
/// RFC 0049 removed the old line-based heuristics — they fed noise into
/// provider queries because PDF text order is not visual order.
fn title_queries(
    evidence: &PdfEvidence,
    llm_title: Option<&str>,
    current_paper: &Paper,
) -> Vec<String> {
    let mut queries = Vec::new();
    let mut seen = HashSet::new();

    let mut raw_titles = Vec::new();
    if let Some(title) = &evidence.largest_font_title {
        raw_titles.push(title.clone());
    }
    if let Some(title) = &evidence.embedded_title {
        raw_titles.push(title.clone());
    }
    if let Some(title) = llm_title {
        raw_titles.push(title.to_string());
    }
    // The filename-derived title is a query of last resort: it only runs when
    // no extracted title survived the plausibility filter (RFC 0050).
    if !raw_titles
        .iter()
        .any(|title| plausible_title(&clean_metadata_text(title)))
    {
        raw_titles.push(current_paper.title.clone());
    }

    for title in raw_titles
        .into_iter()
        .map(|title| clean_metadata_text(&title))
        .filter(|title| plausible_title(title))
    {
        let normalized = normalize_title(&title);
        if seen.insert(normalized) {
            queries.push(title);
        }
    }

    queries
}

/// The blob sent to Crossref `query.bibliographic`: leading page-1 lines,
/// capped. Crossref's server-side fuzzy matching handles the noise.
fn bibliographic_query_blob(page1_text: &str) -> String {
    let blob = page1_text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .take(50)
        .collect::<Vec<_>>()
        .join(" ");
    blob.chars().take(BIBLIOGRAPHIC_QUERY_CHAR_LIMIT).collect()
}

fn verification_for(candidate: &MetadataCandidate, evidence: &PdfEvidence) -> Verification {
    let page1_doi = evidence.page1_doi.as_deref().map(normalize_doi);
    let page1_arxiv = evidence.page1_arxiv_id.as_deref().map(normalize_arxiv_id);
    let doi_matches = match (&candidate.doi, &page1_doi) {
        (Some(candidate_doi), Some(page1_doi)) => normalize_doi(candidate_doi) == *page1_doi,
        _ => false,
    };
    let arxiv_matches = match (&candidate.arxiv_id, &page1_arxiv) {
        (Some(candidate_id), Some(page1_id)) => normalize_arxiv_id(candidate_id) == *page1_id,
        _ => false,
    };
    if doi_matches || arxiv_matches {
        return Verification::Page1IdMatch;
    }

    if title_on_page1(&candidate.title, &evidence.page1_text) {
        return Verification::TitleOnPage1;
    }

    let similar_to_strong_title = [
        evidence.largest_font_title.as_deref(),
        evidence.embedded_title.as_deref(),
    ]
    .into_iter()
    .flatten()
    .any(|title| titles_similar(&candidate.title, title));
    if similar_to_strong_title {
        return Verification::TitleSimilar;
    }

    Verification::Unverified
}

/// The RFC 0049 core check: a candidate's normalized title must literally
/// occur in the normalized page-1 text. Works regardless of extraction order
/// because both sides collapse to lowercase alphanumeric words.
fn title_on_page1(candidate_title: &str, page1_text: &str) -> bool {
    let title = normalize_title(candidate_title);
    if title.len() < 12 {
        return false;
    }
    normalize_title(page1_text).contains(&title)
}

fn is_provider_backed(candidate: &MetadataCandidate) -> bool {
    candidate
        .providers
        .iter()
        .any(|provider| provider != "llm_metadata" && provider != "local_pdf")
}

/// RFC 0050 author corroboration: fraction of the candidate's first three
/// authors whose normalized last name occurs as a word on page 1.
fn authors_on_page1(candidate_authors: &[String], page1_text: &str) -> AuthorMatch {
    let last_names: Vec<String> = candidate_authors
        .iter()
        .take(SEARCH_AUTHOR_LIMIT)
        .filter_map(|author| author_last_name(author))
        .collect();
    if last_names.is_empty() {
        return AuthorMatch::None;
    }

    let page1 = normalize_title(page1_text);
    let words: HashSet<&str> = page1.split_whitespace().collect();
    let matched = last_names
        .iter()
        .filter(|name| words.contains(name.as_str()))
        .count();

    if matched * 3 >= last_names.len() * 2 {
        AuthorMatch::Strong
    } else if matched >= 1 {
        AuthorMatch::Weak
    } else {
        AuthorMatch::None
    }
}

fn score_candidate(candidate: &mut MetadataCandidate, evidence: &PdfEvidence) {
    let provider_backed = is_provider_backed(candidate);
    let author_match = authors_on_page1(&candidate.authors, &evidence.page1_text);
    let strong_authors = author_match == AuthorMatch::Strong;

    let (confidence, note) = match verification_for(candidate, evidence) {
        Verification::Page1IdMatch if provider_backed => {
            (CONFIDENCE_PAGE1_ID, "verified: page-1 identifier match")
        }
        Verification::TitleOnPage1 if provider_backed && strong_authors => (
            CONFIDENCE_VERIFIED_PROVIDER_AUTHORS,
            "verified: title and authors found on page 1",
        ),
        Verification::TitleOnPage1 if provider_backed => (
            CONFIDENCE_VERIFIED_PROVIDER,
            "verified: title found on page 1",
        ),
        Verification::Page1IdMatch | Verification::TitleOnPage1 if strong_authors => (
            CONFIDENCE_VERIFIED_SINGLE_AUTHORS,
            "verified: title and authors on page 1 (single source)",
        ),
        Verification::Page1IdMatch | Verification::TitleOnPage1 => (
            CONFIDENCE_VERIFIED_SINGLE,
            "verified: title found on page 1 (single source)",
        ),
        Verification::TitleSimilar => (
            match author_match {
                AuthorMatch::Strong => 0.72,
                AuthorMatch::Weak => 0.66,
                AuthorMatch::None => 0.60,
            },
            "partially verified: similar to page-1 title",
        ),
        Verification::Unverified => (
            candidate
                .confidence
                .min(CONFIDENCE_UNVERIFIED_CAP)
                .max(0.25),
            "unverified: no page-1 evidence for this candidate",
        ),
    };

    candidate.confidence = confidence;
    candidate.evidence.push(note.to_string());
}

fn extract_doi(text: &str) -> Option<String> {
    let pattern = Regex::new(r"(?i)\b10\.\d{4,9}/[-._;()/:A-Z0-9]+\b").ok()?;
    pattern
        .find(text)
        .map(|match_| trim_identifier_punctuation(match_.as_str()).to_string())
}

fn extract_arxiv_id(text: &str) -> Option<String> {
    let pattern =
        Regex::new(r"(?i)\barxiv(?:\.org/abs/|:|\s+)([a-z-]+/\d{7}|\d{4}\.\d{4,5})(v\d+)?\b")
            .ok()?;
    pattern
        .captures(text)
        .and_then(|captures| captures.get(1).map(|match_| match_.as_str().to_string()))
}

fn extract_bare_arxiv_id(text: &str) -> Option<String> {
    let pattern = Regex::new(r"\b([a-z-]+/\d{7}|\d{4}\.\d{4,5})(v\d+)?\b").ok()?;
    pattern
        .captures(text)
        .and_then(|captures| captures.get(1).map(|match_| match_.as_str().to_string()))
}

fn split_authors(raw: &str) -> Vec<String> {
    raw.split([';', ',', '\n'])
        .map(clean_metadata_text)
        .filter(|author| !author.is_empty())
        .collect()
}

fn clean_metadata_text(raw: &str) -> String {
    raw.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn clean_optional_metadata_text(value: Option<String>) -> Option<String> {
    value
        .map(|value| clean_metadata_text(&value))
        .filter(|value| !value.is_empty())
}

fn plausible_title(title: &str) -> bool {
    let title = title.trim();
    title.len() >= 8
        && title
            .chars()
            .any(|character| character.is_ascii_alphabetic())
        && !title.eq_ignore_ascii_case("untitled")
        && !title.eq_ignore_ascii_case("imported pdf")
        && !title.eq_ignore_ascii_case("local pdf")
        && !title.to_ascii_lowercase().contains("microsoft word")
}

fn titles_similar(left: &str, right: &str) -> bool {
    let left = normalize_title(left);
    let right = normalize_title(right);
    if left.is_empty() || right.is_empty() {
        return false;
    }
    if left == right || left.contains(&right) || right.contains(&left) {
        return true;
    }

    let left_tokens = title_token_set(&left);
    let right_tokens = title_token_set(&right);
    let smaller_len = left_tokens.len().min(right_tokens.len());
    if smaller_len < 3 {
        return false;
    }

    let overlap = left_tokens.intersection(&right_tokens).count();
    overlap as f64 / smaller_len as f64 >= 0.7
}

fn normalize_title(title: &str) -> String {
    title
        .chars()
        .filter(|character| character.is_ascii_alphanumeric() || character.is_whitespace())
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase()
}

fn title_token_set(title: &str) -> HashSet<String> {
    title
        .split_whitespace()
        .filter(|token| token.len() > 2)
        .map(str::to_string)
        .collect()
}

fn normalize_doi(doi: &str) -> String {
    trim_identifier_punctuation(
        doi.trim()
            .trim_start_matches("https://doi.org/")
            .trim_start_matches("http://doi.org/")
            .trim_start_matches("doi:"),
    )
    .to_ascii_lowercase()
}

fn normalize_arxiv_id(arxiv_id: &str) -> String {
    let id = arxiv_id
        .trim()
        .trim_start_matches("https://arxiv.org/abs/")
        .trim_start_matches("http://arxiv.org/abs/");
    if let Some(v_pos) = id.rfind('v') {
        let suffix = &id[v_pos + 1..];
        if !suffix.is_empty() && suffix.chars().all(|c| c.is_ascii_digit()) {
            return id[..v_pos].to_ascii_lowercase();
        }
    }
    id.to_ascii_lowercase()
}

fn trim_identifier_punctuation(value: &str) -> &str {
    value.trim_matches(|character: char| {
        character == '.' || character == ',' || character == ';' || character == ')'
    })
}

fn metadata_log(message: impl AsRef<str>) {
    let timestamp_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0);
    eprintln!("[metadata-enrichment {timestamp_ms}] {}", message.as_ref());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::library::Paper;

    fn paper_with_title(title: &str) -> Paper {
        Paper {
            id: "local:test".to_string(),
            title: title.to_string(),
            authors: Vec::new(),
            venue: "Local PDF".to_string(),
            year: 0,
            citations: 0,
            tags: vec!["local".to_string(), "needs-review".to_string()],
            highlight_count: 0,
            annotation_count: 0,
            status: "UNREAD".to_string(),
            abstract_text: None,
            active_source_id: None,
            active_extraction_id: None,
        }
    }

    #[test]
    fn extracts_doi_from_text() {
        assert_eq!(
            extract_doi("Published as doi:10.1145/3366423.3380138."),
            Some("10.1145/3366423.3380138".to_string())
        );
    }

    #[test]
    fn extracts_arxiv_id_from_text() {
        assert_eq!(
            extract_arxiv_id("See arXiv:2309.08600v2 for details."),
            Some("2309.08600".to_string())
        );
    }

    #[test]
    fn extracts_bare_arxiv_id_from_filename_title() {
        assert_eq!(
            extract_bare_arxiv_id("1706.03762 attention is all you need"),
            Some("1706.03762".to_string())
        );
    }

    #[test]
    fn title_similarity_ignores_case_and_punctuation() {
        assert!(titles_similar(
            "Attention Is All You Need",
            "Attention is all you need."
        ));
    }

    #[test]
    fn title_similarity_allows_filename_noise() {
        assert!(titles_similar(
            "Attention Is All You Need",
            "attention is all you need vaswani 2017 final"
        ));
    }

    #[test]
    fn title_queries_fall_back_to_current_import_title() {
        let evidence = PdfEvidence::default();
        let paper = paper_with_title("attention is all you need vaswani 2017");

        assert_eq!(
            title_queries(&evidence, None, &paper),
            vec!["attention is all you need vaswani 2017".to_string()]
        );
    }

    #[test]
    fn title_queries_prefer_largest_font_and_llm_titles() {
        let evidence = PdfEvidence {
            largest_font_title: Some("A Real Paper Title From The First Page".to_string()),
            ..PdfEvidence::default()
        };
        let paper = paper_with_title("filename fallback");
        let queries = title_queries(&evidence, Some("An LLM Extracted Title"), &paper);

        assert_eq!(queries[0], "A Real Paper Title From The First Page");
        assert!(queries.contains(&"An LLM Extracted Title".to_string()));
    }

    #[test]
    fn local_clues_fallback_uses_pdf_metadata_when_available() {
        let evidence = PdfEvidence {
            embedded_title: Some("A Real Paper Title".to_string()),
            embedded_authors: vec!["Ada Lovelace".to_string()],
            ..PdfEvidence::default()
        };
        let paper = paper_with_title("filename fallback");
        let candidate = metadata_candidate_from_local_clues(&evidence, &paper);

        assert!(candidate.confidence < AUTO_APPLY_CONFIDENCE);
        assert_eq!(candidate.title, "A Real Paper Title");
        assert_eq!(
            candidate.authors.as_slice(),
            [String::from("Ada Lovelace")].as_slice()
        );
    }

    #[test]
    fn local_clues_fallback_uses_current_import_title_when_pdf_metadata_is_empty() {
        let evidence = PdfEvidence::default();
        let paper = paper_with_title("attention is all you need vaswani 2017");
        let candidate = metadata_candidate_from_local_clues(&evidence, &paper);

        assert!(candidate.confidence < AUTO_APPLY_CONFIDENCE);
        assert_eq!(candidate.title, "attention is all you need vaswani 2017");
        assert!(candidate
            .evidence
            .contains(&"Current imported title fallback".to_string()));
    }

    fn candidate_with(title: &str, doi: Option<&str>, providers: Vec<&str>) -> MetadataCandidate {
        MetadataCandidate {
            id: normalize_title(title),
            title: title.to_string(),
            authors: Vec::new(),
            venue: None,
            year: None,
            doi: doi.map(str::to_string),
            arxiv_id: None,
            abstract_text: None,
            providers: providers.into_iter().map(str::to_string).collect(),
            confidence: 0.0,
            evidence: Vec::new(),
        }
    }

    #[test]
    fn verification_promotes_page1_identifier_match() {
        let evidence = PdfEvidence {
            page1_doi: Some("10.1145/3366423.3380138".to_string()),
            page1_text: "Some Page One Text".to_string(),
            ..PdfEvidence::default()
        };
        let mut candidate = candidate_with(
            "A Verified Paper",
            Some("10.1145/3366423.3380138"),
            vec!["crossref"],
        );
        score_candidate(&mut candidate, &evidence);

        assert!(candidate.confidence >= AUTO_APPLY_CONFIDENCE);
    }

    #[test]
    fn verification_accepts_title_found_on_page1() {
        let evidence = PdfEvidence {
            page1_text: "Proceedings header\nAttention Is All You Need\nAda Lovelace".to_string(),
            ..PdfEvidence::default()
        };
        let mut candidate = candidate_with("Attention Is All You Need", None, vec!["openalex"]);
        score_candidate(&mut candidate, &evidence);

        assert!((candidate.confidence - CONFIDENCE_VERIFIED_PROVIDER).abs() < f64::EPSILON);
    }

    #[test]
    fn author_corroboration_boosts_verified_provider_candidates() {
        let evidence = PdfEvidence {
            page1_text: "Attention Is All You Need\nAshish Vaswani, Noam Shazeer, Niki Parmar"
                .to_string(),
            ..PdfEvidence::default()
        };
        let mut candidate = candidate_with("Attention Is All You Need", None, vec!["arxiv"]);
        candidate.authors = vec![
            "Ashish Vaswani".to_string(),
            "Noam Shazeer".to_string(),
            "Niki Parmar".to_string(),
        ];
        score_candidate(&mut candidate, &evidence);

        assert!((candidate.confidence - CONFIDENCE_VERIFIED_PROVIDER_AUTHORS).abs() < f64::EPSILON);
    }

    #[test]
    fn author_corroboration_boosts_single_source_candidates() {
        let evidence = PdfEvidence {
            page1_text: "Attention Is All You Need\nAshish Vaswani, Noam Shazeer".to_string(),
            ..PdfEvidence::default()
        };
        let mut candidate = candidate_with("Attention Is All You Need", None, vec!["llm_metadata"]);
        candidate.authors = vec!["Ashish Vaswani".to_string(), "Noam Shazeer".to_string()];
        score_candidate(&mut candidate, &evidence);

        assert!((candidate.confidence - CONFIDENCE_VERIFIED_SINGLE_AUTHORS).abs() < f64::EPSILON);
    }

    #[test]
    fn wrong_authors_do_not_boost_verified_candidates() {
        let evidence = PdfEvidence {
            page1_text: "Attention Is All You Need\nAda Lovelace".to_string(),
            ..PdfEvidence::default()
        };
        let mut candidate = candidate_with("Attention Is All You Need", None, vec!["openalex"]);
        candidate.authors = vec!["Somebody Else".to_string(), "Another Person".to_string()];
        score_candidate(&mut candidate, &evidence);

        assert!((candidate.confidence - CONFIDENCE_VERIFIED_PROVIDER).abs() < f64::EPSILON);
    }

    #[test]
    fn authors_on_page1_requires_word_level_matches() {
        assert_eq!(
            authors_on_page1(
                &["Ashish Vaswani".to_string(), "Noam Shazeer".to_string()],
                "Ashish Vaswani and Noam Shazeer wrote this",
            ),
            AuthorMatch::Strong
        );
        assert_eq!(
            authors_on_page1(
                &["Ashish Vaswani".to_string(), "Someone Missing".to_string()],
                "Ashish Vaswani wrote this alone",
            ),
            AuthorMatch::Weak
        );
        // "Li" must not match inside "Linear" — matches are whole words.
        assert_eq!(
            authors_on_page1(&["Wei Li".to_string()], "Linear models considered"),
            AuthorMatch::None
        );
    }

    #[test]
    fn search_plan_prefers_llm_title_and_author_last_names() {
        let evidence = PdfEvidence {
            largest_font_title: Some("LARGEST FONT RUN TITLE".to_string()),
            embedded_authors: vec!["Embedded Person".to_string()],
            ..PdfEvidence::default()
        };
        let mut llm = candidate_with("A Clean Extracted Title", None, vec!["llm_metadata"]);
        llm.authors = vec![
            "Ashish Vaswani".to_string(),
            "Noam Shazeer".to_string(),
            "Niki Parmar".to_string(),
            "Jakob Uszkoreit".to_string(),
        ];
        let plan = build_search_plan(&evidence, Some(&llm));

        assert_eq!(plan.title.as_deref(), Some("A Clean Extracted Title"));
        assert_eq!(plan.author_last_names, vec!["vaswani", "shazeer", "parmar"]);
    }

    #[test]
    fn search_plan_falls_back_to_local_evidence() {
        let evidence = PdfEvidence {
            largest_font_title: Some("Largest Font Run Title".to_string()),
            embedded_authors: vec!["Ada Lovelace".to_string()],
            ..PdfEvidence::default()
        };
        let plan = build_search_plan(&evidence, None);

        assert_eq!(plan.title.as_deref(), Some("Largest Font Run Title"));
        assert_eq!(plan.author_last_names, vec!["lovelace"]);
    }

    #[test]
    fn merge_backfills_identifiers_across_versions() {
        let mut published = candidate_with(
            "Attention Is All You Need",
            Some("10.5555/nips.2017"),
            vec!["crossref"],
        );
        published.confidence = 0.9;
        let mut preprint = candidate_with("Attention Is All You Need", None, vec!["arxiv"]);
        preprint.arxiv_id = Some("1706.03762".to_string());
        preprint.confidence = 0.8;

        let merged = merge_metadata_candidates(vec![published, preprint]);

        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].doi.as_deref(), Some("10.5555/nips.2017"));
        assert_eq!(merged[0].arxiv_id.as_deref(), Some("1706.03762"));
    }

    #[test]
    fn title_queries_skip_filename_when_extracted_title_exists() {
        let evidence = PdfEvidence {
            largest_font_title: Some("A Real Extracted Paper Title".to_string()),
            ..PdfEvidence::default()
        };
        let queries = title_queries(
            &evidence,
            None,
            &paper_with_title("2309 08600v2 draft final"),
        );

        assert_eq!(queries, vec!["A Real Extracted Paper Title".to_string()]);
    }

    #[test]
    fn verification_demotes_cited_doi_candidates() {
        // The paper cites another DOI on a later page; that candidate's title
        // does not appear on page 1 and must not auto-apply.
        let evidence = PdfEvidence {
            later_doi: Some("10.9999/cited.paper".to_string()),
            page1_text: "The Actual Paper Title\nSome Author".to_string(),
            ..PdfEvidence::default()
        };
        let mut candidate = candidate_with(
            "A Completely Different Cited Paper",
            Some("10.9999/cited.paper"),
            vec!["crossref"],
        );
        score_candidate(&mut candidate, &evidence);

        assert!(candidate.confidence <= CONFIDENCE_UNVERIFIED_CAP);
    }

    #[test]
    fn verification_caps_unverified_llm_candidates() {
        let evidence = PdfEvidence {
            page1_text: "Totally unrelated page text".to_string(),
            ..PdfEvidence::default()
        };
        let mut candidate = candidate_with("A Hallucinated Title", None, vec!["llm_metadata"]);
        candidate.confidence = 0.74;
        score_candidate(&mut candidate, &evidence);

        assert!(candidate.confidence <= CONFIDENCE_UNVERIFIED_CAP);
    }

    #[test]
    fn title_on_page1_ignores_line_breaks_and_case() {
        assert!(title_on_page1(
            "Attention Is All You Need",
            "conference banner\nATTENTION IS\nALL YOU NEED\nauthors here"
        ));
        assert!(!title_on_page1("Attention Is All You Need", "other text"));
    }

    #[test]
    fn bibliographic_blob_caps_length_and_joins_lines() {
        let text = "Title Line\n\n  Author Line  \n".to_string() + &"x".repeat(5_000);
        let blob = bibliographic_query_blob(&text);

        assert!(blob.starts_with("Title Line Author Line"));
        assert!(blob.len() <= BIBLIOGRAPHIC_QUERY_CHAR_LIMIT);
    }

    /// RFC 0049 dev harness: prints stage-by-stage evidence for a real PDF.
    /// Run with:
    /// `I0I_PROBE_PDF=/path/to.pdf cargo test probe_pdf_evidence -- --ignored --nocapture`
    #[test]
    #[ignore = "manual probe harness; needs I0I_PROBE_PDF"]
    fn probe_pdf_evidence() {
        let path = std::env::var("I0I_PROBE_PDF").expect("set I0I_PROBE_PDF to a PDF path");
        let config = PdfExtractionConfig::default();
        let evidence = extract_pdf_evidence(None, &config, Path::new(&path)).expect("evidence");

        {
            let pdfium = bind_pdfium(None, &config).expect("pdfium");
            let document = pdfium
                .load_pdf_from_file(Path::new(&path), None)
                .expect("pdf");
            let pages = document.pages();
            let page = pages.iter().next();
            if let Some(page) = &page {
                if let Ok(text) = page.text() {
                    let mut sizes: Vec<f32> = text
                        .chars()
                        .iter()
                        .filter(|c| c.unicode_char().map(|c| c.is_alphabetic()).unwrap_or(false))
                        .map(|c| c.scaled_font_size().value)
                        .collect();
                    sizes.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
                    sizes.dedup();
                    println!(
                        "== page-1 font sizes (desc, deduped): {:?}",
                        &sizes[..sizes.len().min(10)]
                    );
                    println!("== raw largest-font run: {:?}", largest_font_run(&text));
                }
            }
        }
        println!("== embedded title:      {:?}", evidence.embedded_title);
        println!("== embedded authors:    {:?}", evidence.embedded_authors);
        println!("== largest-font title:  {:?}", evidence.largest_font_title);
        println!("== page-1 doi:          {:?}", evidence.page1_doi);
        println!("== later doi:           {:?}", evidence.later_doi);
        println!("== page-1 arxiv id:     {:?}", evidence.page1_arxiv_id);
        println!("== later arxiv id:      {:?}", evidence.later_arxiv_id);
        println!(
            "== bibliographic blob:  {}",
            bibliographic_query_blob(&evidence.page1_text)
        );
        println!("== page-1 text (first 40 lines):");
        for line in evidence.page1_text.lines().take(40) {
            println!("   | {line}");
        }
    }

    #[test]
    fn llm_metadata_response_becomes_review_candidate() {
        let response = LlmMetadataResponse {
            title: Some("A Neural Method for Reading Papers".to_string()),
            authors: vec!["Ada Lovelace".to_string(), "Grace Hopper".to_string()],
            venue: Some("Proceedings of Useful Systems".to_string()),
            year: Some(2024),
            doi: Some("https://doi.org/10.1145/3366423.3380138".to_string()),
            arxiv_id: None,
            abstract_text: Some("  A small abstract.  ".to_string()),
            confidence: Some(0.91),
            evidence: vec!["title appears on page 1".to_string()],
        };

        let candidate = metadata_candidate_from_llm_response(response).expect("candidate");

        assert_eq!(candidate.title, "A Neural Method for Reading Papers");
        assert_eq!(candidate.authors.len(), 2);
        assert_eq!(
            candidate.venue.as_deref(),
            Some("Proceedings of Useful Systems")
        );
        assert_eq!(candidate.year, Some(2024));
        assert_eq!(candidate.doi.as_deref(), Some("10.1145/3366423.3380138"));
        assert_eq!(
            candidate.abstract_text.as_deref(),
            Some("A small abstract.")
        );
        assert_eq!(candidate.providers, vec!["llm_metadata".to_string()]);
        assert!(candidate.confidence < AUTO_APPLY_CONFIDENCE);
        assert!(candidate.confidence <= 0.74);
    }

    #[test]
    fn llm_json_extraction_tolerates_wrapping_text() {
        let raw = "Here is the result:\n```json\n{\"title\":\"A Real Title\"}\n```";
        let value = extract_json_object_value(raw).expect("json object");

        assert_eq!(value["title"], "A Real Title");
    }
}
