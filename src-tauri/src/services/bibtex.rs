//! BibTeX generation for vault citation export (RFC 0070).
//!
//! Turns a vault's [`CiteRecord`]s into a `.bib` document. This is a
//! best-effort, lossy, offline export: it emits `@article` / `@inproceedings` /
//! `@misc` entries from stored metadata only (title, authors, venue, year) and
//! deliberately emits **no** `doi` field — DOIs are not persisted on papers.
//!
//! # Ordering and stability
//!
//! Cite keys must stay stable across exports: if two papers share a base key and
//! the disambiguation suffix (`a`/`b`/`c`) were assigned in an unstable query
//! order, a re-export could swap which paper is `lecun2015` vs `lecun2015a` and
//! silently break a `.tex` that already compiled. To prevent that, entries are
//! sorted by `(first-author family, year, title)` before keys are assigned, so a
//! re-export of an unchanged vault produces byte-identical output.

use crate::domain::library::CiteRecord;
use std::collections::HashMap;

/// Title words too generic to seed a cite key; skipped when picking the word.
const TITLE_STOPWORDS: &[&str] = &["a", "an", "the", "of", "on", "in", "for", "and", "to"];

/// Renders `records` as a complete BibTeX document.
///
/// Entries are ordered by `(first-author family, year, title)` and separated by
/// a blank line; the document ends with a trailing newline. An empty slice
/// yields an empty string.
pub fn to_bibtex(records: &[CiteRecord]) -> String {
    if records.is_empty() {
        return String::new();
    }

    // Stable order is what makes cite keys reproducible across exports.
    let mut ordered: Vec<&CiteRecord> = records.iter().collect();
    ordered.sort_by(|a, b| {
        sort_family(a)
            .cmp(&sort_family(b))
            .then(a.year.cmp(&b.year))
            .then(a.title.cmp(&b.title))
    });

    // Assign keys in that order; the Nth paper sharing a base key gets the
    // (N-1)th disambiguation suffix (2nd -> "a", 3rd -> "b", ...).
    let mut counts: HashMap<String, usize> = HashMap::new();
    let entries: Vec<String> = ordered
        .iter()
        .map(|record| {
            let base = cite_key_base(record);
            let seen = counts.entry(base.clone()).or_insert(0);
            let key = if *seen == 0 {
                base.clone()
            } else {
                format!("{base}{}", collision_suffix(*seen - 1))
            };
            *seen += 1;
            format_entry(record, &key)
        })
        .collect();

    format!("{}\n", entries.join("\n\n"))
}

/// Folded first-author family name used only for sorting (empty sorts first).
fn sort_family(record: &CiteRecord) -> String {
    first_author(record)
        .map(|name| ascii_key(&family_name(name)))
        .unwrap_or_default()
}

/// First non-blank author name, if any.
fn first_author(record: &CiteRecord) -> Option<&str> {
    record
        .authors
        .iter()
        .map(|name| name.trim())
        .find(|name| !name.is_empty())
}

/// `firstAuthorFamily + year + firstSignificantTitleWord`, ASCII-folded and
/// lowercased (e.g. `lecun2015deep`). Falls back to `anon` for a missing author
/// and `nd` (no date) for a missing year.
fn cite_key_base(record: &CiteRecord) -> String {
    let author_part = match first_author(record) {
        Some(name) => {
            let folded = ascii_key(&family_name(name));
            if folded.is_empty() {
                "anon".to_string()
            } else {
                folded
            }
        }
        None => "anon".to_string(),
    };
    let year_part = if record.year > 0 {
        record.year.to_string()
    } else {
        "nd".to_string()
    };
    format!(
        "{author_part}{year_part}{}",
        first_significant_title_word(&record.title)
    )
}

/// The family (last) name from a "Given Family" or "Family, Given" string.
fn family_name(name: &str) -> String {
    let name = name.trim();
    if let Some((family, _)) = name.split_once(',') {
        return family.trim().to_string();
    }
    match name.rsplit_once(char::is_whitespace) {
        Some((_, family)) => family.to_string(),
        None => name.to_string(),
    }
}

/// First title word that is neither empty nor a stopword, ASCII-folded.
fn first_significant_title_word(title: &str) -> String {
    for raw in title.split_whitespace() {
        let word = ascii_key(raw);
        if word.is_empty() || TITLE_STOPWORDS.contains(&word.as_str()) {
            continue;
        }
        return word;
    }
    String::new()
}

/// Lowercased, ASCII-alphanumeric-only folding used for cite keys.
fn ascii_key(value: &str) -> String {
    value
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

/// Disambiguation suffix: 0 -> "a", 1 -> "b", ... More than 26 collisions in one
/// vault is implausible; we wrap modulo 26 rather than guard against it.
fn collision_suffix(index: usize) -> String {
    ((b'a' + (index % 26) as u8) as char).to_string()
}

/// Renders one BibTeX entry. The title is double-braced so styles do not
/// lowercase it; `author`/`year`/venue lines are omitted when absent.
fn format_entry(record: &CiteRecord, key: &str) -> String {
    let (entry_type, venue_field) = entry_layout(&record.venue);

    let mut fields: Vec<String> = Vec::new();
    fields.push(format!("  title = {{{{{}}}}}", escape_latex(record.title.trim())));

    let authors = format_authors(&record.authors);
    if !authors.is_empty() {
        fields.push(format!("  author = {{{authors}}}"));
    }
    if let Some(field) = venue_field {
        fields.push(format!("  {field} = {{{}}}", escape_latex(record.venue.trim())));
    }
    if record.year > 0 {
        fields.push(format!("  year = {{{}}}", record.year));
    }

    format!("@{entry_type}{{{key},\n{}\n}}", fields.join(",\n"))
}

/// Picks the entry type and the field name that carries the venue, from the
/// shape of `venue`. Returns `None` for the venue field when there is no venue.
fn entry_layout(venue: &str) -> (&'static str, Option<&'static str>) {
    let venue = venue.trim();
    if venue.is_empty() {
        return ("misc", None);
    }
    let lower = venue.to_lowercase();
    if ["conf", "proceedings", "workshop", "symposium"]
        .iter()
        .any(|marker| lower.contains(marker))
    {
        return ("inproceedings", Some("booktitle"));
    }
    if lower.contains("arxiv") {
        return ("misc", Some("howpublished"));
    }
    ("article", Some("journal"))
}

/// Joins non-blank authors as `Family, Given` with ` and ` (BibTeX convention).
fn format_authors(authors: &[String]) -> String {
    authors
        .iter()
        .map(|name| name.trim())
        .filter(|name| !name.is_empty())
        .map(format_author_name)
        .collect::<Vec<_>>()
        .join(" and ")
}

/// Formats a single author as `Family, Given`, escaping LaTeX specials. Names
/// already in `Family, Given` form and single-token names are left as written.
fn format_author_name(name: &str) -> String {
    let name = name.trim();
    let formatted = if name.contains(',') {
        name.to_string()
    } else {
        match name.rsplit_once(char::is_whitespace) {
            Some((given, family)) => format!("{}, {}", family.trim(), given.trim()),
            None => name.to_string(),
        }
    };
    escape_latex(&formatted)
}

/// Escapes LaTeX special characters in one char-by-char pass (so nothing is
/// double-escaped). Backslash is handled here too, not by a separate pass.
fn escape_latex(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for c in input.chars() {
        match c {
            '\\' => out.push_str("\\textbackslash{}"),
            '&' | '%' | '$' | '#' | '_' | '{' | '}' => {
                out.push('\\');
                out.push(c);
            }
            '~' => out.push_str("\\textasciitilde{}"),
            '^' => out.push_str("\\textasciicircum{}"),
            other => out.push(other),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(title: &str, authors: &[&str], venue: &str, year: i32) -> CiteRecord {
        CiteRecord {
            title: title.to_string(),
            authors: authors.iter().map(|a| a.to_string()).collect(),
            venue: venue.to_string(),
            year,
        }
    }

    #[test]
    fn renders_a_basic_article() {
        let out = to_bibtex(&[record(
            "Deep Learning",
            &["Yann LeCun", "Yoshua Bengio", "Geoffrey Hinton"],
            "Nature",
            2015,
        )]);
        assert_eq!(
            out,
            "@article{lecun2015deep,\n  \
             title = {{Deep Learning}},\n  \
             author = {LeCun, Yann and Bengio, Yoshua and Hinton, Geoffrey},\n  \
             journal = {Nature},\n  \
             year = {2015}\n}\n"
        );
    }

    #[test]
    fn collision_suffixes_are_order_independent() {
        let a = record("Deep nets", &["Yann LeCun"], "Nature", 2015);
        let b = record("Deep dreams", &["Yann LeCun"], "Nature", 2015);

        let forward = to_bibtex(&[a.clone(), b.clone()]);
        let reversed = to_bibtex(&[b, a]);

        // Same input set in any order -> byte-identical output (stable keys).
        assert_eq!(forward, reversed);
        // Sorted by title, "dreams" precedes "nets": base then "a" suffix.
        assert!(forward.contains("@article{lecun2015deep,"));
        assert!(forward.contains("@article{lecun2015deepa,"));
    }

    #[test]
    fn missing_author_and_year_use_fallbacks() {
        let out = to_bibtex(&[record("Manifesto of nothing", &[], "", 0)]);
        assert!(out.contains("@misc{anonndmanifesto,"));
        assert!(!out.contains("author ="));
        assert!(!out.contains("year ="));
    }

    #[test]
    fn entry_type_follows_venue() {
        let conf = to_bibtex(&[record("X", &["A B"], "Proceedings of NeurIPS", 2020)]);
        assert!(conf.contains("@inproceedings{"));
        assert!(conf.contains("booktitle = {Proceedings of NeurIPS}"));

        let preprint = to_bibtex(&[record("Y", &["A B"], "arXiv", 2020)]);
        assert!(preprint.contains("@misc{"));
        assert!(preprint.contains("howpublished = {arXiv}"));

        let journal = to_bibtex(&[record("Z", &["A B"], "Nature", 2020)]);
        assert!(journal.contains("@article{"));
        assert!(journal.contains("journal = {Nature}"));
    }

    #[test]
    fn escapes_specials_without_double_escaping() {
        let out = to_bibtex(&[record("Cost & C_use of 50% \\ x", &["A B"], "Nature", 2020)]);
        assert!(out.contains("title = {{Cost \\& C\\_use of 50\\% \\textbackslash{} x}}"));
    }

    #[test]
    fn formats_author_name_variants() {
        assert_eq!(format_author_name("Yann LeCun"), "LeCun, Yann");
        assert_eq!(format_author_name("Plato"), "Plato");
        assert_eq!(format_author_name("LeCun, Yann"), "LeCun, Yann");
    }

    #[test]
    fn empty_input_is_empty_output() {
        assert_eq!(to_bibtex(&[]), "");
    }
}
