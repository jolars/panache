//! Math AST node wrappers.

use super::{AstNode, PanacheLanguage, SyntaxKind, SyntaxNode};

pub struct DisplayMath(SyntaxNode);

impl AstNode for DisplayMath {
    type Language = PanacheLanguage;

    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::DISPLAY_MATH
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

impl DisplayMath {
    pub fn opening_marker(&self) -> Option<String> {
        self.0.children_with_tokens().find_map(|child| {
            child.into_token().and_then(|token| {
                (token.kind() == SyntaxKind::DISPLAY_MATH_MARKER).then(|| token.text().to_string())
            })
        })
    }

    pub fn closing_marker(&self) -> Option<String> {
        self.0
            .children_with_tokens()
            .filter_map(|child| child.into_token())
            .filter(|token| token.kind() == SyntaxKind::DISPLAY_MATH_MARKER)
            .nth(1)
            .map(|token| token.text().to_string())
    }

    pub fn content(&self) -> String {
        self.0
            .children_with_tokens()
            .filter_map(|child| child.into_token())
            .filter(|token| token.kind() == SyntaxKind::TEXT)
            .map(|token| token.text().to_string())
            .collect::<Vec<_>>()
            .join("")
    }

    pub fn is_environment_form(&self) -> bool {
        let opening = self.opening_marker().unwrap_or_default();
        let closing = self.closing_marker().unwrap_or_default();
        opening.starts_with("\\begin{") && closing.starts_with("\\end{")
    }

    pub fn has_unescaped_single_dollar_in_content(&self) -> bool {
        let content = self.content();
        let chars: Vec<char> = content.chars().collect();
        let mut idx = 0usize;
        let mut backslashes = 0usize;

        while idx < chars.len() {
            let ch = chars[idx];
            if ch == '\\' {
                backslashes += 1;
                idx += 1;
                continue;
            }

            let escaped = backslashes % 2 == 1;
            backslashes = 0;
            if ch == '$' && !escaped {
                if idx + 1 < chars.len() && chars[idx + 1] == '$' {
                    idx += 2;
                    continue;
                }
                return true;
            }
            idx += 1;
        }

        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse;

    #[test]
    fn display_math_dollar_markers_and_content() {
        let tree = parse("$$\nx^2 + y^2\n$$\n", None);
        let math = tree
            .descendants()
            .find_map(DisplayMath::cast)
            .expect("display math");

        assert_eq!(math.opening_marker().as_deref(), Some("$$"));
        assert_eq!(math.closing_marker().as_deref(), Some("$$"));
        assert!(math.content().contains("x^2 + y^2"));
        assert!(!math.is_environment_form());
    }

    #[test]
    fn display_math_environment_form_detection() {
        let tree = parse("\\begin{align}\na &= b\\\\\n\\end{align}\n", None);
        let math = tree
            .descendants()
            .find_map(DisplayMath::cast)
            .expect("display math");

        assert!(math.is_environment_form());
        assert_eq!(math.opening_marker().as_deref(), Some("\\begin{align}"));
        assert_eq!(math.closing_marker().as_deref(), Some("\\end{align}\n"));
    }

    #[test]
    fn display_math_detects_unescaped_single_dollar() {
        let tree = parse("$$\nalpha $beta$ gamma\n$$\n", None);
        let math = tree
            .descendants()
            .find_map(DisplayMath::cast)
            .expect("display math");
        assert!(math.has_unescaped_single_dollar_in_content());
    }
}
