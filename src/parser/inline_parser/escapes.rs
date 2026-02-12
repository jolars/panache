/// Parsing for backslash escape sequences
///
/// Per Pandoc spec (all_symbols_escapable extension):
/// - Any punctuation or space preceded by backslash is treated literally
/// - Backslash-escaped space = nonbreaking space
/// - Backslash-escaped newline = hard line break
/// - Does NOT work in verbatim contexts (code blocks, code spans)
use crate::syntax::SyntaxKind;
use rowan::GreenNodeBuilder;

/// Check if a character can be escaped according to Pandoc's all_symbols_escapable
fn is_escapable(ch: char) -> bool {
    // Per spec: any punctuation or space character
    ch.is_ascii_punctuation() || ch.is_whitespace()
}

/// Try to parse a backslash escape sequence starting at the current position.
/// Returns (total_len, escaped_char, escape_type) or None if not an escape.
pub fn try_parse_escape(text: &str) -> Option<(usize, char, EscapeType)> {
    if !text.starts_with('\\') {
        return None;
    }

    if text.len() < 2 {
        // Backslash at end of input - not an escape
        return None;
    }

    let next_char = text[1..].chars().next()?;

    if !is_escapable(next_char) {
        // Not an escapable character
        return None;
    }

    let escape_type = match next_char {
        ' ' => EscapeType::NonbreakingSpace,
        '\n' => EscapeType::HardLineBreak,
        _ => EscapeType::Literal,
    };

    let total_len = 1 + next_char.len_utf8(); // backslash + character
    Some((total_len, next_char, escape_type))
}

#[derive(Debug, PartialEq, Eq)]
pub enum EscapeType {
    Literal,          // Regular escaped character like \*
    NonbreakingSpace, // \<space>
    HardLineBreak,    // \<newline>
}

/// Emit an escape sequence to the builder.
pub fn emit_escape(builder: &mut GreenNodeBuilder, ch: char, escape_type: EscapeType) {
    match escape_type {
        EscapeType::NonbreakingSpace => {
            // Emit as a special nonbreaking space token
            builder.token(SyntaxKind::NonbreakingSpace.into(), "\u{00A0}");
        }
        EscapeType::HardLineBreak => {
            // Emit as a special hard line break token
            builder.token(SyntaxKind::HardLineBreak.into(), "\n");
        }
        EscapeType::Literal => {
            // Emit the full escape sequence (backslash + character) for losslessness
            let mut s = String::new();
            s.push('\\');
            s.push(ch);
            builder.token(SyntaxKind::EscapedChar.into(), &s);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_escape_asterisk() {
        let result = try_parse_escape(r"\*");
        assert_eq!(result, Some((2, '*', EscapeType::Literal)));
    }

    #[test]
    fn test_escape_backtick() {
        let result = try_parse_escape(r"\`");
        assert_eq!(result, Some((2, '`', EscapeType::Literal)));
    }

    #[test]
    fn test_escape_space() {
        let result = try_parse_escape(r"\ ");
        assert_eq!(result, Some((2, ' ', EscapeType::NonbreakingSpace)));
    }

    #[test]
    fn test_escape_newline() {
        let result = try_parse_escape("\\\n");
        assert_eq!(result, Some((2, '\n', EscapeType::HardLineBreak)));
    }

    #[test]
    fn test_escape_bracket() {
        let result = try_parse_escape(r"\[");
        assert_eq!(result, Some((2, '[', EscapeType::Literal)));
    }

    #[test]
    fn test_escape_dollar() {
        let result = try_parse_escape(r"\$");
        assert_eq!(result, Some((2, '$', EscapeType::Literal)));
    }

    #[test]
    fn test_not_escape_letter() {
        // Letters cannot be escaped in Pandoc
        let result = try_parse_escape(r"\a");
        assert_eq!(result, None);
    }

    #[test]
    fn test_not_escape_at_end() {
        let result = try_parse_escape(r"\");
        assert_eq!(result, None);
    }

    #[test]
    fn test_escape_all_punctuation() {
        // Test the common Markdown punctuation
        for ch in r#"`*_{}[]()>#+-.!"#.chars() {
            let input = format!(r"\{}", ch);
            let result = try_parse_escape(&input);
            assert!(result.is_some(), "Should escape '{}'", ch);
            assert_eq!(result.unwrap().1, ch);
        }
    }

    #[test]
    fn test_is_escapable() {
        // Punctuation
        assert!(is_escapable('*'));
        assert!(is_escapable('`'));
        assert!(is_escapable('['));
        assert!(is_escapable('!'));

        // Space/whitespace
        assert!(is_escapable(' '));
        assert!(is_escapable('\n'));
        assert!(is_escapable('\t'));

        // Not escapable
        assert!(!is_escapable('a'));
        assert!(!is_escapable('Z'));
        assert!(!is_escapable('5'));
    }
}
