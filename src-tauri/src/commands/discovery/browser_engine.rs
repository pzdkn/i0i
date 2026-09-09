//! Search-engine-specific details for browser discovery.
//!
//! The browser executor should not know the HTML vocabulary of DuckDuckGo,
//! Ecosia, or Brave. Each adapter in this module builds one result-page URL,
//! extracts ordinary web results, and recognizes that engine's challenge page.

use scraper::{Html, Selector};
use url::Url;

/// A free browser search page supported by i0i's Obscura search lane.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BrowserSearchEngine {
    DuckDuckGo,
    Ecosia,
    Brave,
    /// Preserves the existing `[discovery].browser_search_url` override.
    Custom,
}

impl BrowserSearchEngine {
    /// Parse a configured engine name.
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().trim_matches('"').to_ascii_lowercase().as_str() {
            "duckduckgo" | "duck_duck_go" | "ddg" => Some(Self::DuckDuckGo),
            "ecosia" => Some(Self::Ecosia),
            "brave" | "brave_search" => Some(Self::Brave),
            "custom" => Some(Self::Custom),
            _ => None,
        }
    }

    /// Stable provider name used in progress and provenance.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::DuckDuckGo => "duckduckgo",
            Self::Ecosia => "ecosia",
            Self::Brave => "brave",
            Self::Custom => "custom",
        }
    }

    /// Build the requested result page URL.
    pub fn search_url(
        self,
        query: &str,
        page: usize,
        custom_template: Option<&str>,
    ) -> Result<String, String> {
        let encoded: String = url::form_urlencoded::byte_serialize(query.as_bytes()).collect();
        let template = match self {
            Self::DuckDuckGo => "https://html.duckduckgo.com/html/?q={query}&s={offset}",
            Self::Ecosia => "https://www.ecosia.org/search?q={query}&p={page_number}",
            Self::Brave => "https://search.brave.com/search?q={query}&source=web&offset={offset}",
            Self::Custom => custom_template.ok_or_else(|| {
                "The custom browser search engine requires browser_search_url".to_string()
            })?,
        };
        if !template.contains("{query}") {
            return Err("[discovery].browser_search_url must contain {query}".to_string());
        }
        Ok(template
            .replace("{query}", &encoded)
            .replace("{page}", &page.to_string())
            .replace("{page_number}", &(page + 1).to_string())
            .replace("{offset}", &(page * 10).to_string()))
    }

    /// Extract result rows while excluding search-page navigation and chrome.
    pub fn parse_results(self, html: &str, limit: usize) -> Vec<BrowserResult> {
        let document = Html::parse_document(html);
        match self {
            Self::DuckDuckGo => parse_rows(
                &document,
                ".result",
                &["a.result__a"],
                &[".result__snippet"],
                limit,
            ),
            Self::Ecosia => parse_rows(
                &document,
                "article.result, .result",
                &["a.result-title", "a[data-test-id=\"result-title-a\"]"],
                &[".result-snippet", "[data-test-id=\"result-snippet\"]"],
                limit,
            ),
            Self::Brave => parse_rows(
                &document,
                "div.snippet[data-type=\"web\"]",
                &["a[href] .title"],
                &[".generic-snippet .content"],
                limit,
            ),
            Self::Custom => parse_custom_results(&document, limit),
        }
    }

    /// Return whether this engine replaced results with a bot challenge.
    pub fn is_challenge(self, final_url: &str, visible_text: &str, html: &str) -> bool {
        let text = visible_text.to_ascii_lowercase();
        let html = html.to_ascii_lowercase();
        if google_challenge(final_url, &text) {
            return true;
        }
        match self {
            Self::DuckDuckGo => {
                text.contains("bots use duckduckgo too")
                    || text.contains("complete the following challenge")
                    || html.contains("anomaly-modal")
            }
            Self::Ecosia => {
                text.contains("prove that you are human")
                    || text.contains("unusual traffic")
                    || text.contains("verify you are human")
            }
            Self::Brave => {
                text.contains("verifying you're not a bot")
                    || text.contains("quick check before you continue searching")
                    || html.contains("challengeset")
            }
            Self::Custom => false,
        }
    }

    /// Return whether the page explicitly reports temporary request throttling.
    pub fn is_rate_limited(self, visible_text: &str) -> bool {
        let text = visible_text.to_ascii_lowercase();
        text.contains("too many requests")
            || text.contains("rate limit exceeded")
            || text.contains("temporarily blocked")
    }
}

/// Raw result emitted by an engine adapter before scholarly normalization.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowserResult {
    pub title: String,
    pub url: String,
    pub snippet: Option<String>,
}

fn parse_rows(
    document: &Html,
    row_selector: &str,
    title_selectors: &[&str],
    snippet_selectors: &[&str],
    limit: usize,
) -> Vec<BrowserResult> {
    let rows = Selector::parse(row_selector).expect("valid browser result selector");
    let title_selectors = title_selectors
        .iter()
        .map(|value| Selector::parse(value).expect("valid browser title selector"))
        .collect::<Vec<_>>();
    let snippet_selectors = snippet_selectors
        .iter()
        .map(|value| Selector::parse(value).expect("valid browser snippet selector"))
        .collect::<Vec<_>>();
    let mut results = Vec::new();

    for row in document.select(&rows) {
        let title_node = title_selectors
            .iter()
            .find_map(|selector| row.select(selector).next());
        let Some(title_node) = title_node else {
            continue;
        };
        let link = if title_node.value().name() == "a" {
            title_node
        } else {
            let Some(link) = title_node.ancestors().find_map(scraper::ElementRef::wrap) else {
                continue;
            };
            if link.value().name() != "a" {
                continue;
            }
            link
        };
        let Some(url) = link.value().attr("href").and_then(normalize_result_url) else {
            continue;
        };
        let title = clean_text(&title_node.text().collect::<Vec<_>>().join(" "));
        let snippet = snippet_selectors.iter().find_map(|selector| {
            row.select(selector)
                .next()
                .map(|node| clean_text(&node.text().collect::<Vec<_>>().join(" ")))
        });
        results.push(BrowserResult {
            title,
            url,
            snippet,
        });
        if results.len() >= limit {
            break;
        }
    }
    results
}

fn parse_custom_results(document: &Html, limit: usize) -> Vec<BrowserResult> {
    let scholar = parse_rows(document, ".gs_ri", &[".gs_rt a"], &[".gs_rs"], limit);
    if !scholar.is_empty() {
        return scholar;
    }
    parse_rows(
        document,
        "div.snippet[data-type=\"web\"]",
        &["a[href] .title"],
        &[".generic-snippet .content"],
        limit,
    )
}

fn normalize_result_url(raw: &str) -> Option<String> {
    let raw = if raw.starts_with("//") {
        format!("https:{raw}")
    } else {
        raw.to_string()
    };
    let absolute = Url::parse(&raw).ok()?;
    if let Some(target) = absolute
        .query_pairs()
        .find(|(key, _)| matches!(key.as_ref(), "q" | "url" | "uddg"))
        .map(|(_, value)| value.into_owned())
        .filter(|value| value.starts_with("http://") || value.starts_with("https://"))
    {
        return Some(target);
    }
    Some(absolute.into())
}

fn google_challenge(final_url: &str, visible_text: &str) -> bool {
    let challenge_url = Url::parse(final_url).is_ok_and(|url| {
        let host = url.host_str().unwrap_or_default();
        (host == "google.com" || host.ends_with(".google.com")) && url.path().starts_with("/sorry/")
    });
    challenge_url || visible_text.contains("our systems have detected unusual traffic")
}

fn clean_text(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_engine_urls_with_page_parameters() {
        assert_eq!(
            BrowserSearchEngine::DuckDuckGo
                .search_url("graph networks", 1, None)
                .unwrap(),
            "https://html.duckduckgo.com/html/?q=graph+networks&s=10"
        );
        assert_eq!(
            BrowserSearchEngine::Ecosia
                .search_url("graph networks", 1, None)
                .unwrap(),
            "https://www.ecosia.org/search?q=graph+networks&p=2"
        );
    }

    #[test]
    fn parses_duckduckgo_redirect_results() {
        let html = r#"
            <div class="result">
              <a class="result__a" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Farxiv.org%2Fabs%2F1706.03762">Attention Is All You Need</a>
              <div class="result__snippet">The original transformer paper.</div>
            </div>
        "#;
        let results = BrowserSearchEngine::DuckDuckGo.parse_results(html, 10);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].url, "https://arxiv.org/abs/1706.03762");
        assert_eq!(
            results[0].snippet.as_deref(),
            Some("The original transformer paper.")
        );
    }

    #[test]
    fn parses_ecosia_results() {
        let html = r#"
            <article class="result">
              <a class="result-title" href="https://example.org/paper">A Useful Empirical Paper</a>
              <p class="result-snippet">A bounded comparison.</p>
            </article>
        "#;
        let results = BrowserSearchEngine::Ecosia.parse_results(html, 10);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "A Useful Empirical Paper");
    }

    #[test]
    fn recognizes_each_known_challenge() {
        assert!(BrowserSearchEngine::Brave.is_challenge(
            "https://search.brave.com/search",
            "Verifying you're not a bot. Quick check before you continue searching.",
            ""
        ));
        assert!(BrowserSearchEngine::DuckDuckGo.is_challenge(
            "https://duckduckgo.com/",
            "Unfortunately, bots use DuckDuckGo too. Complete the following challenge.",
            ""
        ));
        assert!(BrowserSearchEngine::Ecosia.is_challenge(
            "https://www.ecosia.org/search",
            "Please prove that you are human",
            ""
        ));
    }
}
