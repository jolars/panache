//! Indented code block parsing utilities.
//!
//! A block of text indented four spaces (or one tab) is treated as verbatim text.
//! The initial (four space or one tab) indentation is not considered part of the
//! verbatim text and is removed in the output.
//!
//! Note: blank lines in the verbatim text need not begin with four spaces.

use crate::syntax::SyntaxKind;
use rowan::GreenNodeBuilder;

use super::{container_prefix::StrippedLines, vitepress_containers::frame_indent};
use crate::parser::utils::helpers::{is_blank_line, strip_newline};

pub(crate) fn vitepress_indented_code_start(lines: &StrippedLines<'_, '_>) -> bool {
    let tail = lines.first_unconditional();
    let raw = lines.raw_at(0);
    !is_blank_line(tail) && frame_indent(tail, &raw[..raw.len() - tail.len()]).0 >= 4
}

pub(crate) fn parse_vitepress_indented_code(
    builder: &mut GreenNodeBuilder<'static>,
    lines: &StrippedLines<'_, '_>,
) -> usize {
    let start = lines.pos();
    let prefix = lines.prefix();
    let mut end = start + 1;
    for index in start + 1..lines.raw().len() {
        let raw = lines.raw()[index];
        let tail = lines.peek_prefix_at(index);
        let consumed = &raw[..raw.len() - tail.len()];
        if consumed.bytes().filter(|&byte| byte == b'>').count() < prefix.bq_depth() {
            break;
        }
        if is_blank_line(tail) {
            continue;
        }
        if !prefix.resolve(raw).reaches_frame() || frame_indent(tail, consumed).0 < 4 {
            break;
        }
        end = index + 1;
    }

    builder.start_node(SyntaxKind::CODE_BLOCK.into());
    builder.start_node(SyntaxKind::CODE_CONTENT.into());
    for index in start..end {
        let line = lines.emit_or_dispatch_tail(builder, index);
        let (content, newline) = strip_newline(line);
        let text = content.trim_start_matches([' ', '\t']);
        if content.len() > text.len() {
            builder.token(
                SyntaxKind::WHITESPACE.into(),
                &content[..content.len() - text.len()],
            );
        }
        if !text.is_empty() {
            builder.token(SyntaxKind::TEXT.into(), text);
        }
        if !newline.is_empty() {
            builder.token(SyntaxKind::NEWLINE.into(), newline);
        }
    }
    builder.finish_node();
    builder.finish_node();
    end - start
}

/// Check if a line is indented enough to be part of an indented code block.
/// Returns true if the leading whitespace expands to at least 4 columns
/// (tabs count toward the next column-4 stop, so `  \t` is 4 cols).
pub(crate) fn is_indented_code_line(content: &str) -> bool {
    if content.is_empty() {
        return false;
    }
    let (cols, _) = leading_indent(content);
    cols >= 4
}

/// Parse an indented code block, consuming lines from the parser.
/// Returns the new position after the code block.
///
/// An indented code block consists of consecutive lines that are either:
/// - Indented by 4+ spaces or 1+ tab
/// - Blank lines (which don't need indentation)
///
/// The block ends when we hit a non-blank line that isn't indented enough.
/// Parse an indented code block, consuming lines from the parser.
/// Returns the new position after the code block.
///
/// An indented code block consists of consecutive lines that are either:
/// - Indented by 4+ spaces or 1+ tab (beyond base_indent)
/// - Blank lines (which don't need indentation)
///
/// The block ends when we hit a non-blank line that isn't indented enough.
pub(crate) fn parse_indented_code_block(
    builder: &mut GreenNodeBuilder<'static>,
    lines: &[&str],
    start_pos: usize,
    bq_depth: usize,
    base_indent: usize,
) -> usize {
    parse_indented_code_block_with_first(builder, lines, start_pos, bq_depth, base_indent, None)
}

pub(crate) fn parse_indented_code_block_with_first(
    builder: &mut GreenNodeBuilder<'static>,
    lines: &[&str],
    start_pos: usize,
    bq_depth: usize,
    base_indent: usize,
    first: Option<&str>,
) -> usize {
    use super::blockquotes::{
        count_blockquote_markers, emit_one_line_prefix_marker, strip_n_blockquote_markers,
    };
    use crate::parser::utils::marker_utils::parse_blockquote_marker_info;

    builder.start_node(SyntaxKind::CODE_BLOCK.into());
    builder.start_node(SyntaxKind::CODE_CONTENT.into());

    let mut current_pos = start_pos;
    let code_indent = base_indent + 4;

    while current_pos < lines.len() {
        let line = lines[current_pos];

        let (line_bq_depth, _) = count_blockquote_markers(line);
        let first = first.filter(|_| current_pos == start_pos);
        let inner = if let Some(first) = first {
            first
        } else if bq_depth > 0 {
            strip_n_blockquote_markers(line, bq_depth)
        } else {
            line
        };

        if first.is_none() && line_bq_depth < bq_depth {
            break;
        }

        if inner.trim().is_empty() {
            let mut look_pos = current_pos + 1;
            let mut continues = false;
            while look_pos < lines.len() {
                let (look_bq_depth, look_inner) = count_blockquote_markers(lines[look_pos]);
                if look_bq_depth < bq_depth {
                    break;
                }
                if look_inner.trim_end_matches('\n').trim().is_empty() {
                    look_pos += 1;
                    continue;
                }
                let (look_indent, _) = leading_indent(look_inner);
                if look_indent >= code_indent {
                    continues = true;
                }
                break;
            }
            if !continues {
                break;
            }
            if bq_depth > 0 && current_pos > start_pos {
                let marker_info = parse_blockquote_marker_info(line);
                for i in 0..bq_depth {
                    if let Some(info) = marker_info.get(i) {
                        emit_one_line_prefix_marker(
                            builder,
                            info.leading_spaces,
                            info.has_trailing_space,
                        );
                    }
                }
            }
            emit_indented_code_line(builder, inner);
            current_pos += 1;
            continue;
        }

        let (indent_cols, _) = leading_indent(inner);
        if indent_cols < if first.is_some() { 4 } else { code_indent } {
            break;
        }

        if bq_depth > 0 && current_pos > start_pos {
            let marker_info = parse_blockquote_marker_info(line);
            for i in 0..bq_depth {
                if let Some(info) = marker_info.get(i) {
                    emit_one_line_prefix_marker(
                        builder,
                        info.leading_spaces,
                        info.has_trailing_space,
                    );
                }
            }
        }

        emit_indented_code_line(builder, inner);

        current_pos += 1;
    }

    builder.finish_node(); // CodeContent
    builder.finish_node(); // CodeBlock

    current_pos
}

pub(crate) fn emit_indented_code_line(builder: &mut GreenNodeBuilder<'static>, line: &str) {
    let (content, newline) = strip_newline(line);
    let blank = line.trim().is_empty();
    let indent = if blank {
        content.len()
    } else {
        leading_indent(content).1
    };
    if indent > 0 {
        builder.token(SyntaxKind::WHITESPACE.into(), &content[..indent]);
    }
    if blank || indent < content.len() {
        builder.token(SyntaxKind::TEXT.into(), &content[indent..]);
    }
    if !newline.is_empty() || blank {
        builder.token(
            SyntaxKind::NEWLINE.into(),
            if newline.is_empty() { "\n" } else { newline },
        );
    }
}

use crate::parser::utils::container_stack::leading_indent;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_indented_code_line() {
        assert!(is_indented_code_line("    code"));
        assert!(is_indented_code_line("        code"));
        assert!(is_indented_code_line("\tcode"));
        assert!(!is_indented_code_line("   not enough"));
        assert!(!is_indented_code_line(""));
        assert!(!is_indented_code_line("no indent"));
    }

    #[test]
    fn test_parse_simple_code_block() {
        let input = vec!["    code line 1", "    code line 2"];
        let mut builder = GreenNodeBuilder::new();
        let new_pos = parse_indented_code_block(&mut builder, &input, 0, 0, 0);
        assert_eq!(new_pos, 2);
    }

    #[test]
    fn test_parse_code_block_with_blank_line() {
        let input = vec!["    code line 1", "", "    code line 2"];
        let mut builder = GreenNodeBuilder::new();
        let new_pos = parse_indented_code_block(&mut builder, &input, 0, 0, 0);
        assert_eq!(new_pos, 3);
    }

    #[test]
    fn test_parse_code_block_stops_at_unindented() {
        let input = vec!["    code line 1", "    code line 2", "not code"];
        let mut builder = GreenNodeBuilder::new();
        let new_pos = parse_indented_code_block(&mut builder, &input, 0, 0, 0);
        assert_eq!(new_pos, 2);
    }

    #[test]
    fn test_parse_code_block_with_tab() {
        let input = vec!["\tcode with tab", "\tanother line"];
        let mut builder = GreenNodeBuilder::new();
        let new_pos = parse_indented_code_block(&mut builder, &input, 0, 0, 0);
        assert_eq!(new_pos, 2);
    }
}
