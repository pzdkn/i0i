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

use std::collections::HashMap;

use ammonia::Builder;
use dom_smoothie::Readability;
use regex::Regex;
use scraper::{Html, Selector};

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

/// Heading tags, for labelling a reference whose target is a section heading.
const HEADING_TAGS: &[&str] = &["h1", "h2", "h3", "h4", "h5", "h6"];

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

    // RFC 0083 R1.1: resolve `??` placeholders against the *raw* document —
    // Readability may have dropped the figure a reference points at, and the
    // caption numbering we need lives on it.
    let content = resolve_references(&article.content.to_string(), &reference_labels(raw_html));
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
    let clean_html = sanitize(&resolve_references(raw_html, &reference_labels(raw_html)));
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
///
/// RFC 0083 R1.3: `id` is allowed through. Without it ammonia strips every
/// anchor target in the document, and a cross-reference that survives with its
/// `href="#figure-2"` intact has nothing left to point at. An id runs no script
/// and fetches nothing; the reader resolves jumps scoped to the article root,
/// so an id that collides with one of the app's own cannot misdirect one.
fn sanitize(html: &str) -> String {
    let mut builder = Builder::default();
    builder
        .add_tags(MATHML_TAGS.iter().copied())
        .add_tags(EXTRA_HTML_TAGS.iter().copied())
        .add_generic_attributes(MATHML_ATTRS.iter().copied())
        .add_generic_attributes(["id"])
        // Only so the reader can style the marker R1.2 leaves behind.
        .add_tag_attributes("span", ["class"])
        .add_url_schemes(["data"]);
    builder.clean(html).to_string()
}

/// Label for each `id` in the document, for resolving cross-references
/// (RFC 0083 R1.1).
///
/// Scholarly HTML numbers its figures in the markup (`<figcaption><span
/// class="fig-num">Figure 2: </span>…`) even when it leaves the *references* to
/// those figures as `??` placeholders for a script to fill in. So the label is
/// read from the target: a figure's caption prefix, else a heading's text.
fn reference_labels(raw_html: &str) -> HashMap<String, String> {
    let document = Html::parse_document(raw_html);
    let Ok(with_id) = Selector::parse("[id]") else {
        return HashMap::new();
    };
    let caption = Selector::parse("figcaption").expect("static selector");
    let heading = Selector::parse("h1, h2, h3, h4, h5, h6").expect("static selector");

    let mut labels = HashMap::new();
    for element in document.select(&with_id) {
        let Some(id) = element.value().attr("id") else {
            continue;
        };
        let label = element
            .select(&caption)
            .next()
            .and_then(|node| caption_label(&normalize_ws(&node.text().collect::<String>())))
            .or_else(|| {
                // The target may *be* the heading (`<h2 id="methods">`) or
                // contain one (`<section id="methods"><h2>`).
                let text = if HEADING_TAGS.contains(&element.value().name()) {
                    normalize_ws(&element.text().collect::<String>())
                } else {
                    normalize_ws(&element.select(&heading).next()?.text().collect::<String>())
                };
                (!text.is_empty() && text.chars().count() <= 60).then_some(text)
            });
        if let Some(label) = label {
            labels.insert(id.to_string(), label);
        }
    }
    labels
}

/// The numbered prefix of a caption — `"Figure 2: Stylized…"` → `"Figure 2"`.
/// Captions without one (a bare description) give no usable reference label.
fn caption_label(caption_text: &str) -> Option<String> {
    let pattern =
        Regex::new(r"^(Figure|Fig\.?|Table|Listing|Algorithm|Equation|Appendix)\s+([A-Za-z]?[\d.]+)")
            .ok()?;
    let captures = pattern.captures(caption_text)?;
    Some(format!(
        "{} {}",
        &captures[1],
        captures[2].trim_end_matches('.')
    ))
}

/// Rewrite placeholder cross-references (RFC 0083 R1.2).
///
/// An `<a href="#id">??</a>` (or an empty one) becomes the target's label when
/// the target is known, and a plain inert marker when it is not — 44 of the 128
/// distinct references in the paper that prompted this RFC point at sections of
/// *other* documents, and a link that scrolls nowhere is worse than a word that
/// admits it is a reference we cannot follow. Anchors with real text are left
/// exactly as they are.
fn resolve_references(html: &str, labels: &HashMap<String, String>) -> String {
    let Ok(pattern) = Regex::new(r##"(?s)<a\b[^>]*\bhref="#([^"]+)"[^>]*>\s*(?:\?\?)?\s*</a>"##)
    else {
        return html.to_string();
    };
    pattern
        .replace_all(html, |captures: &regex::Captures| {
            let target = &captures[1];
            match labels.get(target) {
                Some(label) => format!(r##"<a href="#{target}">{label}</a>"##),
                None => r#"<span class="i0i-ref-unresolved">ref</span>"#.to_string(),
            }
        })
        .into_owned()
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

    /// RFC 0083 R1.3: without `id` every anchor target in the document is
    /// stripped, and the references R1.2 just repaired point at nothing.
    #[test]
    fn sanitize_keeps_anchor_targets() {
        let clean = sanitize(r#"<figure id="fig-2"><figcaption>Figure 2: x</figcaption></figure>"#);
        assert!(clean.contains(r#"id="fig-2""#), "id dropped: {clean}");
    }

    /// The shape the RFC was written against: the site ships `??` and fills it
    /// in with a script we do not run, while numbering its captions statically.
    #[test]
    fn placeholder_reference_takes_its_targets_caption_number() {
        let html = r##"<figure data-fignum="2" id="fig-structure">
            <figcaption><span class="fig-num">Figure 2: </span>Stylized illustration.</figcaption>
            </figure>
            <p>as established in <a class="fig-ref" data-ref="structure" href="#fig-structure">??</a>.</p>"##;
        let resolved = resolve_references(html, &reference_labels(html));
        assert!(
            resolved.contains(r##"<a href="#fig-structure">Figure 2</a>"##),
            "reference not resolved: {resolved}"
        );
    }

    #[test]
    fn placeholder_reference_to_a_missing_target_stops_being_a_link() {
        let html = r##"<p>see <a href="#ws-modulation">??</a>.</p>"##;
        let resolved = resolve_references(html, &reference_labels(html));
        assert!(!resolved.contains("<a "), "dead link kept: {resolved}");
        assert!(
            resolved.contains("i0i-ref-unresolved"),
            "no marker left behind: {resolved}"
        );
    }

    #[test]
    fn a_reference_that_already_reads_as_one_is_untouched() {
        let html = r##"<p>see <a href="#fig-2">Figure 2</a>.</p>"##;
        assert_eq!(resolve_references(html, &reference_labels(html)), html);
    }

    /// A heading target has no caption to number; its own text is the label.
    #[test]
    fn heading_targets_label_from_their_text() {
        let html = r##"<h2 id="methods">Methods</h2><p>see <a href="#methods">??</a>.</p>"##;
        let resolved = resolve_references(html, &reference_labels(html));
        assert!(
            resolved.contains(r##"<a href="#methods">Methods</a>"##),
            "heading label not used: {resolved}"
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
