//! MyST directive opener/closer detection.
//!
//! MyST directives are fence-delimited blocks whose body is parsed recursively
//! as markdown:
//!
//! ````text
//! ```{note}
//! Body parsed as markdown.
//! ```
//! ````
//!
//! The backtick form (and its `~~~` tilde variant) is always available under
//! the `myst_directives` extension. The colon form (`:::{note}`) mirrors
//! `myst-parser`'s opt-in `colon_fence` extension and is gated separately on
//! `myst_colon_fence`.
//!
//! Only the opener line is parsed here; the body and the matching closer are
//! handled by the container machinery (see `Container::MystDirective`). The
//! closer must repeat the opener's fence character at least as many times as
//! the opener, which is why the container records the fence char and count.

use crate::options::Extensions;
use crate::parser::utils::helpers::{strip_leading_spaces, strip_newline};

/// A detected MyST directive opener.
///
/// The opener line is laid out as
/// `[indent][fence][name]([space][argument])?[newline]`, so emission can slice
/// the line directly from these lengths without re-scanning.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DirectiveOpen {
    /// Fence character: `` b'`' ``, `b'~'`, or `b':'`.
    pub fence_char: u8,
    /// Number of fence characters in the opener (>= 3).
    pub fence_count: usize,
    /// Byte length of leading whitespace before the fence (0..=3).
    pub indent_len: usize,
    /// Byte length of the `{name}` token, braces included.
    pub name_len: usize,
}

fn is_name_char(c: char) -> bool {
    // Directive names are identifiers, optionally domain-qualified
    // (`py:function`) or versioned (`tab-set+`). Keep this permissive but
    // anchored to identifier-ish characters so ordinary code fences such as
    // ```` ```{=html} ```` (leading `=`) fall through to the code-block parser.
    c.is_alphanumeric() || matches!(c, '_' | '-' | '+' | ':' | '.')
}

/// Try to detect a MyST directive opener from a block's first line.
///
/// Returns `None` when the extension is off, when the line is not a
/// `{name}`-tagged fence, or when the directive name is empty/invalid (so the
/// line falls through to the ordinary fenced-code parser).
pub(crate) fn try_parse_directive_open(content: &str, ext: &Extensions) -> Option<DirectiveOpen> {
    if !ext.myst_directives {
        return None;
    }

    let (line, _newline) = strip_newline(content);

    // Up to 3 leading spaces (4+ is indented code).
    let indent_len = line.bytes().take_while(|&b| b == b' ').count();
    if indent_len > 3 {
        return None;
    }
    let rest = &line[indent_len..];
    let fence_char = *rest.as_bytes().first()?;
    if !matches!(fence_char, b'`' | b'~' | b':') {
        return None;
    }
    if fence_char == b':' && !ext.myst_colon_fence {
        return None;
    }

    let fence_count = rest.bytes().take_while(|&b| b == fence_char).count();
    if fence_count < 3 {
        return None;
    }

    // The directive name must immediately follow the fence: `{name}`.
    let after_fence = &rest[fence_count..];
    if !after_fence.starts_with('{') {
        return None;
    }
    let close_brace = after_fence.find('}')?;
    let name_inner = &after_fence[1..close_brace];
    if name_inner.is_empty() || !name_inner.chars().all(is_name_char) {
        return None;
    }

    Some(DirectiveOpen {
        fence_char,
        fence_count,
        indent_len,
        name_len: close_brace + 1,
    })
}

/// Whether `content` closes a MyST directive opened with `fence_char` repeated
/// `open_count` times.
///
/// A closer is a line of at least `open_count` repeats of `fence_char` (after
/// up to 3 leading spaces) followed only by trailing whitespace.
pub(crate) fn is_directive_closing_fence(content: &str, fence_char: u8, open_count: usize) -> bool {
    let trimmed = strip_leading_spaces(content);
    let count = trimmed.bytes().take_while(|&b| b == fence_char).count();
    if count < open_count {
        return false;
    }
    trimmed[count..].trim().is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ext_backtick() -> Extensions {
        Extensions {
            myst_directives: true,
            ..Extensions::for_flavor(crate::options::Flavor::Myst)
        }
    }

    fn ext_colon() -> Extensions {
        Extensions {
            myst_directives: true,
            myst_colon_fence: true,
            ..Extensions::for_flavor(crate::options::Flavor::Myst)
        }
    }

    #[test]
    fn basic_backtick_directive() {
        let d = try_parse_directive_open("```{note}\n", &ext_backtick()).unwrap();
        assert_eq!(d.fence_char, b'`');
        assert_eq!(d.fence_count, 3);
        assert_eq!(d.indent_len, 0);
        assert_eq!(d.name_len, "{note}".len());
    }

    #[test]
    fn domain_qualified_name() {
        let d = try_parse_directive_open("````{py:function}\n", &ext_backtick()).unwrap();
        assert_eq!(d.fence_count, 4);
        assert_eq!(d.name_len, "{py:function}".len());
    }

    #[test]
    fn plain_code_fence_is_not_a_directive() {
        assert!(try_parse_directive_open("```python\n", &ext_backtick()).is_none());
        assert!(try_parse_directive_open("```\n", &ext_backtick()).is_none());
        // Raw blocks keep their leading `=` and must fall through.
        assert!(try_parse_directive_open("```{=html}\n", &ext_backtick()).is_none());
        // Empty braces are not a directive name.
        assert!(try_parse_directive_open("```{}\n", &ext_backtick()).is_none());
    }

    #[test]
    fn colon_fence_gated_on_extension() {
        assert!(try_parse_directive_open(":::{note}\n", &ext_backtick()).is_none());
        let d = try_parse_directive_open(":::{note}\n", &ext_colon()).unwrap();
        assert_eq!(d.fence_char, b':');
        assert_eq!(d.fence_count, 3);
    }

    #[test]
    fn gated_on_directives_extension() {
        let off = Extensions::for_flavor(crate::options::Flavor::CommonMark);
        assert!(try_parse_directive_open("```{note}\n", &off).is_none());
    }

    #[test]
    fn indented_four_spaces_is_not_a_directive() {
        assert!(try_parse_directive_open("    ```{note}\n", &ext_backtick()).is_none());
        let d = try_parse_directive_open("   ```{note}\n", &ext_backtick()).unwrap();
        assert_eq!(d.indent_len, 3);
    }

    #[test]
    fn closing_fence_matches_char_and_count() {
        assert!(is_directive_closing_fence("```\n", b'`', 3));
        assert!(is_directive_closing_fence("````\n", b'`', 3));
        assert!(is_directive_closing_fence("   ```  \n", b'`', 3));
        // Too few backticks does not close a 4-backtick directive.
        assert!(!is_directive_closing_fence("```\n", b'`', 4));
        // Trailing content is not a bare closer.
        assert!(!is_directive_closing_fence("```python\n", b'`', 3));
        // Wrong fence character.
        assert!(!is_directive_closing_fence(":::\n", b'`', 3));
        assert!(is_directive_closing_fence(":::\n", b':', 3));
    }
}
