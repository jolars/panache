use crate::config::Config;
use crate::linter::diagnostics::{Diagnostic, Edit, Fix, Location};
use crate::linter::rules::Rule;
use crate::syntax::{SyntaxKind, SyntaxNode};

pub struct HeadingHierarchyRule;

impl Rule for HeadingHierarchyRule {
    fn name(&self) -> &str {
        "heading-hierarchy"
    }

    fn check(
        &self,
        tree: &SyntaxNode,
        input: &str,
        _config: &Config,
        _metadata: Option<&crate::metadata::DocumentMetadata>,
    ) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();
        let headings = collect_headings(tree);

        let mut prev_level: Option<usize> = None;

        for (range, level) in headings {
            if let Some(prev) = prev_level
                && level > prev + 1
            {
                let location = Location::from_range(range, input);
                let expected_level = prev + 1;

                let diagnostic = Diagnostic::warning(
                    location,
                    "heading-hierarchy",
                    format!(
                        "Heading level skipped from h{} to h{}; expected h{}",
                        prev, level, expected_level
                    ),
                )
                .with_fix({
                    if let Some(node) = heading_node_at_range(tree, range) {
                        create_fix(&node, level, expected_level)
                    } else {
                        Fix {
                            message: "Could not create fix".to_string(),
                            edits: vec![],
                        }
                    }
                });

                diagnostics.push(diagnostic);
            }

            prev_level = Some(level);
        }

        diagnostics
    }
}

fn collect_headings(tree: &SyntaxNode) -> Vec<(rowan::TextRange, usize)> {
    let db = crate::salsa::SalsaDb::default();
    crate::salsa::symbol_usage_index_from_tree(&db, tree)
        .heading_sequence()
        .to_vec()
}

fn heading_node_at_range(tree: &SyntaxNode, range: rowan::TextRange) -> Option<SyntaxNode> {
    tree.descendants()
        .find(|node| node.kind() == SyntaxKind::HEADING && node.text_range() == range)
}

fn create_fix(heading: &SyntaxNode, current_level: usize, expected_level: usize) -> Fix {
    // Find the AtxHeadingMarker node, then get its first token child
    for child in heading.children() {
        if child.kind() == SyntaxKind::ATX_HEADING_MARKER {
            // The marker node contains a token with the actual ### text
            if let Some(token) = child.first_token() {
                let range = token.text_range();
                let replacement = "#".repeat(expected_level);

                return Fix {
                    message: format!(
                        "Change heading level from {} to {}",
                        current_level, expected_level
                    ),
                    edits: vec![Edit { range, replacement }],
                };
            }
        }
    }

    Fix {
        message: "Could not create fix".to_string(),
        edits: vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    fn parse_and_lint(input: &str) -> Vec<Diagnostic> {
        let config = Config::default();
        // Use main parse function which now includes inline parsing
        let tree = crate::parser::parse(input, Some(config.clone()));

        let rule = HeadingHierarchyRule;
        rule.check(&tree, input, &config, None)
    }

    #[test]
    fn test_valid_hierarchy() {
        let input = "# H1\n\n## H2\n\n### H3\n";
        let diagnostics = parse_and_lint(input);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_single_skip() {
        let input = "# H1\n\n### H3\n";
        let diagnostics = parse_and_lint(input);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, "heading-hierarchy");
        assert!(diagnostics[0].message.contains("h1 to h3"));
    }

    #[test]
    fn test_multiple_skips() {
        let input = "# H1\n\n#### H4\n";
        let diagnostics = parse_and_lint(input);
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].message.contains("h1 to h4"));
    }

    #[test]
    fn test_same_level_valid() {
        let input = "# H1\n\n# H1 again\n\n## H2\n";
        let diagnostics = parse_and_lint(input);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_starts_with_h2() {
        let input = "## H2\n\n### H3\n";
        let diagnostics = parse_and_lint(input);
        // Starting with h2 is allowed - no previous heading to compare to
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_fix_generation() {
        let input = "# H1\n\n### H3\n";
        let diagnostics = parse_and_lint(input);
        assert_eq!(diagnostics.len(), 1);

        let fix = diagnostics[0].fix.as_ref().unwrap();
        assert_eq!(fix.edits.len(), 1);
        assert_eq!(fix.edits[0].replacement, "##");
    }

    #[test]
    fn test_ignores_headings_inside_containers() {
        let input = "# H1\n\n- # Nested\n\n### H3\n";
        let diagnostics = parse_and_lint(input);
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].message.contains("h1 to h3"));
    }
}
