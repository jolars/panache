//! Raw TeX block parsing (LaTeX commands and non-math environments)
//!
//! This module handles block-level raw TeX content:
//! 1. LaTeX commands: `\DeclareMathOperator`, `\newcommand`, etc.
//! 2. Non-math environments: `\begin{tabular}`, `\begin{figure}`, etc.
//!
//! Math environments (equation, align, etc.) are handled as INLINE content
//! in paragraphs, not as blocks. See INLINE_MATH_ENVIRONMENTS list below.
//!
//! Per Pandoc behavior:
//! - Consecutive LaTeX command lines are grouped into a single TEX_BLOCK
//! - Non-math environments become TEX_BLOCK
//! - Math environments are parsed inline (in paragraphs)
//! - Blank lines or non-LaTeX content terminate the block
//! - Only enabled when `raw_tex` extension is active

use crate::options::ParserOptions;
use crate::parser::utils::helpers::{is_blank_line, strip_newline};
use crate::syntax::SyntaxKind;
use rowan::GreenNodeBuilder;

use super::container_prefix::{ContainerPrefix, StrippedLines, emit_blockquote_prefix_tokens};
use super::tables::LineView;

/// Inline math environments from Pandoc (parsed as RawInline in Para).
/// These should NOT be parsed as block-level environments.
///
/// Source: pandoc/src/Text/Pandoc/Readers/LaTeX/Math.hs:L97-L123
const INLINE_MATH_ENVIRONMENTS: &[&str] = &[
    "displaymath",
    "math",
    "equation",
    "equation*",
    "gather",
    "gather*",
    "multline",
    "multline*",
    "eqnarray",
    "eqnarray*",
    "align",
    "align*",
    "alignat",
    "alignat*",
    "flalign",
    "flalign*",
    "dmath",
    "dmath*",
    "dgroup",
    "dgroup*",
    "darray",
    "darray*",
    "subequations",
];

/// Check if an environment name is an inline math environment.
pub fn is_inline_math_environment(name: &str) -> bool {
    INLINE_MATH_ENVIRONMENTS.contains(&name)
}

/// Extract environment name from `\begin{name}` line as a borrowed
/// slice. Returns `None` if not a valid `\begin{...}` line.
///
/// This is the hot path used by the block dispatcher / paragraph
/// scanner; allocating a `String` per call shows up in profiles when
/// the doc has many LaTeX-style lines.
pub fn extract_environment_name(line: &str) -> Option<&str> {
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b'\t') {
        i += 1;
    }
    let trimmed = &line[i..];

    let after_begin = trimmed.strip_prefix("\\begin{")?;
    let close_brace = after_begin.find('}')?;
    let env_name = &after_begin[..close_brace];

    if env_name.is_empty() {
        return None;
    }

    Some(env_name)
}

/// Check whether a line can start a raw TeX block with these options.
pub fn can_start_raw_block(content: &str, config: &ParserOptions) -> bool {
    config.extensions.raw_tex
        && match extract_environment_name(content) {
            Some(name) => !is_inline_math_environment(name),
            None => is_latex_command_line(content),
        }
}

/// Check whether a command can start a raw TeX block. Environment openers
/// have their own dispatch rule.
pub(crate) fn can_start_raw_command(content: &str) -> bool {
    is_latex_command_line(content) && extract_environment_name(content).is_none()
}

/// Non-math environments can interrupt a paragraph.
pub(crate) fn raw_tex_environment(content: &str) -> Option<&str> {
    extract_environment_name(content).filter(|name| !is_inline_math_environment(name))
}

fn is_latex_command_line(line: &str) -> bool {
    let trimmed = line.trim_start();

    if !trimmed.starts_with('\\') {
        return false;
    }

    let after_backslash = &trimmed[1..];

    if after_backslash.starts_with('[') || after_backslash.starts_with(']') {
        return false;
    }

    after_backslash
        .chars()
        .next()
        .map(|c| c.is_ascii_alphabetic())
        .unwrap_or(false)
}

/// End of the command run, including lines inside a braced argument.
pub(crate) fn raw_tex_commands_end(window: &StrippedLines<'_, '_>) -> usize {
    let start = window.pos();
    let quoted = window.prefix().bq_depth() > 0;
    let mut brace_depth = 0;

    for (pos, &line) in window.raw().iter().enumerate().skip(start) {
        if pos > start && raw_tex_ends_at_boundary(window, pos) {
            return pos;
        }
        let tail = if pos == start {
            raw_tex_first(window)
        } else {
            window.prefix().strip_available_quote_prefix(line)
        };
        if pos > start && brace_depth == 0 && (quoted || !is_latex_command_line(tail)) {
            return pos;
        }
        brace_depth = (brace_depth + brace_delta(tail)).max(0);
    }
    window.raw().len()
}

fn raw_tex_ends_at_boundary(window: &StrippedLines<'_, '_>, pos: usize) -> bool {
    window.ends_container_lines(pos)
        || (window.prefix().bq_depth() > 0 && is_blank_line(window.raw()[pos]))
        || (is_blank_line(window.raw()[pos - 1]) && !window.starts_in_frame(pos))
}

pub(crate) fn raw_tex_environment_end(window: &StrippedLines<'_, '_>, name: &str) -> usize {
    let end_marker = format!(r"\end{{{name}}}");
    let first = raw_tex_first(window).trim_start();
    if first
        .strip_prefix(r"\begin{")
        .and_then(|rest| rest.get(name.len() + 1..))
        .is_some_and(|body| tex_line_has_closer(body, &end_marker))
    {
        return window.pos() + 1;
    }

    for (pos, &line) in window.raw().iter().enumerate().skip(window.pos() + 1) {
        if raw_tex_ends_at_boundary(window, pos) {
            return pos;
        }
        if tex_line_has_closer(
            window.prefix().strip_available_quote_prefix(line),
            &end_marker,
        ) {
            return pos + 1;
        }
    }
    window.raw().len()
}

pub(super) fn tex_line_has_closer(mut line: &str, closer: &str) -> bool {
    while let Some(index) = line.find(['\\', '%']) {
        let command = &line[index..];
        if command.starts_with('%') {
            return false;
        }
        if command.starts_with(closer) {
            return true;
        }
        if let Some(rest) = command.strip_prefix(r"\verb")
            && !rest.starts_with(|ch: char| ch.is_ascii_alphabetic())
        {
            let rest = rest.strip_prefix('*').unwrap_or(rest);
            let Some(delimiter) = rest.chars().next().filter(|ch| !ch.is_whitespace()) else {
                return false;
            };
            let Some((_, tail)) = rest[delimiter.len_utf8()..].split_once(delimiter) else {
                return false;
            };
            line = tail;
        } else {
            let rest = &command[1..];
            let command_end = rest.chars().next().map_or(0, char::len_utf8);
            line = &rest[command_end..];
        }
    }
    false
}

fn raw_tex_first<'a>(window: &StrippedLines<'a, '_>) -> &'a str {
    if let Some((tail, _)) = window.dispatch_override() {
        return tail;
    }
    let raw = window.raw_at(0);
    window
        .prefix()
        .strip_through_last_quote(raw)
        .unwrap_or_else(|| window.first())
}

pub(crate) fn emit_raw_tex_block(
    builder: &mut GreenNodeBuilder<'static>,
    window: &StrippedLines<'_, '_>,
    end: usize,
) {
    builder.start_node(SyntaxKind::TEX_BLOCK.into());
    for (pos, &line) in window.raw().iter().enumerate().take(end).skip(window.pos()) {
        let tail = if pos == window.pos() {
            if let Some((_, indent)) = window.dispatch_override()
                && !indent.is_empty()
            {
                builder.token(SyntaxKind::LINE_PREFIX.into(), indent);
            }
            raw_tex_first(window)
        } else if window.requires_continuation_prefix() {
            window.emit_prefix_at(builder, pos)
        } else {
            let tail = window.prefix().strip_available_quote_prefix(line);
            emit_blockquote_prefix_tokens(builder, &line[..line.len() - tail.len()]);
            tail
        };
        let (content, newline) = strip_newline(tail);
        if !content.is_empty() {
            builder.token(SyntaxKind::TEXT.into(), content);
        }
        if !newline.is_empty() {
            builder.token(SyntaxKind::NEWLINE.into(), newline);
        }
    }
    builder.finish_node();
}

/// Parse a raw TeX block and return the number of source lines consumed.
pub fn parse_raw_tex_block(
    builder: &mut GreenNodeBuilder<'static>,
    lines: &[&str],
    start_pos: usize,
    blockquote_depth: usize,
) -> usize {
    let prefix = ContainerPrefix::bq_only(blockquote_depth);
    let window = StrippedLines::new(lines, start_pos, &prefix);
    let first = window.first();
    let end = if let Some(name) = extract_environment_name(first) {
        raw_tex_environment_end(&window, name)
    } else if is_latex_command_line(first) {
        raw_tex_commands_end(&window)
    } else {
        start_pos
    };
    emit_raw_tex_block(builder, &window, end);
    end - start_pos
}

fn brace_delta(text: &str) -> i32 {
    let mut delta = 0i32;
    let mut backslashes = 0usize;

    for ch in text.chars() {
        if ch == '\\' {
            backslashes += 1;
            continue;
        }

        let escaped = backslashes % 2 == 1;
        backslashes = 0;

        if escaped {
            continue;
        }

        match ch {
            '{' => delta += 1,
            '}' => delta -= 1,
            _ => {}
        }
    }

    delta
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::options::ParserOptions;
    use crate::syntax::SyntaxNode;

    #[test]
    fn test_is_latex_command_line() {
        assert!(is_latex_command_line("\\newcommand{foo}{bar}"));
        assert!(is_latex_command_line("\\DeclareMathOperator{\\E}{E{}}"));
        assert!(is_latex_command_line("  \\section{Title}"));
        assert!(is_latex_command_line("\\usepackage{amsmath}"));

        assert!(!is_latex_command_line("Regular text"));
        assert!(!is_latex_command_line("\\123 numbers"));
        assert!(!is_latex_command_line("\\  space"));
        assert!(!is_latex_command_line(""));
    }

    #[test]
    fn test_can_start_raw_block() {
        let config = ParserOptions::default();
        assert!(can_start_raw_block("\\newcommand{foo}{bar}", &config));
        assert!(!can_start_raw_block("Regular text", &config));

        let mut config_disabled = ParserOptions::default();
        config_disabled.extensions.raw_tex = false;
        assert!(!can_start_raw_block(
            "\\newcommand{foo}{bar}",
            &config_disabled
        ));
    }

    #[test]
    fn test_parse_single_command() {
        let lines = vec!["\\DeclareMathOperator{\\E}{E{}}\n"];
        let mut builder = GreenNodeBuilder::new();

        let consumed = parse_raw_tex_block(&mut builder, &lines, 0, 0);
        assert_eq!(consumed, 1);

        let green = builder.finish();
        let node = SyntaxNode::new_root(green);
        let text = node.text().to_string();
        assert!(
            text.contains("DeclareMathOperator"),
            "Should contain command text: {}",
            text
        );
    }

    #[test]
    fn test_parse_multiple_commands() {
        let lines = vec![
            "\\newcommand{\\foo}{bar}\n",
            "\\DeclareMathOperator{\\E}{E{}}\n",
        ];
        let mut builder = GreenNodeBuilder::new();

        let consumed = parse_raw_tex_block(&mut builder, &lines, 0, 0);
        assert_eq!(consumed, 2);

        let green = builder.finish();
        let node = SyntaxNode::new_root(green);
        let text = node.text().to_string();
        assert!(
            text.contains("newcommand"),
            "Should contain newcommand: {}",
            text
        );
        assert!(
            text.contains("DeclareMathOperator"),
            "Should contain DeclareMathOperator: {}",
            text
        );
    }

    #[test]
    fn test_stops_at_blank_line() {
        let lines = vec!["\\newcommand{\\foo}{bar}\n", "\n", "Regular paragraph\n"];
        let mut builder = GreenNodeBuilder::new();

        let consumed = parse_raw_tex_block(&mut builder, &lines, 0, 0);
        assert_eq!(consumed, 1);

        let green = builder.finish();
        let node = SyntaxNode::new_root(green);
        let text = node.text().to_string();
        assert!(text.contains("newcommand"));
        assert!(!text.contains("Regular paragraph"));
    }

    #[test]
    fn test_stops_at_non_latex() {
        let lines = vec!["\\newcommand{\\foo}{bar}\n", "Regular text\n"];
        let mut builder = GreenNodeBuilder::new();

        let consumed = parse_raw_tex_block(&mut builder, &lines, 0, 0);
        assert_eq!(consumed, 1);
    }

    #[test]
    fn test_blockquote_line_does_not_loop() {
        let lines = vec!["> \\medskip\n"];
        let mut builder = GreenNodeBuilder::new();

        let consumed = parse_raw_tex_block(&mut builder, &lines, 0, 0);
        assert_eq!(consumed, 0);
    }

    #[test]
    fn test_blockquote_line_parses_tex_command() {
        let lines = vec!["> \\medskip\n"];
        let mut builder = GreenNodeBuilder::new();

        let consumed = parse_raw_tex_block(&mut builder, &lines, 0, 1);
        assert_eq!(consumed, 1);
    }

    #[test]
    fn test_blockquote_multiple_tex_commands_consumes_one_line() {
        let lines = vec!["> \\medskip\n", "> \\hfill---Joe Armstrong\n"];
        let mut builder = GreenNodeBuilder::new();

        let consumed = parse_raw_tex_block(&mut builder, &lines, 0, 1);
        assert_eq!(consumed, 1);
    }

    #[test]
    fn test_parse_braced_command_block_until_closing_brace() {
        let lines = vec!["\\pdfpcnote{\n", "  - blabla\n", "}\n"];
        let mut builder = GreenNodeBuilder::new();

        let consumed = parse_raw_tex_block(&mut builder, &lines, 0, 0);
        assert_eq!(consumed, 3);

        let green = builder.finish();
        let node = SyntaxNode::new_root(green);
        assert_eq!(node.text().to_string(), "\\pdfpcnote{\n  - blabla\n}\n");
    }
}
