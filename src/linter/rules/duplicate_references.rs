use crate::config::Config;
use crate::linter::diagnostics::{Diagnostic, Location};
use crate::linter::rules::Rule;
use crate::syntax::{AstNode, FootnoteDefinition, ReferenceDefinition, SyntaxNode};
use crate::utils::normalize_label;
use std::collections::HashMap;

pub struct DuplicateReferencesRule;

impl Rule for DuplicateReferencesRule {
    fn name(&self) -> &str {
        "duplicate-reference-labels"
    }

    fn check(&self, tree: &SyntaxNode, input: &str, _config: &Config) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();

        // Check for duplicate reference definitions
        diagnostics.extend(check_duplicate_references(tree, input));

        // Check for duplicate footnote definitions
        diagnostics.extend(check_duplicate_footnotes(tree, input));

        diagnostics
    }
}

fn check_duplicate_references(tree: &SyntaxNode, input: &str) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    let mut seen_labels: HashMap<String, SyntaxNode> = HashMap::new();

    for node in tree.descendants() {
        if let Some(ref_def) = ReferenceDefinition::cast(node.clone()) {
            let label = ref_def.label();
            let normalized = normalize_label(&label);

            if let Some(first_node) = seen_labels.get(&normalized) {
                // This is a duplicate - create diagnostic
                let location = Location::from_node(&node, input);
                let first_location = Location::from_node(first_node, input);

                let diagnostic = Diagnostic::warning(
                    location,
                    "duplicate-reference-labels",
                    format!(
                        "Duplicate reference label '[{}]' (first defined at line {})",
                        label, first_location.line
                    ),
                );

                diagnostics.push(diagnostic);
            } else {
                // First occurrence - record it
                seen_labels.insert(normalized, node);
            }
        }
    }

    diagnostics
}

fn check_duplicate_footnotes(tree: &SyntaxNode, input: &str) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    let mut seen_ids: HashMap<String, SyntaxNode> = HashMap::new();

    for node in tree.descendants() {
        if let Some(fn_def) = FootnoteDefinition::cast(node.clone()) {
            let id = fn_def.id();
            let normalized = normalize_label(&id);

            if let Some(first_node) = seen_ids.get(&normalized) {
                // This is a duplicate - create diagnostic
                let location = Location::from_node(&node, input);
                let first_location = Location::from_node(first_node, input);

                let diagnostic = Diagnostic::warning(
                    location,
                    "duplicate-reference-labels",
                    format!(
                        "Duplicate footnote ID '[^{}]' (first defined at line {})",
                        id, first_location.line
                    ),
                );

                diagnostics.push(diagnostic);
            } else {
                // First occurrence - record it
                seen_ids.insert(normalized, node);
            }
        }
    }

    diagnostics
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::parser::block_parser::BlockParser;
    use crate::parser::inline_parser::InlineParser;

    fn parse_and_lint(input: &str) -> Vec<Diagnostic> {
        let config = Config::default();
        let (tree, refs) = BlockParser::new(input, &config).parse();
        let tree = InlineParser::new(tree, config.clone(), refs).parse();

        let rule = DuplicateReferencesRule;
        rule.check(&tree, input, &config)
    }

    #[test]
    fn test_no_duplicates() {
        let input = r#"[ref1]: https://example.com
[ref2]: https://another.com
[ref3]: https://third.com
"#;
        let diagnostics = parse_and_lint(input);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_duplicate_reference_labels() {
        let input = r#"[ref1]: https://example.com
[ref1]: https://duplicate.com
"#;
        let diagnostics = parse_and_lint(input);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, "duplicate-reference-labels");
        assert!(diagnostics[0].message.contains("[ref1]"));
        assert!(diagnostics[0].message.contains("line 1"));
    }

    #[test]
    fn test_multiple_duplicates() {
        let input = r#"[ref1]: https://example.com
[ref1]: https://duplicate1.com
[ref2]: https://another.com
[ref1]: https://duplicate2.com
[ref2]: https://duplicate3.com
"#;
        let diagnostics = parse_and_lint(input);
        assert_eq!(diagnostics.len(), 3); // 2 duplicates of ref1, 1 duplicate of ref2
        assert!(
            diagnostics
                .iter()
                .all(|d| d.code == "duplicate-reference-labels")
        );
    }

    #[test]
    fn test_case_insensitive_matching() {
        let input = r#"[MyRef]: https://example.com
[myref]: https://duplicate.com
[MYREF]: https://another-duplicate.com
"#;
        let diagnostics = parse_and_lint(input);
        assert_eq!(diagnostics.len(), 2); // Second and third are duplicates of first
        assert!(diagnostics[0].message.contains("[myref]"));
        assert!(diagnostics[1].message.contains("[MYREF]"));
    }

    #[test]
    fn test_whitespace_normalization() {
        let input = r#"[my ref]: https://example.com
[my  ref]: https://duplicate.com
[my   ref]: https://another-duplicate.com
"#;
        let diagnostics = parse_and_lint(input);
        assert_eq!(diagnostics.len(), 2); // Extra whitespace should be normalized
    }

    #[test]
    fn test_duplicate_footnotes() {
        let input = r#"[^1]: First footnote
[^1]: Duplicate footnote
"#;
        let diagnostics = parse_and_lint(input);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, "duplicate-reference-labels");
        assert!(diagnostics[0].message.contains("[^1]"));
    }

    #[test]
    fn test_footnote_case_insensitive() {
        let input = r#"[^Note]: First footnote
[^note]: Duplicate footnote
[^NOTE]: Another duplicate
"#;
        let diagnostics = parse_and_lint(input);
        assert_eq!(diagnostics.len(), 2);
        assert!(diagnostics[0].message.contains("[^note]"));
        assert!(diagnostics[1].message.contains("[^NOTE]"));
    }

    #[test]
    fn test_mixed_references_and_footnotes() {
        let input = r#"[ref]: https://example.com
[ref]: https://duplicate.com

[^1]: Footnote content
[^1]: Duplicate footnote
"#;
        let diagnostics = parse_and_lint(input);
        assert_eq!(diagnostics.len(), 2); // One duplicate ref, one duplicate footnote
    }

    #[test]
    fn test_first_definition_not_flagged() {
        let input = r#"[ref]: https://example.com
[ref]: https://duplicate.com
"#;
        let diagnostics = parse_and_lint(input);
        // Only the second occurrence should be flagged
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].location.line, 2);
    }
}
