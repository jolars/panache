use crate::linter::diagnostics::{Diagnostic, DiagnosticNoteKind, Location};
use crate::linter::rules::{LintContext, Rule};
use crate::syntax::{Heading, InlineHtml, SyntaxKind, SyntaxNode};
use rowan::NodeOrToken;
use rowan::ast::AstNode;

pub struct HeadingEatenAttrsRule;

impl Rule for HeadingEatenAttrsRule {
    fn name(&self) -> &str {
        "heading-eaten-attrs"
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
            let content = match heading.content() {
                Some(c) => c,
                None => continue,
            };

            // Scan the content with INLINE_HTML subtrees stripped out, so a
            // brace shape that lives *inside* an annotative comment (e.g.
            // `# Title <!-- {x} -->`) doesn't masquerade as eaten attrs.
            let mut content_text = String::new();
            collect_non_html_text(content.syntax(), &mut content_text);
            if !contains_brace_block(&content_text) {
                continue;
            }

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
                        "heading-eaten-attrs",
                        "Comment on a heading line with `{...}` attributes; pandoc treats the brace block as literal text when anything follows it on the line.",
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

fn collect_non_html_text(node: &SyntaxNode, out: &mut String) {
    for child in node.children_with_tokens() {
        match child {
            NodeOrToken::Token(token) => out.push_str(token.text()),
            NodeOrToken::Node(inner) => {
                if inner.kind() == SyntaxKind::INLINE_HTML {
                    continue;
                }
                collect_non_html_text(&inner, out);
            }
        }
    }
}

fn contains_brace_block(text: &str) -> bool {
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'{' {
            let mut j = i + 1;
            while j < bytes.len() && bytes[j] != b'{' && bytes[j] != b'}' {
                j += 1;
            }
            if j < bytes.len() && bytes[j] == b'}' && j > i + 1 {
                return true;
            }
            i = j.max(i + 1);
        } else {
            i += 1;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    fn lint(input: &str) -> Vec<Diagnostic> {
        let config = Config::default();
        let tree = crate::parser::parse(input, Some(config.clone()));
        HeadingEatenAttrsRule.check_tree(&tree, input, &config, None)
    }

    #[test]
    fn trailing_comment_eats_attrs() {
        let input = "# Title {.unnumbered} <!-- x -->\n";
        let diags = lint(input);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, "heading-eaten-attrs");
        assert!(
            diags[0].message.contains("literal"),
            "got: {}",
            diags[0].message
        );
    }

    #[test]
    fn comment_before_attrs_does_not_fire() {
        // Attrs parse fine — comment sits before the brace block. Case B
        // (--strip-comments residue) is handled by a separate rule.
        let input = "# Title <!-- x --> {.unnumbered}\n";
        let diags = lint(input);
        assert_eq!(diags.len(), 0);
    }

    #[test]
    fn multiple_comments_each_warn() {
        // Both comments live in HEADING_CONTENT (since braces became literal),
        // so each one trips the rule.
        let input = "# T <!-- a --> {.c} <!-- b -->\n";
        let diags = lint(input);
        assert_eq!(diags.len(), 2);
    }

    #[test]
    fn plain_comment_without_braces_is_ignored() {
        let input = "# Title <!-- TODO -->\n";
        let diags = lint(input);
        assert_eq!(diags.len(), 0);
    }

    #[test]
    fn brace_shape_inside_comment_is_not_eaten_attrs() {
        // Brace shape lives entirely inside the comment payload, so the
        // heading has no real attrs and no literal-brace footgun.
        let input = "# Title <!-- {x} -->\n";
        let diags = lint(input);
        assert_eq!(diags.len(), 0, "got: {:#?}", diags);
    }

    #[test]
    fn setext_with_eaten_attrs() {
        let input = "Title {.c} <!-- x -->\n=====\n";
        let diags = lint(input);
        assert_eq!(diags.len(), 1);
    }

    #[test]
    fn comment_in_paragraph_is_ignored() {
        let input = "A paragraph with <!-- {x} --> in it.\n\n# Title\n";
        let diags = lint(input);
        assert_eq!(diags.len(), 0);
    }
}
