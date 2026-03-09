use rowan::TextRange;

use crate::linter::diagnostics::{Diagnostic, Location};
use crate::metadata::{
    DocumentMetadata, InlineBibConflict, InlineReferenceDuplicate, YamlError,
    bibliography_range_map, format_bibliography_load_error, inline_bib_conflicts,
    inline_reference_duplicates,
};

pub fn yaml_error_diagnostic(error: &YamlError, text: &str) -> Option<Diagnostic> {
    match error {
        YamlError::ParseError {
            message,
            line,
            column,
        } => {
            let offset = line_col_to_offset(text, *line as usize, *column as usize);
            let range = TextRange::new((offset as u32).into(), (offset as u32).into());
            Some(Diagnostic::warning(
                Location::from_range(range, text),
                "yaml-parse-error",
                format!("YAML parse error: {}", message),
            ))
        }
        YamlError::StructureError(msg) => Some(Diagnostic::warning(
            Location::from_range(TextRange::default(), text),
            "yaml-structure-error",
            format!("YAML structure error: {}", msg),
        )),
        YamlError::NotFound(_) => None,
    }
}

pub fn metadata_diagnostics(metadata: &DocumentMetadata, text: &str) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    diagnostics.extend(check_bibliography_parse(metadata, text));
    diagnostics.extend(inline_reference_diagnostics(metadata, text));
    diagnostics
}

fn check_bibliography_parse(metadata: &DocumentMetadata, text: &str) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    let Some(parse) = metadata.bibliography_parse.as_ref() else {
        return diagnostics;
    };
    let range_by_path = bibliography_range_map(metadata);
    let source_ranges = metadata
        .bibliography
        .as_ref()
        .map(|info| info.source_ranges.as_slice())
        .unwrap_or_default();
    let fallback_range = source_ranges.first().cloned().unwrap_or_default();

    for error in &parse.index.load_errors {
        let range = range_by_path
            .get(&error.path)
            .copied()
            .unwrap_or(fallback_range);
        let message = format_bibliography_load_error(&error.message);
        diagnostics.push(Diagnostic::error(
            Location::from_range(range, text),
            "bibliography-load-error",
            format!(
                "Failed to load bibliography {}: {}",
                error.path.display(),
                message
            ),
        ));
    }

    for duplicate in &parse.index.duplicates {
        let range = range_by_path
            .get(&duplicate.first.file)
            .or_else(|| range_by_path.get(&duplicate.duplicate.file))
            .copied()
            .unwrap_or(fallback_range);
        diagnostics.push(Diagnostic::warning(
            Location::from_range(range, text),
            "duplicate-bibliography-key",
            format!(
                "Duplicate bibliography key '{}' in {} and {}",
                duplicate.key,
                duplicate.first.file.display(),
                duplicate.duplicate.file.display()
            ),
        ));
    }

    for message in &parse.parse_errors {
        diagnostics.push(Diagnostic::error(
            Location::from_range(fallback_range, text),
            "bibliography-parse-error",
            format!("Invalid bibliography entry: {}", message),
        ));
    }

    diagnostics
}

fn inline_reference_diagnostics(metadata: &DocumentMetadata, text: &str) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    if metadata.inline_references.is_empty() {
        return diagnostics;
    }
    for duplicate in inline_reference_duplicates(&metadata.inline_references) {
        diagnostics.push(inline_reference_duplicate_diagnostic(&duplicate, text));
    }
    if let Some(parse) = metadata.bibliography_parse.as_ref() {
        for conflict in inline_bib_conflicts(&metadata.inline_references, &parse.index) {
            diagnostics.push(inline_reference_conflict_diagnostic(&conflict, text));
        }
    }
    diagnostics
}

fn inline_reference_duplicate_diagnostic(
    duplicate: &InlineReferenceDuplicate,
    text: &str,
) -> Diagnostic {
    Diagnostic::warning(
        Location::from_range(duplicate.duplicate.range, text),
        "duplicate-inline-reference-id",
        format!("Duplicate inline reference id '{}'", duplicate.key),
    )
}

fn inline_reference_conflict_diagnostic(conflict: &InlineBibConflict, text: &str) -> Diagnostic {
    Diagnostic::warning(
        Location::from_range(conflict.inline.range, text),
        "duplicate-inline-reference-id",
        format!(
            "Duplicate inline reference id '{}' in {} and {}",
            conflict.key,
            conflict.inline.path.display(),
            conflict.bib.source_file.display()
        ),
    )
}

fn line_col_to_offset(input: &str, line: usize, column: usize) -> usize {
    let target_line = line.saturating_sub(1);
    let target_col = column.saturating_sub(1);
    let mut current_line = 0usize;
    let mut line_start = 0usize;

    for (idx, ch) in input.char_indices() {
        if current_line == target_line {
            return line_start
                + target_col.min(
                    input[line_start..]
                        .chars()
                        .take_while(|c| *c != '\n')
                        .count(),
                );
        }
        if ch == '\n' {
            current_line += 1;
            line_start = idx + ch.len_utf8();
        }
    }

    if current_line == target_line {
        return line_start + target_col.min(input[line_start..].chars().count());
    }

    input.len()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bib::{BibIndex, BibLoadError};
    use crate::metadata::{BibliographyInfo, BibliographyParse, CitationInfo};
    use std::collections::HashMap;
    use std::path::PathBuf;

    #[test]
    fn bibliography_load_error_uses_source_range() {
        let text = "---\nbibliography: test.bib\n---\n\nText\n";
        let start = text.find("test.bib").unwrap();
        let end = start + "test.bib".len();
        let range = TextRange::new((start as u32).into(), (end as u32).into());
        let path = PathBuf::from("/tmp/test.bib");

        let metadata = DocumentMetadata {
            bibliography: Some(BibliographyInfo {
                paths: vec![path.clone()],
                source_ranges: vec![range],
            }),
            metadata_files: Vec::new(),
            bibliography_parse: Some(BibliographyParse {
                index: BibIndex {
                    entries: HashMap::new(),
                    duplicates: Vec::new(),
                    errors: Vec::new(),
                    load_errors: vec![BibLoadError {
                        path: path.clone(),
                        message: "No such file or directory (os error 2)".to_string(),
                    }],
                },
                parse_errors: Vec::new(),
            }),
            inline_references: Vec::new(),
            citations: CitationInfo { keys: Vec::new() },
            title: None,
            raw_yaml: String::new(),
        };

        let diagnostics = metadata_diagnostics(&metadata, text);
        assert_eq!(diagnostics.len(), 1);
        let diag = &diagnostics[0];
        assert_eq!(diag.location.range, range);
        assert_eq!(
            diag.message,
            "Failed to load bibliography /tmp/test.bib: File not found"
        );
    }
}
