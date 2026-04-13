//! Quarto shortcode AST node wrappers.

use super::{AstNode, PanacheLanguage, SyntaxKind, SyntaxNode};

pub struct Shortcode(SyntaxNode);

impl AstNode for Shortcode {
    type Language = PanacheLanguage;

    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::SHORTCODE
    }

    fn cast(syntax: SyntaxNode) -> Option<Self> {
        if Self::can_cast(syntax.kind()) {
            Some(Self(syntax))
        } else {
            None
        }
    }

    fn syntax(&self) -> &SyntaxNode {
        &self.0
    }
}

impl Shortcode {
    /// Returns true if the shortcode is escaped (`{{{< ... >}}}`).
    pub fn is_escaped(&self) -> bool {
        self.0.children_with_tokens().any(|child| match child {
            rowan::NodeOrToken::Token(token) => {
                token.kind() == SyntaxKind::SHORTCODE_MARKER_OPEN && token.text() == "{{{<"
            }
            _ => false,
        })
    }

    /// Returns shortcode content between markers.
    pub fn content(&self) -> Option<String> {
        self.0.children().find_map(|child| {
            if child.kind() == SyntaxKind::SHORTCODE_CONTENT {
                Some(child.text().to_string())
            } else {
                None
            }
        })
    }

    /// Returns shortcode name (first argument), when present.
    pub fn name(&self) -> Option<String> {
        self.args().first().cloned()
    }

    /// Returns shortcode arguments split on shell-like whitespace/quotes.
    pub fn args(&self) -> Vec<String> {
        let Some(content) = self.content() else {
            return Vec::new();
        };

        split_shortcode_args(&content)
    }
}

pub fn split_shortcode_args(content: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut quote_char = None;

    for ch in content.trim().chars() {
        match ch {
            '"' | '\'' if !in_quotes => {
                in_quotes = true;
                quote_char = Some(ch);
            }
            c if Some(c) == quote_char && in_quotes => {
                in_quotes = false;
                quote_char = None;
            }
            c if c.is_whitespace() && !in_quotes => {
                if !current.is_empty() {
                    args.push(current.clone());
                    current.clear();
                }
            }
            c => current.push(c),
        }
    }

    if !current.is_empty() {
        args.push(current);
    }

    args
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ParserOptions;
    use crate::parser::parse;

    #[test]
    fn shortcode_wrapper_extracts_name_and_args() {
        let tree = parse(
            "{{< include \"chapters/part 1.qmd\" >}}",
            Some(ParserOptions::default()),
        );
        let shortcode = tree
            .descendants()
            .find_map(Shortcode::cast)
            .expect("shortcode");

        assert_eq!(shortcode.name().as_deref(), Some("include"));
        assert_eq!(
            shortcode.args(),
            vec!["include".to_string(), "chapters/part 1.qmd".to_string()]
        );
    }

    #[test]
    fn shortcode_wrapper_detects_escaped_shortcode() {
        let tree = parse(
            "{{{< include child.qmd >}}}",
            Some(ParserOptions::default()),
        );
        let shortcode = tree
            .descendants()
            .find_map(Shortcode::cast)
            .expect("shortcode");

        assert!(shortcode.is_escaped());
    }
}
