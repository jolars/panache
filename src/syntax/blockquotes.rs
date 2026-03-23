//! Block quote AST node wrappers.

use super::ast::{AstChildren, support};
use super::{AstNode, PanacheLanguage, SyntaxKind, SyntaxNode};

pub struct BlockQuote(SyntaxNode);

impl AstNode for BlockQuote {
    type Language = PanacheLanguage;

    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::BLOCKQUOTE
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

impl BlockQuote {
    /// Returns block-level children inside this block quote.
    pub fn blocks(&self) -> impl Iterator<Item = SyntaxNode> {
        self.0.children().filter(|child| {
            !matches!(
                child.kind(),
                SyntaxKind::BLOCKQUOTE_MARKER | SyntaxKind::WHITESPACE
            )
        })
    }

    /// Returns nested block quotes directly inside this block quote.
    pub fn nested_blockquotes(&self) -> AstChildren<BlockQuote> {
        support::children(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse;

    #[test]
    fn blockquote_cast_and_blocks() {
        let tree = parse("> Intro\n>\n> - Item\n>\n> Outro\n", None);

        let bq = tree
            .descendants()
            .find_map(BlockQuote::cast)
            .expect("blockquote");

        let kinds: Vec<_> = bq.blocks().map(|n| n.kind()).collect();
        assert!(kinds.contains(&SyntaxKind::PARAGRAPH));
        assert!(kinds.contains(&SyntaxKind::LIST));
    }

    #[test]
    fn blockquote_nested_blockquotes_iterator() {
        let tree = parse("> outer\n>\n> > inner\n", None);

        let outer = tree
            .descendants()
            .find_map(BlockQuote::cast)
            .expect("outer blockquote");

        assert!(outer.nested_blockquotes().next().is_some());
    }
}
