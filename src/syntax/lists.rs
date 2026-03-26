//! List AST node wrappers.
//!
//! Lists in Markdown/Pandoc can be either:
//! - **Compact (tight)**: List items contain PLAIN nodes (no blank lines between items)
//! - **Loose**: List items contain PARAGRAPH nodes (blank lines between items)

use super::ast::{AstChildren, support};
use super::{AstNode, PanacheLanguage, SyntaxKind, SyntaxNode};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListKind {
    Bullet,
    Ordered,
    Task,
}

pub struct List(SyntaxNode);

impl AstNode for List {
    type Language = PanacheLanguage;

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
    pub fn is_loose(&self) -> bool {
        self.0
            .children()
            .any(|n| n.kind() == SyntaxKind::BLANK_LINE)
    }

    /// Returns true if this is a compact/tight list (no blank lines between items).
    ///
    /// This is the inverse of `is_loose()`.
    pub fn is_compact(&self) -> bool {
        !self.is_loose()
    }

    /// Returns an iterator over the list items (LIST_ITEM nodes).
    pub fn items(&self) -> AstChildren<ListItem> {
        support::children(&self.0)
    }

    /// Returns the semantic kind of this list.
    pub fn kind(&self) -> Option<ListKind> {
        let first_item = self.items().next()?;
        if first_item.is_task() {
            return Some(ListKind::Task);
        }
        let marker = first_item.marker()?;
        if matches!(marker.as_str(), "-" | "*" | "+") {
            Some(ListKind::Bullet)
        } else {
            Some(ListKind::Ordered)
        }
    }
}

pub struct ListItem(SyntaxNode);

impl AstNode for ListItem {
    type Language = PanacheLanguage;

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

    pub fn marker(&self) -> Option<String> {
        self.0.children_with_tokens().find_map(|elem| {
            elem.as_token()
                .filter(|token| token.kind() == SyntaxKind::LIST_MARKER)
                .map(|token| token.text().to_string())
        })
    }

    pub fn is_task(&self) -> bool {
        self.0.children_with_tokens().any(|elem| {
            elem.as_token()
                .is_some_and(|token| token.kind() == SyntaxKind::TASK_CHECKBOX)
        })
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

        let list = tree
            .descendants()
            .find_map(List::cast)
            .expect("Should find List");

        assert_eq!(list.items().count(), 2, "Should have 2 list items");

        let first_item = list.items().next().expect("Should have list item");
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

        assert_eq!(list.items().count(), 4, "Should have 4 items");
        for item in list.items() {
            assert_eq!(
                item.syntax().kind(),
                SyntaxKind::LIST_ITEM,
                "Each item should be LIST_ITEM"
            );
        }
    }

    #[test]
    fn list_kind_detection() {
        let bullet_tree = parse("- First\n- Second\n", None);
        let bullet_list = bullet_tree
            .descendants()
            .find_map(List::cast)
            .expect("Should find bullet list");
        assert_eq!(bullet_list.kind(), Some(ListKind::Bullet));

        let ordered_tree = parse("1. First\n2. Second\n", None);
        let ordered_list = ordered_tree
            .descendants()
            .find_map(List::cast)
            .expect("Should find ordered list");
        assert_eq!(ordered_list.kind(), Some(ListKind::Ordered));

        let task_tree = parse("- [ ] First\n- [x] Second\n", None);
        let task_list = task_tree
            .descendants()
            .find_map(List::cast)
            .expect("Should find task list");
        assert_eq!(task_list.kind(), Some(ListKind::Task));
    }
}
