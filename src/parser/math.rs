//! Math parsing utilities for display math ($$, \[, \\[).
//!
//! This module contains shared parsing logic for display math that can appear
//! both inline (within paragraphs) and as block-level elements. The functions
//! in this module return `Option<(usize, &str)>` tuples containing the length
//! consumed and the math content, allowing calling contexts (inline parser or
//! block parser) to emit appropriate nodes based on their context.
//!
//! # Display Math vs Inline Math
//!
//! - **Display math** (this module): $$...$$, \[...\], \\[...\\]
//!   - Can appear inline within paragraphs or as standalone blocks
//!   - Allows multiline content
//!   - Shared parsing logic used by both inline and block parsers
//!
//! - **Inline math** (inline_parser/inline_math.rs): $...$, \(...\), \\(...\\)
//!   - Only appears inline within paragraphs
//!   - Cannot span multiple lines
//!   - Separate parsing logic specific to inline context

/// Math fence type for block-level display math.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MathFenceType {
    /// Dollar signs: $$
    Dollar,
    /// Backslash brackets: \[
    BackslashBracket,
    /// Double backslash brackets: \\[
    DoubleBackslashBracket,
}

/// Information about a detected math fence opening.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MathFenceInfo {
    pub fence_type: MathFenceType,
    pub fence_count: usize, // For dollars: number of $; for backslash: always 1
}

/// Try to parse display math ($$...$$) starting at the current position.
/// Returns the number of characters consumed and the math content if successful.
/// Display math can span multiple lines in inline contexts.
///
/// Per Pandoc spec (tex_math_dollars extension):
/// - Opening delimiter is at least $$
/// - Closing delimiter must have at least as many $ as opening
/// - Content can span multiple lines
pub fn try_parse_display_math(text: &str) -> Option<(usize, &str)> {
    // Must start with at least $$
    if !text.starts_with("$$") {
        return None;
    }

    // Count opening dollar signs
    let opening_count = text.chars().take_while(|&c| c == '$').count();
    if opening_count < 2 {
        return None;
    }

    let rest = &text[opening_count..];

    // Look for matching closing delimiter
    let mut pos = 0;
    while pos < rest.len() {
        let ch = rest[pos..].chars().next()?;

        if ch == '$' {
            // Check if it's escaped
            if pos > 0 && rest.as_bytes()[pos - 1] == b'\\' {
                // Escaped dollar, continue searching
                pos += ch.len_utf8();
                continue;
            }

            // Count closing dollar signs
            let closing_count = rest[pos..].chars().take_while(|&c| c == '$').count();

            // Must have at least as many closing dollars as opening
            if closing_count >= opening_count {
                let math_content = &rest[..pos];
                let total_len = opening_count + pos + closing_count;
                return Some((total_len, math_content));
            }

            // Not enough dollars, skip this run and continue
            pos += closing_count;
            continue;
        }

        pos += ch.len_utf8();
    }

    // No matching close found
    None
}

/// Try to parse single backslash display math: \[...\]
/// Extension: tex_math_single_backslash
///
/// Per Pandoc spec:
/// - Content can span multiple lines
/// - No escape handling needed (backslash is the delimiter)
pub fn try_parse_single_backslash_display_math(text: &str) -> Option<(usize, &str)> {
    if !text.starts_with(r"\[") {
        return None;
    }

    let rest = &text[2..]; // Skip \[

    // Look for closing \]
    let mut pos = 0;
    while pos < rest.len() {
        let ch = rest[pos..].chars().next()?;

        if ch == '\\' && rest[pos..].starts_with(r"\]") {
            // Found closing \]
            let math_content = &rest[..pos];
            let total_len = 2 + pos + 2; // \[ + content + \]
            return Some((total_len, math_content));
        }

        pos += ch.len_utf8();
    }

    None
}

/// Try to parse double backslash display math: \\[...\\]
/// Extension: tex_math_double_backslash
///
/// Per Pandoc spec:
/// - Content can span multiple lines
/// - Double backslash is the delimiter
pub fn try_parse_double_backslash_display_math(text: &str) -> Option<(usize, &str)> {
    if !text.starts_with(r"\\[") {
        return None;
    }

    let rest = &text[3..]; // Skip \\[

    // Look for closing \\]
    let mut pos = 0;
    while pos < rest.len() {
        let ch = rest[pos..].chars().next()?;

        if ch == '\\' && rest[pos..].starts_with(r"\\]") {
            // Found closing \\]
            let math_content = &rest[..pos];
            let total_len = 3 + pos + 3; // \\[ + content + \\]
            return Some((total_len, math_content));
        }

        pos += ch.len_utf8();
    }

    None
}

/// Try to detect a display math fence opening from a line.
/// Returns fence info if this is a valid opening fence.
///
/// Supports both $$ (dollar) and \[ (backslash bracket) formats.
/// The tex_math_single_backslash parameter controls whether \[ is recognized.
///
/// This is used by the block parser to detect display math at the start of lines.
/// For inline contexts, use `try_parse_display_math()` and related functions directly.
pub fn try_parse_math_fence_open(
    content: &str,
    tex_math_single_backslash: bool,
) -> Option<MathFenceInfo> {
    let trimmed = content.trim_start();

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
///
/// This is used by the block parser when iterating through lines.
pub fn is_closing_math_fence(content: &str, fence: &MathFenceInfo) -> bool {
    let trimmed = content.trim_start();

    match fence.fence_type {
        MathFenceType::BackslashBracket => {
            // Closing fence is \]
            // Content after \] is allowed (becomes paragraph text)
            trimmed.starts_with("\\]")
        }
        MathFenceType::DoubleBackslashBracket => {
            // Closing fence is \\]
            trimmed.starts_with("\\\\]")
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

#[cfg(test)]
mod tests {
    use super::*;

    // Display math tests
    #[test]
    fn test_parse_display_math_simple() {
        let result = try_parse_display_math("$$x = y$$");
        assert_eq!(result, Some((9, "x = y")));
    }

    #[test]
    fn test_parse_display_math_multiline() {
        let result = try_parse_display_math("$$\nx = y\n$$");
        assert_eq!(result, Some((11, "\nx = y\n")));
    }

    #[test]
    fn test_parse_display_math_triple_dollars() {
        let result = try_parse_display_math("$$$x = y$$$");
        assert_eq!(result, Some((11, "x = y")));
    }

    #[test]
    fn test_parse_display_math_no_close() {
        let result = try_parse_display_math("$$no close");
        assert_eq!(result, None);
    }

    #[test]
    fn test_not_display_math() {
        let result = try_parse_display_math("$single dollar");
        assert_eq!(result, None);
    }

    #[test]
    fn test_display_math_with_trailing_text() {
        let result = try_parse_display_math("$$x = y$$ and more");
        assert_eq!(result, Some((9, "x = y")));
    }

    // Single backslash display math tests
    #[test]
    fn test_single_backslash_display_math() {
        let result = try_parse_single_backslash_display_math(r"\[E = mc^2\]");
        assert_eq!(result, Some((12, "E = mc^2")));
    }

    #[test]
    fn test_single_backslash_display_math_multiline() {
        let result = try_parse_single_backslash_display_math("\\[\nx = y\n\\]");
        assert_eq!(result, Some((11, "\nx = y\n")));
    }

    #[test]
    fn test_single_backslash_display_math_no_close() {
        let result = try_parse_single_backslash_display_math(r"\[no close");
        assert_eq!(result, None);
    }

    // Double backslash display math tests
    #[test]
    fn test_double_backslash_display_math() {
        let result = try_parse_double_backslash_display_math(r"\\[E = mc^2\\]");
        assert_eq!(result, Some((14, "E = mc^2")));
    }

    #[test]
    fn test_double_backslash_display_math_multiline() {
        let result = try_parse_double_backslash_display_math("\\\\[\nx = y\n\\\\]");
        assert_eq!(result, Some((13, "\nx = y\n")));
    }

    #[test]
    fn test_double_backslash_display_math_no_close() {
        let result = try_parse_double_backslash_display_math(r"\\[no close");
        assert_eq!(result, None);
    }

    // Fence detection tests
    #[test]
    fn test_fence_open_two_dollar() {
        let fence = try_parse_math_fence_open("$$", false).unwrap();
        assert_eq!(fence.fence_type, MathFenceType::Dollar);
        assert_eq!(fence.fence_count, 2);
    }

    #[test]
    fn test_fence_open_backslash_bracket() {
        let fence = try_parse_math_fence_open("\\[", true).unwrap();
        assert_eq!(fence.fence_type, MathFenceType::BackslashBracket);
    }

    #[test]
    fn test_fence_open_backslash_bracket_disabled() {
        assert!(try_parse_math_fence_open("\\[", false).is_none());
    }

    #[test]
    fn test_closing_fence_dollar() {
        let fence = MathFenceInfo {
            fence_type: MathFenceType::Dollar,
            fence_count: 2,
        };
        assert!(is_closing_math_fence("$$", &fence));
        assert!(is_closing_math_fence("$$$", &fence)); // More dollars OK
        assert!(!is_closing_math_fence("$", &fence)); // Too few
    }

    #[test]
    fn test_closing_fence_backslash() {
        let fence = MathFenceInfo {
            fence_type: MathFenceType::BackslashBracket,
            fence_count: 1,
        };
        assert!(is_closing_math_fence("\\]", &fence));
        assert!(!is_closing_math_fence("\\[", &fence));
    }

    // Additional edge case tests
    #[test]
    fn test_display_math_escaped_dollar() {
        // Escaped dollar should be skipped
        let result = try_parse_display_math(r"$$a = \$100$$");
        assert_eq!(result, Some((13, r"a = \$100")));
    }

    #[test]
    fn test_display_math_with_content_on_fence_line() {
        // Content can appear on same line as opening delimiter
        let result = try_parse_display_math("$$x = y\n$$");
        assert_eq!(result, Some((10, "x = y\n")));
    }

    #[test]
    fn test_fence_open_with_leading_spaces() {
        // Fence detection should handle leading spaces
        let fence = try_parse_math_fence_open("  $$", false).unwrap();
        assert_eq!(fence.fence_type, MathFenceType::Dollar);
        assert_eq!(fence.fence_count, 2);
    }

    #[test]
    fn test_closing_fence_double_backslash() {
        let fence = MathFenceInfo {
            fence_type: MathFenceType::DoubleBackslashBracket,
            fence_count: 1,
        };
        assert!(is_closing_math_fence("\\\\]", &fence));
        assert!(!is_closing_math_fence("\\]", &fence));
        assert!(!is_closing_math_fence("\\\\[", &fence));
    }
}
