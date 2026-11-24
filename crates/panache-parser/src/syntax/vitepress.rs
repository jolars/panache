//! Typed access to VitePress colon containers.

use rowan::ast::AstNode;

use super::{PanacheLanguage, SyntaxKind, SyntaxNode, SyntaxToken};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct VitePressContainer(SyntaxNode);

impl AstNode for VitePressContainer {
    type Language = PanacheLanguage;

    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::VITEPRESS_CONTAINER
    }

    fn cast(node: SyntaxNode) -> Option<Self> {
        Self::can_cast(node.kind()).then_some(Self(node))
    }

    fn syntax(&self) -> &SyntaxNode {
        &self.0
    }
}

impl VitePressContainer {
    pub fn opening_fence(&self) -> Option<SyntaxNode> {
        self.0
            .first_child()
            .filter(|child| child.kind() == SyntaxKind::VITEPRESS_CONTAINER_OPEN)
    }

    pub fn closing_fence(&self) -> Option<SyntaxNode> {
        self.0
            .last_child()
            .filter(|child| child.kind() == SyntaxKind::VITEPRESS_CONTAINER_CLOSE)
    }

    pub fn container_type(&self) -> Option<SyntaxToken> {
        self.opening_token(SyntaxKind::VITEPRESS_CONTAINER_TYPE)
    }

    /// The title as written, including any trailing VitePress attributes.
    pub fn title(&self) -> Option<SyntaxToken> {
        self.opening_token(SyntaxKind::VITEPRESS_CONTAINER_TITLE)
    }

    pub fn body(&self) -> impl Iterator<Item = SyntaxNode> {
        self.0.children().filter(|child| {
            !matches!(
                child.kind(),
                SyntaxKind::VITEPRESS_CONTAINER_OPEN | SyntaxKind::VITEPRESS_CONTAINER_CLOSE
            )
        })
    }

    fn opening_token(&self, kind: SyntaxKind) -> Option<SyntaxToken> {
        self.opening_fence()?
            .children_with_tokens()
            .filter_map(|element| element.into_token())
            .find(|token| token.kind() == kind)
    }
}
