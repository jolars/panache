//! Table AST node wrappers.

use super::ast::{AstChildren, support};
use super::{AstNode, PanacheLanguage, SyntaxKind, SyntaxNode};

pub struct PipeTable(SyntaxNode);

impl AstNode for PipeTable {
    type Language = PanacheLanguage;

    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::PIPE_TABLE
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

impl PipeTable {
    /// Returns the table caption if present.
    pub fn caption(&self) -> Option<TableCaption> {
        support::child(&self.0)
    }

    /// Returns all table rows.
    pub fn rows(&self) -> AstChildren<TableRow> {
        support::children(&self.0)
    }
}

pub enum Table {
    Pipe(PipeTable),
    Grid(GridTable),
    Simple(SimpleTable),
    Multiline(MultilineTable),
}

impl Table {
    pub fn cast(syntax: SyntaxNode) -> Option<Self> {
        if let Some(table) = PipeTable::cast(syntax.clone()) {
            return Some(Self::Pipe(table));
        }
        if let Some(table) = GridTable::cast(syntax.clone()) {
            return Some(Self::Grid(table));
        }
        if let Some(table) = SimpleTable::cast(syntax.clone()) {
            return Some(Self::Simple(table));
        }
        MultilineTable::cast(syntax).map(Self::Multiline)
    }

    pub fn syntax(&self) -> &SyntaxNode {
        match self {
            Self::Pipe(table) => table.syntax(),
            Self::Grid(table) => table.syntax(),
            Self::Simple(table) => table.syntax(),
            Self::Multiline(table) => table.syntax(),
        }
    }

    pub fn caption(&self) -> Option<TableCaption> {
        match self {
            Self::Pipe(table) => table.caption(),
            Self::Grid(table) => table.caption(),
            Self::Simple(table) => table.caption(),
            Self::Multiline(table) => table.caption(),
        }
    }
}

pub struct GridTable(SyntaxNode);

impl AstNode for GridTable {
    type Language = PanacheLanguage;

    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::GRID_TABLE
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

impl GridTable {
    /// Returns the table caption if present.
    pub fn caption(&self) -> Option<TableCaption> {
        support::child(&self.0)
    }

    /// Returns all table rows.
    pub fn rows(&self) -> AstChildren<TableRow> {
        support::children(&self.0)
    }
}

pub struct SimpleTable(SyntaxNode);

impl AstNode for SimpleTable {
    type Language = PanacheLanguage;

    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::SIMPLE_TABLE
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

impl SimpleTable {
    /// Returns the table caption if present.
    pub fn caption(&self) -> Option<TableCaption> {
        support::child(&self.0)
    }

    /// Returns all table rows.
    pub fn rows(&self) -> AstChildren<TableRow> {
        support::children(&self.0)
    }
}

pub struct MultilineTable(SyntaxNode);

impl AstNode for MultilineTable {
    type Language = PanacheLanguage;

    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::MULTILINE_TABLE
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

impl MultilineTable {
    /// Returns the table caption if present.
    pub fn caption(&self) -> Option<TableCaption> {
        support::child(&self.0)
    }

    /// Returns all table rows.
    pub fn rows(&self) -> AstChildren<TableRow> {
        support::children(&self.0)
    }
}

pub struct TableCaption(SyntaxNode);

impl AstNode for TableCaption {
    type Language = PanacheLanguage;

    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::TABLE_CAPTION
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

impl TableCaption {
    /// Returns the caption text.
    pub fn text(&self) -> String {
        self.0
            .descendants_with_tokens()
            .filter_map(|it| it.into_token())
            .filter(|token| token.kind() == SyntaxKind::TEXT)
            .map(|token| token.text().to_string())
            .collect()
    }
}

pub struct TableRow(SyntaxNode);

impl AstNode for TableRow {
    type Language = PanacheLanguage;

    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::TABLE_ROW
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

impl TableRow {
    /// Returns all cells in this row.
    pub fn cells(&self) -> AstChildren<TableCell> {
        support::children(&self.0)
    }
}

pub struct TableCell(SyntaxNode);

impl AstNode for TableCell {
    type Language = PanacheLanguage;

    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::TABLE_CELL
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

    #[test]
    fn table_wrapper_casts_pipe_table_and_reads_caption() {
        let input = "| a | b |\n|---|---|\n| 1 | 2 |\n: Caption\n";
        let tree = crate::parse(input, None);
        let node = tree
            .descendants()
            .find(|n| n.kind() == SyntaxKind::PIPE_TABLE)
            .expect("pipe table node");

        let table = Table::cast(node).expect("table wrapper");
        assert_eq!(
            table.caption().map(|caption| caption.text()),
            Some("Caption".to_string())
        );
    }

    #[test]
    fn table_wrapper_does_not_cast_non_table_nodes() {
        let tree = crate::parse("Paragraph\n", None);
        let paragraph = tree
            .descendants()
            .find(|n| n.kind() == SyntaxKind::PARAGRAPH)
            .expect("paragraph node");
        assert!(Table::cast(paragraph).is_none());
    }
}
