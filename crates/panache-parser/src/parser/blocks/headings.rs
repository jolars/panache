//! ATX heading parsing utilities.

use crate::options::ParserOptions;
use crate::syntax::SyntaxKind;
use rowan::GreenNodeBuilder;

use crate::parser::utils::attributes::try_parse_trailing_attributes_with_pos;
use crate::parser::utils::inline_emission;

fn try_parse_mmd_header_identifier_with_pos(content: &str) -> Option<(String, usize, usize)> {
    let trimmed = content.trim_end_matches([' ', '\t']);
    let end = trimmed.len();
    let bytes = trimmed.as_bytes();

    if end == 0 || bytes[end - 1] != b']' {
        return None;
    }

    let start = trimmed[..end - 1].rfind('[')?;
    let raw = &trimmed[start..end];
    let inner = &raw[1..raw.len() - 1];
    if inner.trim().is_empty() {
        return None;
    }

    let normalized = inner.split_whitespace().collect::<String>().to_lowercase();
    if normalized.is_empty() {
        return None;
    }

    Some((normalized, start, end))
}

/// Try to parse an ATX heading from content, returns heading level (1-6) if found.
pub fn try_parse_atx_heading(content: &str) -> Option<usize> {
    let line = if let Some(stripped) = content.strip_suffix("\r\n") {
        stripped
    } else if let Some(stripped) = content.strip_suffix('\n') {
        stripped
    } else {
        content
    };
    let trimmed = line.trim_start();

    // Must start with 1-6 # characters
    let hash_count = trimmed.chars().take_while(|&c| c == '#').count();
    if hash_count == 0 || hash_count > 6 {
        return None;
    }

    // After hashes, must be end of line, space, or tab.
    // We strip trailing line ending first so empty headings like `##\n`
    // are accepted when this function is called on full source lines.
    let after_hashes = &trimmed[hash_count..];
    if !after_hashes.is_empty() && !after_hashes.starts_with(' ') && !after_hashes.starts_with('\t')
    {
        return None;
    }

    // Check leading spaces (max 3)
    let leading_spaces = line.len() - trimmed.len();
    if leading_spaces > 3 {
        return None;
    }

    Some(hash_count)
}

/// Try to parse a setext heading from lines, returns (level, underline_char) if found.
///
/// Setext headings consist of:
/// 1. A non-empty text line (heading content)
/// 2. An underline of `=` (level 1) or `-` (level 2) characters
///
/// Rules:
/// - Underline can be any non-zero length (CommonMark §4.3 / Pandoc both)
/// - Underline can have leading/trailing spaces (up to 3 leading spaces)
/// - All underline characters must be the same (`=` or `-`)
/// - Text line cannot be indented 4+ spaces (would be code block)
/// - Text line cannot be empty/blank
pub fn try_parse_setext_heading(lines: &[&str], pos: usize) -> Option<(usize, char)> {
    // Need current line (text) and next line (underline)
    if pos >= lines.len() {
        return None;
    }

    let text_line = lines[pos];
    let next_pos = pos + 1;
    if next_pos >= lines.len() {
        return None;
    }

    let underline = lines[next_pos];

    // Text line cannot be empty or blank
    if text_line.trim().is_empty() {
        return None;
    }

    // Text line cannot be indented 4+ spaces (would be code block)
    let leading_spaces = text_line.len() - text_line.trim_start().len();
    if leading_spaces >= 4 {
        return None;
    }

    // Check if underline is valid
    let underline_trimmed = underline.trim();

    // Must be non-empty
    if underline_trimmed.is_empty() {
        return None;
    }

    // Determine underline character and check consistency
    let first_char = underline_trimmed.chars().next()?;
    if first_char != '=' && first_char != '-' {
        return None;
    }

    // All characters must be the same
    if !underline_trimmed.chars().all(|c| c == first_char) {
        return None;
    }

    // Leading spaces in underline (max 3 for consistency with other block rules)
    let underline_leading_spaces = underline.len() - underline.trim_start().len();
    if underline_leading_spaces >= 4 {
        return None;
    }

    // Determine level: '=' is level 1, '-' is level 2
    let level = if first_char == '=' { 1 } else { 2 };

    Some((level, first_char))
}

/// Emit a setext heading node to the builder.
///
/// Setext headings consist of a text line followed by an underline.
/// This function emits the complete HEADING node with both lines.
pub(crate) fn emit_setext_heading(
    builder: &mut GreenNodeBuilder<'static>,
    text_line: &str,
    underline_line: &str,
    level: usize,
    config: &ParserOptions,
) {
    builder.start_node(SyntaxKind::HEADING.into());
    emit_setext_heading_body(builder, text_line, underline_line, level, config);
    builder.finish_node(); // HEADING
}

/// Emit the body of a setext heading (HEADING_CONTENT + underline + newlines).
///
/// The caller is responsible for the surrounding `HEADING` start/finish node.
/// This split lets multi-line setext headings retroactively wrap a previously
/// open paragraph by combining its buffered content with the underline line.
pub(crate) fn emit_setext_heading_body(
    builder: &mut GreenNodeBuilder<'static>,
    text_line: &str,
    underline_line: &str,
    _level: usize,
    config: &ParserOptions,
) {
    // Strip trailing newline from text line for processing
    let (text_without_newline, text_newline_str) =
        if let Some(stripped) = text_line.strip_suffix("\r\n") {
            (stripped, "\r\n")
        } else if let Some(stripped) = text_line.strip_suffix('\n') {
            (stripped, "\n")
        } else {
            (text_line, "")
        };

    // Handle leading spaces in text line
    let text_trimmed = text_without_newline.trim_start();
    let leading_spaces = text_without_newline.len() - text_trimmed.len();

    if leading_spaces > 0 {
        builder.token(
            SyntaxKind::WHITESPACE.into(),
            &text_without_newline[..leading_spaces],
        );
    }

    // Try to parse trailing attributes from heading text
    let (text_content, attr_text, space_before_attrs) =
        if let Some((_attrs, text_before, start_brace_pos)) =
            try_parse_trailing_attributes_with_pos(text_trimmed)
        {
            let space = &text_trimmed[text_before.len()..start_brace_pos];
            let raw_attrs = &text_trimmed[start_brace_pos..];
            (text_before, Some(raw_attrs), space)
        } else if config.extensions.mmd_header_identifiers {
            if let Some((_normalized, start_bracket_pos, end_bracket_pos)) =
                try_parse_mmd_header_identifier_with_pos(text_trimmed)
            {
                let text_before = text_trimmed[..start_bracket_pos].trim_end_matches([' ', '\t']);
                let space = &text_trimmed[text_before.len()..start_bracket_pos];
                let raw_attrs = &text_trimmed[start_bracket_pos..end_bracket_pos];
                (text_before, Some(raw_attrs), space)
            } else {
                (text_trimmed, None, "")
            }
        } else {
            (text_trimmed, None, "")
        };

    // Emit heading content with inline parsing
    builder.start_node(SyntaxKind::HEADING_CONTENT.into());
    if !text_content.is_empty() {
        inline_emission::emit_inlines(builder, text_content, config);
    }
    builder.finish_node();

    // Emit space before attributes if present
    if !space_before_attrs.is_empty() {
        builder.token(SyntaxKind::WHITESPACE.into(), space_before_attrs);
    }

    // Emit attributes if present
    if let Some(attr_text) = attr_text {
        builder.start_node(SyntaxKind::ATTRIBUTE.into());
        builder.token(SyntaxKind::ATTRIBUTE.into(), attr_text);
        builder.finish_node();
    }

    // Emit newline after text line
    if !text_newline_str.is_empty() {
        builder.token(SyntaxKind::NEWLINE.into(), text_newline_str);
    }

    // Strip trailing newline from underline for processing
    let (underline_without_newline, underline_newline_str) =
        if let Some(stripped) = underline_line.strip_suffix("\r\n") {
            (stripped, "\r\n")
        } else if let Some(stripped) = underline_line.strip_suffix('\n') {
            (stripped, "\n")
        } else {
            (underline_line, "")
        };

    // Emit underline leading spaces if present
    let underline_trimmed = underline_without_newline.trim_start();
    let underline_leading_spaces = underline_without_newline.len() - underline_trimmed.len();

    if underline_leading_spaces > 0 {
        builder.token(
            SyntaxKind::WHITESPACE.into(),
            &underline_without_newline[..underline_leading_spaces],
        );
    }

    // Emit the setext underline as a node containing a token
    builder.start_node(SyntaxKind::SETEXT_HEADING_UNDERLINE.into());
    builder.token(
        SyntaxKind::SETEXT_HEADING_UNDERLINE.into(),
        underline_trimmed,
    );
    builder.finish_node();

    // Emit trailing newline after underline
    if !underline_newline_str.is_empty() {
        builder.token(SyntaxKind::NEWLINE.into(), underline_newline_str);
    }
}

/// Emit an ATX heading node to the builder.
pub(crate) fn emit_atx_heading(
    builder: &mut GreenNodeBuilder<'static>,
    content: &str,
    level: usize,
    config: &ParserOptions,
) {
    builder.start_node(SyntaxKind::HEADING.into());

    // Strip trailing newline (LF or CRLF) for processing but remember to emit it later
    let (content_without_newline, newline_str) =
        if let Some(stripped) = content.strip_suffix("\r\n") {
            (stripped, "\r\n")
        } else if let Some(stripped) = content.strip_suffix('\n') {
            (stripped, "\n")
        } else {
            (content, "")
        };

    let trimmed = content_without_newline.trim_start();
    let leading_spaces = content_without_newline.len() - trimmed.len();

    // Emit leading spaces if present
    if leading_spaces > 0 {
        builder.token(
            SyntaxKind::WHITESPACE.into(),
            &content_without_newline[..leading_spaces],
        );
    }

    // Marker node for the hashes (must be a node containing a token, not just a token)
    builder.start_node(SyntaxKind::ATX_HEADING_MARKER.into());
    builder.token(SyntaxKind::ATX_HEADING_MARKER.into(), &trimmed[..level]);
    builder.finish_node();

    // Get content after marker
    let after_marker = &trimmed[level..];
    let spaces_after_marker_count = after_marker
        .find(|c: char| !c.is_whitespace())
        .unwrap_or(after_marker.len());

    // Emit spaces after marker
    if spaces_after_marker_count > 0 {
        builder.token(
            SyntaxKind::WHITESPACE.into(),
            &after_marker[..spaces_after_marker_count],
        );
    }

    // Get actual heading text
    let heading_text = &after_marker[spaces_after_marker_count..];

    // Parse optional closing ATX marker (` ###`) while preserving bytes.
    let (heading_content, closing_suffix) = {
        let without_trailing_ws = heading_text.trim_end_matches([' ', '\t']);
        let trailing_hashes = without_trailing_ws
            .chars()
            .rev()
            .take_while(|&c| c == '#')
            .count();

        if trailing_hashes > 0 {
            let hashes_start = without_trailing_ws.len() - trailing_hashes;
            let before_hashes = &without_trailing_ws[..hashes_start];
            // Closing fence requires the hashes to be preceded by whitespace.
            // That whitespace can be in `before_hashes` (non-empty content case),
            // or it can be the post-marker spaces we already consumed when content
            // is empty (e.g. `### ###` → empty heading with closing fence).
            let preceded_by_ws = before_hashes
                .chars()
                .last()
                .is_some_and(|c| c == ' ' || c == '\t')
                || (before_hashes.is_empty() && spaces_after_marker_count > 0);
            if preceded_by_ws {
                let content_end = before_hashes.trim_end_matches([' ', '\t']).len();
                (&heading_text[..content_end], &heading_text[content_end..])
            } else {
                (heading_text, "")
            }
        } else {
            (heading_text, "")
        }
    };

    // Try to parse trailing attributes
    let (text_content, attr_text, space_before_attrs) =
        if let Some((_attrs, text_before, start_brace_pos)) =
            try_parse_trailing_attributes_with_pos(heading_content)
        {
            let space = &heading_content[text_before.len()..start_brace_pos];
            let raw_attrs = &heading_content[start_brace_pos..];
            (text_before, Some(raw_attrs), space)
        } else if config.extensions.mmd_header_identifiers {
            if let Some((_normalized, start_bracket_pos, end_bracket_pos)) =
                try_parse_mmd_header_identifier_with_pos(heading_content)
            {
                let text_before =
                    heading_content[..start_bracket_pos].trim_end_matches([' ', '\t']);
                let space = &heading_content[text_before.len()..start_bracket_pos];
                let raw_attrs = &heading_content[start_bracket_pos..end_bracket_pos];
                (text_before, Some(raw_attrs), space)
            } else {
                (heading_content, None, "")
            }
        } else {
            (heading_content, None, "")
        };

    // Heading content node
    builder.start_node(SyntaxKind::HEADING_CONTENT.into());
    if !text_content.is_empty() {
        inline_emission::emit_inlines(builder, text_content, config);
    }
    builder.finish_node();

    // Emit space before attributes if present
    if !space_before_attrs.is_empty() {
        builder.token(SyntaxKind::WHITESPACE.into(), space_before_attrs);
    }

    // Emit attributes if present
    if let Some(attr_text) = attr_text {
        builder.start_node(SyntaxKind::ATTRIBUTE.into());
        builder.token(SyntaxKind::ATTRIBUTE.into(), attr_text);
        builder.finish_node();
    }

    if !closing_suffix.is_empty() {
        let closing_trimmed = closing_suffix.trim_matches(|c| c == ' ' || c == '\t');
        let leading_ws_len = closing_suffix
            .find(|c: char| c != ' ' && c != '\t')
            .unwrap_or(closing_suffix.len());
        let trailing_ws_len = closing_suffix.len() - leading_ws_len - closing_trimmed.len();

        if leading_ws_len > 0 {
            builder.token(
                SyntaxKind::WHITESPACE.into(),
                &closing_suffix[..leading_ws_len],
            );
        }
        if !closing_trimmed.is_empty() {
            builder.token(SyntaxKind::ATX_HEADING_MARKER.into(), closing_trimmed);
        }
        if trailing_ws_len > 0 {
            builder.token(
                SyntaxKind::WHITESPACE.into(),
                &closing_suffix[closing_suffix.len() - trailing_ws_len..],
            );
        }
    }

    // Emit trailing newline if present
    if !newline_str.is_empty() {
        builder.token(SyntaxKind::NEWLINE.into(), newline_str);
    }

    builder.finish_node(); // Heading
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_heading() {
        assert_eq!(try_parse_atx_heading("# Heading"), Some(1));
    }

    #[test]
    fn test_level_3_heading() {
        assert_eq!(try_parse_atx_heading("### Level 3"), Some(3));
    }

    #[test]
    fn test_heading_with_leading_spaces() {
        assert_eq!(try_parse_atx_heading("   # Heading"), Some(1));
    }

    #[test]
    fn test_atx_heading_with_attributes_losslessness() {
        use crate::ParserOptions;

        // Regression test for losslessness bug where space before attributes was dropped
        let input = "# Test {#id}\n";
        let config = ParserOptions::default();
        let tree = crate::parse(input, Some(config));

        // Verify losslessness: tree text should exactly match input
        assert_eq!(
            tree.text().to_string(),
            input,
            "Parser must preserve all bytes including space before attributes"
        );

        // Verify structure
        let heading = tree.first_child().unwrap();
        assert_eq!(heading.kind(), SyntaxKind::HEADING);

        // Find the whitespace between content and attribute
        let mut found_whitespace = false;
        for child in heading.children_with_tokens() {
            if child.kind() == SyntaxKind::WHITESPACE
                && let Some(token) = child.as_token()
            {
                let start: usize = token.text_range().start().into();
                if token.text() == " " && start == 6 {
                    found_whitespace = true;
                    break;
                }
            }
        }
        assert!(
            found_whitespace,
            "Whitespace token between heading content and attributes must be present"
        );
    }

    #[test]
    fn test_atx_heading_closing_hashes_are_lossless() {
        let input = "### Extension: `smart` ###\n";
        let tree = crate::parse(input, Some(crate::ParserOptions::default()));
        assert_eq!(tree.text().to_string(), input);
    }

    #[test]
    fn test_four_spaces_not_heading() {
        assert_eq!(try_parse_atx_heading("    # Not heading"), None);
    }

    #[test]
    fn test_no_space_after_hash() {
        assert_eq!(try_parse_atx_heading("#NoSpace"), None);
    }

    #[test]
    fn test_empty_heading() {
        assert_eq!(try_parse_atx_heading("# "), Some(1));
    }

    #[test]
    fn test_level_7_invalid() {
        assert_eq!(try_parse_atx_heading("####### Too many"), None);
    }

    // Setext heading tests
    #[test]
    fn test_setext_level_1() {
        let lines = vec!["Heading", "======="];
        assert_eq!(try_parse_setext_heading(&lines, 0), Some((1, '=')));
    }

    #[test]
    fn test_setext_level_2() {
        let lines = vec!["Heading", "-------"];
        assert_eq!(try_parse_setext_heading(&lines, 0), Some((2, '-')));
    }

    #[test]
    fn test_setext_any_underline_length() {
        // Per CommonMark §4.3 and Pandoc, the setext underline can be any
        // non-zero length. Single `=` or `-` after a non-blank line is a
        // valid setext underline.
        let lines = vec!["Heading", "="];
        assert_eq!(try_parse_setext_heading(&lines, 0), Some((1, '=')));

        let lines = vec!["Heading", "=="];
        assert_eq!(try_parse_setext_heading(&lines, 0), Some((1, '=')));

        let lines = vec!["Heading", "==="];
        assert_eq!(try_parse_setext_heading(&lines, 0), Some((1, '=')));
    }

    #[test]
    fn test_setext_mixed_chars_invalid() {
        let lines = vec!["Heading", "==-=="];
        assert_eq!(try_parse_setext_heading(&lines, 0), None);
    }

    #[test]
    fn test_setext_with_leading_spaces() {
        let lines = vec!["Heading", "   ======="];
        assert_eq!(try_parse_setext_heading(&lines, 0), Some((1, '=')));
    }

    #[test]
    fn test_setext_with_trailing_spaces() {
        let lines = vec!["Heading", "=======   "];
        assert_eq!(try_parse_setext_heading(&lines, 0), Some((1, '=')));
    }

    #[test]
    fn test_setext_empty_text_line() {
        let lines = vec!["", "======="];
        assert_eq!(try_parse_setext_heading(&lines, 0), None);
    }

    #[test]
    fn test_setext_no_next_line() {
        let lines = vec!["Heading"];
        assert_eq!(try_parse_setext_heading(&lines, 0), None);
    }

    #[test]
    fn test_setext_four_spaces_indent() {
        // 4+ spaces means code block, not setext
        let lines = vec!["    Heading", "    ======="];
        assert_eq!(try_parse_setext_heading(&lines, 0), None);
    }

    #[test]
    fn test_setext_long_underline() {
        let underline = "=".repeat(100);
        let lines = vec!["Heading", underline.as_str()];
        assert_eq!(try_parse_setext_heading(&lines, 0), Some((1, '=')));
    }

    #[test]
    fn test_parse_mmd_header_identifier_normalizes_like_pandoc() {
        let parsed = try_parse_mmd_header_identifier_with_pos("A heading [My ID]")
            .expect("should parse mmd header identifier");
        assert_eq!(parsed.0, "myid");
        assert_eq!(parsed.1, 10);
    }
}
