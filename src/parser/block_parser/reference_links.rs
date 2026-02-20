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
/// Returns Some((length, label, url, title)) if successful.
///
/// Syntax:
/// ```markdown
/// [label]: url "title"
/// [label]: <url> 'title'
/// [label]: url
///          (title on next line)
/// ```
pub fn try_parse_reference_definition(
    text: &str,
) -> Option<(usize, String, String, Option<String>)> {
    let bytes = text.as_bytes();

    // Must start at beginning of line with [
    if bytes.is_empty() || bytes[0] != b'[' {
        return None;
    }

    // Check if it's a footnote definition [^id]: - not a reference definition
    if bytes.len() >= 2 && bytes[1] == b'^' {
        return None;
    }

    // Find the closing ] for the label
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
            b'\n' => {
                // Labels can't span lines
                return None;
            }
            _ => {
                pos += 1;
            }
        }
    }

    if pos >= bytes.len() || bytes[pos] != b']' {
        return None;
    }

    let label = &text[1..pos];
    if label.is_empty() {
        return None;
    }

    pos += 1; // Skip ]

    // Must be followed by :
    if pos >= bytes.len() || bytes[pos] != b':' {
        return None;
    }
    pos += 1;

    // Skip whitespace
    while pos < bytes.len() && matches!(bytes[pos], b' ' | b'\t') {
        pos += 1;
    }

    // Parse URL
    let url_start = pos;
    let url_end;

    // Check for angle-bracketed URL <url>
    if pos < bytes.len() && bytes[pos] == b'<' {
        pos += 1;
        let url_content_start = pos;
        // Find closing >
        while pos < bytes.len() && bytes[pos] != b'>' && bytes[pos] != b'\n' && bytes[pos] != b'\r'
        {
            pos += 1;
        }
        if pos >= bytes.len() || bytes[pos] != b'>' {
            return None;
        }
        url_end = pos;
        let url = text[url_content_start..url_end].to_string();
        pos += 1; // Skip >

        // Parse optional title
        let title = parse_title(text, bytes, &mut pos)?;

        Some((pos, label.to_string(), url, title))
    } else {
        // Parse unbracketed URL (until whitespace or newline)
        while pos < bytes.len() && !matches!(bytes[pos], b' ' | b'\t' | b'\n' | b'\r') {
            pos += 1;
        }

        url_end = pos;
        if url_start == url_end {
            return None; // No URL found
        }

        let url = text[url_start..url_end].to_string();

        // Parse optional title
        let title = parse_title(text, bytes, &mut pos)?;

        Some((pos, label.to_string(), url, title))
    }
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

/// Try to parse a complete footnote definition from the current line.
/// Returns Some((id, content)) if successful.
/// This is used for single-line footnote definitions.
#[allow(dead_code)] // May be useful for future features
pub fn try_parse_footnote_definition(line: &str) -> Option<(String, String)> {
    let (id, content_start) = try_parse_footnote_marker(line)?;

    // Extract content (rest of the line after marker)
    let content = line[content_start..].trim_end().to_string();

    Some((id, content))
}
