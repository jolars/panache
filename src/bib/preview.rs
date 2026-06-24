//! Human-readable previews of bibliography entries.
//!
//! Renders a [`BibEntry`] into a compact markdown summary
//! (author, year, title, container, locator) for display in LSP hover and
//! completion previews. Format-agnostic: works with BibTeX, CSL-JSON,
//! CSL-YAML, and RIS entries by consulting the unified field map with the
//! usual cross-format aliases.

use crate::bib::BibEntry;

/// Format a bibliography entry as a one-line markdown summary.
///
/// Returns an empty string when the entry carries none of the recognized
/// fields. The title is emphasized with `*…*`; other parts are plain text.
pub fn format_entry_preview(entry: &BibEntry) -> String {
    let author = entry
        .fields
        .get("author")
        .or_else(|| entry.fields.get("editor"))
        .map(|s| s.as_str())
        .unwrap_or_default();

    let year = entry
        .fields
        .get("year")
        .or_else(|| entry.fields.get("date"))
        .or_else(|| entry.fields.get("issued"))
        .map(|s| s.as_str())
        .unwrap_or_default();

    let title = entry
        .fields
        .get("title")
        .or_else(|| entry.fields.get("booktitle"))
        .map(|s| s.as_str())
        .unwrap_or_default();

    let container = entry
        .fields
        .get("journal")
        .or_else(|| entry.fields.get("journaltitle"))
        .or_else(|| entry.fields.get("container-title"))
        .or_else(|| entry.fields.get("publisher"))
        .map(|s| s.as_str())
        .unwrap_or_default();

    let locator = build_locator(entry);

    let mut summary = String::new();
    if !author.is_empty() {
        summary.push_str(author);
    }
    if !year.is_empty() {
        if !summary.is_empty() {
            summary.push_str(" (");
            summary.push_str(year);
            summary.push(')');
        } else {
            summary.push_str(year);
        }
    }
    if !title.is_empty() {
        if !summary.is_empty() {
            summary.push_str(". ");
        }
        summary.push_str(&format!("*{}*", title));
    }
    if !container.is_empty() {
        summary.push_str(". ");
        summary.push_str(container);
    }
    if !locator.is_empty() {
        summary.push_str(", ");
        summary.push_str(&locator);
    }

    summary.trim().to_string()
}

fn build_locator(entry: &BibEntry) -> String {
    let volume = entry
        .fields
        .get("volume")
        .map(|s| s.as_str())
        .unwrap_or_default();
    let number = entry
        .fields
        .get("number")
        .or_else(|| entry.fields.get("issue"))
        .map(|s| s.as_str())
        .unwrap_or_default();
    let pages = entry
        .fields
        .get("pages")
        .or_else(|| entry.fields.get("page"))
        .map(|s| s.as_str())
        .unwrap_or_default();

    let mut parts = Vec::new();
    if !volume.is_empty() {
        if !number.is_empty() {
            parts.push(format!("{}({})", volume, number));
        } else {
            parts.push(volume.to_string());
        }
    } else if !number.is_empty() {
        parts.push(number.to_string());
    }
    if !pages.is_empty() {
        parts.push(pages.to_string());
    }
    parts.join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bib::{BibFormat, Span};
    use std::collections::HashMap;
    use std::path::PathBuf;

    fn entry(fields: &[(&str, &str)]) -> BibEntry {
        BibEntry {
            key: "k".to_string(),
            entry_type: Some("article".to_string()),
            fields: fields
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect::<HashMap<_, _>>(),
            source_file: PathBuf::from("refs.bib"),
            span: Span { start: 0, end: 0 },
            format: BibFormat::BibTeX,
        }
    }

    #[test]
    fn full_article() {
        let e = entry(&[
            ("author", "Smith, J."),
            ("year", "2020"),
            ("title", "On Things"),
            ("journal", "J. Things"),
            ("volume", "12"),
            ("number", "3"),
            ("pages", "45-67"),
        ]);
        assert_eq!(
            format_entry_preview(&e),
            "Smith, J. (2020). *On Things*. J. Things, 12(3), 45-67"
        );
    }

    #[test]
    fn missing_fields_are_skipped() {
        let e = entry(&[("title", "Just a Title")]);
        assert_eq!(format_entry_preview(&e), "*Just a Title*");
    }

    #[test]
    fn empty_entry_yields_empty_string() {
        let e = entry(&[]);
        assert_eq!(format_entry_preview(&e), "");
    }

    #[test]
    fn year_without_author_stands_alone() {
        let e = entry(&[("year", "1999"), ("title", "Solo")]);
        assert_eq!(format_entry_preview(&e), "1999. *Solo*");
    }

    #[test]
    fn locator_volume_only() {
        let e = entry(&[("title", "T"), ("volume", "5"), ("pages", "1-2")]);
        assert_eq!(format_entry_preview(&e), "*T*, 5, 1-2");
    }

    #[test]
    fn csl_style_aliases() {
        // CSL-YAML/JSON use `issued`, `container-title`, `editor`, `issue`.
        let e = entry(&[
            ("editor", "Doe, A."),
            ("issued", "2021"),
            ("booktitle", "A Collection"),
            ("container-title", "Series X"),
            ("issue", "7"),
        ]);
        assert_eq!(
            format_entry_preview(&e),
            "Doe, A. (2021). *A Collection*. Series X, 7"
        );
    }
}
