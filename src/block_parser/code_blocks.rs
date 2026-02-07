//! Fenced code block parsing utilities.

use crate::syntax::SyntaxKind;
use rowan::GreenNodeBuilder;

use super::blockquotes::count_blockquote_markers;
use super::utils::strip_leading_spaces;

/// Information about a detected code fence opening.
pub(crate) struct FenceInfo {
    pub fence_char: char,
    pub fence_count: usize,
    pub info_string: String,
}

/// Try to detect a fenced code block opening from content.
/// Returns fence info if this is a valid opening fence.
pub(crate) fn try_parse_fence_open(content: &str) -> Option<FenceInfo> {
    let trimmed = strip_leading_spaces(content);

    // Check for fence opening (``` or ~~~)
    let (fence_char, fence_count) = if trimmed.starts_with('`') {
        let count = trimmed.chars().take_while(|&c| c == '`').count();
        ('`', count)
    } else if trimmed.starts_with('~') {
        let count = trimmed.chars().take_while(|&c| c == '~').count();
        ('~', count)
    } else {
        return None;
    };

    if fence_count < 3 {
        return None;
    }

    let info_string_raw = &trimmed[fence_count..];
    // Trim at most one leading space, preserve everything else
    let info_string = if let Some(stripped) = info_string_raw.strip_prefix(' ') {
        stripped.to_string()
    } else {
        info_string_raw.to_string()
    };

    Some(FenceInfo {
        fence_char,
        fence_count,
        info_string,
    })
}

/// Check if a line is a valid closing fence for the given fence info.
pub(crate) fn is_closing_fence(content: &str, fence: &FenceInfo) -> bool {
    let trimmed = strip_leading_spaces(content);

    if !trimmed.starts_with(fence.fence_char) {
        return false;
    }

    let closing_count = trimmed
        .chars()
        .take_while(|&c| c == fence.fence_char)
        .count();

    if closing_count < fence.fence_count {
        return false;
    }

    // Rest of line must be empty
    trimmed[closing_count..].trim().is_empty()
}

/// Parse a fenced code block, consuming lines from the parser.
/// Returns the new position after the code block.
pub(crate) fn parse_fenced_code_block(
    builder: &mut GreenNodeBuilder<'static>,
    lines: &[&str],
    start_pos: usize,
    fence: FenceInfo,
    bq_depth: usize,
) -> usize {
    // Start code block
    builder.start_node(SyntaxKind::CodeBlock.into());

    // Opening fence
    let first_line = lines[start_pos];
    let (_, first_inner) = count_blockquote_markers(first_line);
    let first_trimmed = strip_leading_spaces(first_inner);

    builder.start_node(SyntaxKind::CodeFenceOpen.into());
    builder.token(
        SyntaxKind::CodeFenceMarker.into(),
        &first_trimmed[..fence.fence_count],
    );
    if !fence.info_string.is_empty() {
        builder.token(SyntaxKind::CodeInfo.into(), &fence.info_string);
    }
    builder.token(SyntaxKind::NEWLINE.into(), "\n");
    builder.finish_node(); // CodeFenceOpen

    let mut current_pos = start_pos + 1;
    let mut content_lines: Vec<&str> = Vec::new();
    let mut found_closing = false;

    while current_pos < lines.len() {
        let line = lines[current_pos];

        // Strip blockquote markers to get inner content
        let (line_bq_depth, inner) = count_blockquote_markers(line);

        // If blockquote depth decreases, code block ends (we've left the blockquote)
        if line_bq_depth < bq_depth {
            break;
        }

        // Check for closing fence
        if is_closing_fence(inner, &fence) {
            found_closing = true;
            current_pos += 1;
            break;
        }

        content_lines.push(inner);
        current_pos += 1;
    }

    // Add content
    if !content_lines.is_empty() {
        builder.start_node(SyntaxKind::CodeContent.into());
        for content_line in content_lines.iter() {
            builder.token(SyntaxKind::TEXT.into(), content_line);
            builder.token(SyntaxKind::NEWLINE.into(), "\n");
        }
        builder.finish_node(); // CodeContent
    }

    // Closing fence (if found)
    if found_closing {
        let closing_line = lines[current_pos - 1];
        let (_, closing_inner) = count_blockquote_markers(closing_line);
        let closing_trimmed = strip_leading_spaces(closing_inner);
        let closing_count = closing_trimmed
            .chars()
            .take_while(|&c| c == fence.fence_char)
            .count();

        builder.start_node(SyntaxKind::CodeFenceClose.into());
        builder.token(
            SyntaxKind::CodeFenceMarker.into(),
            &closing_trimmed[..closing_count],
        );
        builder.token(SyntaxKind::NEWLINE.into(), "\n");
        builder.finish_node(); // CodeFenceClose
    }

    builder.finish_node(); // CodeBlock

    current_pos
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_backtick_fence() {
        let fence = try_parse_fence_open("```python").unwrap();
        assert_eq!(fence.fence_char, '`');
        assert_eq!(fence.fence_count, 3);
        assert_eq!(fence.info_string, "python");
    }

    #[test]
    fn test_tilde_fence() {
        let fence = try_parse_fence_open("~~~").unwrap();
        assert_eq!(fence.fence_char, '~');
        assert_eq!(fence.fence_count, 3);
        assert_eq!(fence.info_string, "");
    }

    #[test]
    fn test_long_fence() {
        let fence = try_parse_fence_open("`````").unwrap();
        assert_eq!(fence.fence_count, 5);
    }

    #[test]
    fn test_two_backticks_invalid() {
        assert!(try_parse_fence_open("``").is_none());
    }

    #[test]
    fn test_closing_fence() {
        let fence = FenceInfo {
            fence_char: '`',
            fence_count: 3,
            info_string: String::new(),
        };
        assert!(is_closing_fence("```", &fence));
        assert!(is_closing_fence("````", &fence));
        assert!(!is_closing_fence("``", &fence));
        assert!(!is_closing_fence("~~~", &fence));
    }
}
