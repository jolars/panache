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

use crate::parser::utils::attributes::{AttributeBlock, emit_attribute_node};
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
///
/// `attr_raw` is the raw `{=format}` source slice (braces included). It is
/// structured into `ATTR_*` children via [`emit_attribute_node`] so the node
/// wraps the original bytes losslessly instead of synthesizing them — any
/// interior whitespace (`{ =html }`) round-trips byte-for-byte.
pub fn emit_raw_inline(
    builder: &mut GreenNodeBuilder,
    content: &str,
    backtick_count: usize,
    attr_raw: &str,
) {
    builder.start_node(SyntaxKind::RAW_INLINE.into());

    // Opening backticks
    builder.token(
        SyntaxKind::RAW_INLINE_MARKER.into(),
        &"`".repeat(backtick_count),
    );

    // Raw content
    builder.token(SyntaxKind::RAW_INLINE_CONTENT.into(), content);

    // Closing backticks
    builder.token(
        SyntaxKind::RAW_INLINE_MARKER.into(),
        &"`".repeat(backtick_count),
    );

    // Format attribute `{=format}`, structured over the raw source bytes.
    emit_attribute_node(builder, attr_raw);

    builder.finish_node();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::utils::attributes::AttributeBlock;

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

    /// The `{=format}` attribute is now structured over the raw source bytes
    /// (an `ATTR_CLASS` token wrapping `=format`) rather than synthesized, so
    /// the `RAW_INLINE` node round-trips byte-for-byte and exposes structure.
    #[test]
    fn raw_inline_attribute_is_structured_and_lossless() {
        let input = "`<a>`{=html}\n";
        let tree = crate::parse(input, None);
        assert_eq!(tree.text().to_string(), input);

        let attr = tree
            .descendants()
            .find(|n| n.kind() == SyntaxKind::ATTRIBUTE)
            .expect("ATTRIBUTE node under RAW_INLINE");
        assert_eq!(attr.text().to_string(), "{=html}");
        let class = attr
            .children_with_tokens()
            .find(|el| el.kind() == SyntaxKind::ATTR_CLASS)
            .and_then(|el| el.into_token())
            .expect("ATTR_CLASS token");
        assert_eq!(class.text(), "=html");
    }

    /// Interior whitespace inside the braces is preserved verbatim — the old
    /// synthesizing emitter collapsed `{ =html }` to `{=html}`.
    #[test]
    fn raw_inline_attribute_preserves_interior_whitespace() {
        let input = "`<a>`{ =html }\n";
        let tree = crate::parse(input, None);
        assert_eq!(tree.text().to_string(), input);
    }
}
