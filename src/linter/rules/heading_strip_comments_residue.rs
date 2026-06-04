use crate::linter::diagnostics::{Diagnostic, DiagnosticNoteKind, Location};
use crate::linter::rules::{LintContext, Rule};
use crate::syntax::{AttributeNode, Heading, InlineHtml, SyntaxKind};
use rowan::ast::AstNode;

pub struct HeadingStripCommentsResidueRule;

impl Rule for HeadingStripCommentsResidueRule {
    fn name(&self) -> &str {
        "heading-strip-comments-residue"
    }

    fn node_interests(&self) -> &'static [SyntaxKind] {
        &[SyntaxKind::HEADING]
    }

    fn check(&self, cx: &LintContext) -> Vec<Diagnostic> {
        let input = cx.input;
        let mut diagnostics = Vec::new();

        for heading in cx
            .nodes(SyntaxKind::HEADING)
            .iter()
            .cloned()
            .filter_map(Heading::cast)
        {
            // The companion rule `heading-eaten-attrs` already covers headings
            // where pandoc treated `{...}` as literal text. Here we only care
            // about headings with a *real* AttributeNode — i.e. attrs that
            // parse, but where `pandoc --strip-comments` would leave stray
            // whitespace adjacent to the brace block.
            let has_attr = heading
                .syntax()
                .children()
                .any(|child| AttributeNode::cast(child).is_some());
            if !has_attr {
                continue;
            }

            let content = match heading.content() {
                Some(c) => c,
                None => continue,
            };

            for comment in content
                .syntax()
                .descendants()
                .filter_map(InlineHtml::cast)
                .filter(InlineHtml::is_comment)
            {
                let range = comment.syntax().text_range();
                let location = Location::from_range(range, input);

                diagnostics.push(
                    Diagnostic::warning(
                        location,
                        "heading-strip-comments-residue",
                        "Comment on a heading line adjacent to `{...}` attributes; `pandoc --strip-comments` will leave stray whitespace on the heading.",
                    )
                    .with_note(
                        DiagnosticNoteKind::Help,
                        "Move the comment to its own line before or after the heading.",
                    ),
                );
            }
        }

        diagnostics
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    fn lint(input: &str) -> Vec<Diagnostic> {
        let config = Config::default();
        let tree = crate::parser::parse(input, Some(config.clone()));
        HeadingStripCommentsResidueRule.check_tree(&tree, input, &config, None)
    }

    #[test]
    fn comment_before_attrs_warns() {
        let input = "# Title <!-- x --> {.unnumbered}\n";
        let diags = lint(input);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, "heading-strip-comments-residue");
        assert!(
            diags[0].message.contains("--strip-comments"),
            "got: {}",
            diags[0].message
        );
    }

    #[test]
    fn trailing_comment_does_not_fire_here() {
        // Attrs are eaten — covered by the sibling rule, not this one.
        let input = "# Title {.unnumbered} <!-- x -->\n";
        let diags = lint(input);
        assert_eq!(diags.len(), 0);
    }

    #[test]
    fn heading_without_comment_is_ignored() {
        let input = "# Title {.unnumbered}\n";
        let diags = lint(input);
        assert_eq!(diags.len(), 0);
    }

    #[test]
    fn heading_without_attrs_is_ignored() {
        let input = "# Title <!-- TODO -->\n";
        let diags = lint(input);
        assert_eq!(diags.len(), 0);
    }

    #[test]
    fn comment_elsewhere_is_ignored() {
        let input = "A paragraph with <!-- comment --> in it.\n\n# Title {.x}\n";
        let diags = lint(input);
        assert_eq!(diags.len(), 0);
    }
}
