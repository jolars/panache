//! MyST inline role parsing.
//!
//! A role is `` {name}`content` ``, e.g. `` {math}`a^2` `` or
//! `` {ref}`Text <target>` ``. The name is brace-delimited and immediately
//! followed by a backtick-delimited payload that behaves like a code span: the
//! content runs to the next backtick run of the same length, so backticks can
//! appear inside the content by using a longer delimiter
//! (`` {x}``a`b`` ``). The content is captured verbatim (roles interpret it
//! themselves), keeping the CST lossless.

use super::sink::InlineSink;
use crate::syntax::SyntaxKind;

/// A detected role, with byte offsets into the `text` passed to
/// [`try_parse_role`] (which must start at the opening `{`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Role {
    /// Byte index just past the closing `}` of the name (start of the fence).
    pub name_end: usize,
    /// Number of backticks delimiting the content.
    pub fence_len: usize,
    /// Byte length of the content between the backtick runs.
    pub content_len: usize,
    /// Total bytes consumed by the role.
    pub total_len: usize,
}

fn is_role_name_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '_' | '-' | '+' | ':' | '.')
}

/// Try to parse a MyST role starting at the opening `{` of `text`.
pub(crate) fn try_parse_role(text: &str) -> Option<Role> {
    let bytes = text.as_bytes();
    if bytes.first() != Some(&b'{') {
        return None;
    }

    let close_brace = text.find('}')?;
    let name_inner = &text[1..close_brace];
    if name_inner.is_empty() || !name_inner.chars().all(is_role_name_char) {
        return None;
    }

    let name_end = close_brace + 1;
    let after = &text[name_end..];
    let fence_len = after.bytes().take_while(|&b| b == b'`').count();
    if fence_len == 0 {
        return None;
    }

    // Scan for a closing backtick run of exactly `fence_len`, treating runs of a
    // different length as content (code-span semantics).
    let content_start = name_end + fence_len;
    let rest = bytes.get(content_start..)?;
    let mut i = 0;
    while i < rest.len() {
        if rest[i] == b'`' {
            let run = rest[i..].iter().take_while(|&&b| b == b'`').count();
            if run == fence_len {
                return Some(Role {
                    name_end,
                    fence_len,
                    content_len: i,
                    total_len: content_start + i + fence_len,
                });
            }
            i += run;
        } else {
            i += 1;
        }
    }

    None
}

/// Emit a role node from `text` (starting at the opening `{`) and a parsed
/// [`Role`].
pub(crate) fn emit_role(builder: &mut impl InlineSink, text: &str, role: Role) {
    builder.start_node(SyntaxKind::MYST_ROLE.into());

    builder.token(SyntaxKind::MYST_ROLE_NAME.into(), &text[..role.name_end]);

    let fence_end = role.name_end + role.fence_len;
    builder.token(
        SyntaxKind::MYST_ROLE_MARKER.into(),
        &text[role.name_end..fence_end],
    );

    let content_end = fence_end + role.content_len;
    builder.start_node(SyntaxKind::MYST_ROLE_CONTENT.into());
    if role.content_len > 0 {
        builder.token(SyntaxKind::TEXT.into(), &text[fence_end..content_end]);
    }
    builder.finish_node(); // MYST_ROLE_CONTENT

    builder.token(
        SyntaxKind::MYST_ROLE_MARKER.into(),
        &text[content_end..role.total_len],
    );

    builder.finish_node(); // MYST_ROLE
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_role() {
        let r = try_parse_role("{math}`a^2`").unwrap();
        assert_eq!(r.name_end, "{math}".len());
        assert_eq!(r.fence_len, 1);
        assert_eq!(r.content_len, "a^2".len());
        assert_eq!(r.total_len, "{math}`a^2`".len());
    }

    #[test]
    fn role_with_target() {
        let src = "{ref}`Text <target>`";
        let r = try_parse_role(src).unwrap();
        assert_eq!(r.total_len, src.len());
        let content = &src[r.name_end + r.fence_len..r.name_end + r.fence_len + r.content_len];
        assert_eq!(content, "Text <target>");
    }

    #[test]
    fn double_backtick_allows_inner_backtick() {
        let src = "{x}``a`b``";
        let r = try_parse_role(src).unwrap();
        assert_eq!(r.fence_len, 2);
        assert_eq!(r.content_len, "a`b".len());
        assert_eq!(r.total_len, src.len());
    }

    #[test]
    fn rejects_non_role() {
        // No backtick after the name.
        assert!(try_parse_role("{ref} text").is_none());
        // Empty name.
        assert!(try_parse_role("{}`x`").is_none());
        // Does not start with `{`.
        assert!(try_parse_role("ref}`x`").is_none());
        // Unterminated content.
        assert!(try_parse_role("{math}`a^2").is_none());
    }

    #[test]
    fn domain_qualified_name() {
        let r = try_parse_role("{py:func}`open`").unwrap();
        assert_eq!(r.name_end, "{py:func}".len());
    }
}
