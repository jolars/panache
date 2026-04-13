//! YAML metadata block parsing utilities.

use crate::parser::utils::helpers::{emit_line_tokens, strip_newline};
use crate::syntax::SyntaxKind;
use rowan::GreenNodeBuilder;

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
    let closing_pos = find_yaml_block_closing_pos(lines, pos, at_document_start)?;
    emit_yaml_block(lines, pos, closing_pos, builder)
}

pub(crate) fn find_yaml_block_closing_pos(
    lines: &[&str],
    pos: usize,
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

    // Find a closing delimiter before emitting; otherwise this is not a valid YAML block.
    let mut closing_pos = None;
    for (i, content_line) in lines.iter().enumerate().skip(pos + 1) {
        if content_line.trim() == "---" || content_line.trim() == "..." {
            closing_pos = Some(i);
            break;
        }
    }
    closing_pos
}

pub(crate) fn emit_yaml_block(
    lines: &[&str],
    pos: usize,
    closing_pos: usize,
    builder: &mut GreenNodeBuilder<'static>,
) -> Option<usize> {
    if pos >= lines.len() || closing_pos <= pos || closing_pos >= lines.len() {
        return None;
    }
    // Start metadata node
    builder.start_node(SyntaxKind::YAML_METADATA.into());

    // Opening delimiter - strip newline before emitting
    let (text, newline_str) = strip_newline(lines[pos]);
    builder.token(SyntaxKind::YAML_METADATA_DELIM.into(), text);
    if !newline_str.is_empty() {
        builder.token(SyntaxKind::NEWLINE.into(), newline_str);
    }

    builder.start_node(SyntaxKind::YAML_METADATA_CONTENT.into());
    for content_line in lines.iter().take(closing_pos).skip(pos + 1) {
        emit_line_tokens(builder, content_line);
    }
    builder.finish_node(); // YAML_METADATA_CONTENT

    let (closing_text, closing_newline) = strip_newline(lines[closing_pos]);
    builder.token(SyntaxKind::YAML_METADATA_DELIM.into(), closing_text);
    if !closing_newline.is_empty() {
        builder.token(SyntaxKind::NEWLINE.into(), closing_newline);
    }

    builder.finish_node(); // YamlMetadata

    Some(closing_pos + 1)
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
    builder.start_node(SyntaxKind::PANDOC_TITLE_BLOCK.into());

    let mut current_pos = 0;
    let mut field_count = 0;

    // Parse up to 3 fields (title, author, date)
    while current_pos < lines.len() && field_count < 3 {
        let line = lines[current_pos];

        // Check if this line starts a field (begins with %)
        if line.trim_start().starts_with('%') {
            emit_line_tokens(builder, line);
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
                    emit_line_tokens(builder, cont_line);
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

fn mmd_key_value(line: &str) -> Option<(String, String)> {
    let (key, value) = line.split_once(':')?;
    let key_trimmed = key.trim();
    if key_trimmed.is_empty() {
        return None;
    }
    Some((key_trimmed.to_string(), value.trim().to_string()))
}

/// Try to parse a MultiMarkdown title block starting at the beginning of document.
/// Returns the new position after the block if successful, None otherwise.
///
/// A MultiMarkdown title block:
/// - Must be at document start (pos == 0)
/// - Contains one or more `Key: Value` lines
/// - The first field value must be non-empty
/// - Continuation lines start with leading space or tab
/// - Terminates with a blank line
pub(crate) fn try_parse_mmd_title_block(
    lines: &[&str],
    pos: usize,
    builder: &mut GreenNodeBuilder<'static>,
) -> Option<usize> {
    if pos != 0 || lines.is_empty() {
        return None;
    }

    let mut current_pos = pos;

    // First line must be a key-value pair with non-empty value.
    let first = lines[current_pos];
    let (_first_key, first_value) = mmd_key_value(first)?;
    if first_value.is_empty() {
        return None;
    }

    builder.start_node(SyntaxKind::MMD_TITLE_BLOCK.into());

    while current_pos < lines.len() {
        let line = lines[current_pos];

        if line.trim().is_empty() {
            break;
        }

        if mmd_key_value(line).is_none() {
            builder.finish_node();
            return None;
        }

        emit_line_tokens(builder, line);
        current_pos += 1;

        // Optional continuation lines (must be indented and not key-value starts).
        while current_pos < lines.len() {
            let cont_line = lines[current_pos];
            if cont_line.trim().is_empty() {
                break;
            }

            let trimmed = cont_line.trim_start();
            if mmd_key_value(trimmed).is_some() {
                break;
            }

            if cont_line.starts_with(' ') || cont_line.starts_with('\t') {
                emit_line_tokens(builder, cont_line);
                current_pos += 1;
            } else {
                builder.finish_node();
                return None;
            }
        }
    }

    if current_pos >= lines.len() || !lines[current_pos].trim().is_empty() {
        builder.finish_node();
        return None;
    }

    emit_line_tokens(builder, lines[current_pos]);
    current_pos += 1;

    builder.finish_node(); // MMD_TITLE_BLOCK
    Some(current_pos)
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn test_yaml_without_closing_delimiter_is_not_yaml_block() {
        let lines = vec!["---", "title: Test", "Content"];
        let mut builder = GreenNodeBuilder::new();
        let result = try_parse_yaml_block(&lines, 0, &mut builder, true);
        assert_eq!(result, None);
    }

    #[test]
    fn test_find_yaml_block_closing_pos() {
        let lines = vec!["---", "title: Test", "---", "Content"];
        let result = find_yaml_block_closing_pos(&lines, 0, true);
        assert_eq!(result, Some(2));
    }

    #[test]
    fn test_yaml_block_emits_content_node() {
        let input = "---\ntitle: Test\nlist:\n  - a\n---\n";
        let tree = crate::parse(input, Some(crate::ParserOptions::default()));
        let metadata = tree
            .descendants()
            .find(|n| n.kind() == SyntaxKind::YAML_METADATA)
            .expect("yaml metadata node");
        let content = metadata
            .children()
            .find(|n| n.kind() == SyntaxKind::YAML_METADATA_CONTENT)
            .expect("yaml metadata content node");
        assert_eq!(content.text().to_string(), "title: Test\nlist:\n  - a\n");
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

    #[test]
    fn test_mmd_title_simple() {
        let lines = vec!["Title: My Title", "Author: Jane Doe", "", "Content"];
        let mut builder = GreenNodeBuilder::new();
        let result = try_parse_mmd_title_block(&lines, 0, &mut builder);
        assert_eq!(result, Some(3));
    }

    #[test]
    fn test_mmd_title_with_continuation() {
        let lines = vec![
            "Title: My title",
            "Author: John Doe",
            "Comment: This is a sample mmd title block, with",
            "  a field spanning multiple lines.",
            "",
            "Body",
        ];
        let mut builder = GreenNodeBuilder::new();
        let result = try_parse_mmd_title_block(&lines, 0, &mut builder);
        assert_eq!(result, Some(5));
    }

    #[test]
    fn test_mmd_title_requires_non_empty_first_value() {
        let lines = vec!["Title:", "Author: Jane Doe", "", "Body"];
        let mut builder = GreenNodeBuilder::new();
        let result = try_parse_mmd_title_block(&lines, 0, &mut builder);
        assert_eq!(result, None);
    }

    #[test]
    fn test_mmd_title_requires_trailing_blank_line() {
        let lines = vec!["Title: My Title", "Author: Jane Doe"];
        let mut builder = GreenNodeBuilder::new();
        let result = try_parse_mmd_title_block(&lines, 0, &mut builder);
        assert_eq!(result, None);
    }

    #[test]
    fn test_mmd_title_not_at_start() {
        let lines = vec!["Body", "Title: My Title", ""];
        let mut builder = GreenNodeBuilder::new();
        let result = try_parse_mmd_title_block(&lines, 1, &mut builder);
        assert_eq!(result, None);
    }

    #[test]
    fn test_indented_yaml_delimiters_are_lossless() {
        let input = "    ---\n    title: Test\n    ...\n";
        let tree = crate::parse(input, Some(crate::ParserOptions::default()));
        assert_eq!(tree.text().to_string(), input);
    }
}
