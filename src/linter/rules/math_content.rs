//! `math-syntax` — surface math-content diagnostics.
//!
//! The math parser embeds a lossless structural `MATH_CONTENT` CST into the host
//! tree; unbalanced braces and unclosed/mismatched environments leave an
//! unambiguous signature in that tree. This rule reads those signatures
//! directly — it does **not** re-parse the content. The embedded tokens already
//! carry host-document ranges (they were spliced into the host tree), so a
//! diagnostic's span is just the offending token's `text_range()`; there is no
//! offset remapping and the blockquote-prefix wrinkle never arises.
//!
//! The five conditions mirror the parser's own `parse_math_report` checks
//! (`crates/panache-parser/src/parser/math.rs`), read off the resulting shape:
//!
//! | code | CST signature |
//! |------|---------------|
//! | `math-unclosed-group` | a `MATH_GROUP` with no `MATH_GROUP_CLOSE` child |
//! | `math-unexpected-close-brace` | a `MATH_GROUP_CLOSE` whose parent isn't a `MATH_GROUP` |
//! | `math-unclosed-environment` | a `MATH_ENVIRONMENT` with no `\end` command |
//! | `math-mismatched-environment` | a `MATH_ENVIRONMENT` whose begin/end names differ |
//! | `math-unexpected-end` | a `\end` `MATH_COMMAND` whose parent isn't a `MATH_ENVIRONMENT` |

use rowan::TextRange;

use crate::linter::diagnostics::{Diagnostic, Location};
use crate::linter::rules::{DiagnosticCode, LintContext, Requirement, Rule, RuleMeta};
use crate::syntax::{SyntaxElement, SyntaxKind, SyntaxNode};

pub struct MathContentRule;

impl Rule for MathContentRule {
    fn name(&self) -> &str {
        "math-syntax"
    }

    fn metadata(&self) -> RuleMeta {
        RuleMeta {
            name: "math-syntax",
            default_on: true,
            requires: Requirement::TexMath,
            auto_fix: false,
            codes: const {
                &[
                    DiagnosticCode::error("math-unclosed-group"),
                    DiagnosticCode::error("math-unexpected-close-brace"),
                    DiagnosticCode::error("math-unclosed-environment"),
                    DiagnosticCode::error("math-mismatched-environment"),
                    DiagnosticCode::error("math-unexpected-end"),
                ]
            },
        }
    }

    fn node_interests(&self) -> &'static [SyntaxKind] {
        &[SyntaxKind::MATH_CONTENT]
    }

    fn check(&self, cx: &LintContext) -> Vec<Diagnostic> {
        let input = cx.input;
        let mut out = Vec::new();
        // Bound the walk to math subtrees: every malformed-math token lives under
        // a `MATH_CONTENT` node.
        for content in cx.nodes(SyntaxKind::MATH_CONTENT) {
            for node in content.descendants() {
                match node.kind() {
                    SyntaxKind::MATH_GROUP => check_unclosed_group(&node, input, &mut out),
                    SyntaxKind::MATH_ENVIRONMENT => check_environment(&node, input, &mut out),
                    _ => {}
                }
                // Stray tokens are flagged by their parent context: each token is
                // a direct child of exactly one node, so iterating every node's
                // direct children visits every token once.
                for child in node.children_with_tokens() {
                    let Some(token) = child.as_token() else {
                        continue;
                    };
                    match token.kind() {
                        SyntaxKind::MATH_GROUP_CLOSE if node.kind() != SyntaxKind::MATH_GROUP => {
                            out.push(err(
                                token.text_range(),
                                "math-unexpected-close-brace",
                                "unmatched closing brace `}`",
                                input,
                            ));
                        }
                        SyntaxKind::MATH_COMMAND
                            if node.kind() != SyntaxKind::MATH_ENVIRONMENT
                                && token.text() == r"\end" =>
                        {
                            out.push(err(
                                token.text_range(),
                                "math-unexpected-end",
                                r"`\end` without a matching `\begin`",
                                input,
                            ));
                        }
                        _ => {}
                    }
                }
            }
        }
        out
    }
}

/// A `MATH_GROUP` is well-formed only if it carries a closing `}`; the parser
/// emits the `MATH_GROUP_CLOSE` token solely when the brace is matched.
fn check_unclosed_group(group: &SyntaxNode, input: &str, out: &mut Vec<Diagnostic>) {
    let has_close = group
        .children_with_tokens()
        .any(|c| c.kind() == SyntaxKind::MATH_GROUP_CLOSE);
    if has_close {
        return;
    }
    if let Some(open) = group
        .children_with_tokens()
        .find(|c| c.kind() == SyntaxKind::MATH_GROUP_OPEN)
    {
        out.push(err(
            open.text_range(),
            "math-unclosed-group",
            "unclosed `{` group",
            input,
        ));
    }
}

fn check_environment(env: &SyntaxNode, input: &str, out: &mut Vec<Diagnostic>) {
    let children: Vec<SyntaxElement> = env.children_with_tokens().collect();
    let is_cmd = |el: &SyntaxElement, text: &str| {
        el.as_token()
            .is_some_and(|t| t.kind() == SyntaxKind::MATH_COMMAND && t.text() == text)
    };
    let begin_idx = children.iter().position(|c| is_cmd(c, r"\begin"));

    let Some(end_idx) = children.iter().position(|c| is_cmd(c, r"\end")) else {
        // No closing `\end` command: point at the opening `\begin`.
        let range = begin_idx
            .map(|i| children[i].text_range())
            .unwrap_or_else(|| env.text_range());
        out.push(err(
            range,
            "math-unclosed-environment",
            r"`\begin` without a matching `\end`",
            input,
        ));
        return;
    };

    let begin_name = begin_idx
        .and_then(|bi| group_name_after(&children, bi))
        .unwrap_or_default();
    let end_name = group_name_after(&children, end_idx).unwrap_or_default();
    if begin_name != end_name {
        // Point at the offending `\end` name (or the `\end` token if unnamed).
        let range =
            group_range_after(&children, end_idx).unwrap_or_else(|| children[end_idx].text_range());
        out.push(err(
            range,
            "math-mismatched-environment",
            r"`\end` name does not match the open `\begin`",
            input,
        ));
    }
}

/// Inner text of the first `MATH_GROUP` after `idx` (the environment name
/// group), with its braces stripped — mirrors `parse_environment_name`.
fn group_name_after(children: &[SyntaxElement], idx: usize) -> Option<String> {
    children[idx + 1..].iter().find_map(|c| {
        c.as_node()
            .filter(|n| n.kind() == SyntaxKind::MATH_GROUP)
            .map(|g| {
                g.text()
                    .to_string()
                    .trim_start_matches('{')
                    .trim_end_matches('}')
                    .to_string()
            })
    })
}

fn group_range_after(children: &[SyntaxElement], idx: usize) -> Option<TextRange> {
    children[idx + 1..].iter().find_map(|c| {
        c.as_node()
            .filter(|n| n.kind() == SyntaxKind::MATH_GROUP)
            .map(|g| g.text_range())
    })
}

/// Malformed math is build-breaking — `quarto render` to PDF hard-fails on an
/// unclosed brace or mismatched environment, and MathJax/KaTeX silently drop the
/// equation. So these ride at `Error` severity. The rule stays suppressible via
/// `[lint.rules] math-syntax = false` (or an ignore directive) for the rare
/// macro-expanded TeX that only *looks* unbalanced to a structural parser.
fn err(range: TextRange, code: &'static str, message: &'static str, input: &str) -> Diagnostic {
    Diagnostic::error(Location::from_range(range, input), code, message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    /// Lint with the `tex_math_dollars` extension on — the default `Config` is
    /// plain Markdown and would emit no math nodes at all.
    fn parse_and_lint(input: &str) -> Vec<Diagnostic> {
        let mut config = Config::default();
        config.extensions.tex_math_dollars = true;
        let tree = crate::parser::parse(input, Some(config.clone()));
        MathContentRule.check_tree(&tree, input, &config, None)
    }

    fn codes(diags: &[Diagnostic]) -> Vec<&str> {
        diags.iter().map(|d| d.code.as_str()).collect()
    }

    #[test]
    fn flags_unclosed_group() {
        let input = "$\\frac{1}{2$\n";
        let diags = parse_and_lint(input);
        assert_eq!(codes(&diags), vec!["math-unclosed-group"]);
        // Span points at the unclosed `{`.
        let start: usize = diags[0].location.range.start().into();
        assert_eq!(&input[start..start + 1], "{");
    }

    #[test]
    fn flags_stray_close_brace() {
        let diags = parse_and_lint("$a}b$\n");
        assert_eq!(codes(&diags), vec!["math-unexpected-close-brace"]);
    }

    #[test]
    fn flags_unclosed_environment() {
        let diags = parse_and_lint("$$\n\\begin{aligned} x &= 1\n$$\n");
        assert_eq!(codes(&diags), vec!["math-unclosed-environment"]);
    }

    #[test]
    fn flags_mismatched_environment() {
        let diags = parse_and_lint("$$\\begin{aligned}x\\end{matrix}$$\n");
        assert_eq!(codes(&diags), vec!["math-mismatched-environment"]);
    }

    #[test]
    fn flags_stray_end() {
        let diags = parse_and_lint("$x \\end{aligned}$\n");
        assert_eq!(codes(&diags), vec!["math-unexpected-end"]);
    }

    #[test]
    fn well_formed_math_is_clean() {
        let diags = parse_and_lint("$\\frac{1}{2} + x^{2}$\n");
        assert!(diags.is_empty());
    }

    #[test]
    fn nested_environments_are_clean() {
        let diags = parse_and_lint("$$\\begin{a}\\begin{b}x\\end{b}\\end{a}$$\n");
        assert!(diags.is_empty(), "well-formed nesting: {:?}", codes(&diags));
    }

    #[test]
    fn non_math_document_is_clean() {
        // No extension, no math nodes — and the rule no-ops regardless.
        let config = Config::default();
        let input = "# Heading\n\nText with `\\end{x}` in a code span.\n";
        let tree = crate::parser::parse(input, Some(config.clone()));
        let diags = MathContentRule.check_tree(&tree, input, &config, None);
        assert!(diags.is_empty());
    }

    #[test]
    fn blockquote_display_math_maps_host_range() {
        // The bad `{` is on a `> `-prefixed continuation line; reading the token
        // range directly gives the true host byte with no prefix bookkeeping.
        let input = "> $$\n> \\frac{1\n> $$\n";
        let diags = parse_and_lint(input);
        assert_eq!(codes(&diags), vec!["math-unclosed-group"]);
        let start: usize = diags[0].location.range.start().into();
        assert_eq!(&input[start..start + 1], "{");
    }
}
