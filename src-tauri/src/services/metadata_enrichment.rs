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
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::Semaphore;

use crate::commands::discovery::provider::DiscoveryProvider;
use crate::commands::discovery::providers::{arxiv::ArxivProvider, openalex::OpenAlexProvider};
use crate::domain::discovery::{
    DiscoveryProviderChoice, DiscoverySearchRequest, DiscoverySort, PaperCandidate,
};
use crate::domain::library::{Paper, PaperMetadataEnrichment};
use crate::pdf_extraction::PdfExtractionConfig;
use crate::storage::library_store::LibraryStore;

const MAX_CONCURRENT_METADATA_JOBS: usize = 1;
const FIRST_PAGE_TEXT_LIMIT: usize = 2;
const PROVIDER_RESULT_LIMIT: i32 = 3;

type MetadataResult<T> = Result<T, String>;

#[derive(Clone)]
pub struct MetadataEnrichmentService {
    app: AppHandle,
    store: LibraryStore,
    pdfium_config: PdfExtractionConfig,
    openalex: OpenAlexProvider,
    arxiv: ArxivProvider,
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

#[derive(Debug, Default)]
struct PdfEvidence {
    title: Option<String>,
    authors: Vec<String>,
    doi: Option<String>,
    arxiv_id: Option<String>,
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
            semaphore: Arc::new(Semaphore::new(MAX_CONCURRENT_METADATA_JOBS)),
            queued_or_active: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    pub fn queue_papers(&self, paper_ids: Vec<String>) {
        for paper_id in paper_ids {
            self.queue_paper(paper_id);
        }
    }

    pub fn queue_paper(&self, paper_id: String) {
        if !self.mark_queued(&paper_id) {
            return;
        }

        metadata_log(format!("queued paper_id={paper_id}"));
        let service = self.clone();
        tauri::async_runtime::spawn(async move {
            let paper_id_for_cleanup = paper_id.clone();
            let result = service.enrich_paper(paper_id).await;
            service.mark_finished(&paper_id_for_cleanup);

            if let Err(error) = result {
                metadata_log(format!(
                    "failed paper_id={paper_id_for_cleanup} error={error}"
                ));
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

        let app = self.app.clone();
        let config = self.pdfium_config.clone();
        let evidence = tokio::task::spawn_blocking(move || {
            extract_pdf_evidence(&app, &config, Path::new(&local_path))
        })
        .await
        .map_err(|error| error.to_string())??;

        let enrichment = self
            .enrichment_from_evidence(&evidence, &current_paper)
            .await?;
        let Some(enrichment) = enrichment else {
            metadata_log(format!("skip paper_id={paper_id} no metadata evidence"));
            self.emit_update(&paper_id, "skipped", None);
            return Ok(());
        };

        match self
            .store
            .apply_paper_metadata_enrichment(&paper_id, &enrichment)?
        {
            Some(_) => {
                metadata_log(format!("ready paper_id={paper_id}"));
                self.emit_update(&paper_id, "ready", None);
            }
            None => {
                metadata_log(format!("skip paper_id={paper_id} not eligible"));
                self.emit_update(&paper_id, "skipped", None);
            }
        }

        Ok(())
    }

    async fn enrichment_from_evidence(
        &self,
        evidence: &PdfEvidence,
        current_paper: &Paper,
    ) -> MetadataResult<Option<PaperMetadataEnrichment>> {
        let doi = evidence
            .doi
            .clone()
            .or_else(|| extract_doi(&current_paper.title));
        let arxiv_id = evidence
            .arxiv_id
            .clone()
            .or_else(|| extract_arxiv_id(&current_paper.title))
            .or_else(|| extract_bare_arxiv_id(&current_paper.title));

        if let Some(doi) = &doi {
            match self.lookup_openalex_by_doi(doi).await {
                Ok(Some(candidate)) => {
                    return Ok(Some(enrichment_from_candidate(candidate, true)));
                }
                Ok(None) => {}
                Err(error) => {
                    metadata_log(format!("OpenAlex DOI lookup failed doi={doi}: {error}"))
                }
            }
        }

        if let Some(arxiv_id) = &arxiv_id {
            match self.lookup_arxiv_by_id(arxiv_id).await {
                Ok(Some(candidate)) => {
                    return Ok(Some(enrichment_from_candidate(candidate, true)));
                }
                Ok(None) => {}
                Err(error) => metadata_log(format!("arXiv lookup failed id={arxiv_id}: {error}")),
            }
        }

        for title in title_queries(evidence, current_paper) {
            match self.lookup_openalex_by_title(&title).await {
                Ok(Some(candidate)) => {
                    return Ok(Some(enrichment_from_candidate(candidate, true)));
                }
                Ok(None) => {}
                Err(error) => metadata_log(format!(
                    "OpenAlex title lookup failed title={title}: {error}"
                )),
            }
        }

        let fallback = enrichment_from_pdf_evidence(evidence);
        Ok(fallback)
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

    async fn lookup_openalex_by_title(
        &self,
        title: &str,
    ) -> MetadataResult<Option<PaperCandidate>> {
        let result = self
            .openalex
            .search(&provider_request(title, DiscoveryProviderChoice::OpenAlex))
            .await
            .map_err(|error| error.to_string())?;

        Ok(result
            .candidates
            .into_iter()
            .find(|candidate| titles_similar(&candidate.title, title)))
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
    app: &AppHandle,
    config: &PdfExtractionConfig,
    path: &Path,
) -> MetadataResult<PdfEvidence> {
    let pdfium = bind_pdfium(app, config)?;
    let document = pdfium
        .load_pdf_from_file(path, None)
        .map_err(|error| format!("Pdfium could not open {}: {error}", path.display()))?;
    let title = document
        .metadata()
        .get(PdfDocumentMetadataTagType::Title)
        .map(|tag| clean_metadata_text(tag.value()))
        .filter(|title| plausible_title(title));
    let authors = document
        .metadata()
        .get(PdfDocumentMetadataTagType::Author)
        .map(|tag| split_authors(tag.value()))
        .unwrap_or_default();
    let first_pages_text = document
        .pages()
        .iter()
        .take(FIRST_PAGE_TEXT_LIMIT)
        .filter_map(|page| page.text().ok().map(|text| text.all()))
        .collect::<Vec<_>>()
        .join("\n\n");
    let doi = extract_doi(&first_pages_text);
    let arxiv_id = extract_arxiv_id(&first_pages_text);

    Ok(PdfEvidence {
        title,
        authors,
        doi,
        arxiv_id,
    })
}

fn bind_pdfium(app: &AppHandle, config: &PdfExtractionConfig) -> MetadataResult<Pdfium> {
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

fn pdfium_library_candidates(app: &AppHandle, config: &PdfExtractionConfig) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(path) = &config.pdfium_library_path {
        candidates.push(path.clone());
    }

    if let Ok(resource_dir) = app.path().resource_dir() {
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
        venues: Vec::new(),
        authors: Vec::new(),
        fields_of_study: Vec::new(),
    }
}

fn enrichment_from_candidate(
    candidate: PaperCandidate,
    confident: bool,
) -> PaperMetadataEnrichment {
    PaperMetadataEnrichment {
        title: Some(candidate.title),
        authors: Some(candidate.authors),
        venue: candidate.venue,
        year: candidate.year,
        citations: candidate.citation_count,
        abstract_text: candidate.abstract_text,
        confident,
    }
}

fn enrichment_from_pdf_evidence(evidence: &PdfEvidence) -> Option<PaperMetadataEnrichment> {
    if evidence.title.is_none() && evidence.authors.is_empty() {
        return None;
    }

    Some(PaperMetadataEnrichment {
        title: evidence.title.clone(),
        authors: (!evidence.authors.is_empty()).then_some(evidence.authors.clone()),
        venue: None,
        year: None,
        citations: None,
        abstract_text: None,
        confident: false,
    })
}

fn title_queries(evidence: &PdfEvidence, current_paper: &Paper) -> Vec<String> {
    let mut queries = Vec::new();
    let mut seen = HashSet::new();

    for title in [
        evidence.title.as_deref(),
        Some(current_paper.title.as_str()),
    ]
    .into_iter()
    .flatten()
    .map(clean_metadata_text)
    .filter(|title| plausible_title(title))
    {
        let normalized = normalize_title(&title);
        if seen.insert(normalized) {
            queries.push(title);
        }
    }

    queries
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
            title_queries(&evidence, &paper),
            vec!["attention is all you need vaswani 2017".to_string()]
        );
    }

    #[test]
    fn pdf_evidence_fallback_keeps_needs_review_confidence() {
        let evidence = PdfEvidence {
            title: Some("A Real Paper Title".to_string()),
            authors: vec!["Ada Lovelace".to_string()],
            ..PdfEvidence::default()
        };
        let enrichment = enrichment_from_pdf_evidence(&evidence).expect("fallback exists");

        assert!(!enrichment.confident);
        assert_eq!(enrichment.title.as_deref(), Some("A Real Paper Title"));
        assert_eq!(
            enrichment.authors.as_deref(),
            Some([String::from("Ada Lovelace")].as_slice())
        );
    }
}
