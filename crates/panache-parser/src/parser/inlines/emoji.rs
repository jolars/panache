use crate::syntax::SyntaxKind;
use rowan::GreenNodeBuilder;

/// Try to parse a textual emoji alias like `:smile:`.
///
/// Returns `(total_len, alias)` where alias excludes the surrounding colons.
pub(crate) fn try_parse_emoji(text: &str) -> Option<(usize, &str)> {
    let bytes = text.as_bytes();
    if bytes.len() < 3 || bytes[0] != b':' {
        return None;
    }

    let mut end = 1;
    while end < bytes.len() {
        let ch = bytes[end] as char;
        if ch == ':' {
            break;
        }
        if !ch.is_ascii_alphanumeric() && ch != '_' && ch != '+' && ch != '-' {
            return None;
        }
        end += 1;
    }

    if end >= bytes.len() || bytes[end] != b':' || end == 1 {
        return None;
    }

    // Avoid matching as emoji when immediately followed by word characters.
    if end + 1 < bytes.len() {
        let next = bytes[end + 1] as char;
        if next.is_ascii_alphanumeric() || next == '_' {
            return None;
        }
    }

    Some((end + 1, &text[1..end]))
}

pub(crate) fn emit_emoji(builder: &mut GreenNodeBuilder, raw: &str) {
    builder.start_node(SyntaxKind::EMOJI.into());
    builder.token(SyntaxKind::TEXT.into(), raw);
    builder.finish_node();
}

#[cfg(test)]
mod tests {
    use super::try_parse_emoji;

    #[test]
    fn parses_simple_alias() {
        let parsed = try_parse_emoji(":smile:");
        assert_eq!(parsed, Some((7, "smile")));
    }

    #[test]
    fn parses_plus_one_alias() {
        let parsed = try_parse_emoji(":+1:");
        assert_eq!(parsed, Some((4, "+1")));
    }

    #[test]
    fn rejects_spaces_inside_alias() {
        assert!(try_parse_emoji(":not valid:").is_none());
    }
}
