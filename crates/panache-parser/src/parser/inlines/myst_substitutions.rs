//! MyST inline substitution parsing (`{{ name }}`).
//!
//! Substitutions reference a key defined in the frontmatter or config. The key
//! (and any Jinja filter expression) is captured verbatim, keeping the CST
//! lossless. The `{{<` shortcode opener is explicitly excluded so the two
//! `{{`-initiated constructs never collide.

use super::sink::InlineSink;
use crate::syntax::SyntaxKind;

/// Try to parse a substitution starting at the opening `{` of `text`.
/// Returns `(total_len, inner_len)` where the inner content begins after `{{`.
pub(crate) fn try_parse_substitution(text: &str) -> Option<(usize, usize)> {
    let bytes = text.as_bytes();
    if bytes.first() != Some(&b'{') || bytes.get(1) != Some(&b'{') {
        return None;
    }
    // `{{<` belongs to the shortcode parser.
    if bytes.get(2) == Some(&b'<') {
        return None;
    }

    let close = text.find("}}")?;
    if close < 2 {
        return None;
    }
    let inner = &text[2..close];
    if inner.trim().is_empty() {
        return None;
    }

    Some((close + 2, close - 2))
}

/// Emit a substitution node from `text` (starting at `{{`) and the parsed
/// `inner_len`.
pub(crate) fn emit_substitution(builder: &mut impl InlineSink, text: &str, inner_len: usize) {
    builder.start_node(SyntaxKind::MYST_SUBSTITUTION.into());
    builder.token(SyntaxKind::TEXT.into(), "{{");
    if inner_len > 0 {
        builder.token(
            SyntaxKind::MYST_SUBSTITUTION_NAME.into(),
            &text[2..2 + inner_len],
        );
    }
    builder.token(SyntaxKind::TEXT.into(), "}}");
    builder.finish_node();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_substitution() {
        let (total, inner) = try_parse_substitution("{{ version }}").unwrap();
        assert_eq!(total, "{{ version }}".len());
        assert_eq!(inner, " version ".len());
    }

    #[test]
    fn substitution_with_filter() {
        let src = "{{ name | upper }}";
        let (total, inner) = try_parse_substitution(src).unwrap();
        assert_eq!(total, src.len());
        assert_eq!(&src[2..2 + inner], " name | upper ");
    }

    #[test]
    fn rejects_shortcode_and_empty() {
        assert!(try_parse_substitution("{{< meta >}}").is_none());
        assert!(try_parse_substitution("{{  }}").is_none());
        assert!(try_parse_substitution("{ not }").is_none());
        assert!(try_parse_substitution("{{ unterminated").is_none());
    }
}
