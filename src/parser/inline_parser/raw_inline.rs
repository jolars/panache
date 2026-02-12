//! Parsing for inline raw spans (`content`{=format})
//!
//! Raw inline spans allow embedding raw content for specific output formats.
//! Syntax: `content`{=format}
//! Examples:
//! - `<a>html</a>`{=html}
//! - `\LaTeX`{=latex}
//! - `<w:br/>`{=openxml}
//!
//! This is enabled by the raw_attribute extension.

use crate::parser::block_parser::attributes::AttributeBlock;
use crate::syntax::SyntaxKind;
use rowan::GreenNodeBuilder;

/// Check if a code span with attributes is actually a raw inline span.
/// Raw inline spans have attributes of the form {=format} (no other attributes).
pub fn is_raw_inline(attributes: &AttributeBlock) -> Option<&str> {
    // Raw inline must have exactly one class starting with '='
    // and no identifier or key-value pairs
    if attributes.identifier.is_some() || !attributes.key_values.is_empty() {
        return None;
    }

    if attributes.classes.len() == 1 {
        let class = &attributes.classes[0];
        if let Some(format) = class.strip_prefix('=')
            && !format.is_empty()
        {
            return Some(format);
        }
    }

    None
}

/// Emit a raw inline span node to the builder.
pub fn emit_raw_inline(
    builder: &mut GreenNodeBuilder,
    content: &str,
    backtick_count: usize,
    format: &str,
) {
    builder.start_node(SyntaxKind::RawInline.into());

    // Opening backticks
    builder.token(
        SyntaxKind::RawInlineMarker.into(),
        &"`".repeat(backtick_count),
    );

    // Raw content
    builder.token(SyntaxKind::RawInlineContent.into(), content);

    // Closing backticks
    builder.token(
        SyntaxKind::RawInlineMarker.into(),
        &"`".repeat(backtick_count),
    );

    // Format attribute: {=format}
    builder.start_node(SyntaxKind::Attribute.into());
    builder.token(SyntaxKind::TEXT.into(), &format!("{{={}}}", format));
    builder.finish_node();

    builder.finish_node();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::block_parser::attributes::AttributeBlock;

    #[test]
    fn test_is_raw_inline_html() {
        let attrs = AttributeBlock {
            identifier: None,
            classes: vec!["=html".to_string()],
            key_values: vec![],
        };
        assert_eq!(is_raw_inline(&attrs), Some("html"));
    }

    #[test]
    fn test_is_raw_inline_latex() {
        let attrs = AttributeBlock {
            identifier: None,
            classes: vec!["=latex".to_string()],
            key_values: vec![],
        };
        assert_eq!(is_raw_inline(&attrs), Some("latex"));
    }

    #[test]
    fn test_is_raw_inline_openxml() {
        let attrs = AttributeBlock {
            identifier: None,
            classes: vec!["=openxml".to_string()],
            key_values: vec![],
        };
        assert_eq!(is_raw_inline(&attrs), Some("openxml"));
    }

    #[test]
    fn test_not_raw_inline_regular_class() {
        let attrs = AttributeBlock {
            identifier: None,
            classes: vec!["python".to_string()],
            key_values: vec![],
        };
        assert_eq!(is_raw_inline(&attrs), None);
    }

    #[test]
    fn test_not_raw_inline_with_id() {
        let attrs = AttributeBlock {
            identifier: Some("myid".to_string()),
            classes: vec!["=html".to_string()],
            key_values: vec![],
        };
        assert_eq!(is_raw_inline(&attrs), None);
    }

    #[test]
    fn test_not_raw_inline_with_key_value() {
        let attrs = AttributeBlock {
            identifier: None,
            classes: vec!["=html".to_string()],
            key_values: vec![("key".to_string(), "value".to_string())],
        };
        assert_eq!(is_raw_inline(&attrs), None);
    }

    #[test]
    fn test_not_raw_inline_multiple_classes() {
        let attrs = AttributeBlock {
            identifier: None,
            classes: vec!["=html".to_string(), "other".to_string()],
            key_values: vec![],
        };
        assert_eq!(is_raw_inline(&attrs), None);
    }

    #[test]
    fn test_not_raw_inline_empty_format() {
        let attrs = AttributeBlock {
            identifier: None,
            classes: vec!["=".to_string()],
            key_values: vec![],
        };
        assert_eq!(is_raw_inline(&attrs), None);
    }
}
