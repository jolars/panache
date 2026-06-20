use crate::linter::diagnostics::{Diagnostic, Location};
use crate::linter::rules::{DiagnosticCode, LintContext, Requirement, Rule, RuleMeta};
use crate::syntax::{SyntaxKind, SyntaxNode};
use rowan::NodeOrToken;

pub struct EmptyListItemRule;

impl Rule for EmptyListItemRule {
    fn name(&self) -> &str {
        "empty-list-item"
    }

    fn metadata(&self) -> RuleMeta {
        RuleMeta {
            name: "empty-list-item",
            default_on: true,
            requires: Requirement::Always,
            auto_fix: false,
            codes: const { &[DiagnosticCode::warning("empty-list-item")] },
        }
    }

    fn node_interests(&self) -> &'static [SyntaxKind] {
        &[SyntaxKind::LIST_ITEM]
    }

    fn check(&self, cx: &LintContext) -> Vec<Diagnostic> {
        let input = cx.input;
        let mut diagnostics = Vec::new();

        for node in cx.nodes(SyntaxKind::LIST_ITEM) {
            if let Some(diag) = classify(node, input) {
                diagnostics.push(diag);
            }
        }

        diagnostics
    }
}

fn classify(list_item: &SyntaxNode, input: &str) -> Option<Diagnostic> {
    let marker = list_item
        .children_with_tokens()
        .find(|c| c.kind() == SyntaxKind::LIST_MARKER)?;
    let marker_range = marker.text_range();

    if is_directly_empty(list_item) {
        let location = Location::from_range(marker_range, input);
        return Some(Diagnostic::warning(
            location,
            "empty-list-item",
            "List item has no content",
        ));
    }

    if let Some(underline) = setext_dash_underline_only_content(list_item) {
        let location = Location::from_range(underline.text_range(), input);
        return Some(Diagnostic::warning(
            location,
            "empty-list-item",
            "Bare `-` after a list-item line was parsed as a Setext H2 underline, \
             merging this item with the previous text",
        ));
    }

    None
}

/// A list item is "directly empty" when, after its `LIST_MARKER`, the only
/// inline content is whitespace and the terminating newline. Nested
/// containers (a nested LIST, BLOCKQUOTE, etc.) count as content.
fn is_directly_empty(list_item: &SyntaxNode) -> bool {
    let mut saw_marker = false;
    for child in list_item.children_with_tokens() {
        match child {
            NodeOrToken::Token(tok) => match tok.kind() {
                SyntaxKind::LIST_MARKER => saw_marker = true,
                SyntaxKind::WHITESPACE | SyntaxKind::NEWLINE => {}
                _ => return false,
            },
            NodeOrToken::Node(node) => match node.kind() {
                SyntaxKind::PLAIN | SyntaxKind::PARAGRAPH => {
                    if !inline_container_is_blank(&node) {
                        return false;
                    }
                }
                _ => return false,
            },
        }
    }
    saw_marker
}

fn inline_container_is_blank(node: &SyntaxNode) -> bool {
    node.descendants_with_tokens().all(|c| {
        matches!(
            c.kind(),
            SyntaxKind::PLAIN
                | SyntaxKind::PARAGRAPH
                | SyntaxKind::WHITESPACE
                | SyntaxKind::NEWLINE
        )
    })
}

/// Returns the `SETEXT_HEADING_UNDERLINE` token when the item's sole content
/// is a Setext H2 heading (`-` underline). H1 (`=`) is not flagged because
/// `=` carries no list-marker confusion.
fn setext_dash_underline_only_content(list_item: &SyntaxNode) -> Option<SyntaxNode> {
    let mut heading = None;
    for child in list_item.children_with_tokens() {
        match child {
            NodeOrToken::Token(tok) => match tok.kind() {
                SyntaxKind::LIST_MARKER | SyntaxKind::WHITESPACE | SyntaxKind::NEWLINE => {}
                _ => return None,
            },
            NodeOrToken::Node(node) => {
                if node.kind() != SyntaxKind::HEADING || heading.is_some() {
                    return None;
                }
                heading = Some(node);
            }
        }
    }

    let heading = heading?;
    let underline = heading
        .children()
        .find(|n| n.kind() == SyntaxKind::SETEXT_HEADING_UNDERLINE)?;
    let underline_text = underline.text().to_string();
    underline_text
        .chars()
        .all(|c| c == '-')
        .then_some(underline)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    fn parse_and_lint(input: &str) -> Vec<Diagnostic> {
        let config = Config::default();
        let tree = crate::parser::parse(input, Some(config.clone()));
        EmptyListItemRule.check_tree(&tree, input, &config, None)
    }

    #[test]
    fn flags_bare_bullet_between_items() {
        let diagnostics = parse_and_lint("- one\n-\n- three\n");
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, "empty-list-item");
        assert!(diagnostics[0].message.contains("no content"));
    }

    #[test]
    fn flags_bare_ordered_marker() {
        let diagnostics = parse_and_lint("1.\n2. next\n");
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, "empty-list-item");
    }

    #[test]
    fn flags_marker_with_only_trailing_whitespace() {
        let diagnostics = parse_and_lint("- one\n-   \n- three\n");
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn flags_setext_h2_inside_list_item() {
        let diagnostics = parse_and_lint("- bullet trap\n  -\n");
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].message.contains("Setext"));
    }

    #[test]
    fn does_not_flag_setext_h1_inside_list_item() {
        let diagnostics = parse_and_lint("- heading\n  ===\n");
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn does_not_flag_nonempty_items() {
        let diagnostics = parse_and_lint("- one\n- two\n- three\n");
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn does_not_flag_item_with_nested_list() {
        let diagnostics = parse_and_lint("- parent\n  - child\n");
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn flags_marker_at_end_of_input() {
        let diagnostics = parse_and_lint("-");
        assert_eq!(diagnostics.len(), 1);
    }
}
