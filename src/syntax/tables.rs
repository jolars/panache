//! Table AST node wrappers.

use super::ast::support;
use super::{AstNode, SyntaxKind, SyntaxNode};

pub struct PipeTable(SyntaxNode);

impl AstNode for PipeTable {
    fn kind() -> SyntaxKind {
        SyntaxKind::PIPE_TABLE
    }

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
    pub fn rows(&self) -> impl Iterator<Item = TableRow> + '_ {
        support::children(&self.0)
    }
}

pub struct GridTable(SyntaxNode);

impl AstNode for GridTable {
    fn kind() -> SyntaxKind {
        SyntaxKind::GRID_TABLE
    }

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
    pub fn rows(&self) -> impl Iterator<Item = TableRow> + '_ {
        support::children(&self.0)
    }
}

pub struct SimpleTable(SyntaxNode);

impl AstNode for SimpleTable {
    fn kind() -> SyntaxKind {
        SyntaxKind::SIMPLE_TABLE
    }

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
    pub fn rows(&self) -> impl Iterator<Item = TableRow> + '_ {
        support::children(&self.0)
    }
}

pub struct MultilineTable(SyntaxNode);

impl AstNode for MultilineTable {
    fn kind() -> SyntaxKind {
        SyntaxKind::MULTILINE_TABLE
    }

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
    pub fn rows(&self) -> impl Iterator<Item = TableRow> + '_ {
        support::children(&self.0)
    }
}

pub struct TableCaption(SyntaxNode);

impl AstNode for TableCaption {
    fn kind() -> SyntaxKind {
        SyntaxKind::TABLE_CAPTION
    }

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
            .children_with_tokens()
            .filter_map(|it| it.into_token())
            .filter(|token| token.kind() == SyntaxKind::TEXT)
            .map(|token| token.text().to_string())
            .collect()
    }
}

pub struct TableRow(SyntaxNode);

impl AstNode for TableRow {
    fn kind() -> SyntaxKind {
        SyntaxKind::TABLE_ROW
    }

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
    pub fn cells(&self) -> impl Iterator<Item = TableCell> + '_ {
        support::children(&self.0)
    }
}

pub struct TableCell(SyntaxNode);

impl AstNode for TableCell {
    fn kind() -> SyntaxKind {
        SyntaxKind::TABLE_CELL
    }

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
