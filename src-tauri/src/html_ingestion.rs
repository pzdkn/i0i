//! HTML article ingestion (RFC 0056).
//!
//! Pure transform: raw page HTML in, `{ clean_html, source_text }` out.
//!
//! - **Extract** the article body with a Readability port (`dom_smoothie`).
//! - **Sanitize** it into safe, self-contained HTML with `ammonia` — crucially
//!   with a **MathML-aware allowlist**, since ammonia strips every tag it isn't
//!   told to keep and scholarly HTML is full of `<math>`.
//! - **Derive** `source_text`, the readable plain text used for chat context.
//!
//! No network here (image inlining lives in the acquisition slice), so this is
//! deterministic and fixture-testable. On extraction failure it falls back to
//! sanitizing the whole document, which stays readable and annotatable.
//!
// Foundation slice of RFC 0056: validated in isolation (see tests). Its
// consumers — HTML acquisition, storage, and serving — land in the next slice,
// at which point this allow comes off.
#![allow(dead_code)]

use ammonia::Builder;
use dom_smoothie::Readability;
use scraper::Html;

/// The clean, self-contained article plus its plain-text form.
#[derive(Debug, Clone)]
pub struct IngestedHtml {
    pub title: Option<String>,
    /// Sanitized article HTML, safe to render (no scripts/handlers/externals).
    pub clean_html: String,
    /// Readable plain text of the article (chat context / search).
    pub source_text: String,
}

/// MathML presentation elements. ammonia deletes any tag not listed, so without
/// these every equation silently vanishes (RFC 0056).
const MATHML_TAGS: &[&str] = &[
    "math",
    "semantics",
    "annotation",
    "annotation-xml",
    "mrow",
    "mfrac",
    "msqrt",
    "mroot",
    "mstyle",
    "merror",
    "mpadded",
    "mphantom",
    "menclose",
    "msub",
    "msup",
    "msubsup",
    "munder",
    "mover",
    "munderover",
    "mmultiscripts",
    "mprescripts",
    "none",
    "mtable",
    "mtr",
    "mtd",
    "mlabeledtr",
    "maligngroup",
    "malignmark",
    "mspace",
    "ms",
    "mtext",
    "mn",
    "mo",
    "mi",
];

/// MathML presentational attributes (allowed on any kept tag — harmless
/// elsewhere, and simpler than per-element allowlisting).
const MATHML_ATTRS: &[&str] = &[
    "display",
    "displaystyle",
    "scriptlevel",
    "mathvariant",
    "mathsize",
    "mathcolor",
    "mathbackground",
    "xmlns",
    "encoding",
    "accent",
    "accentunder",
    "stretchy",
    "fence",
    "separator",
    "separators",
    "largeop",
    "movablelimits",
    "lspace",
    "rspace",
    "linethickness",
    "columnalign",
    "rowalign",
    "columnspan",
    "rowspan",
    "open",
    "close",
    "notation",
    "dir",
    "depth",
    "voffset",
];

/// Extra non-default HTML tags worth keeping for articles.
const EXTRA_HTML_TAGS: &[&str] = &["figure", "figcaption", "article", "section", "header"];

/// Ingest raw HTML into clean article HTML + plain text. Never fails: an
/// unextractable page falls back to a sanitized whole-document render.
pub fn ingest_html(raw_html: &str, base_url: Option<&str>) -> IngestedHtml {
    extract(raw_html, base_url).unwrap_or_else(|| fallback(raw_html))
}

fn extract(raw_html: &str, base_url: Option<&str>) -> Option<IngestedHtml> {
    // `dom_smoothie` errors on a non-absolute document URL; drop it if so.
    let mut readability = Readability::new(raw_html, base_url, None)
        .or_else(|_| Readability::new(raw_html, None, None))
        .ok()?;
    let article = readability.parse().ok()?;

    let content = article.content.to_string();
    let clean_html = sanitize(&content);
    if clean_html.trim().is_empty() {
        return None;
    }

    let text_content = article.text_content.to_string();
    let source_text = if text_content.trim().is_empty() {
        text_of(&clean_html)
    } else {
        normalize_ws(&text_content)
    };

    Some(IngestedHtml {
        title: non_empty(article.title),
        clean_html,
        source_text,
    })
}

fn fallback(raw_html: &str) -> IngestedHtml {
    let clean_html = sanitize(raw_html);
    let source_text = text_of(&clean_html);
    IngestedHtml {
        title: None,
        clean_html,
        source_text,
    }
}

/// Sanitize with a MathML-aware allowlist. Scripts, event handlers, and
/// external-loading attributes are dropped; `data:` image URIs are allowed so
/// the acquisition slice can inline images for offline rendering.
fn sanitize(html: &str) -> String {
    let mut builder = Builder::default();
    builder
        .add_tags(MATHML_TAGS.iter().copied())
        .add_tags(EXTRA_HTML_TAGS.iter().copied())
        .add_generic_attributes(MATHML_ATTRS.iter().copied())
        .add_url_schemes(["data"]);
    builder.clean(html).to_string()
}

/// Plain text of an HTML fragment (fallback `source_text` derivation).
fn text_of(html: &str) -> String {
    let document = Html::parse_fragment(html);
    let joined = document.root_element().text().collect::<Vec<_>>().join(" ");
    normalize_ws(&joined)
}

fn normalize_ws(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn non_empty(value: String) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_preserves_mathml() {
        // The load-bearing case: ammonia's default allowlist has no MathML, so
        // this fails red without MATHML_TAGS/ATTRS (RFC 0056).
        let html = r#"<p>Euler: <math display="inline"><mrow><msup><mi>e</mi><mrow><mi>i</mi><mi>&#960;</mi></mrow></msup><mo>+</mo><mn>1</mn><mo>=</mo><mn>0</mn></mrow></math></p>"#;
        let clean = sanitize(html);
        assert!(clean.contains("<math"), "math element dropped: {clean}");
        assert!(clean.contains("<msup"), "msup dropped: {clean}");
        assert!(clean.contains("<mi"), "mi dropped: {clean}");
        assert!(clean.contains("display"), "display attr dropped: {clean}");
    }

    #[test]
    fn sanitize_strips_scripts_and_handlers() {
        let html =
            r#"<p onclick="steal()">hi</p><script>evil()</script><img src="x" onerror="bad()">"#;
        let clean = sanitize(html);
        assert!(!clean.contains("<script"), "script survived: {clean}");
        assert!(!clean.contains("onclick"), "handler survived: {clean}");
        assert!(!clean.contains("onerror"), "handler survived: {clean}");
        assert!(clean.contains("hi"));
    }

    #[test]
    fn sanitize_allows_data_image_uris() {
        let html = r#"<img src="data:image/png;base64,AAAA" alt="fig">"#;
        let clean = sanitize(html);
        assert!(
            clean.contains("data:image/png"),
            "data URI dropped: {clean}"
        );
    }

    #[test]
    fn ingest_extracts_article_body_and_text() {
        let html = r#"<!doctype html><html><head><title>T</title></head><body>
            <nav>menu junk</nav>
            <article><h1>The Title</h1>
            <p>First paragraph about diffusion models and proteins that is long enough to be treated as real article content by the extractor.</p>
            <p>Second paragraph continues the discussion with more substantive body text so the readability heuristics keep it.</p>
            </article>
            <footer>copyright junk</footer></body></html>"#;
        let ingested = ingest_html(html, Some("https://example.org/a"));
        assert!(
            ingested.clean_html.contains("First paragraph"),
            "body missing: {}",
            ingested.clean_html
        );
        assert!(ingested.source_text.contains("Second paragraph"));
        assert!(!ingested.clean_html.contains("<script"));
    }

    #[test]
    fn ingest_falls_back_to_sanitized_body_on_non_article() {
        let html =
            r#"<div>bare content with no article structure at all</div><script>x()</script>"#;
        let ingested = ingest_html(html, None);
        assert!(!ingested.clean_html.trim().is_empty());
        assert!(ingested.clean_html.contains("bare content"));
        assert!(!ingested.clean_html.contains("<script"));
        assert!(ingested.source_text.contains("bare content"));
    }
}
