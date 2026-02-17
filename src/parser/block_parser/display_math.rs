//! Display math block parsing utilities.

use crate::syntax::SyntaxKind;
use rowan::GreenNodeBuilder;

use super::blockquotes::count_blockquote_markers;
use super::utils::{strip_leading_spaces, strip_newline};

// Re-export for use within block_parser module
pub(crate) use crate::parser::math::{
    MathFenceInfo, MathFenceType, find_closing_fence_position, is_closing_math_fence,
    try_parse_math_fence_open,
};

/// Check if a math block starting at this position has a valid closing fence.
/// This performs a lookahead to match Pandoc's behavior - math blocks without
/// closing fences are not recognized as math.
pub(crate) fn has_valid_math_closing(
    lines: &[&str],
    start_pos: usize,
    fence: &MathFenceInfo,
    bq_depth: usize,
) -> bool {
    // First check the current line for closing fence (handles single-line case: $$ x $$)
    let first_line = lines[start_pos];
    let (_, first_inner) = count_blockquote_markers(first_line);

    // Skip past the opening fence to check for closing on same line
    let first_trimmed = strip_leading_spaces(first_inner);
    let content_after_opening = match fence.fence_type {
        MathFenceType::BackslashBracket => &first_trimmed[2..], // Skip \[
        MathFenceType::DoubleBackslashBracket => &first_trimmed[3..], // Skip \\[
        MathFenceType::Dollar => &first_trimmed[fence.fence_count..], // Skip $$
    };

    // Check if closing fence exists after the opening on the same line
    if is_closing_math_fence(content_after_opening, fence) {
        return true;
    }

    // Now check subsequent lines
    let mut current_pos = start_pos + 1;

    while current_pos < lines.len() {
        let line = lines[current_pos];
        let (line_bq_depth, inner) = count_blockquote_markers(line);

        // If blockquote depth decreases, we've left the blockquote
        if line_bq_depth < bq_depth {
            return false;
        }

        // Blank line terminates search - no valid closing found
        if inner.trim().is_empty() {
            return false;
        }

        // Found closing fence
        if is_closing_math_fence(inner, fence) {
            return true;
        }

        current_pos += 1;
    }

    // Reached EOF without finding closing
    false
}

/// Parse a display math block, consuming lines from the parser.
/// Returns the new position after the math block.
pub(crate) fn parse_display_math_block(
    builder: &mut GreenNodeBuilder<'static>,
    lines: &[&str],
    start_pos: usize,
    fence: MathFenceInfo,
    bq_depth: usize,
) -> usize {
    // Start math block
    builder.start_node(SyntaxKind::MathBlock.into());

    // Opening fence
    let first_line = lines[start_pos];
    let (_, first_inner) = count_blockquote_markers(first_line);
    let first_trimmed = strip_leading_spaces(first_inner);

    let (opening_marker, first_line_content) = match fence.fence_type {
        MathFenceType::BackslashBracket => {
            // For \[, content can be on the same line
            let content_after = &first_trimmed[2..]; // Skip \[
            ("\\[", content_after)
        }
        MathFenceType::DoubleBackslashBracket => {
            // For \\[, content can be on the same line
            let content_after = &first_trimmed[3..]; // Skip \\[
            ("\\\\[", content_after)
        }
        MathFenceType::Dollar => {
            // For $$, content can be on the same line per Pandoc spec
            let content_after = &first_trimmed[fence.fence_count..]; // Skip $$
            (&first_trimmed[..fence.fence_count], content_after)
        }
    };

    builder.token(SyntaxKind::BlockMathMarker.into(), opening_marker);

    // Check if closing fence is on the same line (single-line case: $$ x $$)
    let closing_on_same_line = is_closing_math_fence(first_line_content, &fence);

    if closing_on_same_line {
        // Handle single-line display math: $$ content $$
        // Extract content before the closing fence
        if let Some((content_end, fence_len)) =
            find_closing_fence_position(first_line_content, &fence)
        {
            let content = &first_line_content[..content_end];
            let closing_marker = &first_line_content[content_end..content_end + fence_len];

            // Emit content if not empty
            if !content.is_empty() {
                builder.start_node(SyntaxKind::MathContent.into());
                builder.token(SyntaxKind::TEXT.into(), content);
                builder.finish_node(); // MathContent
            }

            // Emit closing marker
            builder.token(SyntaxKind::BlockMathMarker.into(), closing_marker);

            // Emit newline after closing if present
            let (_, newline_str) = strip_newline(first_line);
            if !newline_str.is_empty() {
                builder.token(SyntaxKind::NEWLINE.into(), newline_str);
            }

            builder.finish_node(); // MathBlock
            return start_pos + 1;
        }
    }

    // Multi-line case: opening and closing on different lines
    // For lossless parsing: check if there's content on the same line as the marker
    // Content is anything after the marker that's not just whitespace
    let content_on_same_line = !first_line_content.trim().is_empty();

    // If content is NOT on the same line, emit newline (if present in original)
    if !content_on_same_line {
        let (_, newline_str) = strip_newline(first_trimmed);
        if !newline_str.is_empty() {
            builder.token(SyntaxKind::NEWLINE.into(), newline_str);
        }
    }

    let mut current_pos = start_pos + 1;
    let mut content_lines: Vec<&str> = Vec::new();

    // Add first line content if present (for content on same line as opening)
    if content_on_same_line {
        content_lines.push(first_line_content);
    }

    let mut found_closing = false;

    while current_pos < lines.len() {
        let line = lines[current_pos];

        // Strip blockquote markers to get inner content
        let (line_bq_depth, inner) = count_blockquote_markers(line);

        // If blockquote depth decreases, math block ends (we've left the blockquote)
        if line_bq_depth < bq_depth {
            break;
        }

        // Check for blank line - per Pandoc spec, no blank lines allowed in display math
        if inner.trim().is_empty() {
            // Blank line terminates the math block without a closing fence
            break;
        }

        // Check for closing fence
        if is_closing_math_fence(inner, &fence) {
            found_closing = true;

            // Extract content before the closing fence
            if let Some((content_end, _fence_len)) = find_closing_fence_position(inner, &fence)
                && content_end > 0
            {
                // There's content before the closing fence - add it to math content
                let content_before = &inner[..content_end];
                content_lines.push(content_before);
            }

            current_pos += 1;
            break;
        }

        content_lines.push(inner);
        current_pos += 1;
    }

    // Add content - preserve original structure with newlines
    if !content_lines.is_empty() {
        builder.start_node(SyntaxKind::MathContent.into());
        for content_line in content_lines.iter() {
            // Split off trailing newline if present (from split_inclusive)
            let (text_without_newline, newline_str) = strip_newline(content_line);

            if !text_without_newline.is_empty() {
                builder.token(SyntaxKind::TEXT.into(), text_without_newline);
            }

            if !newline_str.is_empty() {
                builder.token(SyntaxKind::NEWLINE.into(), newline_str);
            }
        }
        builder.finish_node(); // MathContent
    }

    // Closing fence (if found)
    if found_closing {
        let closing_line = lines[current_pos - 1];
        let (_, closing_inner) = count_blockquote_markers(closing_line);

        // Find the closing fence position in the line
        let (fence_start, fence_len) = find_closing_fence_position(closing_inner, &fence)
            .expect("Closing fence must exist since found_closing is true");

        // Extract the actual closing marker from the line
        let closing_marker = &closing_inner[fence_start..fence_start + fence_len];

        builder.token(SyntaxKind::BlockMathMarker.into(), closing_marker);

        // Emit newline after closing marker if present
        let (_, newline_str) = strip_newline(closing_line);
        if !newline_str.is_empty() {
            builder.token(SyntaxKind::NEWLINE.into(), newline_str);
        }
    }

    builder.finish_node(); // MathBlock

    current_pos
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_two_dollar_fence() {
        let fence = try_parse_math_fence_open("$$", false, false).unwrap();
        assert_eq!(fence.fence_type, MathFenceType::Dollar);
        assert_eq!(fence.fence_count, 2);
    }

    #[test]
    fn test_backslash_bracket_fence() {
        let fence = try_parse_math_fence_open("\\[", true, false).unwrap();
        assert_eq!(fence.fence_type, MathFenceType::BackslashBracket);
    }

    #[test]
    fn test_backslash_bracket_disabled() {
        assert!(try_parse_math_fence_open("\\[", false, false).is_none());
    }
}
