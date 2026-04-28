//! Inline raw HTML recognizer per CommonMark §6.6 / Pandoc `raw_html`.
//!
//! Matches a single HTML tag (open/close), comment, processing instruction,
//! declaration, or CDATA section starting at byte 0 of `text`. Returns the
//! length in bytes of the matched span, or `None` if the prefix doesn't
//! parse.
//!
//! The recognizer is intentionally byte-level and conservative: when a span
//! looks plausible but doesn't fully close (e.g. unterminated comment or
//! quoted attribute), it returns `None` so the dispatcher falls back to
//! emitting plain text.
//!
//! Backslash escapes and entity references inside the span are *not*
//! decoded — callers are expected to emit the bytes verbatim into the CST,
//! and the renderer must skip the standard text-token escaping for
//! `INLINE_HTML` nodes.

use crate::syntax::SyntaxKind;
use rowan::GreenNodeBuilder;

/// Try to match an inline raw HTML span starting at `text[0]`.
/// Returns the length in bytes consumed, or `None` if no match.
pub fn try_parse_inline_html(text: &str) -> Option<usize> {
    if !text.starts_with('<') {
        return None;
    }
    parse_html_comment(text)
        .or_else(|| parse_cdata(text))
        .or_else(|| parse_declaration(text))
        .or_else(|| parse_processing_instruction(text))
        .or_else(|| parse_close_tag(text))
        .or_else(|| parse_open_tag(text))
}

/// Emit a single `INLINE_HTML` node holding the verbatim span.
pub fn emit_inline_html(builder: &mut GreenNodeBuilder, raw: &str) {
    builder.start_node(SyntaxKind::INLINE_HTML.into());
    builder.token(SyntaxKind::INLINE_HTML_CONTENT.into(), raw);
    builder.finish_node();
}

fn parse_html_comment(text: &str) -> Option<usize> {
    if !text.starts_with("<!--") {
        return None;
    }
    // Special degenerate forms: <!--> and <!--->
    if text.as_bytes().get(4) == Some(&b'>') {
        return Some(5);
    }
    if text.as_bytes().get(4) == Some(&b'-') && text.as_bytes().get(5) == Some(&b'>') {
        return Some(6);
    }
    let after = &text[4..];
    let end = after.find("-->")?;
    Some(4 + end + 3)
}

fn parse_processing_instruction(text: &str) -> Option<usize> {
    if !text.starts_with("<?") {
        return None;
    }
    let after = &text[2..];
    let end = after.find("?>")?;
    Some(2 + end + 2)
}

fn parse_cdata(text: &str) -> Option<usize> {
    const PREFIX: &str = "<![CDATA[";
    if !text.starts_with(PREFIX) {
        return None;
    }
    let after = &text[PREFIX.len()..];
    let end = after.find("]]>")?;
    Some(PREFIX.len() + end + 3)
}

fn parse_declaration(text: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    if !text.starts_with("<!") || bytes.len() < 3 {
        return None;
    }
    if !bytes[2].is_ascii_alphabetic() {
        return None;
    }
    let mut i = 3;
    while i < bytes.len() {
        if bytes[i] == b'>' {
            return Some(i + 1);
        }
        i += 1;
    }
    None
}

fn parse_close_tag(text: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    if !text.starts_with("</") {
        return None;
    }
    let mut i = 2;
    if i >= bytes.len() || !bytes[i].is_ascii_alphabetic() {
        return None;
    }
    i += 1;
    while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'-') {
        i += 1;
    }
    i = skip_ws_with_optional_lf(bytes, i);
    if bytes.get(i) == Some(&b'>') {
        Some(i + 1)
    } else {
        None
    }
}

fn parse_open_tag(text: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    if !text.starts_with('<') {
        return None;
    }
    let mut i = 1;
    if i >= bytes.len() || !bytes[i].is_ascii_alphabetic() {
        return None;
    }
    i += 1;
    while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'-') {
        i += 1;
    }
    while let Some(after) = parse_attribute(bytes, i) {
        i = after;
    }
    i = skip_ws_with_optional_lf(bytes, i);
    if bytes.get(i) == Some(&b'/') {
        i += 1;
    }
    if bytes.get(i) == Some(&b'>') {
        Some(i + 1)
    } else {
        None
    }
}

fn parse_attribute(bytes: &[u8], start: usize) -> Option<usize> {
    let after_ws = skip_ws_required_with_optional_lf(bytes, start)?;
    let mut i = after_ws;
    let first = *bytes.get(i)?;
    if !is_attr_name_start(first) {
        return None;
    }
    i += 1;
    while i < bytes.len() && is_attr_name_cont(bytes[i]) {
        i += 1;
    }
    if let Some(after_value) = parse_attr_value_spec(bytes, i) {
        i = after_value;
    }
    Some(i)
}

fn parse_attr_value_spec(bytes: &[u8], start: usize) -> Option<usize> {
    let i_after_ws1 = skip_ws_with_optional_lf(bytes, start);
    if bytes.get(i_after_ws1) != Some(&b'=') {
        return None;
    }
    let mut i = i_after_ws1 + 1;
    i = skip_ws_with_optional_lf(bytes, i);
    parse_attr_value(bytes, i)
}

fn parse_attr_value(bytes: &[u8], start: usize) -> Option<usize> {
    let q = *bytes.get(start)?;
    match q {
        b'"' | b'\'' => {
            let mut j = start + 1;
            while j < bytes.len() && bytes[j] != q {
                j += 1;
            }
            if j >= bytes.len() {
                return None;
            }
            Some(j + 1)
        }
        _ => {
            let mut j = start;
            while j < bytes.len() {
                let b = bytes[j];
                if matches!(
                    b,
                    b' ' | b'\t' | b'\n' | b'\r' | b'"' | b'\'' | b'=' | b'<' | b'>' | b'`'
                ) {
                    break;
                }
                j += 1;
            }
            if j == start { None } else { Some(j) }
        }
    }
}

fn is_attr_name_start(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_' || b == b':'
}

fn is_attr_name_cont(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'.' || b == b':' || b == b'-'
}

/// Skip "spaces, tabs, and up to one line ending". Returns the new index.
/// Always succeeds (returns at least `start`).
fn skip_ws_with_optional_lf(bytes: &[u8], start: usize) -> usize {
    let mut i = start;
    let mut saw_lf = false;
    while i < bytes.len() {
        match bytes[i] {
            b' ' | b'\t' => i += 1,
            b'\n' => {
                if saw_lf {
                    break;
                }
                saw_lf = true;
                i += 1;
            }
            b'\r' => {
                if saw_lf {
                    break;
                }
                saw_lf = true;
                i += 1;
                if bytes.get(i) == Some(&b'\n') {
                    i += 1;
                }
            }
            _ => break,
        }
    }
    i
}

/// Like `skip_ws_with_optional_lf`, but requires consuming at least one
/// whitespace character (or one line ending).
fn skip_ws_required_with_optional_lf(bytes: &[u8], start: usize) -> Option<usize> {
    let after = skip_ws_with_optional_lf(bytes, start);
    if after == start { None } else { Some(after) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn matches(input: &str, expected_len: usize) {
        assert_eq!(
            try_parse_inline_html(input),
            Some(expected_len),
            "expected {input:?} to match {expected_len}",
        );
    }

    fn no_match(input: &str) {
        assert_eq!(
            try_parse_inline_html(input),
            None,
            "expected no match for {input:?}"
        );
    }

    #[test]
    fn simple_open_tag() {
        matches("<a>", 3);
        matches("<bab>", 5);
        matches("<c2c>", 5);
    }

    #[test]
    fn empty_element() {
        matches("<a/>", 4);
        matches("<b2/>", 5);
        matches("<a  />", 6);
    }

    #[test]
    fn open_tag_with_attrs() {
        matches(r#"<a href="x">"#, r#"<a href="x">"#.len());
        matches(
            r#"<a foo="bar" baz='qux'>"#,
            r#"<a foo="bar" baz='qux'>"#.len(),
        );
        matches(r#"<a foo=bar>"#, r#"<a foo=bar>"#.len());
    }

    #[test]
    fn open_tag_attr_value_spans_lines() {
        matches("<a href=\"foo\nbar\">", "<a href=\"foo\nbar\">".len());
    }

    #[test]
    fn close_tag() {
        matches("</a>", 4);
        matches("</foo >", 7);
    }

    #[test]
    fn comment_forms() {
        matches("<!-->", 5);
        matches("<!--->", 6);
        matches("<!---->", 7);
        matches("<!-- hi -->", 11);
        matches("<!-- a\nb -->", 12);
    }

    #[test]
    fn processing_instruction() {
        matches("<?php $x; ?>", 12);
    }

    #[test]
    fn cdata() {
        matches("<![CDATA[a]]>", 13);
    }

    #[test]
    fn declaration() {
        matches("<!ELEMENT br EMPTY>", 19);
    }

    #[test]
    fn rejects_illegal() {
        no_match("<33>");
        no_match("<__>");
        no_match("<a h*#ref=\"hi\">");
        no_match(r#"<a href="hi'>"#);
        no_match("< a>");
        no_match("<bar/ >");
        no_match("<a href='bar'title=title>");
        no_match("<");
        no_match("<a");
        no_match("<!--");
        no_match("<![CDATA[abc");
    }

    #[test]
    fn rejects_unclosed_quoted_value() {
        no_match("<a href=\"foo");
    }

    #[test]
    fn ignores_non_lt_prefix() {
        no_match("foo");
        no_match("a<b>");
    }
}
