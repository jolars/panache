//! Reference definition parsing for Markdown reference links/images and footnotes.
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

use std::collections::HashMap;

use crate::utils::normalize_label;

/// A reference definition that maps a label to a URL and optional title.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReferenceDefinition {
    pub label: String,
    pub url: String,
    pub title: Option<String>,
}

/// A footnote definition that maps an ID to content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FootnoteDefinition {
    pub id: String,
    pub content: String,
}

/// Registry that stores all reference definitions and footnotes in a document.
/// Labels are stored in normalized (lowercase) form for case-insensitive lookup.
#[derive(Debug, Clone, Default)]
pub struct ReferenceRegistry {
    definitions: HashMap<String, ReferenceDefinition>,
    footnotes: HashMap<String, FootnoteDefinition>,
}

impl ReferenceRegistry {
    pub fn new() -> Self {
        Self {
            definitions: HashMap::new(),
            footnotes: HashMap::new(),
        }
    }

    /// Add a reference definition to the registry.
    /// Labels are normalized to lowercase for case-insensitive matching.
    pub fn add(&mut self, label: String, url: String, title: Option<String>) {
        let normalized_label = normalize_label(&label);
        self.definitions
            .insert(normalized_label, ReferenceDefinition { label, url, title });
    }

    /// Look up a reference definition by label (case-insensitive).
    pub fn get(&self, label: &str) -> Option<&ReferenceDefinition> {
        let normalized = normalize_label(label);
        self.definitions.get(&normalized)
    }

    /// Check if a label exists in the registry.
    pub fn contains(&self, label: &str) -> bool {
        let normalized = normalize_label(label);
        self.definitions.contains_key(&normalized)
    }

    /// Add a footnote definition to the registry.
    /// IDs are normalized to lowercase for case-insensitive matching.
    pub fn add_footnote(&mut self, id: String, content: String) {
        let normalized_id = normalize_label(&id);
        self.footnotes
            .insert(normalized_id, FootnoteDefinition { id, content });
    }

    /// Look up a footnote definition by ID (case-insensitive).
    pub fn get_footnote(&self, id: &str) -> Option<&FootnoteDefinition> {
        let normalized = normalize_label(id);
        self.footnotes.get(&normalized)
    }

    /// Check if a footnote ID exists in the registry.
    pub fn contains_footnote(&self, id: &str) -> bool {
        let normalized = normalize_label(id);
        self.footnotes.contains_key(&normalized)
    }
}

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

    // Skip whitespace after colon
    while pos < bytes.len() && matches!(bytes[pos], b' ' | b'\t') {
        pos += 1;
    }

    // Return the ID and the column where content starts
    Some((id.to_string(), pos))
}

/// Try to parse a footnote definition starting at the current position.
/// Returns Some((consumed_lines, id, content)) if successful.
///
/// Syntax:
/// ```markdown
/// [^id]: Footnote content.
///     Can continue on indented lines.
/// ```
pub fn try_parse_footnote_definition(
    lines: &[&str],
    start_line: usize,
) -> Option<(usize, String, String)> {
    if start_line >= lines.len() {
        return None;
    }

    let first_line = lines[start_line];
    let bytes = first_line.as_bytes();

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

    let id = &first_line[2..pos];
    if id.is_empty() {
        return None;
    }

    pos += 1; // Skip ]

    // Must be followed by :
    if pos >= bytes.len() || bytes[pos] != b':' {
        return None;
    }
    pos += 1;

    // Skip whitespace after colon
    while pos < bytes.len() && matches!(bytes[pos], b' ' | b'\t') {
        pos += 1;
    }

    // Collect content from first line
    let mut content = if pos < bytes.len() {
        first_line[pos..].to_string()
    } else {
        String::new()
    };

    // Collect continuation lines (indented by 4 spaces or 1 tab)
    let mut consumed_lines = 1;
    let mut previous_was_blank = false;

    for line in lines.iter().skip(start_line + 1) {
        // Blank lines are allowed in footnote content
        if line.trim().is_empty() {
            content.push('\n');
            consumed_lines += 1;
            previous_was_blank = true;
            continue;
        }

        // Check if line is indented (4 spaces or 1 tab)
        let line_bytes = line.as_bytes();
        let is_indented = if line_bytes.len() >= 4
            && line_bytes[0] == b' '
            && line_bytes[1] == b' '
            && line_bytes[2] == b' '
            && line_bytes[3] == b' '
        {
            true
        } else {
            !line_bytes.is_empty() && line_bytes[0] == b'\t'
        };

        if !is_indented {
            // If previous line was blank, this unindented line ends the footnote
            // Otherwise, it's a lazy continuation of the current paragraph
            if previous_was_blank {
                break;
            }
            // Lazy continuation: add the line as-is to the current paragraph
            if !content.is_empty() && !content.ends_with('\n') {
                content.push('\n');
            }
            content.push_str(line);
            consumed_lines += 1;
            previous_was_blank = false;
            continue;
        }

        // Remove indentation and add to content
        let dedented = if line_bytes.len() >= 4 && line_bytes[..4] == [b' ', b' ', b' ', b' '] {
            &line[4..]
        } else if !line_bytes.is_empty() && line_bytes[0] == b'\t' {
            &line[1..]
        } else {
            line
        };

        if !content.is_empty() && !content.ends_with('\n') {
            content.push('\n');
        }
        content.push_str(dedented);
        consumed_lines += 1;
        previous_was_blank = false;
    }

    Some((consumed_lines, id.to_string(), content))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_reference() {
        let result = try_parse_reference_definition("[foo]: /url");
        assert_eq!(
            result,
            Some((11, "foo".to_string(), "/url".to_string(), None))
        );
    }

    #[test]
    fn test_parse_reference_with_title_double_quotes() {
        let result = try_parse_reference_definition(r#"[foo]: /url "title""#);
        assert_eq!(
            result,
            Some((
                19,
                "foo".to_string(),
                "/url".to_string(),
                Some("title".to_string())
            ))
        );
    }

    #[test]
    fn test_parse_reference_with_title_single_quotes() {
        let result = try_parse_reference_definition("[foo]: /url 'title'");
        assert_eq!(
            result,
            Some((
                19,
                "foo".to_string(),
                "/url".to_string(),
                Some("title".to_string())
            ))
        );
    }

    #[test]
    fn test_parse_reference_with_title_parens() {
        let result = try_parse_reference_definition("[foo]: /url (title)");
        assert_eq!(
            result,
            Some((
                19,
                "foo".to_string(),
                "/url".to_string(),
                Some("title".to_string())
            ))
        );
    }

    #[test]
    fn test_parse_reference_angle_bracketed_url() {
        let result = try_parse_reference_definition("[foo]: <http://example.com>");
        assert_eq!(
            result,
            Some((
                27,
                "foo".to_string(),
                "http://example.com".to_string(),
                None
            ))
        );
    }

    #[test]
    fn test_parse_reference_with_spaces() {
        let result = try_parse_reference_definition("[foo]:  /url  ");
        assert_eq!(
            result,
            Some((14, "foo".to_string(), "/url".to_string(), None))
        );
    }

    #[test]
    fn test_parse_reference_multiword_label() {
        let result = try_parse_reference_definition("[my link]: /url");
        assert_eq!(
            result,
            Some((15, "my link".to_string(), "/url".to_string(), None))
        );
    }

    #[test]
    fn test_not_reference_no_colon() {
        let result = try_parse_reference_definition("[foo] /url");
        assert_eq!(result, None);
    }

    #[test]
    fn test_not_reference_no_url() {
        let result = try_parse_reference_definition("[foo]: ");
        assert_eq!(result, None);
    }

    #[test]
    fn test_not_reference_label_spans_line() {
        let result = try_parse_reference_definition("[foo\nbar]: /url");
        assert_eq!(result, None);
    }

    #[test]
    fn test_registry_add_and_get() {
        let mut registry = ReferenceRegistry::new();
        registry.add("foo".to_string(), "/url".to_string(), None);

        let def = registry.get("foo");
        assert!(def.is_some());
        assert_eq!(def.unwrap().url, "/url");
    }

    #[test]
    fn test_registry_case_insensitive() {
        let mut registry = ReferenceRegistry::new();
        registry.add("FOO".to_string(), "/url".to_string(), None);

        assert!(registry.contains("foo"));
        assert!(registry.contains("Foo"));
        assert!(registry.contains("FOO"));

        let def = registry.get("foo");
        assert_eq!(def.unwrap().url, "/url");
    }

    #[test]
    fn test_normalize_label_whitespace() {
        assert_eq!(normalize_label("foo  bar"), "foo bar");
        assert_eq!(normalize_label("  foo  "), "foo");
        assert_eq!(normalize_label("foo\tbar"), "foo bar");
    }

    #[test]
    fn test_footnote_definition_id_with_crlf() {
        // Footnote definition IDs should not contain CRLF
        // Note: try_parse_footnote_definition expects a line, which by definition
        // won't contain line breaks, so this test verifies the function handles
        // the case correctly even if called with invalid input
        let input = "[^foo\r\nbar]: content";
        let lines = vec![input];
        let result = try_parse_footnote_definition(&lines, 0);

        // Should fail to parse because ID contains line break
        assert_eq!(
            result, None,
            "Should not parse footnote definition with CRLF in ID"
        );
    }

    #[test]
    fn test_reference_link_url_with_crlf() {
        // Reference link URLs in angle brackets should not span lines with CRLF
        let input = "[ref]: <http://example.com\r\n/path>";
        let result = try_parse_reference_definition(input);

        // Should fail to parse because URL contains line break
        assert_eq!(
            result, None,
            "Should not parse reference with CRLF in angle-bracketed URL"
        );
    }

    #[test]
    fn test_reference_link_url_with_lf() {
        // Reference link URLs in angle brackets should not span lines with LF
        let input = "[ref]: <http://example.com\n/path>";
        let result = try_parse_reference_definition(input);

        // Should fail to parse because URL contains line break
        assert_eq!(
            result, None,
            "Should not parse reference with LF in angle-bracketed URL"
        );
    }
}
