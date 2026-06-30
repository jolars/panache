//! MyST block-level leaf recognizers (targets, comments, block breaks).
//!
//! These are the three constructs `myst-parser` registers via
//! `myst_block_plugin`:
//!
//! - Targets are anchor lines of the form `(label)=` that precede a block and
//!   make it cross-referenceable.
//! - Comments are lines beginning with `%`; the whole line is a comment.
//! - Block breaks are lines of 3+ `+` markers (`+++`), optionally carrying
//!   trailing cell metadata; they separate notebook-style cells.
//!
//! All are single-line leaf blocks; only structural recognition lives here,
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

/// A detected `+++` block break. Offsets are byte indices into the line passed
/// to [`try_parse_block_break`] (newline excluded).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct BlockBreak {
    /// Byte length of leading whitespace before the first `+`.
    pub indent_len: usize,
    /// Byte index just past the last `+` in the marker run. The marker run
    /// itself (`content[indent_len..marker_end]`) may contain interspersed
    /// spaces or tabs, mirroring markdown-it's `myst_block_break` rule.
    pub marker_end: usize,
    /// Range of the trimmed trailing metadata, or an empty range
    /// (`(marker_end, marker_end)`) when there is none.
    pub metadata: (usize, usize),
}

/// Try to parse a `+++` block break line.
///
/// After up to 3 leading spaces the line must begin with `+` and contain at
/// least three `+` markers (spaces and tabs may be interspersed), matching
/// markdown-it's `myst_block_break` block rule. Any non-marker, non-space
/// character ends the marker run; the remainder (trimmed) is cell metadata.
pub(crate) fn try_parse_block_break(content: &str) -> Option<BlockBreak> {
    let (line, _newline) = strip_newline(content);

    let indent_len = line.bytes().take(4).take_while(|&b| b == b' ').count();
    if indent_len > 3 {
        return None;
    }
    let rest = line.as_bytes();
    if rest.get(indent_len) != Some(&b'+') {
        return None;
    }

    let mut count = 0usize;
    let mut last_plus = indent_len;
    let mut pos = indent_len;
    while pos < rest.len() {
        match rest[pos] {
            b'+' => {
                count += 1;
                last_plus = pos + 1;
            }
            b' ' | b'\t' => {}
            _ => break,
        }
        pos += 1;
    }
    if count < 3 {
        return None;
    }

    let marker_end = last_plus;
    let after = &line[marker_end..];
    let trimmed = after.trim();
    let metadata = if trimmed.is_empty() {
        (marker_end, marker_end)
    } else {
        let lead = after.len() - after.trim_start().len();
        let start = marker_end + lead;
        (start, start + trimmed.len())
    };

    Some(BlockBreak {
        indent_len,
        marker_end,
        metadata,
    })
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

    #[test]
    fn basic_block_break() {
        let b = try_parse_block_break("+++\n").unwrap();
        assert_eq!(b.indent_len, 0);
        assert_eq!(b.marker_end, 3);
        assert_eq!(b.metadata, (3, 3)); // empty
    }

    #[test]
    fn block_break_with_metadata() {
        let src = "+++ {\"key\": 1}\n";
        let b = try_parse_block_break(src).unwrap();
        assert_eq!(b.marker_end, 3);
        assert_eq!(&src[b.metadata.0..b.metadata.1], "{\"key\": 1}");
    }

    #[test]
    fn block_break_allows_indent_and_spaced_markers() {
        let src = "  + + +\n";
        let b = try_parse_block_break(src).unwrap();
        assert_eq!(b.indent_len, 2);
        // marker run spans the last `+`
        assert_eq!(&src[b.indent_len..b.marker_end], "+ + +");
        assert_eq!(b.metadata, (b.marker_end, b.marker_end));
    }

    #[test]
    fn rejects_non_block_breaks() {
        assert!(try_parse_block_break("++\n").is_none()); // only two markers
        assert!(try_parse_block_break("+ +\n").is_none()); // only two markers
        assert!(try_parse_block_break("    +++\n").is_none()); // 4-space indent
        assert!(try_parse_block_break("text\n").is_none());
        assert!(try_parse_block_break("+++text\n").is_some()); // marker run, then meta
    }
}
