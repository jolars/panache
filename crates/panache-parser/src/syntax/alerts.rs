//! Alert block AST node wrappers.

use super::ast::support;
use super::{AstChildren, AstNode, PanacheLanguage, Paragraph, SyntaxKind, SyntaxNode};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Alert(SyntaxNode);

impl AstNode for Alert {
    type Language = PanacheLanguage;

    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::ALERT
    }

    fn cast(syntax: SyntaxNode) -> Option<Self> {
        Self::can_cast(syntax.kind()).then(|| Self(syntax))
    }

    fn syntax(&self) -> &SyntaxNode {
        &self.0
    }
}

impl Alert {
    pub fn marker(&self) -> Option<String> {
        self.0
            .children_with_tokens()
            .filter_map(|child| child.into_token())
            .find(|token| token.kind() == SyntaxKind::ALERT_MARKER)
            .map(|token| token.text().to_string())
    }

    pub fn paragraphs(&self) -> AstChildren<Paragraph> {
        support::children(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Extensions, Flavor, ParserOptions};
    use crate::parse;

    #[test]
    fn alert_wrapper_extracts_marker_and_paragraphs() {
        let mut extensions = Extensions::for_flavor(Flavor::Gfm);
        extensions.alerts = true;
        let config = ParserOptions {
            flavor: Flavor::Gfm,
            extensions,
            ..Default::default()
        };
        let tree = parse("> [!NOTE]\n> Heads up\n> More context\n", Some(config));

        let alert = tree.descendants().find_map(Alert::cast).expect("alert");
        assert_eq!(alert.marker().as_deref(), Some("[!NOTE]"));
        let paragraphs = alert.paragraphs().collect::<Vec<_>>();
        assert_eq!(paragraphs.len(), 1);
        let text = paragraphs[0].syntax().text().to_string();
        assert!(text.contains("Heads up"));
        assert!(text.contains("More context"));
    }
}
