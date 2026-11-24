//! AST node trait and support utilities.

pub(super) use rowan::ast::support;
pub use rowan::ast::{AstChildren, AstNode};

use super::{SyntaxKind, SyntaxNode, SyntaxToken};

pub(super) fn raw_text(node: &SyntaxNode) -> String {
    node.descendants_with_tokens()
        .filter_map(|element| element.into_token())
        .filter(|token| token.kind() != SyntaxKind::LINE_PREFIX)
        .fold(String::new(), |mut text, token| {
            text.push_str(token.text());
            text
        })
}

pub(super) fn visible_text_tokens(node: &SyntaxNode) -> impl Iterator<Item = SyntaxToken> {
    visible_text_tokens_excluding(node, |_| false)
}

pub(super) fn visible_text_tokens_excluding(
    node: &SyntaxNode,
    exclude: impl Fn(SyntaxKind) -> bool,
) -> impl Iterator<Item = SyntaxToken> {
    let mut walk = node.preorder_with_tokens();
    std::iter::from_fn(move || {
        loop {
            match walk.next()? {
                rowan::WalkEvent::Enter(rowan::NodeOrToken::Node(node))
                    if matches!(
                        node.kind(),
                        SyntaxKind::HTML_ATTRS | SyntaxKind::INLINE_HTML
                    ) || exclude(node.kind()) =>
                {
                    walk.skip_subtree();
                }
                rowan::WalkEvent::Enter(rowan::NodeOrToken::Token(token))
                    if token
                        .parent()
                        .is_none_or(|parent| parent.kind() != SyntaxKind::INLINE_HTML_SPAN) =>
                {
                    // Native HTML spans keep their tag delimiters as direct
                    // TEXT children; their visible content has its own wrapper.
                    return Some(token);
                }
                _ => {}
            }
        }
    })
}
