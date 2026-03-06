use crate::config::Config;
use crate::linter::diagnostics::{Diagnostic, Location};
use crate::linter::rules::Rule;
use crate::metadata::{
    bibliography_range_map, format_bibliography_load_error, inline_bib_conflicts,
    inline_reference_contains, inline_reference_duplicates,
};
use crate::syntax::{AstNode, Citation, Crossref, SyntaxNode};

pub struct CitationKeysRule;

impl Rule for CitationKeysRule {
    fn name(&self) -> &str {
        "citation-keys"
    }

    fn check(
        &self,
        tree: &SyntaxNode,
        input: &str,
        config: &Config,
        metadata: Option<&crate::metadata::DocumentMetadata>,
    ) -> Vec<Diagnostic> {
        if !config.extensions.citations {
            return Vec::new();
        }

        let mut diagnostics = Vec::new();

        let Some(metadata) = metadata else {
            return diagnostics;
        };

        let parse = metadata.bibliography_parse.as_ref();
        if let Some(parse) = parse {
            let range_by_path = bibliography_range_map(metadata);
            for error in &parse.index.load_errors {
                let range = range_by_path
                    .get(&error.path)
                    .copied()
                    .unwrap_or_else(|| tree.text_range());
                let location = Location::from_range(range, input);
                diagnostics.push(Diagnostic::error(
                    location,
                    "bibliography-load-error",
                    format!(
                        "Failed to load bibliography {}: {}",
                        error.path.display(),
                        format_bibliography_load_error(&error.message)
                    ),
                ));
            }

            for message in &parse.parse_errors {
                let location = Location::from_range(tree.text_range(), input);
                diagnostics.push(Diagnostic::error(
                    location,
                    "bibliography-parse-error",
                    format!("Invalid bibliography entry: {}", message),
                ));
            }

            for duplicate in &parse.index.duplicates {
                let range = range_by_path
                    .get(&duplicate.first.file)
                    .or_else(|| range_by_path.get(&duplicate.duplicate.file))
                    .copied()
                    .unwrap_or_else(|| tree.text_range());
                let location = Location::from_range(range, input);
                diagnostics.push(Diagnostic::warning(
                    location,
                    "duplicate-bibliography-key",
                    format!(
                        "Duplicate bibliography key '{}' in {} and {}",
                        duplicate.key,
                        duplicate.first.file.display(),
                        duplicate.duplicate.file.display()
                    ),
                ));
            }
        }

        for duplicate in inline_reference_duplicates(&metadata.inline_references) {
            let location = Location::from_range(duplicate.duplicate.range, input);
            diagnostics.push(Diagnostic::warning(
                location,
                "duplicate-inline-reference-id",
                format!("Duplicate inline reference id '{}'", duplicate.key),
            ));
        }

        if let Some(parse) = parse {
            for conflict in inline_bib_conflicts(&metadata.inline_references, &parse.index) {
                let location = Location::from_range(conflict.inline.range, input);
                diagnostics.push(Diagnostic::warning(
                    location,
                    "duplicate-inline-reference-id",
                    format!(
                        "Duplicate inline reference id '{}' in {} and {}",
                        conflict.key,
                        conflict.inline.path.display(),
                        conflict.bib.file.display()
                    ),
                ));
            }
        }

        if parse.is_none() && metadata.inline_references.is_empty() {
            return diagnostics;
        }

        for key_text in &metadata.citations.keys {
            if tree
                .descendants()
                .filter_map(Crossref::cast)
                .flat_map(|crossref| crossref.keys())
                .any(|crossref_key| crossref_key.text() == *key_text)
            {
                continue;
            }
            if parse.and_then(|parse| parse.index.get(key_text)).is_none()
                && !inline_reference_contains(&metadata.inline_references, key_text)
            {
                // Find all citation nodes that reference this missing key
                for citation in tree.descendants().filter_map(Citation::cast) {
                    for citation_key in citation.keys() {
                        if citation_key.text() == *key_text {
                            // Use the citation node range (includes @) instead of just the key
                            let location =
                                Location::from_range(citation.syntax().text_range(), input);
                            diagnostics.push(Diagnostic::warning(
                                location,
                                "missing-bibliography-key",
                                format!("Citation key '{}' not found in bibliography", key_text),
                            ));
                        }
                    }
                }
            }
        }

        diagnostics
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use rowan::{TextRange, TextSize};

    fn parse_and_lint(
        input: &str,
        metadata: Option<crate::metadata::DocumentMetadata>,
    ) -> Vec<Diagnostic> {
        let config = Config::default();
        let tree = crate::parser::parse(input, Some(config.clone()));
        let rule = CitationKeysRule;
        if let Some(metadata) = metadata {
            return rule.check(&tree, input, &config, Some(&metadata));
        }
        rule.check(&tree, input, &config, None)
    }

    #[test]
    fn missing_key_emits_warning() {
        let input = "Text [@missing].";
        let metadata = crate::metadata::DocumentMetadata {
            bibliography: None,
            metadata_files: Vec::new(),
            bibliography_parse: Some(crate::metadata::BibliographyParse {
                index: crate::bibtex::BibIndex {
                    entries: std::collections::HashMap::new(),
                    duplicates: Vec::new(),
                    errors: Vec::new(),
                    files: Vec::new(),
                    load_errors: Vec::new(),
                },
                parse_errors: Vec::new(),
            }),
            inline_references: Vec::new(),
            citations: crate::metadata::CitationInfo {
                keys: vec!["missing".to_string()],
            },
            title: None,
            raw_yaml: String::new(),
        };

        let diagnostics = parse_and_lint(input, Some(metadata));
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, "missing-bibliography-key");
        assert!(diagnostics[0].message.contains("missing"));
    }

    #[test]
    fn missing_key_reports_correct_position() {
        let input = "Text [@missing].";
        let metadata = crate::metadata::DocumentMetadata {
            bibliography: None,
            metadata_files: Vec::new(),
            bibliography_parse: Some(crate::metadata::BibliographyParse {
                index: crate::bibtex::BibIndex {
                    entries: std::collections::HashMap::new(),
                    duplicates: Vec::new(),
                    errors: Vec::new(),
                    files: Vec::new(),
                    load_errors: Vec::new(),
                },
                parse_errors: Vec::new(),
            }),
            inline_references: Vec::new(),
            citations: crate::metadata::CitationInfo {
                keys: vec!["missing".to_string()],
            },
            title: None,
            raw_yaml: String::new(),
        };

        let diagnostics = parse_and_lint(input, Some(metadata));
        assert_eq!(diagnostics.len(), 1);

        // The citation [@missing] starts at position 5 (after "Text ")
        // But we report it at the CITATION node level which includes brackets
        // Line 1, column 6 (1-indexed, pointing to '[')
        assert_eq!(diagnostics[0].location.line, 1);
        assert_eq!(diagnostics[0].location.column, 6);

        // The range should cover the entire citation including brackets
        let start: usize = diagnostics[0].location.range.start().into();
        let end: usize = diagnostics[0].location.range.end().into();
        assert_eq!(start, 5); // Position of '['
        assert_eq!(end, 15); // Position after ']'
    }

    #[test]
    fn bibliography_load_error_uses_yaml_range() {
        let input = "---\nbibliography: test.bib\n---\n\nText\n";
        let start = input.find("test.bib").unwrap();
        let end = start + "test.bib".len();
        let range = TextRange::new(TextSize::from(start as u32), TextSize::from(end as u32));
        let path = std::path::PathBuf::from("/tmp/test.bib");
        let metadata = crate::metadata::DocumentMetadata {
            bibliography: Some(crate::metadata::BibliographyInfo {
                paths: vec![path.clone()],
                source_ranges: vec![range],
            }),
            metadata_files: Vec::new(),
            bibliography_parse: Some(crate::metadata::BibliographyParse {
                index: crate::bibtex::BibIndex {
                    entries: std::collections::HashMap::new(),
                    duplicates: Vec::new(),
                    errors: Vec::new(),
                    files: Vec::new(),
                    load_errors: vec![crate::bibtex::BibLoadError {
                        path,
                        message: "No such file or directory (os error 2)".to_string(),
                    }],
                },
                parse_errors: Vec::new(),
            }),
            inline_references: Vec::new(),
            citations: crate::metadata::CitationInfo { keys: Vec::new() },
            title: None,
            raw_yaml: String::new(),
        };

        let diagnostics = parse_and_lint(input, Some(metadata));
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, "bibliography-load-error");
        assert_eq!(diagnostics[0].location.range.start(), range.start());
        assert_eq!(diagnostics[0].location.range.end(), range.end());
        assert!(diagnostics[0].message.ends_with("File not found"));
    }

    #[test]
    fn crossref_keys_do_not_emit_warning() {
        let input = "See @eq-missing for details.";
        let mut config = Config::default();
        config.extensions.quarto_crossrefs = true;

        let tree = crate::parser::parse(input, Some(config.clone()));
        let rule = CitationKeysRule;
        let metadata = crate::metadata::DocumentMetadata {
            bibliography: None,
            metadata_files: Vec::new(),
            bibliography_parse: Some(crate::metadata::BibliographyParse {
                index: crate::bibtex::BibIndex {
                    entries: std::collections::HashMap::new(),
                    duplicates: Vec::new(),
                    errors: Vec::new(),
                    files: Vec::new(),
                    load_errors: Vec::new(),
                },
                parse_errors: Vec::new(),
            }),
            inline_references: Vec::new(),
            citations: crate::metadata::CitationInfo {
                keys: vec!["eq-missing".to_string()],
            },
            title: None,
            raw_yaml: String::new(),
        };

        let diagnostics = rule.check(&tree, input, &config, Some(&metadata));
        assert!(diagnostics.is_empty());
    }
}
