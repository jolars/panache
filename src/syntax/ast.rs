//! AST node trait and support utilities.

use super::{SyntaxKind, SyntaxNode, SyntaxToken};

/// Trait for typed AST node wrappers.
///
/// This provides type-safe access to syntax tree nodes with ergonomic APIs.
/// Pattern borrowed from rust-analyzer.
pub trait AstNode: Sized {
    /// Returns the `SyntaxKind` for this node type.
    fn kind() -> SyntaxKind
    where
        Self: Sized;

    /// Checks if a `SyntaxKind` can be cast to this node type.
    fn can_cast(kind: SyntaxKind) -> bool
    where
        Self: Sized;

    /// Attempts to cast a `SyntaxNode` to this typed wrapper.
    fn cast(syntax: SyntaxNode) -> Option<Self>
    where
        Self: Sized;

    /// Returns a reference to the underlying `SyntaxNode`.
    fn syntax(&self) -> &SyntaxNode;
}

/// Helper functions for accessing children.
pub(super) mod support {
    use super::{AstNode, SyntaxKind, SyntaxNode, SyntaxToken};

    /// Find the first child node of a specific type.
    pub(crate) fn child<N: AstNode>(node: &SyntaxNode) -> Option<N> {
        node.children().find_map(N::cast)
    }

    /// Find all child nodes of a specific type.
    pub(crate) fn children<'a, N: AstNode + 'a>(
        node: &'a SyntaxNode,
    ) -> impl Iterator<Item = N> + 'a {
        node.children().filter_map(N::cast)
    }

    /// Find the first token of a specific kind.
    pub(crate) fn token(node: &SyntaxNode, kind: SyntaxKind) -> Option<SyntaxToken> {
        node.children_with_tokens()
            .filter_map(|it| it.into_token())
            .find(|it| it.kind() == kind)
    }
}
