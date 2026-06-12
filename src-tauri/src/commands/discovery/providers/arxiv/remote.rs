//! arXiv Atom feed wire types and parser.
//!
//! arXiv returns Atom XML, not JSON. This module owns the XML parsing so the
//! rest of the adapter works with plain Rust structs.

use quick_xml::events::Event;
use quick_xml::Reader;

/// One entry extracted from an arXiv Atom search response.
#[derive(Debug, Default)]
pub(super) struct ArxivEntry {
    /// Raw `<id>` URL, e.g. `http://arxiv.org/abs/2309.08600v2`.
    pub id: String,
    pub title: String,
    pub summary: Option<String>,
    /// ISO 8601 timestamp, e.g. `2023-09-15T17:48:05Z`.
    pub published: Option<String>,
    pub authors: Vec<String>,
    /// arXiv category term, e.g. `cs.LG`.
    pub primary_category: Option<String>,
    pub doi: Option<String>,
}

/// Parse an arXiv Atom XML feed and return all `<entry>` records.
///
/// Returns `Err` on any XML parse error so callers can distinguish a clean
/// empty result from a truncated or malformed response. Entries with an empty
/// `<id>` are silently skipped since they cannot be uniquely identified.
pub(super) fn parse_feed(xml: &str) -> Result<Vec<ArxivEntry>, quick_xml::Error> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut entries: Vec<ArxivEntry> = Vec::new();
    let mut current: Option<ArxivEntry> = None;
    let mut text_field: Option<TextField> = None;
    let mut in_author = false;

    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e)) => {
                let name = local_name(e.name().as_ref());
                match name.as_str() {
                    "entry" => current = Some(ArxivEntry::default()),
                    "id" if current.is_some() => text_field = Some(TextField::Id),
                    "title" if current.is_some() && !in_author => {
                        text_field = Some(TextField::Title)
                    }
                    "summary" if current.is_some() => text_field = Some(TextField::Summary),
                    "published" if current.is_some() => text_field = Some(TextField::Published),
                    "author" if current.is_some() => in_author = true,
                    "name" if current.is_some() && in_author => {
                        text_field = Some(TextField::AuthorName)
                    }
                    // `arxiv:doi` local name is `doi`
                    "doi" if current.is_some() => text_field = Some(TextField::Doi),
                    _ => {}
                }
            }
            Ok(Event::Empty(ref e)) => {
                // `<arxiv:primary_category term="cs.LG"/>` is a self-closing element.
                let name = local_name(e.name().as_ref());
                if name == "primary_category" {
                    if let Some(ref mut entry) = current {
                        entry.primary_category = read_term_attr(e, &reader);
                    }
                }
            }
            Ok(Event::Text(ref e)) => {
                if let Some(field) = text_field.take() {
                    if let (Some(ref mut entry), Ok(text)) = (&mut current, e.unescape()) {
                        apply_text_field(entry, field, text.trim());
                    }
                }
            }
            Ok(Event::CData(ref e)) => {
                if let Some(field) = text_field.take() {
                    if let Some(ref mut entry) = current {
                        let text = String::from_utf8_lossy(e.as_ref()).trim().to_string();
                        apply_text_field(entry, field, &text);
                    }
                }
            }
            Ok(Event::End(ref e)) => {
                let name = local_name(e.name().as_ref());
                match name.as_str() {
                    "entry" => {
                        if let Some(entry) = current.take() {
                            if !entry.id.is_empty() {
                                entries.push(entry);
                            }
                        }
                    }
                    "author" => in_author = false,
                    _ => {}
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(e),
            _ => {}
        }
    }

    Ok(entries)
}

enum TextField {
    Id,
    Title,
    Summary,
    Published,
    AuthorName,
    Doi,
}

fn apply_text_field(entry: &mut ArxivEntry, field: TextField, text: &str) {
    let owned = text.to_string();
    match field {
        TextField::Id => entry.id = owned,
        TextField::Title => entry.title = owned,
        TextField::Summary => entry.summary = Some(owned),
        TextField::Published => entry.published = Some(owned),
        TextField::AuthorName => entry.authors.push(owned),
        TextField::Doi => entry.doi = Some(owned),
    }
}

/// Strip the XML namespace prefix and return just the local element name.
fn local_name(raw: &[u8]) -> String {
    let s = std::str::from_utf8(raw).unwrap_or_default();
    s.rsplit_once(':').map(|(_, local)| local).unwrap_or(s).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    // Minimal realistic arXiv Atom entry used across several tests.
    const SINGLE_ENTRY_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<feed xmlns="http://www.w3.org/2005/Atom" xmlns:arxiv="http://arxiv.org/schemas/atom">
  <entry>
    <id>http://arxiv.org/abs/2309.08600v2</id>
    <title>Sparse Autoencoders Find Highly Interpretable Features in Language Models</title>
    <summary>We show that sparse autoencoders can find interpretable features.</summary>
    <published>2023-09-15T17:48:05Z</published>
    <author><name>Hoagy Cunningham</name></author>
    <author><name>Aidan Beren</name></author>
    <arxiv:primary_category xmlns:arxiv="http://arxiv.org/schemas/atom" term="cs.LG" scheme="http://arxiv.org/schemas/atom"/>
    <arxiv:doi xmlns:arxiv="http://arxiv.org/schemas/atom">10.1234/example</arxiv:doi>
  </entry>
</feed>"#;

    #[test]
    fn parses_basic_entry_fields() {
        let entries = parse_feed(SINGLE_ENTRY_XML).unwrap();
        assert_eq!(entries.len(), 1);
        let e = &entries[0];
        assert_eq!(e.id, "http://arxiv.org/abs/2309.08600v2");
        assert_eq!(e.title, "Sparse Autoencoders Find Highly Interpretable Features in Language Models");
        assert_eq!(e.summary.as_deref(), Some("We show that sparse autoencoders can find interpretable features."));
        assert_eq!(e.published.as_deref(), Some("2023-09-15T17:48:05Z"));
    }

    #[test]
    fn parses_multiple_authors() {
        let entries = parse_feed(SINGLE_ENTRY_XML).unwrap();
        assert_eq!(entries[0].authors, vec!["Hoagy Cunningham", "Aidan Beren"]);
    }

    #[test]
    fn parses_primary_category_from_namespace_element() {
        let entries = parse_feed(SINGLE_ENTRY_XML).unwrap();
        assert_eq!(entries[0].primary_category.as_deref(), Some("cs.LG"));
    }

    #[test]
    fn parses_doi_from_namespace_element() {
        let entries = parse_feed(SINGLE_ENTRY_XML).unwrap();
        assert_eq!(entries[0].doi.as_deref(), Some("10.1234/example"));
    }

    #[test]
    fn entry_without_summary_has_none() {
        let xml = r#"<feed xmlns="http://www.w3.org/2005/Atom">
  <entry>
    <id>http://arxiv.org/abs/2309.08600v1</id>
    <title>No Abstract Paper</title>
    <published>2023-01-01T00:00:00Z</published>
    <author><name>Author One</name></author>
  </entry>
</feed>"#;
        let entries = parse_feed(xml).unwrap();
        assert_eq!(entries.len(), 1);
        assert!(entries[0].summary.is_none());
    }

    #[test]
    fn entry_without_id_is_skipped() {
        let xml = r#"<feed xmlns="http://www.w3.org/2005/Atom">
  <entry>
    <id></id>
    <title>No ID Paper</title>
  </entry>
</feed>"#;
        let entries = parse_feed(xml).unwrap();
        assert_eq!(entries.len(), 0);
    }

    #[test]
    fn multiple_entries_all_parsed() {
        let xml = r#"<feed xmlns="http://www.w3.org/2005/Atom">
  <entry>
    <id>http://arxiv.org/abs/2309.00001v1</id>
    <title>First Paper</title>
  </entry>
  <entry>
    <id>http://arxiv.org/abs/2309.00002v1</id>
    <title>Second Paper</title>
  </entry>
</feed>"#;
        let entries = parse_feed(xml).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].title, "First Paper");
        assert_eq!(entries[1].title, "Second Paper");
    }

    #[test]
    fn mismatched_closing_tag_returns_error() {
        // quick-xml treats truncated input as Eof (Ok), but a mismatched end
        // element triggers IllFormed::MismatchedEnd when check_end_names is on.
        let xml = "<feed><entry><id>http://arxiv.org/abs/123</id></wrong></feed>";
        assert!(parse_feed(xml).is_err());
    }

    #[test]
    fn author_name_does_not_clobber_paper_title() {
        // The in_author guard ensures <name> inside <author> does not
        // overwrite the already-parsed paper title.
        let entries = parse_feed(SINGLE_ENTRY_XML).unwrap();
        assert_eq!(
            entries[0].title,
            "Sparse Autoencoders Find Highly Interpretable Features in Language Models"
        );
    }
}

/// Read the `term` attribute from a `<primary_category>` element.
fn read_term_attr(e: &quick_xml::events::BytesStart<'_>, reader: &Reader<&[u8]>) -> Option<String> {
    let decoder = reader.decoder();
    e.attributes()
        .filter_map(|attr| attr.ok())
        .find(|attr| local_name(attr.key.as_ref()) == "term")
        .and_then(|attr| attr.decode_and_unescape_value(decoder).ok())
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}
