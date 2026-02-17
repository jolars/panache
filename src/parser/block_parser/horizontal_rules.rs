//! Horizontal rule parsing utilities.

use crate::syntax::SyntaxKind;
use rowan::GreenNodeBuilder;

use super::utils::strip_newline;

/// Try to parse a horizontal rule from a line.
/// Returns true if this line is a valid horizontal rule.
///
/// A horizontal rule is 3 or more `*`, `-`, or `_` characters,
/// optionally separated by spaces.
pub(crate) fn try_parse_horizontal_rule(line: &str) -> Option<char> {
    let trimmed = line.trim();

    // Must have at least 3 characters
    if trimmed.len() < 3 {
        return None;
    }

    // Determine which character is being used
    let rule_char = trimmed.chars().next()?;
    if !matches!(rule_char, '*' | '-' | '_') {
        return None;
    }

    // Check that the line only contains the rule character and spaces
    let mut count = 0;
    for ch in trimmed.chars() {
        match ch {
            c if c == rule_char => count += 1,
            ' ' | '\t' => continue,
            _ => return None,
        }
    }

    // Must have at least 3 of the rule character
    if count >= 3 { Some(rule_char) } else { None }
}

/// Emit a horizontal rule node to the builder.
pub(crate) fn emit_horizontal_rule(builder: &mut GreenNodeBuilder<'static>, line: &str) {
    builder.start_node(SyntaxKind::HORIZONTAL_RULE.into());

    // Strip trailing newline and emit the rule content (trimmed)
    let (line_without_newline, newline_str) = strip_newline(line);
    let content = line_without_newline.trim();
    builder.token(SyntaxKind::HORIZONTAL_RULE.into(), content);

    // Emit newline separately if present
    if !newline_str.is_empty() {
        builder.token(SyntaxKind::NEWLINE.into(), newline_str);
    }

    builder.finish_node();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_asterisk_rule() {
        assert_eq!(try_parse_horizontal_rule("***"), Some('*'));
        assert_eq!(try_parse_horizontal_rule("* * *"), Some('*'));
        assert_eq!(try_parse_horizontal_rule("*  *  *"), Some('*'));
        assert_eq!(try_parse_horizontal_rule("****"), Some('*'));
    }

    #[test]
    fn test_dash_rule() {
        assert_eq!(try_parse_horizontal_rule("---"), Some('-'));
        assert_eq!(try_parse_horizontal_rule("- - -"), Some('-'));
        assert_eq!(try_parse_horizontal_rule("---------------"), Some('-'));
    }

    #[test]
    fn test_underscore_rule() {
        assert_eq!(try_parse_horizontal_rule("___"), Some('_'));
        assert_eq!(try_parse_horizontal_rule("_ _ _"), Some('_'));
        assert_eq!(try_parse_horizontal_rule("_____"), Some('_'));
    }

    #[test]
    fn test_with_leading_trailing_spaces() {
        assert_eq!(try_parse_horizontal_rule("  ***  "), Some('*'));
        assert_eq!(try_parse_horizontal_rule("\t---\t"), Some('-'));
    }

    #[test]
    fn test_too_few_characters() {
        assert_eq!(try_parse_horizontal_rule("**"), None);
        assert_eq!(try_parse_horizontal_rule("--"), None);
        assert_eq!(try_parse_horizontal_rule("__"), None);
    }

    #[test]
    fn test_mixed_characters() {
        assert_eq!(try_parse_horizontal_rule("*-*"), None);
        assert_eq!(try_parse_horizontal_rule("*_*"), None);
    }

    #[test]
    fn test_with_other_content() {
        assert_eq!(try_parse_horizontal_rule("*** hello"), None);
        assert_eq!(try_parse_horizontal_rule("---a"), None);
    }

    #[test]
    fn test_empty_line() {
        assert_eq!(try_parse_horizontal_rule(""), None);
        assert_eq!(try_parse_horizontal_rule("   "), None);
    }
}
