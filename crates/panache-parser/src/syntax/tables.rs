//! Table AST node wrappers.

use super::ast::{AstChildren, support};
use super::{AstNode, PanacheLanguage, SyntaxKind, SyntaxNode, SyntaxToken};
use unicode_width::UnicodeWidthChar;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TableAlignment {
    Default,
    Left,
    Center,
    Right,
}

/// Slice table text at display-column boundaries, keeping combining marks
/// with the character before them. Separators are ASCII, but cell text is not.
pub fn display_column_slice(text: &str, start: usize, end: usize) -> &str {
    column_slice_with_width(text, start, end, |ch| ch.width().unwrap_or(0))
}

pub(crate) fn column_slice_with_width(
    text: &str,
    start: usize,
    end: usize,
    char_width: fn(char) -> usize,
) -> &str {
    let mut width = 0;
    let mut first = None;
    let mut last = text.len();
    for (offset, ch) in text.char_indices() {
        let char_width = char_width(ch);
        if first.is_none() && width >= start && (char_width > 0 || offset == 0) {
            first = Some(offset);
        }
        if width >= end && char_width > 0 {
            last = offset;
            break;
        }
        width += char_width;
    }
    &text[first.unwrap_or(last)..last]
}

/// `node`'s text with each line's container-prefix tokens skipped.
///
/// A continuation line inside a block node carries the enclosing
/// containers' prefix bytes as `LINE_PREFIX` tokens at line start;
/// line 0's prefix sits *outside* the node. So `node.text()` is line 0
/// dedented and lines 1..n still prefixed — wrong by construction for
/// any consumer doing column geometry across lines (grid layout, table
/// measurement).
///
/// The skip is structural: prefix bytes carry their own kind, so a
/// line-leading `WHITESPACE` token that is genuine content survives.
pub fn text_without_line_prefixes(node: &SyntaxNode) -> String {
    let mut out = String::new();
    for token in node
        .descendants_with_tokens()
        .filter_map(|el| el.into_token())
    {
        if token.kind() != SyntaxKind::LINE_PREFIX {
            out.push_str(token.text());
        }
    }
    out
}

/// The separator-marker tokens (`TABLE_SEP_*`) of a `TABLE_SEPARATOR` node,
/// in order. Skips the container prefix (`LINE_PREFIX`)
/// and the trailing `NEWLINE` so callers see only the separator's own
/// structure.
pub fn separator_marker_tokens(separator: &SyntaxNode) -> impl Iterator<Item = SyntaxToken> {
    separator
        .children_with_tokens()
        .filter_map(|el| el.into_token())
        .filter(|t| {
            matches!(
                t.kind(),
                SyntaxKind::TABLE_SEP_DELIM
                    | SyntaxKind::TABLE_SEP_DASHES
                    | SyntaxKind::TABLE_SEP_EQUALS
                    | SyntaxKind::TABLE_SEP_COLON
                    | SyntaxKind::TABLE_SEP_WHITESPACE
            )
        })
}

/// A pipe-table delimiter row split into one segment of marker tokens per
/// column. This is the authoritative definition of how many columns a pipe
/// table has: pandoc's `pipeTable` reads the count off the delimiter row and
/// pads or truncates every other row to it.
///
/// Reproduces pandoc's
/// `raw.trim().trim_start_matches('|').trim_end_matches('|').split('|')`:
/// trim interior whitespace at the ends, drop the bounding delimiter runs,
/// then take one column per interior delimiter gap. A delimiter row always
/// declares at least one column.
pub fn separator_column_segments(separator: &SyntaxNode) -> Vec<Vec<SyntaxToken>> {
    let toks: Vec<SyntaxToken> = separator_marker_tokens(separator).collect();
    let is_ws = |t: &SyntaxToken| t.kind() == SyntaxKind::TABLE_SEP_WHITESPACE;
    let is_delim = |t: &SyntaxToken| t.kind() == SyntaxKind::TABLE_SEP_DELIM;
    let lo = toks.iter().position(|t| !is_ws(t));
    let hi = toks.iter().rposition(|t| !is_ws(t));
    let inner = match (lo, hi) {
        (Some(lo), Some(hi)) => &toks[lo..=hi],
        _ => &[][..], // whitespace-only: empty inner → one default column below
    };
    let lead = inner.iter().take_while(|t| is_delim(t)).count();
    let inner = &inner[lead..];
    let trail = inner.iter().rev().take_while(|t| is_delim(t)).count();
    let inner = &inner[..inner.len() - trail];

    let mut segments = Vec::new();
    let mut seg_start = 0usize;
    for (i, t) in inner.iter().enumerate() {
        if is_delim(t) {
            segments.push(inner[seg_start..i].to_vec());
            seg_start = i + 1;
        }
    }
    segments.push(inner[seg_start..].to_vec());
    segments
}

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

    /// The delimiter row, which owns the table's column count.
    pub fn separator(&self) -> Option<SyntaxNode> {
        self.0
            .children()
            .find(|c| c.kind() == SyntaxKind::TABLE_SEPARATOR)
    }

    /// How many columns the delimiter row declares. Cells past this count are
    /// dropped when the table is rendered, and rows short of it are padded.
    pub fn column_count(&self) -> Option<usize> {
        self.separator()
            .map(|sep| separator_column_segments(&sep).len())
    }

    pub fn alignments(&self) -> Vec<TableAlignment> {
        self.separator()
            .map(|separator| {
                separator_column_segments(&separator)
                    .into_iter()
                    .map(|segment| {
                        let first = segment
                            .iter()
                            .find(|token| token.kind() != SyntaxKind::TABLE_SEP_WHITESPACE);
                        let last = segment
                            .iter()
                            .rev()
                            .find(|token| token.kind() != SyntaxKind::TABLE_SEP_WHITESPACE);
                        match (
                            first.is_some_and(|token| token.kind() == SyntaxKind::TABLE_SEP_COLON),
                            last.is_some_and(|token| token.kind() == SyntaxKind::TABLE_SEP_COLON),
                        ) {
                            (true, true) => TableAlignment::Center,
                            (true, false) => TableAlignment::Left,
                            (false, true) => TableAlignment::Right,
                            (false, false) => TableAlignment::Default,
                        }
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Every row that carries cells, header included. `TABLE_HEADER` is a
    /// distinct kind from `TABLE_ROW`, so [`Self::rows`] alone skips it.
    pub fn cell_rows(&self) -> impl Iterator<Item = SyntaxNode> {
        self.0.children().filter(|child| {
            matches!(
                child.kind(),
                SyntaxKind::TABLE_HEADER | SyntaxKind::TABLE_ROW
            )
        })
    }

    /// Typed consumer rows, including the header and body.
    pub fn all_rows(&self) -> impl Iterator<Item = TableRowNode> {
        self.0.children().filter_map(TableRowNode::cast)
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

    pub fn rows(&self) -> Vec<TableRowNode> {
        self.syntax()
            .children()
            .filter_map(TableRowNode::cast)
            .collect()
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
    /// Recover cell geometry after removing captions and container prefixes.
    pub fn layout(&self) -> Option<crate::GridLayout> {
        let text: String = self
            .0
            .children()
            .filter(|child| child.kind() != SyntaxKind::TABLE_CAPTION)
            .map(|child| text_without_line_prefixes(&child))
            .collect();
        crate::analyze_grid(&text.lines().collect::<Vec<_>>())
    }

    /// The header separator controls alignment when present; otherwise the
    /// opening border does. Colons on later body borders are not alignment.
    pub fn alignment_separator(&self) -> Option<SyntaxNode> {
        let mut separators = self
            .0
            .children()
            .filter(|child| child.kind() == SyntaxKind::TABLE_SEPARATOR);
        let first = separators.next()?;
        Some(
            separators
                .find(|separator| {
                    separator_marker_tokens(separator)
                        .any(|token| token.kind() == SyntaxKind::TABLE_SEP_EQUALS)
                })
                .unwrap_or(first),
        )
    }

    pub fn alignments(&self) -> Vec<TableAlignment> {
        self.alignment_separator()
            .map(|separator| {
                separator_column_segments(&separator)
                    .iter()
                    .map(|segment| {
                        match (
                            segment
                                .first()
                                .is_some_and(|t| t.kind() == SyntaxKind::TABLE_SEP_COLON),
                            segment
                                .last()
                                .is_some_and(|t| t.kind() == SyntaxKind::TABLE_SEP_COLON),
                        ) {
                            (true, true) => TableAlignment::Center,
                            (true, false) => TableAlignment::Left,
                            (false, true) => TableAlignment::Right,
                            (false, false) => TableAlignment::Default,
                        }
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

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

/// Consumer row view that includes both a table header and body rows.
pub struct TableRowNode(SyntaxNode);

impl AstNode for TableRowNode {
    type Language = PanacheLanguage;

    fn can_cast(kind: SyntaxKind) -> bool {
        matches!(kind, SyntaxKind::TABLE_HEADER | SyntaxKind::TABLE_ROW)
    }

    fn cast(syntax: SyntaxNode) -> Option<Self> {
        Self::can_cast(syntax.kind()).then(|| Self(syntax))
    }

    fn syntax(&self) -> &SyntaxNode {
        &self.0
    }
}

impl TableRowNode {
    pub fn is_header(&self) -> bool {
        self.0.kind() == SyntaxKind::TABLE_HEADER
    }

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
    fn text_without_line_prefixes_drops_item_indent() {
        let input = "- +---+---+\n  | a | b |\n  +===+===+\n  | 1 | 2 |\n  +---+---+\n";
        let tree = crate::parse(input, None);
        let table = tree
            .descendants()
            .find(|n| n.kind() == SyntaxKind::GRID_TABLE)
            .expect("grid table node");
        assert_eq!(
            text_without_line_prefixes(&table),
            "+---+---+\n| a | b |\n+===+===+\n| 1 | 2 |\n+---+---+\n"
        );
    }

    #[test]
    fn text_without_line_prefixes_drops_blockquote_markers() {
        let input = "> +---+---+\n> | a | b |\n> +===+===+\n> | 1 | 2 |\n> +---+---+\n";
        let tree = crate::parse(input, None);
        let table = tree
            .descendants()
            .find(|n| n.kind() == SyntaxKind::GRID_TABLE)
            .expect("grid table node");
        assert_eq!(
            text_without_line_prefixes(&table),
            "+---+---+\n| a | b |\n+===+===+\n| 1 | 2 |\n+---+---+\n"
        );
    }

    #[test]
    fn prefix_runs_inside_content_nodes_are_line_prefix_tokens() {
        for input in [
            "- +---+---+\n  | a | b |\n  +===+===+\n  | 1 | 2 |\n  +---+---+\n",
            "> +---+---+\n> | a | b |\n> +===+===+\n> | 1 | 2 |\n> +---+---+\n",
        ] {
            let tree = crate::parse(input, None);
            let table = tree
                .descendants()
                .find(|n| n.kind() == SyntaxKind::GRID_TABLE)
                .expect("grid table node");
            let mut at_line_start = true;
            let mut prefix_tokens = 0usize;
            for token in table
                .descendants_with_tokens()
                .filter_map(|el| el.into_token())
            {
                match token.kind() {
                    SyntaxKind::LINE_PREFIX => {
                        assert!(
                            at_line_start,
                            "LINE_PREFIX off line start: {token:?} in {input:?}"
                        );
                        prefix_tokens += 1;
                    }
                    SyntaxKind::NEWLINE | SyntaxKind::BLANK_LINE => at_line_start = true,
                    SyntaxKind::WHITESPACE if at_line_start => {
                        panic!("untagged line-leading WHITESPACE: {token:?} in {input:?}")
                    }
                    SyntaxKind::BLOCK_QUOTE_MARKER => {
                        panic!("untagged prefix marker: {token:?} in {input:?}")
                    }
                    _ => at_line_start = false,
                }
            }
            assert!(prefix_tokens >= 4, "expected prefix tokens in {input:?}");
        }
    }

    #[test]
    fn text_without_line_prefixes_keeps_unprefixed_text() {
        let input = "+---+---+\n| a | b |\n+===+===+\n| 1 | 2 |\n+---+---+\n";
        let tree = crate::parse(input, None);
        let table = tree
            .descendants()
            .find(|n| n.kind() == SyntaxKind::GRID_TABLE)
            .expect("grid table node");
        assert_eq!(text_without_line_prefixes(&table), input);
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
