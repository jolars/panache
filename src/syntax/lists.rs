//! List AST node wrappers.
//!
//! Lists in Markdown/Pandoc can be either:
//! - **Compact (tight)**: List items contain PLAIN nodes (no blank lines between items)
//! - **Loose**: List items contain PARAGRAPH nodes (blank lines between items)

use super::{AstNode, SyntaxKind, SyntaxNode};

pub struct List(SyntaxNode);

impl AstNode for List {
    fn kind() -> SyntaxKind {
        SyntaxKind::LIST
    }

    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::LIST
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

impl List {
    /// Returns true if this is a loose list (has blank lines between items).
    ///
    /// Loose lists have list items with PARAGRAPH children, while compact lists
    /// have list items with PLAIN children.
    pub fn is_loose(&self) -> bool {
        self.items().any(|item| {
            item.children()
                .any(|child| child.kind() == SyntaxKind::PARAGRAPH)
        })
    }

    /// Returns true if this is a compact/tight list (no blank lines between items).
    ///
    /// This is the inverse of `is_loose()`.
    pub fn is_compact(&self) -> bool {
        !self.is_loose()
    }

    /// Returns an iterator over the list items (LIST_ITEM nodes).
    pub fn items(&self) -> impl Iterator<Item = SyntaxNode> + '_ {
        self.0
            .children()
            .filter(|n| n.kind() == SyntaxKind::LIST_ITEM)
    }
}

pub struct ListItem(SyntaxNode);

impl AstNode for ListItem {
    fn kind() -> SyntaxKind {
        SyntaxKind::LIST_ITEM
    }

    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::LIST_ITEM
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

impl ListItem {
    /// Returns true if this list item contains PARAGRAPH nodes (loose style).
    pub fn is_loose(&self) -> bool {
        self.0
            .children()
            .any(|child| child.kind() == SyntaxKind::PARAGRAPH)
    }

    /// Returns true if this list item contains PLAIN nodes (compact style).
    pub fn is_compact(&self) -> bool {
        self.0
            .children()
            .any(|child| child.kind() == SyntaxKind::PLAIN)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse;

    #[test]
    fn list_wrapper_compact() {
        let input = "- First\n- Second\n- Third\n";
        let tree = parse(input, None);

        let list_node = tree
            .descendants()
            .find(|n| n.kind() == SyntaxKind::LIST)
            .expect("Should find LIST node");

        let list = List::cast(list_node).expect("Should cast to List");

        assert!(list.is_compact(), "List should be compact");
        assert!(!list.is_loose(), "List should not be loose");
        assert_eq!(list.items().count(), 3, "Should have 3 items");
    }

    #[test]
    fn list_wrapper_loose() {
        let input = "- First\n\n- Second\n\n- Third\n";
        let tree = parse(input, None);

        let list_node = tree
            .descendants()
            .find(|n| n.kind() == SyntaxKind::LIST)
            .expect("Should find LIST node");

        let list = List::cast(list_node).expect("Should cast to List");

        assert!(list.is_loose(), "List should be loose");
        assert!(!list.is_compact(), "List should not be compact");
        assert_eq!(list.items().count(), 3, "Should have 3 items");
    }

    #[test]
    fn list_item_wrapper() {
        let input = "- First item\n- Second item\n";
        let tree = parse(input, None);

        let item_nodes: Vec<_> = tree
            .descendants()
            .filter(|n| n.kind() == SyntaxKind::LIST_ITEM)
            .collect();

        assert_eq!(item_nodes.len(), 2, "Should have 2 list items");

        let first_item = ListItem::cast(item_nodes[0].clone()).expect("Should cast to ListItem");
        assert!(
            first_item.is_compact(),
            "First item should be compact (PLAIN)"
        );
        assert!(!first_item.is_loose(), "First item should not be loose");
    }

    #[test]
    fn list_items_iterator() {
        let input = "1. First\n2. Second\n3. Third\n4. Fourth\n";
        let tree = parse(input, None);

        let list_node = tree
            .descendants()
            .find(|n| n.kind() == SyntaxKind::LIST)
            .expect("Should find LIST node");

        let list = List::cast(list_node).expect("Should cast to List");
        let items: Vec<_> = list.items().collect();

        assert_eq!(items.len(), 4, "Should have 4 items");
        for item in items {
            assert_eq!(
                item.kind(),
                SyntaxKind::LIST_ITEM,
                "Each item should be LIST_ITEM"
            );
        }
    }
}
