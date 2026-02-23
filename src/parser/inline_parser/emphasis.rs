//! Parsing for emphasis

use crate::config::Config;
use crate::syntax::SyntaxKind;
use rowan::GreenNodeBuilder;

/// Check if a character is Unicode whitespace
fn is_whitespace(c: char) -> bool {
    c.is_whitespace()
}

/// Check if a character is Unicode punctuation
fn is_punctuation(c: char) -> bool {
    c.is_ascii_punctuation()
}

/// Determine if a delimiter run can open/close emphasis based on flanking rules.
fn analyze_delimiter_run(
    text: &str,
    run_start: usize,
    run_char: char,
    run_count: usize,
) -> (bool, bool) {
    let run_end = run_start + run_count;

    let char_before = if run_start > 0 {
        text[..run_start].chars().last()
    } else {
        None
    };

    let char_after = if run_end < text.len() {
        text[run_end..].chars().next()
    } else {
        None
    };

    let followed_by_whitespace = char_after.is_none_or(is_whitespace);
    let followed_by_punctuation = char_after.is_some_and(is_punctuation);
    let preceded_by_whitespace = char_before.is_none_or(is_whitespace);
    let preceded_by_punctuation = char_before.is_some_and(is_punctuation);

    let left_flanking = !followed_by_whitespace
        && (!followed_by_punctuation || preceded_by_whitespace || preceded_by_punctuation);

    let right_flanking = !preceded_by_whitespace
        && (!preceded_by_punctuation || followed_by_whitespace || followed_by_punctuation);

    // Special rules for underscores (Pandoc intraword_underscores extension)
    if run_char == '_' {
        let preceded_by_alnum = char_before.is_some_and(|c| c.is_alphanumeric());
        let followed_by_alnum = char_after.is_some_and(|c| c.is_alphanumeric());

        let can_open = left_flanking && !preceded_by_alnum;
        let can_close = right_flanking && !followed_by_alnum;
        (can_open, can_close)
    } else {
        // Asterisks
        let can_open = left_flanking && (!right_flanking || preceded_by_punctuation);
        let can_close = right_flanking && (!left_flanking || followed_by_punctuation);
        (can_open, can_close)
    }
}

/// Try to parse emphasis starting at the given position.
/// Returns (total_bytes_consumed, inner_text, delimiter_level, delimiter_char) if successful.
///
/// This uses a simplified approach: match the opening delimiter run with the first
/// valid closing delimiter of the same type. Nested emphasis is handled by recursive
/// parsing of the inner content.
pub fn try_parse_emphasis(text: &str) -> Option<(usize, &str, u8, char)> {
    if text.is_empty() {
        return None;
    }

    let first_char = text.chars().next()?;
    if first_char != '*' && first_char != '_' {
        return None;
    }

    // Count opening delimiters
    let bytes = text.as_bytes();
    let mut open_count = 0;
    while open_count < bytes.len() && bytes[open_count] == first_char as u8 {
        open_count += 1;
    }

    // Check if this can open emphasis
    let (can_open, _) = analyze_delimiter_run(text, 0, first_char, open_count);
    if !can_open {
        return None;
    }

    // For *** or more, we'll match greedily but cap at 3 for the return level
    // Search for matching closing delimiter
    let mut search_pos = open_count;

    while search_pos < text.len() {
        // Find next occurrence of delimiter char
        let remaining = &text[search_pos..];
        let next_delim = remaining.find(first_char)?;
        let close_start = search_pos + next_delim;

        // Count closing delimiters
        let mut close_count = 0;
        let mut pos = close_start;
        while pos < bytes.len() && bytes[pos] == first_char as u8 {
            close_count += 1;
            pos += 1;
        }

        // Check if this can close emphasis
        let (_, can_close) = analyze_delimiter_run(text, close_start, first_char, close_count);

        if can_close {
            // Determine how many delimiters to match
            let match_count = open_count.min(close_count).min(3);

            // The match spans from position 0 to close_start + match_count
            let total_len = close_start + match_count;
            let inner = &text[match_count..close_start];
            let level = match_count as u8;

            return Some((total_len, inner, level, first_char));
        }

        // Skip past this delimiter run and continue searching
        search_pos = close_start + close_count;
    }

    None
}

/// Emit emphasis node to the builder
pub fn emit_emphasis(
    builder: &mut GreenNodeBuilder,
    inner_text: &str,
    level: u8,
    delim_char: char,
    _config: &Config,
) {
    let delim = match level {
        1 => {
            if delim_char == '_' {
                "_"
            } else {
                "*"
            }
        }
        2 => {
            if delim_char == '_' {
                "__"
            } else {
                "**"
            }
        }
        _ => {
            if delim_char == '_' {
                "___"
            } else {
                "***"
            }
        }
    };

    match level {
        1 => {
            builder.start_node(SyntaxKind::EMPHASIS.into());
            builder.token(SyntaxKind::EMPHASIS_MARKER.into(), delim);
            builder.token(SyntaxKind::TEXT.into(), inner_text);
            builder.token(SyntaxKind::EMPHASIS_MARKER.into(), delim);
            builder.finish_node();
        }
        2 => {
            builder.start_node(SyntaxKind::STRONG.into());
            builder.token(SyntaxKind::STRONG_MARKER.into(), delim);
            builder.token(SyntaxKind::TEXT.into(), inner_text);
            builder.token(SyntaxKind::STRONG_MARKER.into(), delim);
            builder.finish_node();
        }
        _ => {
            // Level 3+ = nested Strong + Emphasis
            let inner_delim = if delim_char == '_' { "_" } else { "*" };
            let outer_delim = if delim_char == '_' { "__" } else { "**" };

            builder.start_node(SyntaxKind::STRONG.into());
            builder.token(SyntaxKind::STRONG_MARKER.into(), outer_delim);

            builder.start_node(SyntaxKind::EMPHASIS.into());
            builder.token(SyntaxKind::EMPHASIS_MARKER.into(), inner_delim);
            builder.token(SyntaxKind::TEXT.into(), inner_text);
            builder.token(SyntaxKind::EMPHASIS_MARKER.into(), inner_delim);
            builder.finish_node();

            builder.token(SyntaxKind::STRONG_MARKER.into(), outer_delim);
            builder.finish_node();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // === Flanking rule tests ===

    #[test]
    fn test_asterisk_can_open() {
        let (can_open, _) = analyze_delimiter_run("*word", 0, '*', 1);
        assert!(can_open);
    }

    #[test]
    fn test_asterisk_can_close() {
        let (_, can_close) = analyze_delimiter_run("word*", 4, '*', 1);
        assert!(can_close);
    }

    #[test]
    fn test_asterisk_space_no_emphasis() {
        let (can_open, _) = analyze_delimiter_run("* word", 0, '*', 1);
        assert!(!can_open);

        let (_, can_close) = analyze_delimiter_run("word *", 5, '*', 1);
        assert!(!can_close);
    }

    #[test]
    fn test_underscore_intraword() {
        let (can_open, can_close) = analyze_delimiter_run("feas_ible", 4, '_', 1);
        assert!(!can_open, "Underscore in word shouldn't open");
        assert!(!can_close, "Underscore in word shouldn't close");
    }

    #[test]
    fn test_underscore_start_of_word() {
        let (can_open, _) = analyze_delimiter_run("_word", 0, '_', 1);
        assert!(can_open);
    }

    #[test]
    fn test_underscore_end_of_word() {
        let (_, can_close) = analyze_delimiter_run("word_", 4, '_', 1);
        assert!(can_close);
    }

    // === Full parsing tests ===

    #[test]
    fn test_try_parse_simple_emphasis() {
        let result = try_parse_emphasis("*hello*");
        assert_eq!(result, Some((7, "hello", 1, '*')));
    }

    #[test]
    fn test_try_parse_strong() {
        let result = try_parse_emphasis("**bold**");
        assert_eq!(result, Some((8, "bold", 2, '*')));
    }

    #[test]
    fn test_try_parse_triple() {
        let text = "***both***";
        let result = try_parse_emphasis(text);

        assert!(result.is_some(), "Triple emphasis should parse");
        let (len, inner, level, ch) = result.unwrap();
        assert_eq!(ch, '*');
        assert_eq!(level, 3, "Triple asterisks should give level 3");
        assert_eq!(inner, "both");
        assert_eq!(len, 10);
    }

    #[test]
    fn test_try_parse_no_closing() {
        let result = try_parse_emphasis("*hello");
        assert_eq!(result, None);
    }

    #[test]
    fn test_try_parse_underscore() {
        let result = try_parse_emphasis("_italic_");
        assert_eq!(result, Some((8, "italic", 1, '_')));
    }

    #[test]
    fn test_try_parse_not_opener() {
        let result = try_parse_emphasis("* hello");
        assert_eq!(result, None);
    }
}
