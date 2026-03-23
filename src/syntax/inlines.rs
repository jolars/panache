//! Inline AST node wrappers.

use super::{AstNode, PanacheLanguage, SyntaxKind, SyntaxNode};

pub struct InlineMath(SyntaxNode);

impl AstNode for InlineMath {
    type Language = PanacheLanguage;

    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::INLINE_MATH
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

impl InlineMath {
    pub fn opening_marker(&self) -> Option<String> {
        self.0.children_with_tokens().find_map(|child| {
            child.into_token().and_then(|token| {
                (token.kind() == SyntaxKind::INLINE_MATH_MARKER).then(|| token.text().to_string())
            })
        })
    }

    pub fn closing_marker(&self) -> Option<String> {
        self.0
            .children_with_tokens()
            .filter_map(|child| child.into_token())
            .filter(|token| token.kind() == SyntaxKind::INLINE_MATH_MARKER)
            .nth(1)
            .map(|token| token.text().to_string())
    }

    pub fn content(&self) -> String {
        self.0
            .children_with_tokens()
            .filter_map(|child| child.into_token())
            .filter(|token| token.kind() != SyntaxKind::INLINE_MATH_MARKER)
            .map(|token| token.text().to_string())
            .collect::<Vec<_>>()
            .join("")
    }

    pub fn content_range(&self) -> Option<rowan::TextRange> {
        let mut markers = self
            .0
            .children_with_tokens()
            .filter_map(|child| child.into_token())
            .filter(|token| token.kind() == SyntaxKind::INLINE_MATH_MARKER);

        let start = markers.next()?.text_range().end();
        let end = markers.next()?.text_range().start();
        (start <= end).then(|| rowan::TextRange::new(start, end))
    }
}

pub struct CodeSpan(SyntaxNode);

impl AstNode for CodeSpan {
    type Language = PanacheLanguage;

    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::CODE_SPAN
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

impl CodeSpan {
    pub fn marker(&self) -> Option<String> {
        self.0.children_with_tokens().find_map(|child| {
            child.into_token().and_then(|token| {
                (token.kind() == SyntaxKind::CODE_SPAN_MARKER).then(|| token.text().to_string())
            })
        })
    }

    pub fn content(&self) -> String {
        self.0
            .children_with_tokens()
            .filter_map(|child| child.into_token())
            .filter(|token| token.kind() != SyntaxKind::CODE_SPAN_MARKER)
            .map(|token| token.text().to_string())
            .collect::<Vec<_>>()
            .join("")
    }

    pub fn content_range(&self) -> Option<rowan::TextRange> {
        let mut markers = self
            .0
            .children_with_tokens()
            .filter_map(|child| child.into_token())
            .filter(|token| token.kind() == SyntaxKind::CODE_SPAN_MARKER);

        let start = markers.next()?.text_range().end();
        let end = markers.next()?.text_range().start();
        (start <= end).then(|| rowan::TextRange::new(start, end))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inline_math_extracts_markers_and_content() {
        let input = "Before $x^2 + y^2$ after\n";
        let tree = crate::parse(input, None);
        let math = tree
            .descendants()
            .find_map(InlineMath::cast)
            .expect("inline math");

        assert_eq!(math.opening_marker().as_deref(), Some("$"));
        assert_eq!(math.closing_marker().as_deref(), Some("$"));
        assert_eq!(math.content(), "x^2 + y^2");
        let range = math.content_range().expect("content range");
        let start: usize = range.start().into();
        let end: usize = range.end().into();
        assert_eq!(&input[start..end], "x^2 + y^2");
    }

    #[test]
    fn code_span_extracts_marker_and_content() {
        let input = "Use `code` here\n";
        let tree = crate::parse(input, None);
        let code = tree
            .descendants()
            .find_map(CodeSpan::cast)
            .expect("code span");

        assert_eq!(code.marker().as_deref(), Some("`"));
        assert_eq!(code.content(), "code");
        let range = code.content_range().expect("content range");
        let start: usize = range.start().into();
        let end: usize = range.end().into();
        assert_eq!(&input[start..end], "code");
    }
}
