//! HTML block parsing utilities.

use crate::syntax::SyntaxKind;
use rowan::GreenNodeBuilder;

use super::blockquotes::count_blockquote_markers;
use super::utils::{strip_leading_spaces, strip_newline};

/// HTML block-level tags as defined by CommonMark spec.
/// These tags start an HTML block when found at the start of a line.
const BLOCK_TAGS: &[&str] = &[
    "address",
    "article",
    "aside",
    "base",
    "basefont",
    "blockquote",
    "body",
    "caption",
    "center",
    "col",
    "colgroup",
    "dd",
    "details",
    "dialog",
    "dir",
    "div",
    "dl",
    "dt",
    "fieldset",
    "figcaption",
    "figure",
    "footer",
    "form",
    "frame",
    "frameset",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "head",
    "header",
    "hr",
    "html",
    "iframe",
    "legend",
    "li",
    "link",
    "main",
    "menu",
    "menuitem",
    "nav",
    "noframes",
    "ol",
    "optgroup",
    "option",
    "p",
    "param",
    "section",
    "source",
    "summary",
    "table",
    "tbody",
    "td",
    "tfoot",
    "th",
    "thead",
    "title",
    "tr",
    "track",
    "ul",
];

/// Tags that contain raw/verbatim content (no Markdown processing inside).
const VERBATIM_TAGS: &[&str] = &["script", "style", "pre", "textarea"];

/// Information about a detected HTML block opening.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum HtmlBlockType {
    /// HTML comment: <!-- ... -->
    Comment,
    /// Processing instruction: <? ... ?>
    ProcessingInstruction,
    /// Declaration: <!...>
    Declaration,
    /// CDATA section: <![CDATA[ ... ]]>
    CData,
    /// Block-level tag
    BlockTag { tag_name: String, is_verbatim: bool },
}

/// Try to detect an HTML block opening from content.
/// Returns block type if this is a valid HTML block start.
pub(crate) fn try_parse_html_block_start(content: &str) -> Option<HtmlBlockType> {
    let trimmed = strip_leading_spaces(content);

    // Must start with <
    if !trimmed.starts_with('<') {
        return None;
    }

    // HTML comment
    if trimmed.starts_with("<!--") {
        return Some(HtmlBlockType::Comment);
    }

    // Processing instruction
    if trimmed.starts_with("<?") {
        return Some(HtmlBlockType::ProcessingInstruction);
    }

    // CDATA section
    if trimmed.starts_with("<![CDATA[") {
        return Some(HtmlBlockType::CData);
    }

    // Declaration (DOCTYPE, etc.)
    if trimmed.starts_with("<!") && trimmed.len() > 2 {
        let after_bang = &trimmed[2..];
        if after_bang.chars().next()?.is_ascii_uppercase() {
            return Some(HtmlBlockType::Declaration);
        }
    }

    // Try to parse as opening tag
    if let Some(tag_name) = extract_opening_tag_name(trimmed) {
        let tag_lower = tag_name.to_lowercase();

        // Check if it's a block-level tag
        if BLOCK_TAGS.contains(&tag_lower.as_str()) {
            let is_verbatim = VERBATIM_TAGS.contains(&tag_lower.as_str());
            return Some(HtmlBlockType::BlockTag {
                tag_name: tag_lower,
                is_verbatim,
            });
        }

        // Also accept verbatim tags even if not in BLOCK_TAGS list
        if VERBATIM_TAGS.contains(&tag_lower.as_str()) {
            return Some(HtmlBlockType::BlockTag {
                tag_name: tag_lower,
                is_verbatim: true,
            });
        }
    }

    None
}

/// Extract the tag name from an opening tag.
/// Returns Some(tag_name) if valid opening tag, None otherwise.
fn extract_opening_tag_name(text: &str) -> Option<String> {
    if !text.starts_with('<') {
        return None;
    }

    let after_bracket = &text[1..];

    // Skip closing tags
    if after_bracket.starts_with('/') {
        return None;
    }

    // Extract tag name (alphanumeric, ends at space, >, or /)
    let tag_end = after_bracket
        .find(|c: char| c.is_whitespace() || c == '>' || c == '/')
        .unwrap_or(after_bracket.len());

    if tag_end == 0 {
        return None;
    }

    let tag_name = &after_bracket[..tag_end];

    // Tag name must be valid (ASCII alphabetic start, alphanumeric)
    if !tag_name.chars().next()?.is_ascii_alphabetic() {
        return None;
    }

    if !tag_name.chars().all(|c| c.is_ascii_alphanumeric()) {
        return None;
    }

    Some(tag_name.to_string())
}

/// Check if a line contains the closing marker for the given HTML block type.
fn is_closing_marker(line: &str, block_type: &HtmlBlockType) -> bool {
    match block_type {
        HtmlBlockType::Comment => line.contains("-->"),
        HtmlBlockType::ProcessingInstruction => line.contains("?>"),
        HtmlBlockType::Declaration => line.contains('>'),
        HtmlBlockType::CData => line.contains("]]>"),
        HtmlBlockType::BlockTag { tag_name, .. } => {
            // Look for closing tag </tagname>
            let closing_tag = format!("</{}>", tag_name);
            line.to_lowercase().contains(&closing_tag)
        }
    }
}

/// Parse an HTML block, consuming lines from the parser.
/// Returns the new position after the HTML block.
pub(crate) fn parse_html_block(
    builder: &mut GreenNodeBuilder<'static>,
    lines: &[&str],
    start_pos: usize,
    block_type: HtmlBlockType,
    bq_depth: usize,
) -> usize {
    // Start HTML block
    builder.start_node(SyntaxKind::HTML_BLOCK.into());

    let first_line = lines[start_pos];

    // Emit opening line
    builder.start_node(SyntaxKind::HTML_BLOCK_TAG.into());

    // Split off trailing newline if present
    let (line_without_newline, newline_str) = strip_newline(first_line);

    if !line_without_newline.is_empty() {
        builder.token(SyntaxKind::TEXT.into(), line_without_newline);
    }

    if !newline_str.is_empty() {
        builder.token(SyntaxKind::NEWLINE.into(), newline_str);
    }

    builder.finish_node(); // HtmlBlockTag

    // Check if opening line also contains closing marker
    let closes_on_first_line = is_closing_marker(first_line, &block_type);

    if closes_on_first_line {
        log::debug!(
            "HTML block at line {} opens and closes on same line",
            start_pos + 1
        );
        builder.finish_node(); // HtmlBlock
        return start_pos + 1;
    }

    let mut current_pos = start_pos + 1;
    let mut content_lines: Vec<&str> = Vec::new();
    let mut found_closing = false;

    // Parse content until we find the closing marker
    while current_pos < lines.len() {
        let line = lines[current_pos];
        let (line_bq_depth, _inner_content) = count_blockquote_markers(line);

        // Only process lines at the same or deeper blockquote depth
        if line_bq_depth < bq_depth {
            break;
        }

        // Check for closing marker
        if is_closing_marker(line, &block_type) {
            log::debug!("Found HTML block closing at line {}", current_pos + 1);
            found_closing = true;

            // Emit content
            if !content_lines.is_empty() {
                builder.start_node(SyntaxKind::HTML_BLOCK_CONTENT.into());
                for content_line in &content_lines {
                    // Split off trailing newline if present
                    let (line_without_newline, newline_str) = strip_newline(content_line);

                    if !line_without_newline.is_empty() {
                        builder.token(SyntaxKind::TEXT.into(), line_without_newline);
                    }

                    if !newline_str.is_empty() {
                        builder.token(SyntaxKind::NEWLINE.into(), newline_str);
                    }
                }
                builder.finish_node(); // HtmlBlockContent
            }

            // Emit closing line
            builder.start_node(SyntaxKind::HTML_BLOCK_TAG.into());

            // Split off trailing newline if present
            let (line_without_newline, newline_str) = strip_newline(line);

            if !line_without_newline.is_empty() {
                builder.token(SyntaxKind::TEXT.into(), line_without_newline);
            }

            if !newline_str.is_empty() {
                builder.token(SyntaxKind::NEWLINE.into(), newline_str);
            }

            builder.finish_node(); // HtmlBlockTag

            current_pos += 1;
            break;
        }

        // Regular content line
        content_lines.push(line);
        current_pos += 1;
    }

    // If we didn't find a closing marker, emit what we collected
    if !found_closing {
        log::debug!("HTML block at line {} has no closing marker", start_pos + 1);
        if !content_lines.is_empty() {
            builder.start_node(SyntaxKind::HTML_BLOCK_CONTENT.into());
            for content_line in &content_lines {
                // Split off trailing newline if present
                let (line_without_newline, newline_str) = strip_newline(content_line);

                if !line_without_newline.is_empty() {
                    builder.token(SyntaxKind::TEXT.into(), line_without_newline);
                }

                if !newline_str.is_empty() {
                    builder.token(SyntaxKind::NEWLINE.into(), newline_str);
                }
            }
            builder.finish_node(); // HtmlBlockContent
        }
    }

    builder.finish_node(); // HtmlBlock
    current_pos
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_try_parse_html_comment() {
        assert_eq!(
            try_parse_html_block_start("<!-- comment -->"),
            Some(HtmlBlockType::Comment)
        );
        assert_eq!(
            try_parse_html_block_start("  <!-- comment -->"),
            Some(HtmlBlockType::Comment)
        );
    }

    #[test]
    fn test_try_parse_div_tag() {
        assert_eq!(
            try_parse_html_block_start("<div>"),
            Some(HtmlBlockType::BlockTag {
                tag_name: "div".to_string(),
                is_verbatim: false
            })
        );
        assert_eq!(
            try_parse_html_block_start("<div class=\"test\">"),
            Some(HtmlBlockType::BlockTag {
                tag_name: "div".to_string(),
                is_verbatim: false
            })
        );
    }

    #[test]
    fn test_try_parse_script_tag() {
        assert_eq!(
            try_parse_html_block_start("<script>"),
            Some(HtmlBlockType::BlockTag {
                tag_name: "script".to_string(),
                is_verbatim: true
            })
        );
    }

    #[test]
    fn test_try_parse_processing_instruction() {
        assert_eq!(
            try_parse_html_block_start("<?xml version=\"1.0\"?>"),
            Some(HtmlBlockType::ProcessingInstruction)
        );
    }

    #[test]
    fn test_try_parse_declaration() {
        assert_eq!(
            try_parse_html_block_start("<!DOCTYPE html>"),
            Some(HtmlBlockType::Declaration)
        );
    }

    #[test]
    fn test_try_parse_cdata() {
        assert_eq!(
            try_parse_html_block_start("<![CDATA[content]]>"),
            Some(HtmlBlockType::CData)
        );
    }

    #[test]
    fn test_extract_opening_tag_name() {
        assert_eq!(extract_opening_tag_name("<div>"), Some("div".to_string()));
        assert_eq!(
            extract_opening_tag_name("<div class=\"test\">"),
            Some("div".to_string())
        );
        assert_eq!(extract_opening_tag_name("<div/>"), Some("div".to_string()));
        assert_eq!(extract_opening_tag_name("</div>"), None);
        assert_eq!(extract_opening_tag_name("<>"), None);
        assert_eq!(extract_opening_tag_name("< div>"), None);
    }

    #[test]
    fn test_is_closing_marker_comment() {
        let block_type = HtmlBlockType::Comment;
        assert!(is_closing_marker("-->", &block_type));
        assert!(is_closing_marker("end -->", &block_type));
        assert!(!is_closing_marker("<!--", &block_type));
    }

    #[test]
    fn test_is_closing_marker_tag() {
        let block_type = HtmlBlockType::BlockTag {
            tag_name: "div".to_string(),
            is_verbatim: false,
        };
        assert!(is_closing_marker("</div>", &block_type));
        assert!(is_closing_marker("</DIV>", &block_type)); // Case insensitive
        assert!(is_closing_marker("content</div>", &block_type));
        assert!(!is_closing_marker("<div>", &block_type));
    }

    #[test]
    fn test_parse_html_comment_block() {
        let input = "<!-- comment -->\n";
        let lines: Vec<&str> = input.lines().collect();
        let mut builder = GreenNodeBuilder::new();

        let block_type = try_parse_html_block_start(lines[0]).unwrap();
        let new_pos = parse_html_block(&mut builder, &lines, 0, block_type, 0);

        assert_eq!(new_pos, 1);
    }

    #[test]
    fn test_parse_div_block() {
        let input = "<div>\ncontent\n</div>\n";
        let lines: Vec<&str> = input.lines().collect();
        let mut builder = GreenNodeBuilder::new();

        let block_type = try_parse_html_block_start(lines[0]).unwrap();
        let new_pos = parse_html_block(&mut builder, &lines, 0, block_type, 0);

        assert_eq!(new_pos, 3);
    }

    #[test]
    fn test_parse_html_block_no_closing() {
        let input = "<div>\ncontent\n";
        let lines: Vec<&str> = input.lines().collect();
        let mut builder = GreenNodeBuilder::new();

        let block_type = try_parse_html_block_start(lines[0]).unwrap();
        let new_pos = parse_html_block(&mut builder, &lines, 0, block_type, 0);

        // Should consume all lines even without closing tag
        assert_eq!(new_pos, 2);
    }
}
