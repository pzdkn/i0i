use std::time::Duration;

use serde::Deserialize;

/// Everything we know about where a paper's PDF might live (RFC 0051).
///
/// Built by callers from candidate/source metadata; the acquisition service
/// expands it into a ranked list of concrete URLs before downloading anything.
#[derive(Debug, Clone, Default)]
pub struct PdfLocationHints {
    pub pdf_url: Option<String>,
    pub landing_url: Option<String>,
    pub doi: Option<String>,
    pub arxiv_id: Option<String>,
}

impl PdfLocationHints {
    pub fn from_url(pdf_url: &str, landing_url: Option<&str>) -> Self {
        Self {
            pdf_url: Some(pdf_url.to_string()),
            landing_url: landing_url.map(str::to_string),
            doi: None,
            arxiv_id: None,
        }
    }

    /// Whether the hints can produce at least one download location.
    pub fn has_any_location(&self) -> bool {
        self.pdf_url.is_some() || self.doi.is_some() || self.arxiv_id.is_some()
    }
}

#[derive(Debug, Clone)]
pub struct PdfLocation {
    pub url: String,
    pub source: &'static str,
    pub rank: u8,
}

const UNPAYWALL_TIMEOUT: Duration = Duration::from_secs(5);
const UNPAYWALL_LOCATION_LIMIT: usize = 3;

#[derive(Debug, Deserialize)]
struct UnpaywallResponse {
    best_oa_location: Option<UnpaywallLocation>,
    #[serde(default)]
    oa_locations: Vec<UnpaywallLocation>,
}

#[derive(Debug, Deserialize)]
struct UnpaywallLocation {
    url_for_pdf: Option<String>,
}

/// Expand hints into a ranked, deduplicated list of candidate PDF URLs.
///
/// Ranks: 0 = constructed arXiv URL (most reliable for this app's papers),
/// 1-2 = Unpaywall OA copies by DOI, 4 = the provider-supplied URL that used
/// to be our only bet. Resolver failures are ignored — the plan just gets
/// fewer entries and degrades to today's behavior.
pub async fn build_location_plan(
    client: &reqwest::Client,
    hints: &PdfLocationHints,
) -> Vec<PdfLocation> {
    let mut plan: Vec<PdfLocation> = Vec::new();

    if let Some(arxiv_id) = normalized_arxiv_id(hints.arxiv_id.as_deref()) {
        plan.push(PdfLocation {
            url: format!("https://arxiv.org/pdf/{arxiv_id}"),
            source: "arxiv_id",
            rank: 0,
        });
    }

    if let Some(doi) = normalized_doi(hints.doi.as_deref()) {
        for (index, url) in unpaywall_pdf_urls(client, &doi)
            .await
            .into_iter()
            .enumerate()
        {
            plan.push(PdfLocation {
                url,
                source: "unpaywall",
                rank: 1 + (index.min(1) as u8),
            });
        }
    }

    if let Some(pdf_url) = hints.pdf_url.as_deref() {
        if !pdf_url.trim().is_empty() {
            plan.push(PdfLocation {
                url: pdf_url.trim().to_string(),
                source: "provider",
                rank: 4,
            });
        }
    }

    dedup_by_url(&mut plan);
    plan.sort_by_key(|location| location.rank);
    plan
}

async fn unpaywall_pdf_urls(client: &reqwest::Client, doi: &str) -> Vec<String> {
    // Unpaywall requires a contact email; without one we skip the resolver
    // rather than send anonymous traffic.
    let Some(email) = unpaywall_email() else {
        return Vec::new();
    };

    let url = format!("https://api.unpaywall.org/v2/{doi}?email={email}");
    let response = match client.get(&url).timeout(UNPAYWALL_TIMEOUT).send().await {
        Ok(response) if response.status().is_success() => response,
        _ => return Vec::new(),
    };
    let Ok(parsed) = response.json::<UnpaywallResponse>().await else {
        return Vec::new();
    };

    let mut urls = Vec::new();
    let locations = parsed
        .best_oa_location
        .into_iter()
        .chain(parsed.oa_locations);
    for location in locations {
        if let Some(pdf_url) = location.url_for_pdf {
            let trimmed = pdf_url.trim().to_string();
            if !trimmed.is_empty() && !urls.contains(&trimmed) {
                urls.push(trimmed);
            }
        }
        if urls.len() >= UNPAYWALL_LOCATION_LIMIT {
            break;
        }
    }
    urls
}

fn unpaywall_email() -> Option<String> {
    // User settings (`secret.email`) win; then the historical env-var chain,
    // each also falling back to `.env` via `resolve_secret` (RFC 0055).
    ["IOI_EMAIL", "I0I_UNPAYWALL_EMAIL", "I0I_CROSSREF_MAILTO"]
        .into_iter()
        .find_map(|env_key| crate::services::settings::resolve_secret("secret.email", env_key))
}

fn normalized_doi(doi: Option<&str>) -> Option<String> {
    let doi = doi?.trim();
    let doi = doi
        .strip_prefix("https://doi.org/")
        .or_else(|| doi.strip_prefix("http://doi.org/"))
        .or_else(|| doi.strip_prefix("doi:"))
        .unwrap_or(doi);
    if doi.is_empty() {
        return None;
    }
    Some(doi.to_string())
}

fn normalized_arxiv_id(arxiv_id: Option<&str>) -> Option<String> {
    let id = arxiv_id?.trim().trim_start_matches("arXiv:").trim();
    if id.is_empty() {
        return None;
    }
    Some(id.to_string())
}

fn dedup_by_url(plan: &mut Vec<PdfLocation>) {
    let mut seen: Vec<String> = Vec::new();
    plan.retain(|location| {
        let key = location.url.trim_end_matches('/').to_lowercase();
        if seen.contains(&key) {
            false
        } else {
            seen.push(key);
            true
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn doi_normalization_strips_url_and_prefix() {
        assert_eq!(
            normalized_doi(Some("https://doi.org/10.1000/xyz")),
            Some("10.1000/xyz".to_string())
        );
        assert_eq!(
            normalized_doi(Some("doi:10.1000/xyz")),
            Some("10.1000/xyz".to_string())
        );
        assert_eq!(normalized_doi(Some("  ")), None);
        assert_eq!(normalized_doi(None), None);
    }

    #[test]
    fn arxiv_id_normalization_strips_prefix() {
        assert_eq!(
            normalized_arxiv_id(Some("arXiv:2309.08600")),
            Some("2309.08600".to_string())
        );
        assert_eq!(normalized_arxiv_id(Some("")), None);
    }

    #[tokio::test]
    async fn plan_orders_arxiv_before_provider_and_dedups() {
        let client = reqwest::Client::new();
        let hints = PdfLocationHints {
            pdf_url: Some("https://arxiv.org/pdf/2309.08600/".to_string()),
            landing_url: None,
            doi: None,
            arxiv_id: Some("2309.08600".to_string()),
        };

        let plan = build_location_plan(&client, &hints).await;

        // The provider URL is the same paper's arXiv URL (trailing slash aside)
        // and must collapse into the single rank-0 entry.
        assert_eq!(plan.len(), 1);
        assert_eq!(plan[0].source, "arxiv_id");
        assert_eq!(plan[0].rank, 0);
    }

    #[tokio::test]
    async fn plan_without_identifiers_uses_provider_url_only() {
        let client = reqwest::Client::new();
        let hints = PdfLocationHints::from_url("https://publisher.example/paper.pdf", None);

        let plan = build_location_plan(&client, &hints).await;

        assert_eq!(plan.len(), 1);
        assert_eq!(plan[0].source, "provider");
    }
}
