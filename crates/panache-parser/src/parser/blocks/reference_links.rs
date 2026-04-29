//! Reference definition and footnote parsing functions.
//!
//! Reference definitions have the form:
//! ```markdown
//! [label]: url "optional title"
//! [label]: url 'optional title'
//! [label]: url (optional title)
//! [label]: <url> "title"
//! ```
//!
//! Footnote definitions have the form:
//! ```markdown
//! [^id]: Footnote content here.
//!     Can continue on multiple lines
//!     as long as they're indented.
//! ```

/// Try to parse a reference definition starting at the current position.
/// Returns Some((bytes_consumed, label, url, title)) on success.
///
/// `text` may span multiple lines. The destination and title may each be
/// preceded by at most one newline (per CommonMark §4.7). Blank lines
/// terminate the definition: callers should stop the input at the first
/// blank line so the parser cannot cross one.
///
/// Syntax:
/// ```markdown
/// [label]: url "title"
/// [label]: <url> 'title'
/// [label]:
///   url
///   "title"
/// ```
pub fn try_parse_reference_definition(
    text: &str,
) -> Option<(usize, String, String, Option<String>)> {
    try_parse_reference_definition_with_mode(text, true)
}

/// Multimarkdown-flavored variant: tolerates trailing content after the title
/// on the same line (e.g. `[ref]: /url "title" width=20px ...`). Callers in
/// the MMD code path then keep collecting attribute-continuation lines.
pub fn try_parse_reference_definition_lax(
    text: &str,
) -> Option<(usize, String, String, Option<String>)> {
    try_parse_reference_definition_with_mode(text, false)
}

fn try_parse_reference_definition_with_mode(
    text: &str,
    strict_eol: bool,
) -> Option<(usize, String, String, Option<String>)> {
    let leading_spaces = text.chars().take_while(|&c| c == ' ').count();
    if leading_spaces > 3 {
        return None;
    }
    let inner = &text[leading_spaces..];
    let bytes = inner.as_bytes();

    // Must start at beginning of line with [
    if bytes.is_empty() || bytes[0] != b'[' {
        return None;
    }

    // Check if it's a footnote definition [^id]: - not a reference definition
    if bytes.len() >= 2 && bytes[1] == b'^' {
        return None;
    }

    // Find the closing ] for the label. Labels may span lines (CommonMark
    // §4.7) but a blank line inside the label terminates the attempt. We also
    // reject unescaped `[` inside the label per spec.
    let mut pos = 1;
    let mut escape_next = false;

    while pos < bytes.len() {
        if escape_next {
            escape_next = false;
            pos += 1;
            continue;
        }

        match bytes[pos] {
            b'\\' => {
                escape_next = true;
                pos += 1;
            }
            b']' => {
                break;
            }
            b'[' => {
                return None;
            }
            b'\n' | b'\r' => {
                let nl_end =
                    if bytes[pos] == b'\r' && pos + 1 < bytes.len() && bytes[pos + 1] == b'\n' {
                        pos + 2
                    } else {
                        pos + 1
                    };
                let mut probe = nl_end;
                while probe < bytes.len() && matches!(bytes[probe], b' ' | b'\t') {
                    probe += 1;
                }
                if probe >= bytes.len() || bytes[probe] == b'\n' || bytes[probe] == b'\r' {
                    return None;
                }
                pos = nl_end;
            }
            _ => {
                pos += 1;
            }
        }
    }

    if pos >= bytes.len() || bytes[pos] != b']' {
        return None;
    }

    let label = &inner[1..pos];
    if label.trim().is_empty() {
        return None;
    }

    pos += 1; // Skip ]

    // Must be followed by :
    if pos >= bytes.len() || bytes[pos] != b':' {
        return None;
    }
    pos += 1;

    // Skip ws + at most one newline + ws to the URL.
    pos = skip_ws_one_newline(bytes, pos)?;

    // Parse URL
    let url_start = pos;

    let url = if pos < bytes.len() && bytes[pos] == b'<' {
        pos += 1;
        let url_content_start = pos;
        while pos < bytes.len() && bytes[pos] != b'>' && bytes[pos] != b'\n' && bytes[pos] != b'\r'
        {
            pos += 1;
        }
        if pos >= bytes.len() || bytes[pos] != b'>' {
            return None;
        }
        let url = inner[url_content_start..pos].to_string();
        pos += 1; // Skip >
        url
    } else {
        while pos < bytes.len() && !matches!(bytes[pos], b' ' | b'\t' | b'\n' | b'\r') {
            pos += 1;
        }
        if pos == url_start {
            return None;
        }
        inner[url_start..pos].to_string()
    };

    // After URL, try optional title. If a title attempt is malformed but we
    // had to cross a newline to reach it, fall back to "no title, end of URL
    // line" — the next line is then parsed independently (e.g.
    // `[foo]: /url\n"title" ok\n` → ref def `[foo]: /url`, paragraph
    // `"title" ok`).
    let after_url = pos;
    let url_line_end = consume_to_eol(bytes, after_url);
    let url_line_end_lax = if strict_eol {
        url_line_end
    } else {
        Some(consume_to_eol_lax(bytes, after_url))
    };

    let mut title: Option<String> = None;
    let mut end_pos: Option<usize> = None;

    if let Some(title_start) = skip_ws_one_newline(bytes, after_url) {
        let crossed_newline = bytes[after_url..title_start]
            .iter()
            .any(|&b| b == b'\n' || b == b'\r');
        let mut title_pos = title_start;
        match parse_title(inner, bytes, &mut title_pos) {
            Some(Some(t)) => {
                let line_end = if strict_eol {
                    consume_to_eol(bytes, title_pos)
                } else {
                    Some(consume_to_eol_lax(bytes, title_pos))
                };
                if let Some(end) = line_end {
                    title = Some(t);
                    end_pos = Some(end);
                } else if !crossed_newline {
                    return None;
                }
            }
            None => {
                if !crossed_newline {
                    return None;
                }
            }
            Some(None) => {}
        }
    }

    let end = match end_pos {
        Some(p) => p,
        None => url_line_end_lax?,
    };

    Some((leading_spaces + end, label.to_string(), url, title))
}

/// Like `consume_to_eol` but returns the end-of-line position regardless of
/// whether the line had non-whitespace content after the parsed segment.
fn consume_to_eol_lax(bytes: &[u8], mut pos: usize) -> usize {
    while pos < bytes.len() && bytes[pos] != b'\n' && bytes[pos] != b'\r' {
        pos += 1;
    }
    if pos < bytes.len() {
        if bytes[pos] == b'\r' && pos + 1 < bytes.len() && bytes[pos + 1] == b'\n' {
            pos += 2;
        } else {
            pos += 1;
        }
    }
    pos
}

/// Skip space/tab from `pos`, then consume one line ending if present.
/// Returns `None` if non-whitespace is found before the line ending.
fn consume_to_eol(bytes: &[u8], mut pos: usize) -> Option<usize> {
    while pos < bytes.len() && matches!(bytes[pos], b' ' | b'\t') {
        pos += 1;
    }
    if pos >= bytes.len() {
        return Some(pos);
    }
    match bytes[pos] {
        b'\n' => Some(pos + 1),
        b'\r' => {
            if pos + 1 < bytes.len() && bytes[pos + 1] == b'\n' {
                Some(pos + 2)
            } else {
                Some(pos + 1)
            }
        }
        _ => None,
    }
}

/// Skip space/tab and optionally one line ending followed by more space/tab,
/// per the "optional spaces or tabs (including up to one [line ending])" rule
/// in CommonMark §4.7. Returns `None` if a *second* line ending is encountered
/// (i.e. a blank line), which terminates the definition.
fn skip_ws_one_newline(bytes: &[u8], mut pos: usize) -> Option<usize> {
    while pos < bytes.len() && matches!(bytes[pos], b' ' | b'\t') {
        pos += 1;
    }
    if pos < bytes.len() && (bytes[pos] == b'\n' || bytes[pos] == b'\r') {
        if bytes[pos] == b'\r' && pos + 1 < bytes.len() && bytes[pos + 1] == b'\n' {
            pos += 2;
        } else {
            pos += 1;
        }
        while pos < bytes.len() && matches!(bytes[pos], b' ' | b'\t') {
            pos += 1;
        }
        if pos < bytes.len() && (bytes[pos] == b'\n' || bytes[pos] == b'\r') {
            return None;
        }
    }
    Some(pos)
}

pub fn line_is_mmd_link_attribute_continuation(line: &str) -> bool {
    if !(line.starts_with(' ') || line.starts_with('\t')) {
        return false;
    }

    let trimmed = line.trim();
    if trimmed.is_empty() {
        return false;
    }

    let bytes = trimmed.as_bytes();
    let mut pos = 0usize;
    let len = bytes.len();
    let mut saw_pair = false;

    while pos < len {
        // Skip inter-token whitespace.
        while pos < len && (bytes[pos] == b' ' || bytes[pos] == b'\t') {
            pos += 1;
        }
        if pos >= len {
            break;
        }

        // Parse key until '=' or whitespace.
        let key_start = pos;
        while pos < len && bytes[pos] != b'=' && bytes[pos] != b' ' && bytes[pos] != b'\t' {
            pos += 1;
        }
        if pos == key_start || pos >= len || bytes[pos] != b'=' {
            return false;
        }
        pos += 1; // skip '='

        // Parse value (quoted or unquoted), require non-empty value.
        if pos >= len {
            return false;
        }
        if bytes[pos] == b'"' || bytes[pos] == b'\'' {
            let quote = bytes[pos];
            pos += 1;
            let value_start = pos;
            while pos < len && bytes[pos] != quote {
                pos += 1;
            }
            if pos == value_start || pos >= len {
                return false;
            }
            pos += 1; // skip closing quote
        } else {
            let value_start = pos;
            while pos < len && bytes[pos] != b' ' && bytes[pos] != b'\t' {
                pos += 1;
            }
            if pos == value_start {
                return false;
            }
        }

        saw_pair = true;
    }

    saw_pair
}

/// Parse an optional title after the URL.
/// Titles can be in double quotes, single quotes, or parentheses.
/// Returns Some(Some(title)) if title found, Some(None) if no title, None if malformed.
fn parse_title(text: &str, bytes: &[u8], pos: &mut usize) -> Option<Option<String>> {
    let base_pos = *pos;

    // Skip whitespace (including newlines for multi-line titles)
    while *pos < bytes.len() && matches!(bytes[*pos], b' ' | b'\t' | b'\n' | b'\r') {
        *pos += 1;
    }

    // Check if there's a title
    if *pos >= bytes.len() {
        return Some(None);
    }

    let quote_char = bytes[*pos];
    if !matches!(quote_char, b'"' | b'\'' | b'(') {
        // No title, that's okay
        *pos = base_pos; // Reset position
        return Some(None);
    }

    let closing_char = if quote_char == b'(' { b')' } else { quote_char };

    *pos += 1; // Skip opening quote
    let title_start = *pos;

    // Find closing quote
    let mut escape_next = false;
    while *pos < bytes.len() {
        if escape_next {
            escape_next = false;
            *pos += 1;
            continue;
        }

        match bytes[*pos] {
            b'\\' => {
                escape_next = true;
                *pos += 1;
            }
            c if c == closing_char => {
                let title_end = *pos;
                *pos += 1; // Skip closing quote

                // Skip trailing whitespace to end of line
                while *pos < bytes.len() && matches!(bytes[*pos], b' ' | b'\t') {
                    *pos += 1;
                }

                // Extract title from the original text using correct indices
                let title = text[title_start..title_end].to_string();
                return Some(Some(title));
            }
            b'\n' if quote_char == b'(' => {
                // Parenthetical titles can span lines
                *pos += 1;
            }
            _ => {
                *pos += 1;
            }
        }
    }

    // No closing quote found
    None
}

/// Try to parse just the footnote marker [^id]: from a line.
/// Returns Some((id, content_start_col)) if the line starts with a footnote marker.
///
/// Syntax:
/// ```markdown
/// [^id]: Footnote content.
/// ```
pub fn try_parse_footnote_marker(line: &str) -> Option<(String, usize)> {
    let bytes = line.as_bytes();

    // Must start with [^
    if bytes.len() < 4 || bytes[0] != b'[' || bytes[1] != b'^' {
        return None;
    }

    // Find the closing ] for the ID
    let mut pos = 2;
    while pos < bytes.len() && bytes[pos] != b']' && bytes[pos] != b'\n' && bytes[pos] != b'\r' {
        pos += 1;
    }

    if pos >= bytes.len() || bytes[pos] != b']' {
        return None;
    }

    let id = &line[2..pos];
    if id.is_empty() {
        return None;
    }

    pos += 1; // Skip ]

    // Must be followed by :
    if pos >= bytes.len() || bytes[pos] != b':' {
        return None;
    }
    pos += 1;

    // Skip spaces/tabs until content (or end of line)
    while pos < bytes.len() && matches!(bytes[pos], b' ' | b'\t') {
        pos += 1;
    }

    Some((id.to_string(), pos))
}

#[cfg(test)]
mod tests {
    use super::{line_is_mmd_link_attribute_continuation, try_parse_reference_definition};
    use crate::syntax::SyntaxKind;

    #[test]
    fn test_footnote_definition_body_layout_is_lossless() {
        let input = "[^note-on-refs]:\n    Note that if `--file-scope` is used,\n";
        let tree = crate::parse(input, Some(crate::ParserOptions::default()));
        assert_eq!(tree.text().to_string(), input);
    }

    #[test]
    fn test_footnote_definition_marker_emits_structural_tokens() {
        let input = "[^note-on-refs]: body\n";
        let tree = crate::parse(input, Some(crate::ParserOptions::default()));
        let def = tree
            .descendants()
            .find(|n| n.kind() == SyntaxKind::FOOTNOTE_DEFINITION)
            .expect("footnote definition");
        let token_kinds: Vec<_> = def
            .children_with_tokens()
            .filter_map(|e| e.into_token())
            .map(|t| t.kind())
            .collect();
        assert!(token_kinds.contains(&SyntaxKind::FOOTNOTE_LABEL_START));
        assert!(token_kinds.contains(&SyntaxKind::FOOTNOTE_LABEL_ID));
        assert!(token_kinds.contains(&SyntaxKind::FOOTNOTE_LABEL_END));
        assert!(token_kinds.contains(&SyntaxKind::FOOTNOTE_LABEL_COLON));
    }

    #[test]
    fn footnote_multiline_dollar_math_parses_as_display_math_not_tex_block() {
        let input = "[^note]: Intro line before math:\n    $$\n    \\begin{aligned} a &= b \\\\ c &= d \\end{aligned}\n    $$\n";
        let tree = crate::parse(input, Some(crate::ParserOptions::default()));

        let def = tree
            .descendants()
            .find(|n| n.kind() == SyntaxKind::FOOTNOTE_DEFINITION)
            .expect("footnote definition");

        let has_display_math = def
            .descendants()
            .any(|n| n.kind() == SyntaxKind::DISPLAY_MATH);
        let has_tex_block = def.descendants().any(|n| n.kind() == SyntaxKind::TEX_BLOCK);

        assert!(
            has_display_math,
            "Expected DISPLAY_MATH in footnote definition, got:\n{}",
            tree
        );
        assert!(
            !has_tex_block,
            "Did not expect TEX_BLOCK in footnote definition for $$...$$ math, got:\n{}",
            tree
        );
    }

    #[test]
    fn test_reference_definition_with_up_to_three_leading_spaces() {
        assert!(try_parse_reference_definition("   [foo]: #bar").is_some());
        assert!(try_parse_reference_definition("    [foo]: #bar").is_none());
    }

    #[test]
    fn mmd_link_attribute_continuation_detects_valid_tokens() {
        assert!(line_is_mmd_link_attribute_continuation(
            "    width=20px height=30px id=myId"
        ));
        assert!(line_is_mmd_link_attribute_continuation(
            "\tclass=\"myClass1 myClass2\""
        ));
    }

    #[test]
    fn mmd_link_attribute_continuation_rejects_non_attribute_lines() {
        assert!(!line_is_mmd_link_attribute_continuation(
            "not-indented width=20px"
        ));
        assert!(!line_is_mmd_link_attribute_continuation(
            "    not-an-attr token"
        ));
    }
}
