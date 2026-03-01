use crate::config::Config;
use crate::linter::diagnostics::{Diagnostic, Location};
use crate::linter::rules::Rule;
use crate::syntax::SyntaxNode;

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

        let Some(parse) = metadata.bibliography_parse.as_ref() else {
            return diagnostics;
        };

        for error in &parse.index.load_errors {
            let location = Location::from_range(tree.text_range(), input);
            diagnostics.push(Diagnostic::error(
                location,
                "bibliography-load-error",
                format!(
                    "Failed to load bibliography {}: {}",
                    error.path.display(),
                    error.message
                ),
            ));
        }

        for duplicate in &parse.index.duplicates {
            let location = Location::from_range(tree.text_range(), input);
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

        for key_text in &metadata.citations.keys {
            if parse.index.get(key_text).is_none() {
                let location = Location::from_range(tree.text_range(), input);
                diagnostics.push(Diagnostic::warning(
                    location,
                    "missing-bibliography-key",
                    format!("Citation key '{}' not found in bibliography", key_text),
                ));
            }
        }

        diagnostics
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

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
            bibliography_parse: Some(crate::metadata::BibliographyParse {
                index: crate::bibtex::BibIndex {
                    entries: std::collections::HashMap::new(),
                    duplicates: Vec::new(),
                    errors: Vec::new(),
                    files: Vec::new(),
                    load_errors: Vec::new(),
                },
            }),
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
}
