//! YAML metadata block parsing utilities.

use crate::syntax::SyntaxKind;
use rowan::GreenNodeBuilder;

/// Check if a line is a YAML metadata delimiter (`---` or `...`).
/// Returns true if the line is exactly `---` or `...` (with optional leading/trailing spaces).
#[allow(dead_code)]
pub(crate) fn is_yaml_delimiter(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed == "---" || trimmed == "..."
}

/// Try to parse a YAML metadata block starting at the given position.
/// Returns the new position after the block if successful, None otherwise.
///
/// A YAML block:
/// - Starts with `---` (not followed by blank line)
/// - Ends with `---` or `...`
/// - At document start OR preceded by blank line
pub(crate) fn try_parse_yaml_block(
    lines: &[&str],
    pos: usize,
    builder: &mut GreenNodeBuilder<'static>,
    at_document_start: bool,
) -> Option<usize> {
    if pos >= lines.len() {
        return None;
    }

    let line = lines[pos];

    // Must start with ---
    if line.trim() != "---" {
        return None;
    }

    // If not at document start, previous line must be blank
    if !at_document_start && pos > 0 {
        let prev_line = lines[pos - 1];
        if !prev_line.trim().is_empty() {
            return None;
        }
    }

    // Check that next line (if exists) is NOT blank (this distinguishes from horizontal rule)
    if pos + 1 < lines.len() {
        let next_line = lines[pos + 1];
        if next_line.trim().is_empty() {
            // This is likely a horizontal rule, not YAML
            return None;
        }
    } else {
        // No content after ---, can't be a YAML block
        return None;
    }

    // Start metadata node
    builder.start_node(SyntaxKind::YamlMetadata.into());

    // Opening delimiter - strip newline before emitting
    if let Some(text) = line.strip_suffix('\n') {
        builder.token(SyntaxKind::YamlMetadataDelim.into(), text.trim());
        builder.token(SyntaxKind::NEWLINE.into(), "\n");
    } else {
        builder.token(SyntaxKind::YamlMetadataDelim.into(), line.trim());
    }

    let mut current_pos = pos + 1;
    let mut found_closing = false;

    // Collect content until we find closing delimiter
    while current_pos < lines.len() {
        let content_line = lines[current_pos];

        // Check for closing delimiter
        if content_line.trim() == "---" || content_line.trim() == "..." {
            found_closing = true;
            if let Some(text) = content_line.strip_suffix('\n') {
                builder.token(SyntaxKind::YamlMetadataDelim.into(), text.trim());
                builder.token(SyntaxKind::NEWLINE.into(), "\n");
            } else {
                builder.token(SyntaxKind::YamlMetadataDelim.into(), content_line.trim());
            }
            current_pos += 1;
            break;
        }

        // Add content line
        super::utils::emit_line_tokens(builder, content_line);
        current_pos += 1;
    }

    builder.finish_node(); // YamlMetadata

    if found_closing {
        Some(current_pos)
    } else {
        // No closing delimiter found - this might be a horizontal rule after all
        // or malformed YAML. For now, accept it.
        Some(current_pos)
    }
}

/// Try to parse a Pandoc title block starting at the beginning of document.
/// Returns the new position after the block if successful, None otherwise.
///
/// A Pandoc title block:
/// - Must be at document start (pos == 0)
/// - Has 1-3 lines starting with `%`
/// - Format: % title, % author(s), % date
/// - Continuation lines start with leading space
pub(crate) fn try_parse_pandoc_title_block(
    lines: &[&str],
    pos: usize,
    builder: &mut GreenNodeBuilder<'static>,
) -> Option<usize> {
    if pos != 0 || lines.is_empty() {
        return None;
    }

    let first_line = lines[0];
    if !first_line.trim_start().starts_with('%') {
        return None;
    }

    // Start title block node
    builder.start_node(SyntaxKind::PandocTitleBlock.into());

    let mut current_pos = 0;
    let mut field_count = 0;

    // Parse up to 3 fields (title, author, date)
    while current_pos < lines.len() && field_count < 3 {
        let line = lines[current_pos];

        // Check if this line starts a field (begins with %)
        if line.trim_start().starts_with('%') {
            super::utils::emit_line_tokens(builder, line);
            field_count += 1;
            current_pos += 1;

            // Collect continuation lines (start with leading space, not with %)
            while current_pos < lines.len() {
                let cont_line = lines[current_pos];
                if cont_line.is_empty() {
                    // Blank line ends title block
                    break;
                }
                if cont_line.trim_start().starts_with('%') {
                    // Next field
                    break;
                }
                if cont_line.starts_with(' ') || cont_line.starts_with('\t') {
                    // Continuation line
                    super::utils::emit_line_tokens(builder, cont_line);
                    current_pos += 1;
                } else {
                    // Non-continuation, non-% line ends title block
                    break;
                }
            }
        } else {
            // Line doesn't start with %, title block ends
            break;
        }
    }

    builder.finish_node(); // PandocTitleBlock

    if field_count > 0 {
        Some(current_pos)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_yaml_delimiter() {
        assert!(is_yaml_delimiter("---"));
        assert!(is_yaml_delimiter("  ---  "));
        assert!(is_yaml_delimiter("..."));
        assert!(is_yaml_delimiter("  ...  "));
        assert!(!is_yaml_delimiter("----"));
        assert!(!is_yaml_delimiter("-- -"));
    }

    #[test]
    fn test_yaml_block_at_start() {
        let lines = vec!["---", "title: Test", "---", "Content"];
        let mut builder = GreenNodeBuilder::new();
        let result = try_parse_yaml_block(&lines, 0, &mut builder, true);
        assert_eq!(result, Some(3));
    }

    #[test]
    fn test_yaml_block_not_at_start() {
        let lines = vec!["Paragraph", "", "---", "title: Test", "---", "Content"];
        let mut builder = GreenNodeBuilder::new();
        let result = try_parse_yaml_block(&lines, 2, &mut builder, false);
        assert_eq!(result, Some(5));
    }

    #[test]
    fn test_horizontal_rule_not_yaml() {
        let lines = vec!["---", "", "Content"];
        let mut builder = GreenNodeBuilder::new();
        let result = try_parse_yaml_block(&lines, 0, &mut builder, true);
        assert_eq!(result, None); // Followed by blank line, so not YAML
    }

    #[test]
    fn test_yaml_with_dots_closer() {
        let lines = vec!["---", "title: Test", "...", "Content"];
        let mut builder = GreenNodeBuilder::new();
        let result = try_parse_yaml_block(&lines, 0, &mut builder, true);
        assert_eq!(result, Some(3));
    }

    #[test]
    fn test_pandoc_title_simple() {
        let lines = vec!["% My Title", "% Author", "% Date", "", "Content"];
        let mut builder = GreenNodeBuilder::new();
        let result = try_parse_pandoc_title_block(&lines, 0, &mut builder);
        assert_eq!(result, Some(3));
    }

    #[test]
    fn test_pandoc_title_with_continuation() {
        let lines = vec![
            "% My Title",
            "  on multiple lines",
            "% Author One",
            "  Author Two",
            "% June 15, 2006",
            "",
            "Content",
        ];
        let mut builder = GreenNodeBuilder::new();
        let result = try_parse_pandoc_title_block(&lines, 0, &mut builder);
        assert_eq!(result, Some(5));
    }

    #[test]
    fn test_pandoc_title_partial() {
        let lines = vec!["% My Title", "%", "% June 15, 2006", "", "Content"];
        let mut builder = GreenNodeBuilder::new();
        let result = try_parse_pandoc_title_block(&lines, 0, &mut builder);
        assert_eq!(result, Some(3));
    }

    #[test]
    fn test_pandoc_title_not_at_start() {
        let lines = vec!["Content", "% Title"];
        let mut builder = GreenNodeBuilder::new();
        let result = try_parse_pandoc_title_block(&lines, 1, &mut builder);
        assert_eq!(result, None);
    }
}
