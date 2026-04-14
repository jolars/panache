//! Formatting for Quarto shortcodes.
//!
//! Normalizes shortcode spacing to a consistent format:
//! - Normal: `{{< name arg1 arg2 >}}`
//! - Escaped: `{{{< name arg1 arg2 >}}}`

use crate::syntax::{SyntaxKind, SyntaxNode};
use rowan::NodeOrToken;

/// Format a shortcode node with normalized spacing.
///
/// Input examples:
/// - `{{<meta title>}}` → `{{< meta title >}}`
/// - `{{<  meta  title  >}}` → `{{< meta title >}}`
/// - `{{{< var version >}}}` → `{{{< var version >}}}`
pub(crate) fn format_shortcode(node: &SyntaxNode) -> String {
    let mut content = String::new();
    let mut is_escaped = false;

    for child in node.children_with_tokens() {
        match child {
            NodeOrToken::Token(t) => {
                if t.kind() == SyntaxKind::SHORTCODE_MARKER_OPEN {
                    // Determine if escaped based on marker
                    is_escaped = t.text() == "{{{<";
                }
            }
            NodeOrToken::Node(n) if n.kind() == SyntaxKind::SHORTCODE_CONTENT => {
                // Content is a node containing TEXT tokens
                content = normalize_shortcode_content(&n.text().to_string());
            }
            _ => {}
        }
    }

    // Build formatted shortcode
    if is_escaped {
        format!("{{{{{{< {} >}}}}}}", content)
    } else {
        format!("{{{{< {} >}}}}", content)
    }
}

/// Normalize shortcode content by:
/// 1. Trimming leading/trailing whitespace
/// 2. Collapsing internal runs of whitespace to single spaces
/// 3. Preserving quoted strings as-is
fn normalize_shortcode_content(text: &str) -> String {
    let mut result = String::new();
    let mut in_quotes = false;
    let mut prev_was_space = false;
    let mut quote_char = None;

    for ch in text.trim().chars() {
        match ch {
            '"' | '\'' if !in_quotes => {
                in_quotes = true;
                quote_char = Some(ch);
                result.push(ch);
                prev_was_space = false;
            }
            c if Some(c) == quote_char && in_quotes => {
                in_quotes = false;
                quote_char = None;
                result.push(c);
                prev_was_space = false;
            }
            c if c.is_whitespace() && !in_quotes => {
                if !prev_was_space && !result.is_empty() {
                    result.push(' ');
                    prev_was_space = true;
                }
            }
            c => {
                result.push(c);
                prev_was_space = false;
            }
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_simple_content() {
        assert_eq!(normalize_shortcode_content("meta title"), "meta title");
    }

    #[test]
    fn trims_whitespace() {
        assert_eq!(normalize_shortcode_content("  meta title  "), "meta title");
    }

    #[test]
    fn collapses_multiple_spaces() {
        assert_eq!(
            normalize_shortcode_content("meta  title   test"),
            "meta title test"
        );
    }

    #[test]
    fn preserves_quoted_strings() {
        assert_eq!(
            normalize_shortcode_content("video src=\"url with  spaces\""),
            "video src=\"url with  spaces\""
        );
    }

    #[test]
    fn preserves_single_quotes() {
        assert_eq!(
            normalize_shortcode_content("env VAR 'default  value'"),
            "env VAR 'default  value'"
        );
    }

    #[test]
    fn handles_multiple_arguments() {
        assert_eq!(
            normalize_shortcode_content("video  src=\"url\"  width=\"100%\""),
            "video src=\"url\" width=\"100%\""
        );
    }

    #[test]
    fn preserves_dots_and_escapes() {
        assert_eq!(
            normalize_shortcode_content(r"meta field\\.with\\.dots"),
            r"meta field\\.with\\.dots"
        );
    }

    #[test]
    fn preserves_braces() {
        assert_eq!(
            normalize_shortcode_content("meta key={nested}"),
            "meta key={nested}"
        );
    }

    #[test]
    fn end_to_end_formatting_with_quotes() {
        use crate::config::{Extensions, Flavor};
        use crate::{Config, format};

        let input = "{{< video src=\"https://example.com/video.mp4\" >}}";
        let flavor = Flavor::Quarto;
        let config = Config {
            flavor,
            extensions: Extensions::for_flavor(flavor),
            ..Default::default()
        };

        let output = format(input, Some(config), None);
        assert_eq!(
            output.trim(),
            "{{< video src=\"https://example.com/video.mp4\" >}}"
        );
    }

    #[test]
    fn end_to_end_formatting_without_spaces() {
        use crate::config::{Extensions, Flavor};
        use crate::{Config, format};

        let input = "{{<meta title>}}";
        let flavor = Flavor::Quarto;
        let config = Config {
            flavor,
            extensions: Extensions::for_flavor(flavor),
            ..Default::default()
        };

        let output = format(input, Some(config), None);
        assert_eq!(output.trim(), "{{< meta title >}}");
    }
}
