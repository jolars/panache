use rowan::TextRange;

use crate::linter::diagnostics::{Diagnostic, DiagnosticNoteKind, Location};
use crate::linter::rules::{DiagnosticCode, LintContext, Requirement, Rule, RuleMeta};
use crate::syntax::SyntaxKind;

pub struct FootnoteRefInFootnoteDefRule;

impl Rule for FootnoteRefInFootnoteDefRule {
    fn name(&self) -> &str {
        "footnote-ref-in-footnote-def"
    }

    fn metadata(&self) -> RuleMeta {
        RuleMeta {
            name: "footnote-ref-in-footnote-def",
            default_on: true,
            requires: Requirement::Footnotes,
            auto_fix: false,
            codes: const { &[DiagnosticCode::warning("footnote-ref-in-footnote-def")] },
        }
    }

    fn node_interests(&self) -> &'static [SyntaxKind] {
        &[SyntaxKind::FOOTNOTE_DEFINITION]
    }

    fn check(&self, cx: &LintContext) -> Vec<Diagnostic> {
        let input = cx.input;
        let mut diagnostics = Vec::new();

        for def in cx.nodes(SyntaxKind::FOOTNOTE_DEFINITION) {
            // Walk every TEXT token under this definition and scan for
            // `[^id]` shapes. Iterating TEXT tokens (not the raw byte range)
            // naturally excludes inline code spans, math, raw HTML, etc. —
            // those are sibling CST nodes whose contents are not TEXT
            // tokens, so a `[^x]` inside `` `code` `` won't fire.
            for token in def
                .descendants_with_tokens()
                .filter_map(|t| t.into_token())
                .filter(|t| t.kind() == SyntaxKind::TEXT)
            {
                let text = token.text();
                let token_start: u32 = token.text_range().start().into();

                for hit in scan_footnote_ref_shapes(text) {
                    let abs_start = token_start + hit.start as u32;
                    let abs_end = token_start + hit.end as u32;
                    let range = TextRange::new(abs_start.into(), abs_end.into());
                    let location = Location::from_range(range, input);
                    let marker = &text[hit.start..hit.end];
                    let diag = Diagnostic::warning(
                        location,
                        "footnote-ref-in-footnote-def",
                        format!(
                            "Footnote reference '{marker}' inside a footnote definition body \
                             is silently dropped by pandoc (rendered as literal text)"
                        ),
                    )
                    .with_note(
                        DiagnosticNoteKind::Help,
                        "footnotes do not nest in pandoc; inline the prose, restructure to \
                         keep the reference outside the definition body, or remove it",
                    );
                    diagnostics.push(diag);
                }
            }
        }

        diagnostics
    }
}

struct Hit {
    start: usize,
    end: usize,
}

/// Scan `text` for `[^id]` byte patterns that pandoc would tokenise as a
/// footnote reference and return their byte ranges.
///
/// Tighter than `try_parse_footnote_reference` in the parser: pandoc
/// terminates the id at any whitespace (so `[^ foo]` becomes literal text,
/// not a ref). The lint reports only would-be refs that pandoc actually
/// would have silently dropped — over-flagging genuinely malformed bracket
/// shapes (`[^]`, `[^ and …]`) would be a false positive.
fn scan_footnote_ref_shapes(text: &str) -> Vec<Hit> {
    let bytes = text.as_bytes();
    let mut hits = Vec::new();
    let mut i = 0;
    while i + 3 < bytes.len() {
        if bytes[i] == b'[' && bytes[i + 1] == b'^' {
            let id_start = i + 2;
            let mut j = id_start;
            while j < bytes.len()
                && bytes[j] != b']'
                && bytes[j] != b'\n'
                && bytes[j] != b'\r'
                && !bytes[j].is_ascii_whitespace()
            {
                j += 1;
            }
            if j < bytes.len() && bytes[j] == b']' && j > id_start {
                hits.push(Hit {
                    start: i,
                    end: j + 1,
                });
                i = j + 1;
                continue;
            }
        }
        i += 1;
    }
    hits
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, Flavor};

    fn parse_and_lint(input: &str) -> Vec<Diagnostic> {
        let config = Config {
            flavor: Flavor::Pandoc,
            ..Config::default()
        };
        let tree = crate::parser::parse(input, Some(config.clone()));
        FootnoteRefInFootnoteDefRule.check_tree(&tree, input, &config, None)
    }

    #[test]
    fn flags_ref_in_def_body() {
        let input = "Outer[^a].\n\n[^a]: Body with [^b] ref.\n\n[^b]: B.\n";
        let diags = parse_and_lint(input);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, "footnote-ref-in-footnote-def");
        assert!(diags[0].message.contains("[^b]"));
        assert!(diags[0].fix.is_none());
        // Help note explains the silent-drop semantics.
        assert!(diags[0].notes.iter().any(|n| n.message.contains("nest")));
    }

    #[test]
    fn flags_ref_on_lazy_continuation_line_of_def_body() {
        // Pandoc treats `Another[^1].` (no blank line before it) as a lazy
        // continuation of the `[^1]:` def body, so the inner `[^1]` is text.
        let input = "Outer[^1].\n\n[^1]: Body line.\nAnother[^1] in continuation.\n";
        let diags = parse_and_lint(input);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("[^1]"));
    }

    #[test]
    fn does_not_flag_ref_in_code_span_in_def_body() {
        // Backtick code span content is a CODE_SPAN node, not a TEXT token —
        // the rule's TEXT-token walk naturally skips it.
        let input = "Outer[^a].\n\n[^a]: Body has `[^b]` in code.\n\n[^b]: B.\n";
        let diags = parse_and_lint(input);
        assert_eq!(diags.len(), 0);
    }

    #[test]
    fn does_not_flag_ref_outside_def_body() {
        let input = "Outer[^a] and [^b] in para.\n\n[^a]: A.\n\n[^b]: B.\n";
        let diags = parse_and_lint(input);
        assert_eq!(diags.len(), 0);
    }

    #[test]
    fn does_not_flag_ref_inside_inline_footnote_inside_def_body() {
        // Under the parser fix, `[^b]` inside `^[...]` inside `[^a]:` body is
        // ALSO text (pandoc cascades the suppression). The rule should flag it
        // — pandoc still drops it, so the user warning is correct.
        let input = "Outer[^a].\n\n[^a]: Body has ^[inline with [^b] ref] tail.\n\n[^b]: B.\n";
        let diags = parse_and_lint(input);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("[^b]"));
    }

    #[test]
    fn does_not_flag_empty_or_malformed_brackets() {
        let input = "Outer[^a].\n\n[^a]: Body has [^] and [^ and [no caret].\n";
        let diags = parse_and_lint(input);
        assert_eq!(diags.len(), 0);
    }

    #[test]
    fn flags_multiple_refs_in_one_def_body() {
        let input = "Outer[^a].\n\n[^a]: Body has [^b] and [^c] and [^d].\n\n[^b]: B.\n\n[^c]: C.\n\n[^d]: D.\n";
        let diags = parse_and_lint(input);
        assert_eq!(diags.len(), 3);
    }

    #[test]
    fn flags_ref_in_nested_blockquote_inside_def_body() {
        let input = "Outer[^a].\n\n[^a]: Body.\n\n    > Nested has [^b] ref.\n\n[^b]: B.\n";
        let diags = parse_and_lint(input);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("[^b]"));
    }

    #[test]
    fn flags_ref_inside_strong_inside_def_body() {
        let input = "Outer[^a].\n\n[^a]: Body has **bold [^b] inside** tail.\n\n[^b]: B.\n";
        let diags = parse_and_lint(input);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("[^b]"));
    }
}
