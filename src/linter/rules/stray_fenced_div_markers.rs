use rowan::TextRange;

use crate::config::Config;
use crate::linter::diagnostics::{Diagnostic, DiagnosticNoteKind, Location};
use crate::linter::rules::Rule;
use crate::syntax::{SyntaxKind, SyntaxNode};

pub struct StrayFencedDivMarkersRule;

impl Rule for StrayFencedDivMarkersRule {
    fn name(&self) -> &str {
        "stray-fenced-div-markers"
    }

    fn check(
        &self,
        tree: &SyntaxNode,
        input: &str,
        _config: &Config,
        _metadata: Option<&crate::metadata::DocumentMetadata>,
    ) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();

        for node in tree.descendants() {
            if node.kind() != SyntaxKind::PARAGRAPH {
                continue;
            }
            let para_range = node.text_range();
            let para_start: u32 = para_range.start().into();
            let Some(slice) =
                input.get(usize::from(para_range.start())..usize::from(para_range.end()))
            else {
                continue;
            };

            let mut line_offset = 0usize;
            for line in slice.split_inclusive('\n') {
                let raw_line = line.trim_end_matches('\n').trim_end_matches('\r');
                if let Some(hit) = match_stray_fence(raw_line) {
                    let abs_start = para_start + (line_offset + hit.start) as u32;
                    let abs_end = para_start + (line_offset + hit.end) as u32;
                    let range = TextRange::new(abs_start.into(), abs_end.into());
                    let location = Location::from_range(range, input);
                    let marker = &raw_line[hit.start..hit.end];
                    let diag = Diagnostic::warning(
                        location,
                        "stray-fenced-div-markers",
                        format!("Stray fenced div marker '{marker}' has no matching opener"),
                    )
                    .with_note(
                        DiagnosticNoteKind::Help,
                        "if this is meant to close a div, check the opener's class/attributes; \
                         otherwise escape the colons or rewrite the line",
                    );
                    diagnostics.push(diag);
                }
                line_offset += line.len();
            }
        }

        diagnostics
    }
}

struct FenceHit {
    start: usize,
    end: usize,
}

/// A line is a stray fence if, after up to 3 leading spaces, it consists of
/// only `:` characters (length >= 3) followed by optional trailing whitespace.
///
/// Pandoc treats `:::` (or longer) with no attributes as a closing fence; if
/// it lands inside a paragraph as text, the parser found no matching opener
/// and almost always this indicates a typo (e.g. `:::` instead of `::::`).
fn match_stray_fence(line: &str) -> Option<FenceHit> {
    let bytes = line.as_bytes();
    let leading_spaces = bytes.iter().take(3).take_while(|&&b| b == b' ').count();
    let after_spaces = &bytes[leading_spaces..];
    let colon_count = after_spaces.iter().take_while(|&&b| b == b':').count();
    if colon_count < 3 {
        return None;
    }
    // Everything after the colon run must be trailing whitespace only.
    let tail = &after_spaces[colon_count..];
    if !tail.iter().all(|&b| b == b' ' || b == b'\t') {
        return None;
    }
    Some(FenceHit {
        start: leading_spaces,
        end: leading_spaces + colon_count,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    fn parse_and_lint(input: &str) -> Vec<Diagnostic> {
        let config = Config::default();
        let tree = crate::parser::parse(input, Some(config.clone()));
        StrayFencedDivMarkersRule.check(&tree, input, &config, None)
    }

    #[test]
    fn balanced_div_is_clean() {
        let input = "::: warning\nbody\n:::\n";
        assert!(parse_and_lint(input).is_empty());
    }

    #[test]
    fn flags_lone_triple_colon() {
        let input = "Hello.\n\n:::\n\nGoodbye.\n";
        let diagnostics = parse_and_lint(input);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, "stray-fenced-div-markers");
        assert_eq!(diagnostics[0].location.line, 3);
        assert!(diagnostics[0].message.contains(":::"));
        assert!(diagnostics[0].fix.is_none());
    }

    #[test]
    fn flags_longer_runs() {
        let input = "para\n\n::::::\n\nmore\n";
        let diagnostics = parse_and_lint(input);
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].message.contains("::::::"));
    }

    #[test]
    fn ignores_two_colons() {
        let input = "para\n\n::\n\nmore\n";
        assert!(parse_and_lint(input).is_empty());
    }

    #[test]
    fn ignores_mid_line_triple_colon() {
        let input = "Use ::: to start a div.\n";
        assert!(parse_and_lint(input).is_empty());
    }

    #[test]
    fn ignores_inline_code_span() {
        let input = "Type `:::` to open a div.\n";
        assert!(parse_and_lint(input).is_empty());
    }

    #[test]
    fn ignores_indented_code_block() {
        // 4+ spaces => indented code block, not a paragraph.
        let input = "para\n\n    :::\n\nmore\n";
        assert!(parse_and_lint(input).is_empty());
    }

    #[test]
    fn ignores_text_after_colons() {
        // `::: foo` parses as an opener; `::: foo bar` is rejected as opener
        // and falls through, but it's not a closing-fence shape so we don't
        // flag it (avoid false positives for arbitrary `:::`-prefixed text).
        let input = "para\n\n::: not a fence shape with words\n\nmore\n";
        assert!(parse_and_lint(input).is_empty());
    }

    #[test]
    fn allows_up_to_three_leading_spaces() {
        let input = "para\n\n   :::\n\nmore\n";
        let diagnostics = parse_and_lint(input);
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn flags_multiple_strays_in_one_document() {
        let input = "p1\n\n:::\n\np2\n\n::::\n\np3\n";
        let diagnostics = parse_and_lint(input);
        assert_eq!(diagnostics.len(), 2);
        assert_eq!(diagnostics[0].location.line, 3);
        assert_eq!(diagnostics[1].location.line, 7);
    }

    #[test]
    fn flags_trailing_whitespace_after_colons() {
        let input = "p\n\n:::   \n\nmore\n";
        let diagnostics = parse_and_lint(input);
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn flags_crlf_line_endings() {
        let input = "p\r\n\r\n:::\r\n\r\nmore\r\n";
        let diagnostics = parse_and_lint(input);
        assert_eq!(diagnostics.len(), 1);
    }
}
