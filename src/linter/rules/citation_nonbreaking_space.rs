use rowan::{NodeOrToken, TextRange, TextSize};

use crate::linter::diagnostics::{Diagnostic, Edit, Fix, Location};
use crate::linter::rules::{DiagnosticCode, LintContext, Requirement, Rule, RuleMeta};
use crate::syntax::{SyntaxKind, SyntaxNode};

pub struct CitationNonbreakingSpaceRule;

/// The breakable gap (space run or softbreak) separating a bracketed citation
/// from the text before it, or `None` when the citation is already tied,
/// starts its container, or follows an intentional hard break.
fn breakable_gap(citation: &SyntaxNode) -> Option<TextRange> {
    let end = citation.text_range().start();
    let mut start = end;
    let mut saw_breakable = false;
    let mut element = citation.prev_sibling_or_token();

    // Walk backward over the whitespace-like run (spaces, tabs, one softbreak,
    // existing non-breaking spaces) until real content. Absorbing existing
    // NBSPs into the replaced range keeps the fix from stacking ties.
    let content = loop {
        let Some(current) = element else {
            // Only whitespace precedes the citation in this container: there
            // is nothing to tie it to.
            return None;
        };
        let token = match current {
            NodeOrToken::Node(_) => break current,
            NodeOrToken::Token(ref token) => token.clone(),
        };
        match token.kind() {
            SyntaxKind::NEWLINE => {
                saw_breakable = true;
                start = token.text_range().start();
                element = token.prev_sibling_or_token();
            }
            SyntaxKind::NONBREAKING_SPACE => {
                start = token.text_range().start();
                element = token.prev_sibling_or_token();
            }
            SyntaxKind::TEXT => {
                let text = token.text();
                let trimmed = text.trim_end_matches([' ', '\t', '\u{a0}']);
                if text[trimmed.len()..].contains([' ', '\t']) {
                    saw_breakable = true;
                }
                start = token.text_range().start() + TextSize::of(trimmed);
                if trimmed.is_empty() {
                    element = token.prev_sibling_or_token();
                } else {
                    break current;
                }
            }
            _ => break current,
        }
    };

    if !saw_breakable || content.kind() == SyntaxKind::HARD_LINE_BREAK {
        return None;
    }
    Some(TextRange::new(start, end))
}

fn is_bracketed(citation: &SyntaxNode) -> bool {
    citation
        .children_with_tokens()
        .next()
        .is_some_and(|element| element.kind() == SyntaxKind::LINK_START)
}

impl Rule for CitationNonbreakingSpaceRule {
    fn name(&self) -> &str {
        "citation-nonbreaking-space"
    }

    fn metadata(&self) -> RuleMeta {
        RuleMeta {
            name: "citation-nonbreaking-space",
            default_on: true,
            requires: Requirement::Citations,
            auto_fix: true,
            codes: const { &[DiagnosticCode::warning("citation-nonbreaking-space")] },
        }
    }

    fn node_interests(&self) -> &'static [SyntaxKind] {
        &[SyntaxKind::CITATION]
    }

    fn check(&self, cx: &LintContext) -> Vec<Diagnostic> {
        let input = cx.input;
        // `\ ` only parses as a non-breaking space under Pandoc's
        // all_symbols_escapable; otherwise fall back to a literal U+00A0.
        let (replacement, fix_message) = if cx.config.extensions.all_symbols_escapable {
            ("\\ ", "Replace with a non-breaking space (`\\ `)")
        } else {
            ("\u{a0}", "Replace with a non-breaking space (U+00A0)")
        };

        let mut diagnostics = Vec::new();
        for node in cx.nodes(SyntaxKind::CITATION) {
            if !is_bracketed(node) {
                continue;
            }
            let Some(gap) = breakable_gap(node) else {
                continue;
            };
            let fix = Fix::safe(
                fix_message,
                vec![Edit {
                    range: gap,
                    replacement: replacement.to_string(),
                }],
            );
            diagnostics.push(
                Diagnostic::warning(
                    Location::from_range(gap, input),
                    "citation-nonbreaking-space",
                    "Breakable space before citation; the rendered citation can be \
                     stranded at the start of a line",
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
        lint_with_config(input, &config)
    }

    fn lint_with_config(input: &str, config: &Config) -> Vec<Diagnostic> {
        let tree = crate::parser::parse(input, Some(config.clone()));
        CitationNonbreakingSpaceRule.check_tree(&tree, input, config, None)
    }

    fn apply_fix(input: &str, diagnostic: &Diagnostic) -> String {
        let edit = &diagnostic.fix.as_ref().expect("fix").edits[0];
        let start: usize = edit.range.start().into();
        let end: usize = edit.range.end().into();
        format!("{}{}{}", &input[..start], edit.replacement, &input[end..])
    }

    #[test]
    fn flags_space_before_bracketed_citation() {
        let input = "Some fact [@smith2020] here.\n";
        let diagnostics = parse_and_lint(input);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, "citation-nonbreaking-space");
        let edit = &diagnostics[0].fix.as_ref().expect("fix").edits[0];
        assert_eq!(&input[edit.range], " ");
        assert_eq!(edit.replacement, "\\ ");
        assert_eq!(
            apply_fix(input, &diagnostics[0]),
            "Some fact\\ [@smith2020] here.\n"
        );
    }

    #[test]
    fn flags_softbreak_before_citation() {
        let input = "A line\n[@smith2020] follows.\n";
        let diagnostics = parse_and_lint(input);
        assert_eq!(diagnostics.len(), 1);
        let edit = &diagnostics[0].fix.as_ref().expect("fix").edits[0];
        assert_eq!(&input[edit.range], "\n");
        assert_eq!(
            apply_fix(input, &diagnostics[0]),
            "A line\\ [@smith2020] follows.\n"
        );
    }

    #[test]
    fn softbreak_absorbs_trailing_spaces() {
        let input = "A line \n[@smith2020] follows.\n";
        let diagnostics = parse_and_lint(input);
        assert_eq!(diagnostics.len(), 1);
        let edit = &diagnostics[0].fix.as_ref().expect("fix").edits[0];
        assert_eq!(&input[edit.range], " \n");
    }

    #[test]
    fn space_run_is_replaced_entirely() {
        let input = "Some fact  [@smith2020] here.\n";
        let diagnostics = parse_and_lint(input);
        assert_eq!(diagnostics.len(), 1);
        let edit = &diagnostics[0].fix.as_ref().expect("fix").edits[0];
        assert_eq!(&input[edit.range], "  ");
    }

    #[test]
    fn nonbreaking_escape_is_compliant() {
        let diagnostics = parse_and_lint("Tied\\ [@smith2020] ok.\n");
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn literal_nbsp_is_compliant() {
        let diagnostics = parse_and_lint("Tied\u{a0}[@smith2020] ok.\n");
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn in_text_citation_is_not_flagged() {
        let diagnostics = parse_and_lint("In-text @smith2020 says so.\n");
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn citation_at_paragraph_start_is_not_flagged() {
        let diagnostics = parse_and_lint("[@smith2020] starts the paragraph.\n");
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn adjacent_punctuation_is_not_flagged() {
        let diagnostics = parse_and_lint("A fact ([@smith2020]) here.\n");
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn hard_line_break_is_not_flagged() {
        let diagnostics = parse_and_lint("A fact\\\n[@smith2020] here.\n");
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn suppress_author_citation_is_flagged() {
        let diagnostics = parse_and_lint("A fact [-@smith2020] here.\n");
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn fix_falls_back_to_unicode_nbsp_without_escape_extension() {
        let mut config = Config::default();
        config.extensions.all_symbols_escapable = false;
        let input = "Some fact [@smith2020] here.\n";
        let diagnostics = lint_with_config(input, &config);
        assert_eq!(diagnostics.len(), 1);
        let edit = &diagnostics[0].fix.as_ref().expect("fix").edits[0];
        assert_eq!(edit.replacement, "\u{a0}");
    }

    #[test]
    fn badge_link_with_at_in_url_is_not_flagged() {
        // An @ inside a nested image destination must not make the bracket a
        // citation (pandoc parses this as Link [ Image ... ]).
        let input = "[![PyPI](https://badge.fury.io/py/arity.svg)](https://pypi.org/project/arity/)\n\
                     [![npm](https://badge.fury.io/js/@arity-cli%2Farity-cli.svg)](https://www.npmjs.com/package/arity-cli)\n";
        let diagnostics = parse_and_lint(input);
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn fix_is_idempotent() {
        let input = "Some fact [@smith2020] here.\n";
        let diagnostics = parse_and_lint(input);
        let fixed = apply_fix(input, &diagnostics[0]);
        assert!(parse_and_lint(&fixed).is_empty());
    }
}
