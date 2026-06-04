use rowan::{NodeOrToken, TextRange};

use crate::linter::diagnostics::{Diagnostic, Edit, Fix, Location};
use crate::linter::rules::{LintContext, Rule};
use crate::syntax::SyntaxKind;

pub struct AdjacentFootnoteRefsRule;

impl Rule for AdjacentFootnoteRefsRule {
    fn name(&self) -> &str {
        "adjacent-footnote-refs"
    }

    fn node_interests(&self) -> &'static [SyntaxKind] {
        &[SyntaxKind::FOOTNOTE_REFERENCE]
    }

    fn check(&self, cx: &LintContext) -> Vec<Diagnostic> {
        let input = cx.input;
        let mut diagnostics = Vec::new();

        for node in cx.nodes(SyntaxKind::FOOTNOTE_REFERENCE) {
            let Some(prev) = node.prev_sibling_or_token() else {
                continue;
            };
            let prev_node = match prev {
                NodeOrToken::Node(n) => n,
                NodeOrToken::Token(_) => continue,
            };
            if prev_node.kind() != SyntaxKind::FOOTNOTE_REFERENCE {
                continue;
            }

            let insert_at = node.text_range().start();
            let location = Location::from_range(node.text_range(), input);
            let fix = Fix {
                message: "Insert a space between the footnote references".to_string(),
                edits: vec![Edit {
                    range: TextRange::new(insert_at, insert_at),
                    replacement: " ".to_string(),
                }],
            };
            diagnostics.push(
                Diagnostic::warning(
                    location,
                    "adjacent-footnote-refs",
                    "Adjacent footnote references render as a single superscript number; \
                     insert a space to keep them visually distinct",
                )
                .with_fix(fix),
            );
        }

        diagnostics
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    fn parse_and_lint(input: &str) -> Vec<Diagnostic> {
        let config = Config::default();
        let tree = crate::parser::parse(input, Some(config.clone()));
        AdjacentFootnoteRefsRule.check_tree(&tree, input, &config, None)
    }

    #[test]
    fn flags_adjacent_pair() {
        let input = "Text[^a][^b].\n\n[^a]: a\n\n[^b]: b\n";
        let diagnostics = parse_and_lint(input);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, "adjacent-footnote-refs");
        let fix = diagnostics[0].fix.as_ref().expect("fix");
        assert_eq!(fix.edits.len(), 1);
        assert_eq!(fix.edits[0].replacement, " ");
    }

    #[test]
    fn flags_each_in_chain() {
        // [^a][^b][^c] should produce two diagnostics (one per gap).
        let input = "Text[^a][^b][^c].\n\n[^a]: a\n\n[^b]: b\n\n[^c]: c\n";
        let diagnostics = parse_and_lint(input);
        assert_eq!(diagnostics.len(), 2);
    }

    #[test]
    fn ignores_space_separated() {
        let input = "Text[^a] [^b].\n\n[^a]: a\n\n[^b]: b\n";
        let diagnostics = parse_and_lint(input);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn ignores_lone_reference() {
        let input = "Text[^a] more.\n\n[^a]: a\n";
        let diagnostics = parse_and_lint(input);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn ignores_text_between() {
        let input = "Text[^a]x[^b].\n\n[^a]: a\n\n[^b]: b\n";
        let diagnostics = parse_and_lint(input);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn fix_inserts_space_at_boundary() {
        let input = "Text[^a][^b].\n\n[^a]: a\n\n[^b]: b\n";
        let diagnostics = parse_and_lint(input);
        let edit = &diagnostics[0].fix.as_ref().unwrap().edits[0];
        let start: usize = edit.range.start().into();
        let end: usize = edit.range.end().into();
        assert_eq!(start, end);
        // The insertion point sits at the start of the second reference.
        assert_eq!(&input[..start], "Text[^a]");
    }
}
