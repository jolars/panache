//! Tree emission stage - emit InlineElement tree to GreenNodeBuilder

use super::elements::{EscapeType, InlineElement};
use super::*;
use crate::config::Config;
use crate::syntax::SyntaxKind;
use rowan::GreenNodeBuilder;

/// Emit an inline element tree to the GreenNodeBuilder.
///
/// This traverses the tree in document order and emits the appropriate
/// tokens and nodes to construct the final CST.
pub fn emit_inline_tree(
    builder: &mut GreenNodeBuilder,
    elements: &[InlineElement],
    config: &Config,
) {
    log::trace!("Emitting {} inline elements", elements.len());

    for element in elements {
        emit_element(builder, element, config);
    }
}

/// Emit a single inline element to the builder.
fn emit_element(builder: &mut GreenNodeBuilder, element: &InlineElement, config: &Config) {
    match element {
        InlineElement::Text { content, .. } => {
            if !content.is_empty() {
                builder.token(SyntaxKind::TEXT.into(), content);
            }
        }

        InlineElement::CodeSpan {
            content,
            backtick_count,
            attributes,
            ..
        } => {
            code_spans::emit_code_span(builder, content, *backtick_count, attributes.clone());
        }

        InlineElement::RawInline {
            content,
            format,
            backtick_count,
            ..
        } => {
            raw_inline::emit_raw_inline(builder, content, *backtick_count, format);
        }

        InlineElement::Escape {
            char: ch,
            escape_type,
            ..
        } => {
            let escape_type_enum = match escape_type {
                EscapeType::Literal => escapes::EscapeType::Literal,
                EscapeType::NonbreakingSpace => escapes::EscapeType::NonbreakingSpace,
                EscapeType::HardLineBreak => escapes::EscapeType::HardLineBreak,
            };
            escapes::emit_escape(builder, *ch, escape_type_enum);
        }

        InlineElement::LaTeXCommand { full_text, .. } => {
            let len = full_text.len();
            latex::parse_latex_command(builder, full_text, len);
        }

        InlineElement::InlineMath { content, .. } => {
            math::emit_inline_math(builder, content);
        }

        InlineElement::DisplayMath {
            content,
            dollar_count,
            attributes,
            ..
        } => {
            if let Some(count) = dollar_count {
                math::emit_display_math(builder, content, *count);

                // Emit attributes if present
                if let Some(attr_text) = attributes {
                    use crate::parser::block_parser::attributes::{
                        emit_attributes, try_parse_trailing_attributes,
                    };

                    if let Some((attr_block, _)) = try_parse_trailing_attributes(attr_text) {
                        // Emit whitespace before attributes
                        let trimmed = attr_text.trim_start();
                        let ws_len = attr_text.len() - trimmed.len();
                        if ws_len > 0 {
                            builder.token(SyntaxKind::WHITESPACE.into(), &attr_text[..ws_len]);
                        }
                        emit_attributes(builder, &attr_block);
                    }
                }
            } else {
                // \[...\] style display math - not yet implemented in this path
                // For now, emit as text
                builder.token(SyntaxKind::TEXT.into(), content);
            }
        }

        InlineElement::SingleBackslashMath {
            content,
            is_display,
            ..
        } => {
            if *is_display {
                math::emit_single_backslash_display_math(builder, content);
            } else {
                math::emit_single_backslash_inline_math(builder, content);
            }
        }

        InlineElement::DoubleBackslashMath {
            content,
            is_display,
            ..
        } => {
            if *is_display {
                math::emit_double_backslash_display_math(builder, content);
            } else {
                math::emit_double_backslash_inline_math(builder, content);
            }
        }

        InlineElement::InlineLink {
            full_text,
            link_text,
            dest,
            attributes,
            ..
        } => {
            links::emit_inline_link(
                builder,
                full_text,
                link_text,
                dest,
                attributes.as_deref(),
                config,
            );
        }

        InlineElement::ReferenceLink {
            link_text,
            label,
            is_shortcut,
            ..
        } => {
            links::emit_reference_link(builder, link_text, label, *is_shortcut, config);
        }

        InlineElement::InlineImage {
            full_text,
            alt_text,
            dest,
            attributes,
            ..
        } => {
            links::emit_inline_image(
                builder,
                full_text,
                alt_text,
                dest,
                attributes.as_deref(),
                config,
            );
        }

        InlineElement::ReferenceImage {
            alt_text,
            label,
            is_shortcut,
            ..
        } => {
            links::emit_reference_image(builder, alt_text, label, *is_shortcut, config);
        }

        InlineElement::Autolink { full_text, url, .. } => {
            links::emit_autolink(builder, full_text, url);
        }

        InlineElement::Emphasis {
            delim_char,
            children,
            ..
        } => {
            // Emit emphasis with recursive inline parsing of children
            builder.start_node(SyntaxKind::EMPHASIS.into());

            let delim_str = delim_char.to_string();
            builder.token(SyntaxKind::EMPHASIS_MARKER.into(), &delim_str);

            // Recursively emit children
            emit_inline_tree(builder, children, config);

            builder.token(SyntaxKind::EMPHASIS_MARKER.into(), &delim_str);
            builder.finish_node();
        }

        InlineElement::Strong {
            delim_char,
            children,
            ..
        } => {
            // Emit strong with recursive inline parsing of children
            builder.start_node(SyntaxKind::STRONG.into());

            let delim_str = format!("{}{}", delim_char, delim_char);
            builder.token(SyntaxKind::STRONG_MARKER.into(), &delim_str);

            // Recursively emit children
            emit_inline_tree(builder, children, config);

            builder.token(SyntaxKind::STRONG_MARKER.into(), &delim_str);
            builder.finish_node();
        }

        InlineElement::Strikeout { content, .. } => {
            strikeout::emit_strikeout(builder, content, config);
        }

        InlineElement::Superscript { content, .. } => {
            superscript::emit_superscript(builder, content, config);
        }

        InlineElement::Subscript { content, .. } => {
            subscript::emit_subscript(builder, content, config);
        }

        InlineElement::InlineFootnote { content, .. } => {
            inline_footnotes::emit_inline_footnote(builder, content, config);
        }

        InlineElement::FootnoteReference { id, .. } => {
            inline_footnotes::emit_footnote_reference(builder, id);
        }

        InlineElement::Shortcode {
            content,
            is_escaped,
            ..
        } => {
            shortcodes::emit_shortcode(builder, content, *is_escaped);
        }

        InlineElement::NativeSpan {
            content,
            attributes,
            ..
        } => {
            native_spans::emit_native_span(builder, content, attributes, config);
        }

        InlineElement::BracketedSpan {
            content,
            attributes,
            ..
        } => {
            bracketed_spans::emit_bracketed_span(builder, content, attributes, config);
        }

        InlineElement::BracketedCitation { content, .. } => {
            citations::emit_bracketed_citation(builder, content);
        }

        InlineElement::BareCitation {
            key, has_suppress, ..
        } => {
            citations::emit_bare_citation(builder, key, *has_suppress);
        }

        InlineElement::DelimiterRun { .. } => {
            // DelimiterRun should have been resolved by this point
            // If we encounter one here, it's a bug
            log::warn!("Encountered unresolved DelimiterRun during emission");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_emit_text() {
        let mut builder = GreenNodeBuilder::new();

        // Need to wrap in a node for rowan
        builder.start_node(SyntaxKind::PARAGRAPH.into());

        let elements = vec![InlineElement::Text {
            content: "hello".to_string(),
            start: 0,
            end: 5,
        }];

        let config = Config::default();
        emit_inline_tree(&mut builder, &elements, &config);

        builder.finish_node();
        let green = builder.finish();
        let text = green.to_string();
        assert_eq!(text, "hello");
    }

    #[test]
    fn test_emit_emphasis() {
        let mut builder = GreenNodeBuilder::new();

        // Need to wrap in a node for rowan
        builder.start_node(SyntaxKind::PARAGRAPH.into());

        let elements = vec![InlineElement::Emphasis {
            delim_char: '*',
            children: vec![InlineElement::Text {
                content: "foo".to_string(),
                start: 1,
                end: 4,
            }],
            start: 0,
            end: 5,
        }];

        let config = Config::default();
        emit_inline_tree(&mut builder, &elements, &config);

        builder.finish_node();
        let green = builder.finish();
        let text = green.to_string();
        assert_eq!(text, "*foo*");
    }

    #[test]
    fn test_emit_strong() {
        let mut builder = GreenNodeBuilder::new();

        // Need to wrap in a node for rowan
        builder.start_node(SyntaxKind::PARAGRAPH.into());

        let elements = vec![InlineElement::Strong {
            delim_char: '*',
            children: vec![InlineElement::Text {
                content: "bar".to_string(),
                start: 2,
                end: 5,
            }],
            start: 0,
            end: 7,
        }];

        let config = Config::default();
        emit_inline_tree(&mut builder, &elements, &config);

        builder.finish_node();
        let green = builder.finish();
        let text = green.to_string();
        assert_eq!(text, "**bar**");
    }
}
