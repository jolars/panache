//! Display math block parsing utilities.

use crate::syntax::SyntaxKind;
use rowan::GreenNodeBuilder;

use super::blockquotes::count_blockquote_markers;
use super::utils::{strip_leading_spaces, strip_newline};

/// Math fence type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MathFenceType {
    /// Dollar signs: $$
    Dollar,
    /// Backslash brackets: \[
    BackslashBracket,
}

/// Information about a detected math fence opening.
pub(crate) struct MathFenceInfo {
    pub fence_type: MathFenceType,
    pub fence_count: usize, // For dollars: number of $; for backslash: always 1
}

/// Try to detect a display math block opening from content.
/// Returns fence info if this is a valid opening fence.
/// Supports both $$ (dollar) and \[ (backslash bracket) formats.
/// The tex_math_single_backslash parameter controls whether \[ is recognized.
pub(crate) fn try_parse_math_fence_open(
    content: &str,
    tex_math_single_backslash: bool,
) -> Option<MathFenceInfo> {
    let trimmed = strip_leading_spaces(content);

    // Check for backslash bracket opening: \[
    // Per Pandoc spec, content can be on the same line
    if tex_math_single_backslash && trimmed.starts_with("\\[") {
        return Some(MathFenceInfo {
            fence_type: MathFenceType::BackslashBracket,
            fence_count: 1,
        });
    }

    // Check for math fence opening ($$)
    // Per Pandoc spec: "the delimiters may be separated from the formula by whitespace"
    // This means content can be on the same line as the opening $$
    if !trimmed.starts_with('$') {
        return None;
    }

    let fence_count = trimmed.chars().take_while(|&c| c == '$').count();

    if fence_count < 2 {
        return None;
    }

    Some(MathFenceInfo {
        fence_type: MathFenceType::Dollar,
        fence_count,
    })
}

/// Check if a line is a valid closing fence for the given fence info.
pub(crate) fn is_closing_math_fence(content: &str, fence: &MathFenceInfo) -> bool {
    let trimmed = strip_leading_spaces(content);

    match fence.fence_type {
        MathFenceType::BackslashBracket => {
            // Closing fence is \]
            // Content after \] is allowed (becomes paragraph text)
            trimmed.starts_with("\\]")
        }
        MathFenceType::Dollar => {
            if !trimmed.starts_with('$') {
                return false;
            }

            let closing_count = trimmed.chars().take_while(|&c| c == '$').count();

            // Must have at least as many $ as the opening
            // Content after $$ is allowed (becomes paragraph text)
            closing_count >= fence.fence_count
        }
    }
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
        MathFenceType::Dollar => {
            // For $$, content can be on the same line per Pandoc spec
            let content_after = &first_trimmed[fence.fence_count..]; // Skip $$
            (&first_trimmed[..fence.fence_count], content_after)
        }
    };

    builder.token(SyntaxKind::BlockMathMarker.into(), opening_marker);

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
        let closing_trimmed = strip_leading_spaces(closing_inner);

        let closing_marker = match fence.fence_type {
            MathFenceType::BackslashBracket => "\\]",
            MathFenceType::Dollar => {
                let closing_count = closing_trimmed.chars().take_while(|&c| c == '$').count();
                &closing_trimmed[..closing_count]
            }
        };

        builder.token(SyntaxKind::BlockMathMarker.into(), closing_marker);

        // Emit newline after closing marker if present
        let (_, newline_str) = strip_newline(closing_trimmed);
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
        let fence = try_parse_math_fence_open("$$", false).unwrap();
        assert_eq!(fence.fence_type, MathFenceType::Dollar);
        assert_eq!(fence.fence_count, 2);
    }

    #[test]
    fn test_backslash_bracket_fence() {
        let fence = try_parse_math_fence_open("\\[", true).unwrap();
        assert_eq!(fence.fence_type, MathFenceType::BackslashBracket);
    }

    #[test]
    fn test_backslash_bracket_disabled() {
        assert!(try_parse_math_fence_open("\\[", false).is_none());
    }
}
