//! Reference definition parsing for Markdown reference links and images.
//!
//! Reference definitions have the form:
//! ```markdown
//! [label]: url "optional title"
//! [label]: url 'optional title'
//! [label]: url (optional title)
//! [label]: <url> "title"
//! ```

use std::collections::HashMap;

/// A reference definition that maps a label to a URL and optional title.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReferenceDefinition {
    pub label: String,
    pub url: String,
    pub title: Option<String>,
}

/// Registry that stores all reference definitions in a document.
/// Labels are stored in normalized (lowercase) form for case-insensitive lookup.
#[derive(Debug, Clone, Default)]
pub struct ReferenceRegistry {
    definitions: HashMap<String, ReferenceDefinition>,
}

impl ReferenceRegistry {
    pub fn new() -> Self {
        Self {
            definitions: HashMap::new(),
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
}

/// Normalize a label for case-insensitive matching.
/// Collapses whitespace and converts to lowercase.
fn normalize_label(label: &str) -> String {
    label
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
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
        while pos < bytes.len() && bytes[pos] != b'>' && bytes[pos] != b'\n' {
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
}
