//! `math-syntax` — surface math-content diagnostics.
//!
//! The math parser embeds a lossless structural `MATH_CONTENT` CST into the host
//! tree; unbalanced braces and unclosed/mismatched environments leave an
//! unambiguous signature in that tree. This rule is a thin adapter over
//! [`math_diagnostics`], the shared CST walk that is the single source of truth
//! for math diagnostics (also consumed by the formatter and LSP). The embedded
//! tokens already carry host-document ranges (they were spliced into the host
//! tree), so a diagnostic's span is just the offending token's `text_range()`;
//! there is no offset remapping and the blockquote-prefix wrinkle never arises.
//!
//! The rule's only job is to map each neutral [`MathDiagnosticKind`] to a linter
//! code and message:
//!
//! | code | kind |
//! |------|------|
//! | `math-unclosed-group` | [`MathDiagnosticKind::UnclosedGroup`] |
//! | `math-unexpected-close-brace` | [`MathDiagnosticKind::UnexpectedCloseBrace`] |
//! | `math-unclosed-environment` | [`MathDiagnosticKind::UnclosedEnvironment`] |
//! | `math-mismatched-environment` | [`MathDiagnosticKind::MismatchedEnvironment`] |
//! | `math-unexpected-end` | [`MathDiagnosticKind::UnexpectedEnd`] |
//! | `math-unclosed-delimiter` | [`MathDiagnosticKind::UnclosedDelimiter`] |
//! | `math-unexpected-right` | [`MathDiagnosticKind::UnexpectedRight`] |
//! | `math-unexpected-dollar` | [`MathDiagnosticKind::UnexpectedDollar`] |

use crate::linter::diagnostics::{Diagnostic, Location};
use crate::linter::rules::{DiagnosticCode, LintContext, Requirement, Rule, RuleMeta};
use crate::syntax::{MathDiagnosticKind, SyntaxKind, math_diagnostics};

pub struct MathContentRule;

impl Rule for MathContentRule {
    fn description(&self) -> &'static str {
        include_str!("docs/math-syntax.md")
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn examples(&self) -> &'static [crate::linter::docs::Example] {
        use crate::linter::docs::Example;
        const {
            &[
                Example::new("$\\frac{1}{2$\n", &["math-unclosed-group"])
                    .caption("A group is missing its closing brace:"),
                Example::new("$a}b$\n", &["math-unexpected-close-brace"])
                    .caption("A closing brace has no matching opener:"),
                Example::new(
                    "$$\n\\begin{aligned} x &= 1\n$$\n",
                    &["math-unclosed-environment"],
                )
                .caption("An environment is missing its end:"),
                Example::new(
                    "$$\\begin{aligned}x\\end{matrix}$$\n",
                    &["math-mismatched-environment"],
                )
                .caption("The environment names do not match:"),
                Example::new("$x \\end{aligned}$\n", &["math-unexpected-end"])
                    .caption("An environment ends without being opened:"),
                Example::new("$\\left(x$\n", &["math-unclosed-delimiter"])
                    .caption("A scalable delimiter is missing its partner:"),
                Example::new("$x\\right)$\n", &["math-unexpected-right"])
                    .caption("A closing scalable delimiter has no opener:"),
                Example::new("$$\n$ a\n$$\n", &["math-unexpected-dollar"])
                    .caption("A dollar sign appears inside display math:"),
            ]
        }
    }

    fn code_description(&self, code: &str) -> Option<&'static str> {
        Some(match code {
            "math-unclosed-group" => "An opening `{` has no matching `}`.",
            "math-unexpected-close-brace" => "A closing `}` has no matching `{`.",
            "math-unclosed-environment" => "A `\\begin{...}` has no matching `\\end{...}`.",
            "math-mismatched-environment" => {
                "The name in `\\end{...}` differs from the open environment."
            }
            "math-unexpected-end" => "A `\\end{...}` has no matching `\\begin{...}`.",
            "math-unclosed-delimiter" => "A `\\left` has no matching `\\right`.",
            "math-unexpected-right" => "A `\\right` has no matching `\\left`.",
            "math-unexpected-dollar" => {
                "An unescaped `$` appears inside TeX math. Use `\\$` for a literal dollar sign."
            }
            _ => return None,
        })
    }

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
                    DiagnosticCode::error("math-unclosed-delimiter"),
                    DiagnosticCode::error("math-unexpected-right"),
                    DiagnosticCode::error("math-unexpected-dollar"),
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
        // Every malformed-math token lives under a `MATH_CONTENT` node; the
        // shared walk derives the diagnostics (with host-aligned ranges) from
        // each subtree's shape.
        for content in cx.nodes(SyntaxKind::MATH_CONTENT) {
            for d in math_diagnostics(content) {
                let (code, message) = describe(d.kind);
                out.push(Diagnostic::error(
                    Location::from_range(d.range, input),
                    code,
                    message,
                ));
            }
        }
        out
    }
}

/// Map a neutral [`MathDiagnosticKind`] to this rule's `(code, message)`.
///
/// Malformed math is build-breaking — `quarto render` to PDF hard-fails on an
/// unclosed brace or mismatched environment, and MathJax/KaTeX silently drop the
/// equation. So these ride at `Error` severity. The rule stays suppressible via
/// `[lint] ignore = ["math-syntax"]` (or an ignore directive) for the rare
/// macro-expanded TeX that only *looks* unbalanced to a structural parser.
fn describe(kind: MathDiagnosticKind) -> (&'static str, &'static str) {
    match kind {
        MathDiagnosticKind::UnclosedGroup => ("math-unclosed-group", "unclosed `{` group"),
        MathDiagnosticKind::UnexpectedCloseBrace => {
            ("math-unexpected-close-brace", "unmatched closing brace `}`")
        }
        MathDiagnosticKind::UnclosedEnvironment => (
            "math-unclosed-environment",
            r"`\begin` without a matching `\end`",
        ),
        MathDiagnosticKind::MismatchedEnvironment => (
            "math-mismatched-environment",
            r"`\end` name does not match the open `\begin`",
        ),
        MathDiagnosticKind::UnexpectedEnd => {
            ("math-unexpected-end", r"`\end` without a matching `\begin`")
        }
        MathDiagnosticKind::UnclosedDelimiter => (
            "math-unclosed-delimiter",
            r"`\left` without a matching `\right`",
        ),
        MathDiagnosticKind::UnexpectedRight => (
            "math-unexpected-right",
            r"`\right` without a matching `\left`",
        ),
        MathDiagnosticKind::UnexpectedDollar => (
            "math-unexpected-dollar",
            "unescaped `$` inside TeX math ends math mode",
        ),
    }
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
    fn flags_unescaped_dollar() {
        let input = "$$\n$ a\n$$\n";
        let diags = parse_and_lint(input);
        assert_eq!(codes(&diags), vec!["math-unexpected-dollar"]);
        let start: usize = diags[0].location.range.start().into();
        assert_eq!(&input[start..start + 1], "$");
    }

    #[test]
    fn invalid_inline_closer_does_not_trigger_math_diagnostic() {
        let diags = parse_and_lint("- We have $a = b $ and $l$.\n");
        assert!(diags.is_empty());
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
