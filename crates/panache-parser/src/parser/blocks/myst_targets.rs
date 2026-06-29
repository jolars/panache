//! MyST target and line-comment detection (block level).
//!
//! - Targets are anchor lines of the form `(label)=` that precede a block and
//!   make it cross-referenceable.
//! - Comments are lines beginning with `%`; the whole line is a comment.
//!
//! Both are single-line leaf blocks; only structural recognition lives here,
//! emission is handled by their dispatcher parsers.

use crate::parser::utils::helpers::strip_newline;

/// A detected `(label)=` target. Offsets are byte indices into the line passed
/// to [`try_parse_target`] (newline excluded).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Target {
    /// Byte length of leading whitespace before `(`.
    pub indent_len: usize,
    /// Range of the label, between `(` and `)=`.
    pub label: (usize, usize),
    /// Byte index just past the `)=` marker.
    pub marker_end: usize,
}

/// Try to parse a `(label)=` target line.
///
/// The line must be exactly `(<label>)=` after up to 3 leading spaces, with
/// only trailing whitespace allowed afterward. The label must be non-empty.
pub(crate) fn try_parse_target(content: &str) -> Option<Target> {
    let (line, _newline) = strip_newline(content);

    let indent_len = line.bytes().take_while(|&b| b == b' ').count();
    if indent_len > 3 {
        return None;
    }
    let rest = &line[indent_len..];
    if !rest.starts_with('(') {
        return None;
    }

    // The marker is the LAST `)=` on the line so labels may contain `)`.
    let close = rest.rfind(")=")?;
    if close < 2 {
        return None; // empty label `()=`
    }
    let after = &rest[close + 2..];
    if !after.trim().is_empty() {
        return None;
    }

    let label_start = indent_len + 1;
    let label_end = indent_len + close;
    Some(Target {
        indent_len,
        label: (label_start, label_end),
        marker_end: indent_len + close + 2,
    })
}

/// Whether `content` is a MyST line comment (`%` after up to 3 leading spaces).
pub(crate) fn is_comment_line(content: &str) -> bool {
    let (line, _newline) = strip_newline(content);
    let indent_len = line.bytes().take(4).take_while(|&b| b == b' ').count();
    indent_len <= 3 && line[indent_len..].starts_with('%')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_target() {
        let t = try_parse_target("(my-label)=\n").unwrap();
        assert_eq!(t.indent_len, 0);
        assert_eq!(t.label, (1, 9));
        assert_eq!(t.marker_end, 11);
    }

    #[test]
    fn target_allows_trailing_whitespace_and_indent() {
        let t = try_parse_target("  (sec:intro)=  \n").unwrap();
        assert_eq!(t.indent_len, 2);
        // label between `(` and `)=`
        let src = "  (sec:intro)=  \n";
        assert_eq!(&src[t.label.0..t.label.1], "sec:intro");
    }

    #[test]
    fn rejects_non_targets() {
        assert!(try_parse_target("()=\n").is_none()); // empty label
        assert!(try_parse_target("(label)= trailing\n").is_none()); // trailing text
        assert!(try_parse_target("not a target\n").is_none());
        assert!(try_parse_target("    (x)=\n").is_none()); // 4-space indent
    }

    #[test]
    fn comment_detection() {
        assert!(is_comment_line("% a comment\n"));
        assert!(is_comment_line("   % indented comment\n"));
        assert!(is_comment_line("%\n"));
        assert!(!is_comment_line("    % four-space indent\n"));
        assert!(!is_comment_line("not % a comment\n"));
    }
}
