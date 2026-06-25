//! Admonition opener detection.
//!
//! Recognizes python-markdown admonitions (`!!! type "title"`) and the
//! pymdownx.details collapsible variants (`???` / `???+`). Both open a
//! container whose 4-space-indented body is parsed recursively (see the
//! `Admonition` container, which closes on dedent like a footnote
//! definition). Only the opener line is parsed here; the body is handled
//! by the container machinery.
//!
//! Syntax (mirrors python-markdown / pymdownx):
//!
//! ```text
//! !!! type "Optional title"
//! ??? type "Optional title"     (collapsed)
//! ???+ type "Optional title"    (expanded)
//! ```
//!
//! `type` is one or more space-separated class words (`[\w\-]+`); the
//! first is the admonition type, the rest extra classes. The quoted title
//! is optional. For `!!!` the type is required; for `???`/`???+` it is
//! optional. Anything other than class words plus an optional trailing
//! quoted title disqualifies the line (so ordinary paragraphs starting
//! with `!!!` are left alone).

use crate::options::Extensions;
use crate::parser::utils::helpers::strip_newline;

/// Which marker opened the admonition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AdmonitionMarker {
    /// `!!!` — python-markdown admonition.
    Admonition,
    /// `???` — collapsed pymdownx details.
    DetailsCollapsed,
    /// `???+` — expanded pymdownx details.
    DetailsExpanded,
}

/// A detected admonition opener. All ranges are byte offsets into the
/// original `content` passed to [`try_parse_admonition_open`], so the
/// dispatcher can emit losslessly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AdmonitionOpen {
    pub marker: AdmonitionMarker,
    /// Bytes of leading whitespace before the marker (0..=3).
    pub indent_len: usize,
    /// Length of the marker in bytes (`!!!`/`???` = 3, `???+` = 4).
    pub marker_len: usize,
    /// Range of the type/class words, if present.
    pub type_range: Option<(usize, usize)>,
    /// Range of the quoted title (including the surrounding quotes), if
    /// present.
    pub title_range: Option<(usize, usize)>,
}

fn is_class_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '-'
}

/// Try to detect an admonition opener from a block's first line.
///
/// Returns `None` when neither extension is enabled, when the marker does
/// not match, or when the post-marker content isn't a valid type/title.
pub(crate) fn try_parse_admonition_open(content: &str, ext: &Extensions) -> Option<AdmonitionOpen> {
    if !ext.python_markdown_admonitions && !ext.pymdownx_details {
        return None;
    }

    let (line, _newline) = strip_newline(content);

    // Up to 3 leading spaces, per CommonMark indentation convention.
    let indent_len = line.bytes().take_while(|&b| b == b' ').count();
    if indent_len > 3 {
        return None;
    }
    let rest = &line[indent_len..];
    let rest_bytes = rest.as_bytes();

    let (marker, marker_len) = if rest.starts_with("!!!") {
        if !ext.python_markdown_admonitions || rest_bytes.get(3) == Some(&b'!') {
            return None;
        }
        (AdmonitionMarker::Admonition, 3)
    } else if rest.starts_with("???") {
        if !ext.pymdownx_details || rest_bytes.get(3) == Some(&b'?') {
            return None;
        }
        if rest_bytes.get(3) == Some(&b'+') {
            (AdmonitionMarker::DetailsExpanded, 4)
        } else {
            (AdmonitionMarker::DetailsCollapsed, 3)
        }
    } else {
        return None;
    };

    // Offset of the first byte after the marker, in `content`.
    let after_marker_abs = indent_len + marker_len;
    let after_marker = &line[after_marker_abs..];

    // Skip spaces between the marker and the type/title.
    let lead = after_marker.bytes().take_while(|&b| b == b' ').count();
    let body_abs = after_marker_abs + lead;
    let body = after_marker[lead..].trim_end_matches(' ');

    // A trailing quoted title: `... "title"`.
    let (type_str, type_abs, title_range) = if body.ends_with('"') && body.matches('"').count() >= 2
    {
        let first_q = body.find('"').unwrap();
        // python-markdown requires a space before the opening quote
        // (unless the title is the entire body, i.e. no type).
        if first_q > 0 && body.as_bytes()[first_q - 1] != b' ' {
            return None;
        }
        let type_str = body[..first_q].trim_end();
        let title_abs = body_abs + first_q;
        (
            type_str,
            body_abs,
            Some((title_abs, title_abs + body[first_q..].len())),
        )
    } else {
        (body, body_abs, None)
    };

    // The type may only be class words (and the spaces between them).
    if !type_str.chars().all(|c| is_class_char(c) || c == ' ') {
        return None;
    }

    let type_range = if type_str.is_empty() {
        None
    } else {
        Some((type_abs, type_abs + type_str.len()))
    };

    // python-markdown admonitions require a type; details do not.
    if marker == AdmonitionMarker::Admonition && type_range.is_none() {
        return None;
    }

    Some(AdmonitionOpen {
        marker,
        indent_len,
        marker_len,
        type_range,
        title_range,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn both() -> Extensions {
        // `Extensions::default()` (Pandoc) leaves both admonition flags off.
        Extensions {
            python_markdown_admonitions: true,
            pymdownx_details: true,
            ..Extensions::default()
        }
    }

    fn slice(content: &str, range: (usize, usize)) -> &str {
        &content[range.0..range.1]
    }

    #[test]
    fn basic_admonition() {
        let c = "!!! note\n";
        let a = try_parse_admonition_open(c, &both()).unwrap();
        assert_eq!(a.marker, AdmonitionMarker::Admonition);
        assert_eq!(a.indent_len, 0);
        assert_eq!(a.marker_len, 3);
        assert_eq!(slice(c, a.type_range.unwrap()), "note");
        assert!(a.title_range.is_none());
    }

    #[test]
    fn admonition_with_title() {
        let c = "!!! note \"Heads up\"\n";
        let a = try_parse_admonition_open(c, &both()).unwrap();
        assert_eq!(slice(c, a.type_range.unwrap()), "note");
        assert_eq!(slice(c, a.title_range.unwrap()), "\"Heads up\"");
    }

    #[test]
    fn admonition_with_extra_classes() {
        let c = "!!! danger highlight \"Don't\"\n";
        let a = try_parse_admonition_open(c, &both()).unwrap();
        assert_eq!(slice(c, a.type_range.unwrap()), "danger highlight");
        assert_eq!(slice(c, a.title_range.unwrap()), "\"Don't\"");
    }

    #[test]
    fn admonition_empty_title() {
        let c = "!!! note \"\"\n";
        let a = try_parse_admonition_open(c, &both()).unwrap();
        assert_eq!(slice(c, a.type_range.unwrap()), "note");
        assert_eq!(slice(c, a.title_range.unwrap()), "\"\"");
    }

    #[test]
    fn details_collapsed_and_expanded() {
        let collapsed = try_parse_admonition_open("??? note\n", &both()).unwrap();
        assert_eq!(collapsed.marker, AdmonitionMarker::DetailsCollapsed);
        assert_eq!(collapsed.marker_len, 3);

        let c = "???+ note\n";
        let expanded = try_parse_admonition_open(c, &both()).unwrap();
        assert_eq!(expanded.marker, AdmonitionMarker::DetailsExpanded);
        assert_eq!(expanded.marker_len, 4);
        assert_eq!(slice(c, expanded.type_range.unwrap()), "note");
    }

    #[test]
    fn details_allow_empty_type() {
        let a = try_parse_admonition_open("???\n", &both()).unwrap();
        assert_eq!(a.marker, AdmonitionMarker::DetailsCollapsed);
        assert!(a.type_range.is_none());
    }

    #[test]
    fn admonition_requires_type() {
        assert!(try_parse_admonition_open("!!!\n", &both()).is_none());
        assert!(try_parse_admonition_open("!!! \"only title\"\n", &both()).is_none());
    }

    #[test]
    fn leading_indent_allowed_up_to_three() {
        let c = "   !!! note\n";
        let a = try_parse_admonition_open(c, &both()).unwrap();
        assert_eq!(a.indent_len, 3);
        assert_eq!(slice(c, a.type_range.unwrap()), "note");

        // Four spaces is indented code, not an admonition.
        assert!(try_parse_admonition_open("    !!! note\n", &both()).is_none());
    }

    #[test]
    fn rejects_non_class_content() {
        // Trailing prose after the type is not an admonition.
        assert!(try_parse_admonition_open("!!! warning, this is bad.\n", &both()).is_none());
        assert!(try_parse_admonition_open("!!! note.\n", &both()).is_none());
        // Multi-word all-class is fine (python-markdown treats words as classes).
        assert!(try_parse_admonition_open("!!! note two three\n", &both()).is_some());
    }

    #[test]
    fn four_bangs_is_not_a_marker() {
        assert!(try_parse_admonition_open("!!!! note\n", &both()).is_none());
        assert!(try_parse_admonition_open("???? note\n", &both()).is_none());
    }

    #[test]
    fn gated_on_extension() {
        let only_adm = Extensions {
            python_markdown_admonitions: true,
            ..Extensions::default()
        };
        assert!(try_parse_admonition_open("!!! note\n", &only_adm).is_some());
        assert!(try_parse_admonition_open("??? note\n", &only_adm).is_none());

        let only_det = Extensions {
            pymdownx_details: true,
            ..Extensions::default()
        };
        assert!(try_parse_admonition_open("??? note\n", &only_det).is_some());
        assert!(try_parse_admonition_open("!!! note\n", &only_det).is_none());

        let off = Extensions::default();
        assert!(try_parse_admonition_open("!!! note\n", &off).is_none());
        assert!(try_parse_admonition_open("??? note\n", &off).is_none());
    }

    #[test]
    fn no_space_before_type_is_allowed() {
        let c = "!!!note\n";
        let a = try_parse_admonition_open(c, &both()).unwrap();
        assert_eq!(slice(c, a.type_range.unwrap()), "note");
    }
}
