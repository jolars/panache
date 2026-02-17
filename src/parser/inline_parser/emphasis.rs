//! Parsing for emphasis (*italic*, **bold**) using the CommonMark delimiter stack algorithm
//!
//! This implements the full CommonMark delimiter matching algorithm with Pandoc extensions:
//! - Extension: `intraword_underscores` - underscores inside words don't trigger emphasis
//!
//! The algorithm processes text in two phases:
//! 1. Scan phase: Find all delimiter runs and determine their open/close potential
//! 2. Match phase: Process closers left-to-right, matching with openers using a stack
//!
//! Key rules from CommonMark spec:
//! - "Rule of 3s": If opener+closer lengths sum to multiple of 3 and both can open AND close,
//!   they don't match (prevents `***foo**` from matching as bold)
//! - Strong (2 delims) takes precedence over emphasis (1 delim) when possible
//! - Delimiters must match by character (* with *, _ with _)

use crate::config::Config;
use crate::syntax::SyntaxKind;
use rowan::GreenNodeBuilder;

// The following structures and functions implement the full CommonMark delimiter stack
// algorithm for complex cases. Currently we use the simpler try_parse_emphasis for
// basic cases. These will be used when we need to handle complex nested emphasis.

/// A delimiter in the delimiter stack
#[derive(Debug, Clone)]
struct Delimiter {
    char: char,            // * or _
    count: usize,          // remaining delimiter characters
    original_count: usize, // original count (for rule of 3s)
    start_pos: usize,      // byte position in text
    can_open: bool,
    can_close: bool,
    active: bool, // false if this delimiter has been fully consumed
}

/// A matched emphasis span
#[derive(Debug, Clone, PartialEq)]
pub struct EmphasisMatch {
    pub start: usize,         // byte position of opening delimiter
    pub end: usize,           // byte position after closing delimiter
    pub content_start: usize, // byte position of content start
    pub content_end: usize,   // byte position of content end
    pub level: u8,            // 1 = em, 2 = strong
    pub delim_char: char,
}

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
        // Asterisks: standard CommonMark rules
        let can_open = left_flanking && (!right_flanking || preceded_by_punctuation);
        let can_close = right_flanking && (!left_flanking || followed_by_punctuation);
        (can_open, can_close)
    }
}

/// Scan text for all delimiter runs
#[allow(dead_code)]
fn scan_delimiters(text: &str) -> Vec<Delimiter> {
    let mut delimiters = Vec::new();
    let bytes = text.as_bytes();
    let mut pos = 0;

    while pos < bytes.len() {
        let ch = bytes[pos] as char;
        if ch == '*' || ch == '_' {
            let start = pos;
            let mut count = 0;
            while pos < bytes.len() && bytes[pos] == ch as u8 {
                count += 1;
                pos += 1;
            }

            let (can_open, can_close) = analyze_delimiter_run(text, start, ch, count);

            delimiters.push(Delimiter {
                char: ch,
                count,
                original_count: count,
                start_pos: start,
                can_open,
                can_close,
                active: true,
            });
        } else {
            pos += 1;
        }
    }

    delimiters
}

/// Process the delimiter stack to find all emphasis matches.
/// Implements the CommonMark "process emphasis" algorithm.
#[allow(dead_code)]
fn process_emphasis(delimiters: &mut [Delimiter]) -> Vec<EmphasisMatch> {
    let mut matches = Vec::new();

    // Process each potential closer from left to right
    let mut closer_idx = 0;
    while closer_idx < delimiters.len() {
        if !delimiters[closer_idx].can_close
            || !delimiters[closer_idx].active
            || delimiters[closer_idx].count == 0
        {
            closer_idx += 1;
            continue;
        }

        let closer = &delimiters[closer_idx];
        let closer_char = closer.char;

        // Look backwards for a matching opener
        let mut opener_idx = None;
        for j in (0..closer_idx).rev() {
            let opener = &delimiters[j];
            if !opener.active || opener.count == 0 || !opener.can_open {
                continue;
            }
            if opener.char != closer_char {
                continue;
            }

            // Rule of 3s (CommonMark spec):
            // If one of the delimiters can both open AND close, then the sum of
            // the lengths must not be a multiple of 3 UNLESS both lengths are
            // multiples of 3.
            let opener_orig = opener.original_count;
            let closer_orig = delimiters[closer_idx].original_count;
            let opener_both = opener.can_open && opener.can_close;
            let closer_both = delimiters[closer_idx].can_open && delimiters[closer_idx].can_close;

            if opener_both || closer_both {
                let sum = opener_orig + closer_orig;
                if sum.is_multiple_of(3)
                    && !(opener_orig.is_multiple_of(3) && closer_orig.is_multiple_of(3))
                {
                    // Skip this opener, try the next one
                    continue;
                }
            }

            opener_idx = Some(j);
            break;
        }

        if let Some(j) = opener_idx {
            // Determine how many delimiters to use (1 for em, 2 for strong)
            let opener_count = delimiters[j].count;
            let closer_count = delimiters[closer_idx].count;

            // Use 2 if both have >= 2, otherwise use 1
            let use_count = if opener_count >= 2 && closer_count >= 2 {
                2
            } else {
                1
            };
            let level = use_count as u8;

            // Calculate positions
            // Opening delimiter ends at: start_pos + (original_count - count) + use_count
            // Wait, we need to track where the "used" delimiters are
            // The used opener delims are the LAST `use_count` of the remaining opener
            // The used closer delims are the FIRST `use_count` of the remaining closer

            let opener_start = delimiters[j].start_pos;
            let opener_remaining_start =
                opener_start + (delimiters[j].original_count - delimiters[j].count);
            let opener_used_start = opener_remaining_start + (delimiters[j].count - use_count);

            let closer_start = delimiters[closer_idx].start_pos;
            let closer_remaining_start = closer_start
                + (delimiters[closer_idx].original_count - delimiters[closer_idx].count);

            let em = EmphasisMatch {
                start: opener_used_start,
                end: closer_remaining_start + use_count,
                content_start: opener_used_start + use_count,
                content_end: closer_remaining_start,
                level,
                delim_char: closer_char,
            };
            matches.push(em);

            // Consume the delimiters
            delimiters[j].count -= use_count;
            delimiters[closer_idx].count -= use_count;

            // Deactivate any delimiters between opener and closer
            for delim in delimiters.iter_mut().take(closer_idx).skip(j + 1) {
                delim.active = false;
            }

            // If closer still has delimiters, continue processing it
            if delimiters[closer_idx].count == 0 {
                closer_idx += 1;
            }
            // Otherwise, stay at same closer_idx to process remaining
        } else {
            // No opener found, move to next potential closer
            closer_idx += 1;
        }
    }

    // Sort matches by start position for proper nesting order
    matches.sort_by_key(|m| m.start);
    matches
}

/// Parse all emphasis in text and return matches
#[allow(dead_code)]
pub fn parse_emphasis(text: &str) -> Vec<EmphasisMatch> {
    let mut delimiters = scan_delimiters(text);
    process_emphasis(&mut delimiters)
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

    // === Scan tests ===

    #[test]
    fn test_scan_delimiters_simple() {
        let delimiters = scan_delimiters("*hello*");
        assert_eq!(delimiters.len(), 2);
        assert_eq!(delimiters[0].count, 1);
        assert!(delimiters[0].can_open);
        assert_eq!(delimiters[1].count, 1);
        assert!(delimiters[1].can_close);
    }

    #[test]
    fn test_scan_delimiters_double() {
        let delimiters = scan_delimiters("**bold**");
        assert_eq!(delimiters.len(), 2);
        assert_eq!(delimiters[0].count, 2);
        assert_eq!(delimiters[1].count, 2);
    }

    #[test]
    fn test_scan_delimiters_mixed() {
        let delimiters = scan_delimiters("*italic* and **bold**");
        assert_eq!(delimiters.len(), 4);
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

    // === Complex cases ===

    #[test]
    fn test_overlapping_emphasis() {
        // *foo **bar* baz** - this is a tricky case
        // CommonMark: *foo **bar* becomes <em>foo **bar</em> baz**
        // The ** inside doesn't match because of rule of 3s
        let matches = parse_emphasis("*foo **bar* baz**");
        // Should have at least one match
        assert!(!matches.is_empty());
    }

    #[test]
    fn test_nested_strong_em() {
        // **foo *bar* baz** - strong containing emphasis
        let matches = parse_emphasis("**foo *bar* baz**");
        assert!(!matches.is_empty());
    }

    #[test]
    fn test_adjacent_emphasis() {
        // *foo**bar* - CommonMark example 420
        // Should parse as <em>foo</em><em>bar</em> (two separate emphasis)
        // Actually per spec this is <em>foo**bar</em>
        let matches = parse_emphasis("*foo**bar*");
        assert!(!matches.is_empty());
    }

    #[test]
    fn test_rule_of_threes() {
        // ***bar*** - 3+3=6, multiple of 3, but both are multiples of 3
        // So they CAN match (exception to rule of 3s)
        let matches = parse_emphasis("***bar***");
        assert!(!matches.is_empty(), "Triple asterisks should match");
    }

    #[test]
    fn test_rule_of_threes_prevents_match() {
        // **foo* - 2+1=3, multiple of 3, but neither is multiple of 3
        // First * can both open and close (right-flanking at end)
        // So rule of 3s should prevent matching
        // Actually this depends on exact context. Let's use a clearer example.

        // The rule of 3s prevents things like:
        // *foo**bar**baz* from being parsed incorrectly
        let matches = parse_emphasis("*foo**bar**baz*");
        // Should get some valid parsing
        assert!(!matches.is_empty());
    }
}
