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
            if !matches!(node.kind(), SyntaxKind::PARAGRAPH | SyntaxKind::PLAIN) {
                continue;
            }
            for elem in node.descendants_with_tokens() {
                let Some(token) = elem.into_token() else {
                    continue;
                };
                if token.kind() != SyntaxKind::TEXT {
                    continue;
                }
                let text = token.text();
                let token_start: u32 = token.text_range().start().into();
                let para_start: usize = node.text_range().start().into();
                for (run_start, run_end) in find_colon_runs(text) {
                    let abs_start = token_start + run_start as u32;
                    let abs_end = token_start + run_end as u32;
                    let range = TextRange::new(abs_start.into(), abs_end.into());
                    let location = Location::from_range(range, input);
                    let marker = &text[run_start..run_end];

                    let diag = if is_swept_fence_shape(input, para_start, abs_start as usize) {
                        Diagnostic::warning(
                            location,
                            "stray-fenced-div-markers",
                            format!(
                                "'{marker}' looks like a fenced div marker, but the preceding \
                                 line pulls it into a paragraph"
                            ),
                        )
                        .with_note(
                            DiagnosticNoteKind::Help,
                            "Insert a blank line above this line so Pandoc parses it as a \
                             fenced div instead of paragraph text",
                        )
                    } else {
                        Diagnostic::warning(
                            location,
                            "stray-fenced-div-markers",
                            format!("'{marker}' appears as text, not as a fenced div marker"),
                        )
                        .with_note(
                            DiagnosticNoteKind::Help,
                            "Pandoc only treats ':::' as a fenced div marker when it starts a \
                             line on its own (optionally followed by a class or attributes). \
                             Add a newline before it, or wrap it in backticks if it's \
                             intentional text",
                        )
                    };
                    diagnostics.push(diag);
                }
            }
        }

        diagnostics
    }
}

/// True when a `:::` run sits at the start of its source line (after up to
/// three leading spaces), the rest of the line forms a valid fence opener or
/// closer shape, and the enclosing paragraph already has non-whitespace
/// content before the line. Together those mean the user almost certainly
/// intended a fenced div block but a missing blank line above is pulling it
/// into the preceding paragraph (cf. issue #340).
fn is_swept_fence_shape(input: &str, para_start: usize, run_start: usize) -> bool {
    let bytes = input.as_bytes();
    if run_start > bytes.len() {
        return false;
    }

    let line_start = input[..run_start].rfind('\n').map_or(0, |i| i + 1);
    let line_end = input[run_start..]
        .find('\n')
        .map_or(bytes.len(), |i| run_start + i);
    let line = &input[line_start..line_end];

    let leading_spaces = line.bytes().take_while(|b| *b == b' ').count();
    if leading_spaces > 3 || line_start + leading_spaces != run_start {
        return false;
    }

    if !input[para_start..line_start]
        .chars()
        .any(|c| !c.is_whitespace())
    {
        return false;
    }

    looks_like_div_fence(&line[leading_spaces..])
}

/// Minimal version of the parser's fence-shape recognizer: accepts both
/// opener (`::: {.cls}` / `::: classname` / `::::: {#id} :::::`) and closer
/// (`:::` / `::::`) shapes. Inlined here to avoid widening
/// `panache-parser`'s public API for a lint-only check.
fn looks_like_div_fence(content: &str) -> bool {
    let colon_count = content.bytes().take_while(|b| *b == b':').count();
    if colon_count < 3 {
        return false;
    }
    let after = content[colon_count..].trim();

    if after.is_empty() {
        return true;
    }

    if let Some(rest) = after.strip_prefix('{') {
        return rest.contains('}');
    }

    let word_end = after
        .find(|c: char| c.is_whitespace() || c == ':')
        .unwrap_or(after.len());
    let (first, rest) = after.split_at(word_end);
    if first.is_empty() {
        return false;
    }
    let trailing = rest.trim();
    if trailing.is_empty() {
        return true;
    }
    trailing.chars().all(|c| c == ':') && trailing.len() >= 3
}

/// Non-overlapping byte offsets of every run of three or more `:` characters
/// in `text`.
fn find_colon_runs(text: &str) -> Vec<(usize, usize)> {
    let bytes = text.as_bytes();
    let mut runs = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b':' {
            let start = i;
            while i < bytes.len() && bytes[i] == b':' {
                i += 1;
            }
            if i - start >= 3 {
                runs.push((start, i));
            }
        } else {
            i += 1;
        }
    }
    runs
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
    fn flags_mid_line_triple_colon() {
        let input = "Use ::: to start a div.\n";
        let diagnostics = parse_and_lint(input);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].location.line, 1);
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
    fn flags_colons_glued_to_inline_span() {
        // Issue #333: ':::' glued to the end of a span on the same line — the
        // user almost certainly meant a closing fence on its own line.
        let input = "::: {lang=en-US}\n[contact Ms. N]{lang=en-US}:::\n";
        let diagnostics = parse_and_lint(input);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].location.line, 2);
        assert!(diagnostics[0].message.contains(":::"));
    }

    #[test]
    fn flags_text_after_colons() {
        // `::: foo bar` is rejected as an opener and falls through to text;
        // it's still a strong "did the user mean a fence?" signal.
        let input = "para\n\n::: not a fence shape with words\n\nmore\n";
        let diagnostics = parse_and_lint(input);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].location.line, 3);
    }

    #[test]
    fn flags_up_to_three_leading_spaces() {
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
    fn flags_multiple_runs_on_one_line() {
        let input = "start ::: middle ::::: end\n";
        let diagnostics = parse_and_lint(input);
        assert_eq!(diagnostics.len(), 2);
        assert!(diagnostics[0].message.contains(":::"));
        assert!(diagnostics[1].message.contains(":::::"));
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

    #[test]
    fn upgrades_message_for_opener_swept_into_paragraph() {
        // Issue #340: `[]{#hmm}` immediately followed by `::: {lang=zh-TW}`
        // with no blank line. Both ::: lines fall into the same paragraph;
        // the diagnostic should call this out specifically rather than the
        // generic "appears as text" message.
        let input = "[]{#hmm}\n::: {lang=zh-TW}\nbla\n:::\n";
        let diagnostics = parse_and_lint(input);
        assert_eq!(diagnostics.len(), 2);

        assert_eq!(diagnostics[0].location.line, 2);
        assert!(diagnostics[0].message.contains("looks like a fenced div"));
        assert!(diagnostics[0].message.contains("preceding line pulls it"));
        assert!(
            diagnostics[0]
                .notes
                .iter()
                .any(|n| n.message.contains("Insert a blank line"))
        );

        assert_eq!(diagnostics[1].location.line, 4);
        assert!(diagnostics[1].message.contains("looks like a fenced div"));
    }

    #[test]
    fn upgrades_message_for_plain_text_then_fence_shape() {
        let input = "hello world\n::: warning\n";
        let diagnostics = parse_and_lint(input);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].location.line, 2);
        assert!(diagnostics[0].message.contains("looks like a fenced div"));
    }

    #[test]
    fn lone_triple_colon_paragraph_keeps_generic_message() {
        // No preceding paragraph content means this isn't the swept-in shape;
        // the generic "appears as text" message is still appropriate.
        let input = "Hello.\n\n:::\n\nGoodbye.\n";
        let diagnostics = parse_and_lint(input);
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].message.contains("appears as text"));
    }

    #[test]
    fn mid_line_colons_keep_generic_message() {
        // `:::` mid-line isn't a fence-shape line, so don't upgrade.
        let input = "Use ::: to start a div.\n";
        let diagnostics = parse_and_lint(input);
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].message.contains("appears as text"));
    }

    #[test]
    fn flags_colons_trailing_a_tight_list_item() {
        // Issue #333 follow-up: in a tight list item the inline content lives
        // under PLAIN, not PARAGRAPH. The rule must still catch ':::' there.
        let input = "- a list item :::\n";
        let diagnostics = parse_and_lint(input);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, "stray-fenced-div-markers");
        assert_eq!(diagnostics[0].location.line, 1);
        assert!(diagnostics[0].message.contains(":::"));
    }

    #[test]
    fn non_fence_shape_after_colons_keeps_generic_message() {
        // Multi-word junk after `:::` isn't a valid fence opener; even though
        // the run starts the line and has paragraph content above, the line
        // isn't a fence shape, so don't upgrade.
        let input = "para\n::: not a fence shape with words\n";
        let diagnostics = parse_and_lint(input);
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].message.contains("appears as text"));
    }
}
