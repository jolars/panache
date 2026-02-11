use crate::config::Config;
use crate::linter::diagnostics::{Diagnostic, Edit, Fix, Location};
use crate::linter::rules::Rule;
use crate::syntax::{SyntaxKind, SyntaxNode};

pub struct HeadingHierarchyRule;

impl Rule for HeadingHierarchyRule {
    fn name(&self) -> &str {
        "heading-hierarchy"
    }

    fn check(&self, tree: &SyntaxNode, input: &str, _config: &Config) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();
        let headings = collect_headings(tree);

        let mut prev_level: Option<usize> = None;

        for (node, level) in headings {
            if let Some(prev) = prev_level
                && level > prev + 1
            {
                let location = Location::from_node(&node, input);
                let expected_level = prev + 1;

                let diagnostic = Diagnostic::warning(
                    location,
                    "heading-hierarchy",
                    format!(
                        "Heading level skipped from h{} to h{}; expected h{}",
                        prev, level, expected_level
                    ),
                )
                .with_fix(create_fix(&node, level, expected_level));

                diagnostics.push(diagnostic);
            }

            prev_level = Some(level);
        }

        diagnostics
    }
}

fn collect_headings(tree: &SyntaxNode) -> Vec<(SyntaxNode, usize)> {
    let mut headings = Vec::new();

    fn walk(node: &SyntaxNode, headings: &mut Vec<(SyntaxNode, usize)>) {
        if node.kind() == SyntaxKind::Heading
            && let Some(level) = extract_heading_level(node)
        {
            headings.push((node.clone(), level));
        }

        for child in node.children() {
            walk(&child, headings);
        }
    }

    walk(tree, &mut headings);
    headings
}

fn extract_heading_level(heading: &SyntaxNode) -> Option<usize> {
    for child in heading.children() {
        if child.kind() == SyntaxKind::AtxHeadingMarker {
            let marker_text = child.text().to_string();
            return Some(marker_text.trim().chars().filter(|&c| c == '#').count());
        }
    }
    None
}

fn create_fix(heading: &SyntaxNode, current_level: usize, expected_level: usize) -> Fix {
    // Find the AtxHeadingMarker node, then get its first token child
    for child in heading.children() {
        if child.kind() == SyntaxKind::AtxHeadingMarker {
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
    use crate::block_parser::BlockParser;
    use crate::config::Config;
    use crate::inline_parser::InlineParser;

    fn parse_and_lint(input: &str) -> Vec<Diagnostic> {
        let config = Config::default();
        let (tree, refs) = BlockParser::new(input, &config).parse();
        let tree = InlineParser::new(tree, config.clone(), refs).parse();

        let rule = HeadingHierarchyRule;
        rule.check(&tree, input, &config)
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
}
