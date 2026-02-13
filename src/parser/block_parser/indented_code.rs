//! Indented code block parsing utilities.
//!
//! A block of text indented four spaces (or one tab) is treated as verbatim text.
//! The initial (four space or one tab) indentation is not considered part of the
//! verbatim text and is removed in the output.
//!
//! Note: blank lines in the verbatim text need not begin with four spaces.

use crate::syntax::SyntaxKind;
use rowan::GreenNodeBuilder;

use super::utils::strip_newline;

/// Check if a line is indented enough to be part of an indented code block.
/// Returns true if the line starts with 4+ spaces or 1+ tab.
pub(crate) fn is_indented_code_line(content: &str) -> bool {
    if content.is_empty() {
        return false;
    }

    // Check for tab
    if content.starts_with('\t') {
        return true;
    }

    // Check for 4+ spaces
    let spaces = content.chars().take_while(|&c| c == ' ').count();
    spaces >= 4
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
    use super::blockquotes::count_blockquote_markers;

    builder.start_node(SyntaxKind::CodeBlock.into());
    builder.start_node(SyntaxKind::CodeContent.into());

    let mut current_pos = start_pos;
    // Total indent needed: base (e.g., footnote) + 4 for code
    let code_indent = base_indent + 4;

    while current_pos < lines.len() {
        let line = lines[current_pos];

        // Strip blockquote markers to get inner content
        let (line_bq_depth, inner) = count_blockquote_markers(line);

        // If blockquote depth decreases, code block ends (we've left the blockquote)
        if line_bq_depth < bq_depth {
            break;
        }

        // Blank lines need look-ahead: only include if next non-blank line continues the code
        if inner.trim().is_empty() {
            // Check if code continues after this blank line
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
            builder.token(SyntaxKind::TEXT.into(), "");
            builder.token(SyntaxKind::NEWLINE.into(), "\n");
            current_pos += 1;
            continue;
        }

        // Check if line is indented enough (base_indent + 4 for code)
        let (indent_cols, indent_bytes) = leading_indent(inner);
        if indent_cols < code_indent {
            break;
        }

        // For losslessness: emit ALL indentation as WHITESPACE, then emit remaining content
        // The formatter can decide how to handle the indentation
        if indent_bytes > 0 {
            let indent_str = &inner[..indent_bytes];
            builder.token(SyntaxKind::WHITESPACE.into(), indent_str);
        }

        // Get the content after the indentation
        let content = &inner[indent_bytes..];

        // Split off trailing newline if present (from split_inclusive)
        let (content_without_newline, newline_str) = strip_newline(content);

        if !content_without_newline.is_empty() {
            builder.token(SyntaxKind::TEXT.into(), content_without_newline);
        }

        if !newline_str.is_empty() {
            builder.token(SyntaxKind::NEWLINE.into(), newline_str);
        }

        current_pos += 1;
    }

    builder.finish_node(); // CodeContent
    builder.finish_node(); // CodeBlock

    current_pos
}

use super::container_stack::leading_indent;

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
