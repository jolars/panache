use panache_parser::parser::blocks::headings::content_reads_as_decoration;
use rowan::NodeOrToken;

use super::core::normalize_attribute_text;
use super::inline::{collapse_spaces, format_inline_node_with_spacing};
use super::smart::normalize_smart_punctuation;
use crate::config::{Config, Flavor};
use crate::syntax::{SyntaxKind, SyntaxNode};

/// Render a single heading line (`### content {attrs}`), formatting inline
/// elements and normalizing attributes. Surrounding blank lines and the
/// trailing newline are the caller's responsibility.
///
/// This is the single source of truth for heading content: both the
/// document-body `HEADING` branch and the list-nested heading path call it, so
/// inline content (code spans, links, emphasis) and attributes are reformatted
/// identically regardless of where the heading appears. The `#` prefix means
/// the rendered line can never collide with a thematic break.
pub(super) fn format_heading(node: &SyntaxNode, config: &Config) -> String {
    // GitHub and Hugo derive anchors from each source space, so collapsing them can break links.
    let collapse_ws = !matches!(config.flavor, Flavor::Gfm | Flavor::Hugo);
    let mut level = 1;
    let mut attributes = String::new();
    let mut content = String::new();

    for child in node.children() {
        match child.kind() {
            SyntaxKind::ATX_HEADING_MARKER => {
                let t = child.text().to_string();
                level = t.chars().take_while(|&c| c == '#').count().clamp(1, 6);
            }
            SyntaxKind::SETEXT_HEADING_UNDERLINE => {
                let t = child.text().to_string();
                level = if t.trim().starts_with('=') { 1 } else { 2 };
            }
            SyntaxKind::HEADING_CONTENT => {
                for element in child.children_with_tokens() {
                    match element {
                        NodeOrToken::Token(t) => {
                            if t.kind() == SyntaxKind::NEWLINE {
                                if !content.ends_with(' ') {
                                    content.push(' ');
                                }
                            } else {
                                let text = if collapse_ws && t.kind() == SyntaxKind::TEXT {
                                    std::borrow::Cow::Owned(collapse_spaces(t.text()))
                                } else {
                                    std::borrow::Cow::Borrowed(t.text())
                                };
                                content.push_str(
                                    normalize_smart_punctuation(
                                        &text,
                                        config.formatter_extensions.smart,
                                        config.formatter_extensions.smart_quotes,
                                    )
                                    .as_ref(),
                                );
                            }
                        }
                        NodeOrToken::Node(n) => {
                            content.push_str(&format_inline_node_with_spacing(
                                &n,
                                config,
                                collapse_ws,
                            ));
                        }
                    }
                }
            }
            SyntaxKind::ATTRIBUTE => {
                attributes = normalize_attribute_text(&child.text().to_string());
            }
            _ => {}
        }
    }

    let content = content.trim();

    let mut out = "#".repeat(level);
    if !content.is_empty() {
        out.push(' ');
        out.push_str(content);
    }
    if content_reads_as_decoration(content, &config.parser_options()) {
        out.push_str(" #");
    }
    if !attributes.is_empty() {
        out.push(' ');
        out.push_str(&attributes);
    }
    out
}
