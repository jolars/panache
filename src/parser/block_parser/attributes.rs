//! Parsing for Pandoc-style attributes: {#id .class key=value}
//!
//! Attributes can appear after headings, fenced code blocks, fenced divs, etc.
//! Syntax: {#identifier .class1 .class2 key1=val1 key2="val2"}
//!
//! Rules:
//! - Surrounded by { }
//! - Identifier: #id (optional, only first one counts)
//! - Classes: .class (can have multiple)
//! - Key-value pairs: key=value or key="value" or key='value' (can have multiple)
//! - Whitespace flexible between items

use crate::syntax::SyntaxKind;
use rowan::GreenNodeBuilder;

#[derive(Debug, PartialEq)]
pub struct AttributeBlock {
    pub identifier: Option<String>,
    pub classes: Vec<String>,
    pub key_values: Vec<(String, String)>,
}

/// Try to parse an attribute block from the end of a string
/// Returns: (attribute_block, text_before_attributes)
pub fn try_parse_trailing_attributes(text: &str) -> Option<(AttributeBlock, &str)> {
    let trimmed = text.trim_end();

    // Must end with }
    if !trimmed.ends_with('}') {
        return None;
    }

    // Find matching {
    let open_brace = trimmed.rfind('{')?;

    // Check if this is a bracketed span like [text]{.class} rather than a heading attribute
    // If the { is immediately after ] (with optional whitespace), this should be parsed as a span
    let before_brace = &trimmed[..open_brace];
    if before_brace.trim_end().ends_with(']') {
        log::debug!("Skipping attribute parsing for bracketed span: {}", text);
        return None;
    }

    // Parse the content between { and }
    let attr_content = &trimmed[open_brace + 1..trimmed.len() - 1];
    let attr_block = parse_attribute_content(attr_content)?;

    // Get text before attributes (trim trailing whitespace)
    let before_attrs = trimmed[..open_brace].trim_end();

    Some((attr_block, before_attrs))
}

/// Parse the content inside the attribute braces
fn parse_attribute_content(content: &str) -> Option<AttributeBlock> {
    let mut identifier = None;
    let mut classes = Vec::new();
    let mut key_values = Vec::new();

    let content = content.trim();
    if content.is_empty() {
        return None; // Empty {} is not valid
    }

    let mut pos = 0;
    let bytes = content.as_bytes();

    while pos < bytes.len() {
        // Skip whitespace
        while pos < bytes.len() && bytes[pos].is_ascii_whitespace() {
            pos += 1;
        }

        if pos >= bytes.len() {
            break;
        }

        // Check what kind of attribute this is
        if bytes[pos] == b'=' {
            // Special case: {=format} for raw attributes
            // This is treated as a class ".=format" for compatibility
            pos += 1; // Skip =
            let start = pos;
            while pos < bytes.len() && !bytes[pos].is_ascii_whitespace() && bytes[pos] != b'}' {
                pos += 1;
            }
            if pos > start {
                // Store as "=format" class (with the = prefix)
                classes.push(format!("={}", &content[start..pos]));
            }
        } else if bytes[pos] == b'#' {
            // Identifier (only take first one)
            if identifier.is_none() {
                pos += 1; // Skip #
                let start = pos;
                while pos < bytes.len() && !bytes[pos].is_ascii_whitespace() && bytes[pos] != b'}' {
                    pos += 1;
                }
                if pos > start {
                    identifier = Some(content[start..pos].to_string());
                }
            } else {
                // Skip duplicate identifiers
                pos += 1;
                while pos < bytes.len() && !bytes[pos].is_ascii_whitespace() && bytes[pos] != b'}' {
                    pos += 1;
                }
            }
        } else if bytes[pos] == b'.' {
            // Class
            pos += 1; // Skip .
            let start = pos;
            while pos < bytes.len() && !bytes[pos].is_ascii_whitespace() && bytes[pos] != b'}' {
                pos += 1;
            }
            if pos > start {
                classes.push(content[start..pos].to_string());
            }
        } else {
            // Key-value pair
            let key_start = pos;
            while pos < bytes.len() && bytes[pos] != b'=' && !bytes[pos].is_ascii_whitespace() {
                pos += 1;
            }

            if pos >= bytes.len() || bytes[pos] != b'=' {
                // Not a valid key=value, skip this token
                while pos < bytes.len() && !bytes[pos].is_ascii_whitespace() {
                    pos += 1;
                }
                continue;
            }

            let key = content[key_start..pos].to_string();
            pos += 1; // Skip =

            // Parse value (may be quoted)
            let value = if pos < bytes.len() && (bytes[pos] == b'"' || bytes[pos] == b'\'') {
                let quote = bytes[pos];
                pos += 1; // Skip opening quote
                let val_start = pos;
                while pos < bytes.len() && bytes[pos] != quote {
                    pos += 1;
                }
                let val = content[val_start..pos].to_string();
                if pos < bytes.len() {
                    pos += 1; // Skip closing quote
                }
                val
            } else {
                // Unquoted value
                let val_start = pos;
                while pos < bytes.len() && !bytes[pos].is_ascii_whitespace() && bytes[pos] != b'}' {
                    pos += 1;
                }
                content[val_start..pos].to_string()
            };

            if !key.is_empty() {
                key_values.push((key, value));
            }
        }
    }

    // At least one attribute must be present
    if identifier.is_none() && classes.is_empty() && key_values.is_empty() {
        return None;
    }

    Some(AttributeBlock {
        identifier,
        classes,
        key_values,
    })
}

/// Emit attribute block as AST nodes
pub fn emit_attributes(builder: &mut GreenNodeBuilder, attrs: &AttributeBlock) {
    builder.start_node(SyntaxKind::Attribute.into());

    // Build the attribute string to emit
    let mut attr_str = String::from("{");

    if let Some(ref id) = attrs.identifier {
        attr_str.push('#');
        attr_str.push_str(id);
    }

    for class in &attrs.classes {
        if attr_str.len() > 1 {
            attr_str.push(' ');
        }
        // Special case: if class starts with =, it's a raw format specifier
        // Emit as {=format} not {.=format}
        if class.starts_with('=') {
            attr_str.push_str(class);
        } else {
            attr_str.push('.');
            attr_str.push_str(class);
        }
    }

    for (key, value) in &attrs.key_values {
        if attr_str.len() > 1 {
            attr_str.push(' ');
        }
        attr_str.push_str(key);
        attr_str.push('=');

        // Always quote attribute values to match Pandoc's behavior
        attr_str.push('"');
        attr_str.push_str(&value.replace('"', "\\\""));
        attr_str.push('"');
    }

    attr_str.push('}');

    builder.token(SyntaxKind::Attribute.into(), &attr_str);
    builder.finish_node();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_id() {
        let result = try_parse_trailing_attributes("Heading {#my-id}");
        assert!(result.is_some());
        let (attrs, before) = result.unwrap();
        assert_eq!(before, "Heading");
        assert_eq!(attrs.identifier, Some("my-id".to_string()));
        assert!(attrs.classes.is_empty());
        assert!(attrs.key_values.is_empty());
    }

    #[test]
    fn test_single_class() {
        let result = try_parse_trailing_attributes("Text {.myclass}");
        assert!(result.is_some());
        let (attrs, _) = result.unwrap();
        assert_eq!(attrs.classes, vec!["myclass"]);
    }

    #[test]
    fn test_multiple_classes() {
        let result = try_parse_trailing_attributes("Text {.class1 .class2 .class3}");
        assert!(result.is_some());
        let (attrs, _) = result.unwrap();
        assert_eq!(attrs.classes, vec!["class1", "class2", "class3"]);
    }

    #[test]
    fn test_key_value_unquoted() {
        let result = try_parse_trailing_attributes("Text {key=value}");
        assert!(result.is_some());
        let (attrs, _) = result.unwrap();
        assert_eq!(
            attrs.key_values,
            vec![("key".to_string(), "value".to_string())]
        );
    }

    #[test]
    fn test_key_value_quoted() {
        let result = try_parse_trailing_attributes("Text {key=\"value with spaces\"}");
        assert!(result.is_some());
        let (attrs, _) = result.unwrap();
        assert_eq!(
            attrs.key_values,
            vec![("key".to_string(), "value with spaces".to_string())]
        );
    }

    #[test]
    fn test_full_attributes() {
        let result =
            try_parse_trailing_attributes("Heading {#id .class1 .class2 key1=val1 key2=\"val 2\"}");
        assert!(result.is_some());
        let (attrs, before) = result.unwrap();
        assert_eq!(before, "Heading");
        assert_eq!(attrs.identifier, Some("id".to_string()));
        assert_eq!(attrs.classes, vec!["class1", "class2"]);
        assert_eq!(attrs.key_values.len(), 2);
        assert_eq!(
            attrs.key_values[0],
            ("key1".to_string(), "val1".to_string())
        );
        assert_eq!(
            attrs.key_values[1],
            ("key2".to_string(), "val 2".to_string())
        );
    }

    #[test]
    fn test_no_attributes() {
        let result = try_parse_trailing_attributes("Heading with no attributes");
        assert!(result.is_none());
    }

    #[test]
    fn test_empty_braces() {
        let result = try_parse_trailing_attributes("Heading {}");
        assert!(result.is_none());
    }

    #[test]
    fn test_only_first_id_counts() {
        let result = try_parse_trailing_attributes("Text {#id1 #id2}");
        assert!(result.is_some());
        let (attrs, _) = result.unwrap();
        assert_eq!(attrs.identifier, Some("id1".to_string()));
    }

    #[test]
    fn test_whitespace_handling() {
        let result = try_parse_trailing_attributes("Text {  #id   .class   key=val  }");
        assert!(result.is_some());
        let (attrs, _) = result.unwrap();
        assert_eq!(attrs.identifier, Some("id".to_string()));
        assert_eq!(attrs.classes, vec!["class"]);
        assert_eq!(
            attrs.key_values,
            vec![("key".to_string(), "val".to_string())]
        );
    }

    #[test]
    fn test_trailing_whitespace_before_attrs() {
        let result = try_parse_trailing_attributes("Heading   {#id}");
        assert!(result.is_some());
        let (_, before) = result.unwrap();
        assert_eq!(before, "Heading");
    }
}
