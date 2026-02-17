use crate::syntax::SyntaxKind;
use rowan::GreenNodeBuilder;

use super::utils::strip_newline;

/// Try to parse the start of a line block.
/// Returns Some(()) if this line starts a line block (| followed by space or end of line).
pub fn try_parse_line_block_start(line: &str) -> Option<()> {
    let trimmed = line.trim_start();
    if trimmed.starts_with("| ") || trimmed == "|" {
        Some(())
    } else {
        None
    }
}

/// Parse a complete line block starting at current position.
/// Returns the new position after the line block.
pub fn parse_line_block(
    lines: &[&str],
    start_pos: usize,
    builder: &mut GreenNodeBuilder<'static>,
) -> usize {
    log::debug!("Parsing line block at line {}", start_pos + 1);

    builder.start_node(SyntaxKind::LINE_BLOCK.into());

    let mut pos = start_pos;

    while pos < lines.len() {
        let line = lines[pos];

        // Check if this is a line block line (starts with |)
        if let Some(content_start) = parse_line_block_line_marker(line) {
            // This is a line block line
            builder.start_node(SyntaxKind::LINE_BLOCK_LINE.into());

            // Emit the marker
            builder.token(SyntaxKind::LINE_BLOCK_MARKER.into(), "| ");

            // Emit the content (preserving leading spaces)
            let content = &line[content_start..];

            // Split off trailing newline if present
            let (content_without_newline, newline_str) = strip_newline(content);

            if !content_without_newline.is_empty() {
                builder.token(SyntaxKind::TEXT.into(), content_without_newline);
            }

            if !newline_str.is_empty() {
                builder.token(SyntaxKind::NEWLINE.into(), newline_str);
            }

            builder.finish_node(); // LineBlockLine
            pos += 1;

            // Check for continuation lines (lines that start with space)
            while pos < lines.len() {
                let next_line = lines[pos];

                // Continuation line must start with space and not be a new line block line
                if next_line.starts_with(' ') && !next_line.trim_start().starts_with("| ") {
                    // This is a continuation of the previous line
                    builder.start_node(SyntaxKind::LINE_BLOCK_LINE.into());

                    // Split off trailing newline if present
                    let (line_without_newline, newline_str) = strip_newline(next_line);

                    if !line_without_newline.is_empty() {
                        builder.token(SyntaxKind::TEXT.into(), line_without_newline);
                    }

                    if !newline_str.is_empty() {
                        builder.token(SyntaxKind::NEWLINE.into(), newline_str);
                    }

                    builder.finish_node(); // LineBlockLine
                    pos += 1;
                } else {
                    break;
                }
            }
        } else {
            // Not a line block line, end the line block
            break;
        }
    }

    builder.finish_node(); // LineBlock

    log::debug!("Parsed line block: lines {}-{}", start_pos + 1, pos);

    pos
}

/// Parse a line block marker and return the index where content starts.
/// Returns Some(index) if the line starts with "| " or just "|", None otherwise.
fn parse_line_block_line_marker(line: &str) -> Option<usize> {
    // Line block lines start with | followed by a space or end of line
    // We need to handle leading whitespace (indentation)
    let trimmed_start = line.len() - line.trim_start().len();
    let after_indent = &line[trimmed_start..];

    if after_indent.starts_with("| ") {
        Some(trimmed_start + 2) // Skip "| "
    } else if after_indent == "|" || after_indent == "|\n" {
        Some(trimmed_start + 1) // Just "|", no space
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_try_parse_line_block_start() {
        assert!(try_parse_line_block_start("| Some text").is_some());
        assert!(try_parse_line_block_start("| ").is_some());
        assert!(try_parse_line_block_start("|").is_some()); // Empty line block
        assert!(try_parse_line_block_start("  | Some text").is_some());

        // Not line blocks
        assert!(try_parse_line_block_start("|No space").is_none());
        assert!(try_parse_line_block_start("Regular text").is_none());
        assert!(try_parse_line_block_start("").is_none());
    }

    #[test]
    fn test_parse_line_block_marker() {
        assert_eq!(parse_line_block_line_marker("| Some text"), Some(2));
        assert_eq!(parse_line_block_line_marker("| "), Some(2));
        assert_eq!(parse_line_block_line_marker("|"), Some(1)); // Empty line block
        assert_eq!(parse_line_block_line_marker("  | Indented"), Some(4));

        // Not valid
        assert_eq!(parse_line_block_line_marker("|No space"), None);
        assert_eq!(parse_line_block_line_marker("Regular"), None);
    }

    #[test]
    fn test_simple_line_block() {
        let input = vec!["| Line one", "| Line two", "| Line three"];

        let mut builder = GreenNodeBuilder::new();
        let new_pos = parse_line_block(&input, 0, &mut builder);

        assert_eq!(new_pos, 3);
    }

    #[test]
    fn test_line_block_with_continuation() {
        let input = vec![
            "| This is a long line",
            "  that continues here",
            "| Second line",
        ];

        let mut builder = GreenNodeBuilder::new();
        let new_pos = parse_line_block(&input, 0, &mut builder);

        assert_eq!(new_pos, 3);
    }

    #[test]
    fn test_line_block_with_indentation() {
        let input = vec!["| First line", "|    Indented line", "| Back to normal"];

        let mut builder = GreenNodeBuilder::new();
        let new_pos = parse_line_block(&input, 0, &mut builder);

        assert_eq!(new_pos, 3);
    }

    #[test]
    fn test_line_block_stops_at_non_line_block() {
        let input = vec!["| Line one", "| Line two", "Regular paragraph"];

        let mut builder = GreenNodeBuilder::new();
        let new_pos = parse_line_block(&input, 0, &mut builder);

        assert_eq!(new_pos, 2); // Should stop before "Regular paragraph"
    }
}
