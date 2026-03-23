//! Definition list AST node wrappers.

use super::ast::{AstChildren, support};
use super::{AstNode, PanacheLanguage, SyntaxKind, SyntaxNode};

pub struct DefinitionList(SyntaxNode);

impl AstNode for DefinitionList {
    type Language = PanacheLanguage;

    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::DEFINITION_LIST
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

impl DefinitionList {
    pub fn items(&self) -> AstChildren<DefinitionItem> {
        support::children(&self.0)
    }
}

pub struct DefinitionItem(SyntaxNode);

impl AstNode for DefinitionItem {
    type Language = PanacheLanguage;

    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::DEFINITION_ITEM
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

impl DefinitionItem {
    pub fn definitions(&self) -> AstChildren<Definition> {
        support::children(&self.0)
    }

    pub fn is_compact(&self) -> bool {
        let definitions: Vec<_> = self.definitions().collect();
        if definitions.is_empty() {
            return true;
        }

        definitions.into_iter().all(|definition| {
            let blocks: Vec<_> = definition
                .syntax()
                .children()
                .filter(|child| child.kind() != SyntaxKind::BLANK_LINE)
                .collect();

            if blocks.len() != 1 {
                return false;
            }

            match blocks[0].kind() {
                SyntaxKind::PLAIN | SyntaxKind::PARAGRAPH => {
                    !has_leading_atx_heading_with_remainder(&blocks[0].text().to_string())
                }
                SyntaxKind::CODE_BLOCK => true,
                _ => false,
            }
        })
    }

    pub fn is_loose(&self) -> bool {
        !self.is_compact()
    }
}

fn has_leading_atx_heading_with_remainder(text: &str) -> bool {
    let mut lines = text.lines();
    let Some(first_line) = lines.next() else {
        return false;
    };

    if !looks_like_atx_heading(first_line) {
        return false;
    }

    lines.flat_map(str::split_whitespace).next().is_some()
}

fn looks_like_atx_heading(line: &str) -> bool {
    let trimmed = line.trim_start_matches([' ', '\t']);
    let level = trimmed.chars().take_while(|ch| *ch == '#').count();
    if !(1..=6).contains(&level) {
        return false;
    }

    match trimmed.chars().nth(level) {
        Some(ch) => ch == ' ' || ch == '\t',
        None => true,
    }
}

pub struct Definition(SyntaxNode);

impl AstNode for Definition {
    type Language = PanacheLanguage;

    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::DEFINITION
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse;

    #[test]
    fn definition_item_compact_single_plain_block() {
        let tree = parse("Term\n: Def\n", None);
        let item = tree
            .descendants()
            .find_map(DefinitionItem::cast)
            .expect("definition item");
        assert!(item.is_compact());
        assert!(!item.is_loose());
    }

    #[test]
    fn definition_item_compact_single_code_block() {
        let tree = parse("Term\n: ```r\n  a <- 1\n  ```\n", None);
        let item = tree
            .descendants()
            .find_map(DefinitionItem::cast)
            .expect("definition item");
        assert!(item.is_compact());
    }

    #[test]
    fn definition_item_loose_when_definition_is_multiblock() {
        let tree = parse("Term\n: # Heading\n\n  Text\n", None);
        let item = tree
            .descendants()
            .find_map(DefinitionItem::cast)
            .expect("definition item");
        assert!(item.is_loose());
    }

    #[test]
    fn definition_item_loose_for_plain_heading_with_remainder() {
        let tree = parse("Term\n: # Heading\n  Some text\n", None);
        let item = tree
            .descendants()
            .find_map(DefinitionItem::cast)
            .expect("definition item");
        assert!(item.is_loose());
    }

    #[test]
    fn definition_item_compact_for_multiple_simple_definitions() {
        let tree = parse("Term\n: Def one\n\n: Def two\n", None);
        let item = tree
            .descendants()
            .find_map(DefinitionItem::cast)
            .expect("definition item");
        assert!(item.is_compact());
    }
}
