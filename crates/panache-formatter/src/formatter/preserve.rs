//! Line emission for `wrap = preserve`.
//!
//! Preserve mode emits source tokens to keep authored line breaks intact,
//! while normalizing emphasis markers and hard breaks and removing trailing
//! prose whitespace. Verbatim content retains its original bytes.

use crate::syntax::{LinkDestUrl, SyntaxKind, SyntaxNode, text_without_line_prefixes};
use rowan::NodeOrToken;
use rowan::ast::AstNode;

/// Split a block into the lines `wrap = preserve` should emit, with trailing
/// whitespace removed and emphasis markers and hard breaks normalized.
///
/// Only `NEWLINE` and `HARD_LINE_BREAK` tokens end a line. Newlines inside
/// verbatim constructs never reach us as either kind -- a code span keeps its
/// whole body in one `INLINE_CODE_CONTENT` token, math uses `MATH_NEWLINE`, and
/// inline HTML uses `WHITESPACE` -- so their bytes are copied untouched and
/// this stays a prose-only trim. When callers rebuild container prefixes,
/// every physical literal line is exposed without trimming its contents.
///
/// A trailing run that genuinely *is* a hard break becomes `\` when
/// `escaped_line_breaks` is on, matching what the inline formatter emits for
/// every other wrap mode. Otherwise the original whitespace run is kept, since
/// it is the only thing carrying the break.
///
/// Container `LINE_PREFIX` tokens are kept, since most callers emit the
/// block's own source indentation. Callers that rebuild the prefix themselves
/// want [`preserve_lines_unprefixed`] instead.
pub(super) fn preserve_lines(node: &SyntaxNode, escaped_line_breaks: bool) -> Vec<String> {
    lines_with_prefix(node, escaped_line_breaks, LinePrefix::Keep, None)
}

/// Normalize prose punctuation while preserving executable source bytes.
pub(super) fn preserve_lines_normalized(
    node: &SyntaxNode,
    config: &crate::config::Config,
) -> Vec<String> {
    lines_with_prefix(
        node,
        config.formatter_extensions.escaped_line_breaks,
        LinePrefix::Keep,
        Some((
            config.formatter_extensions.smart,
            config.formatter_extensions.smart_quotes,
        )),
    )
}

/// Like [`preserve_lines`], but drops container prefixes and leading prose
/// padding. Callers rebuild that spacing, while literal tokens retain their
/// leading whitespace.
pub(super) fn preserve_lines_unprefixed(
    node: &SyntaxNode,
    escaped_line_breaks: bool,
) -> Vec<String> {
    lines_with_prefix(node, escaped_line_breaks, LinePrefix::Drop, None)
}

/// Containers rebuild their own prefixes while literal content keeps its
/// internal whitespace and prose receives the configured punctuation style.
pub(super) fn preserve_lines_unprefixed_normalized(
    node: &SyntaxNode,
    config: &crate::config::Config,
) -> Vec<String> {
    lines_with_prefix(
        node,
        config.formatter_extensions.escaped_line_breaks,
        LinePrefix::Drop,
        Some((
            config.formatter_extensions.smart,
            config.formatter_extensions.smart_quotes,
        )),
    )
}

#[derive(Clone, Copy, PartialEq)]
enum LinePrefix {
    Keep,
    Drop,
}

fn lines_with_prefix(
    node: &SyntaxNode,
    escaped_line_breaks: bool,
    prefix: LinePrefix,
    smart: Option<(bool, bool)>,
) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current = String::new();
    collect(
        node,
        escaped_line_breaks,
        prefix,
        smart,
        &mut lines,
        &mut current,
    );

    if !current.is_empty() {
        lines.push(trim_padding(&current));
    }
    lines
}

fn collect(
    node: &SyntaxNode,
    escaped_line_breaks: bool,
    prefix: LinePrefix,
    smart: Option<(bool, bool)>,
    lines: &mut Vec<String>,
    current: &mut String,
) {
    let normalize_emphasis = can_normalize_emphasis_markers(node);
    for item in node.children_with_tokens() {
        match item {
            NodeOrToken::Node(child) => {
                if LinkDestUrl::can_cast(child.kind()) {
                    let text = if prefix == LinePrefix::Drop {
                        text_without_line_prefixes(&child)
                    } else {
                        child.text().to_string()
                    };
                    append_verbatim_lines(&text, lines, current);
                    continue;
                }
                let smart = if matches!(
                    child.kind(),
                    SyntaxKind::INLINE_EXEC_SPAN | SyntaxKind::HUGO_SHORTCODE
                ) {
                    None
                } else {
                    smart
                };
                collect(&child, escaped_line_breaks, prefix, smart, lines, current)
            }
            NodeOrToken::Token(token) => match token.kind() {
                SyntaxKind::LINE_PREFIX if prefix == LinePrefix::Drop => {}
                SyntaxKind::EMPHASIS_MARKER if normalize_emphasis => current.push('*'),
                SyntaxKind::STRONG_MARKER if normalize_emphasis => current.push_str("**"),
                SyntaxKind::NEWLINE => {
                    lines.push(trim_padding(current));
                    current.clear();
                }
                SyntaxKind::HARD_LINE_BREAK => {
                    if escaped_line_breaks {
                        current.push('\\');
                    } else {
                        current.push_str(crate::utils::hard_break_marker(token.text()));
                    }
                    lines.push(std::mem::take(current));
                }
                SyntaxKind::INLINE_EXEC_CONTENT | SyntaxKind::HUGO_SHORTCODE_TEXT => {
                    append_verbatim_lines(token.text(), lines, current);
                }
                _ => {
                    let text = if prefix == LinePrefix::Drop
                        && current.is_empty()
                        && matches!(token.kind(), SyntaxKind::TEXT | SyntaxKind::WHITESPACE)
                    {
                        token.text().trim_start()
                    } else {
                        token.text()
                    };
                    let text = if let Some((enabled, quotes)) = smart {
                        super::smart::normalize_smart_punctuation(text, enabled, quotes)
                    } else {
                        std::borrow::Cow::Borrowed(text)
                    };
                    if prefix == LinePrefix::Drop {
                        append_verbatim_lines(&text, lines, current);
                    } else {
                        current.push_str(&text);
                    }
                }
            },
        }
    }
}

fn append_verbatim_lines(text: &str, lines: &mut Vec<String>, current: &mut String) {
    for part in text.split_inclusive('\n') {
        if let Some(line) = part.strip_suffix('\n') {
            current.push_str(line);
            lines.push(std::mem::take(current));
        } else {
            current.push_str(part);
        }
    }
}

fn can_normalize_emphasis_markers(node: &SyntaxNode) -> bool {
    // Merging delimiter runs can change which characters receive emphasis.
    // Keep mixed delimiters where adjacent emphasis or literal stars would
    // make replacing underscores ambiguous.
    node.children_with_tokens()
        .filter_map(|item| item.into_token())
        .all(|token| match token.kind() {
            SyntaxKind::TEXT => !token.text().contains('*'),
            SyntaxKind::EMPHASIS_MARKER | SyntaxKind::STRONG_MARKER => {
                let adjacent_marker =
                    |neighbor: &crate::syntax::SyntaxToken| neighbor.kind() == token.kind();
                let ambiguous_before = token.prev_token().is_some_and(|previous| {
                    adjacent_marker(&previous)
                        || (previous.kind() == SyntaxKind::TEXT && previous.text().ends_with('*'))
                });
                let ambiguous_after = token.next_token().is_some_and(|next| {
                    adjacent_marker(&next)
                        || (next.kind() == SyntaxKind::TEXT && next.text().starts_with('*'))
                });
                !ambiguous_before && !ambiguous_after
            }
            _ => true,
        })
}

fn trim_padding(line: &str) -> String {
    line.trim_end_matches([' ', '\t']).to_string()
}
