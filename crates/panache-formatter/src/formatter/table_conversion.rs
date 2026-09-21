//! Explicit, content-preserving conversions between Markdown table styles.

use crate::{
    Config,
    syntax::{
        AstNode, InlineHtml, LatexCommand, SyntaxKind, SyntaxNode, Table, TableAlignment,
        display_column_slice as column_slice, text_without_line_prefixes,
    },
};
use panache_parser::{
    grid_layout::grid_display_width,
    parser::{
        blocks::html_blocks::is_pandoc_block_tag_name, inlines::core::parse_inline_text_recursive,
    },
};
use rowan::{GreenNodeBuilder, NodeOrToken};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use super::{
    inline::collapse_spaces,
    tables::{Alignment, pad_simple_cell},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TableStyle {
    Pipe,
    Simple,
    Multiline,
    Grid,
}

impl TableStyle {
    pub fn syntax_kind(self) -> SyntaxKind {
        match self {
            Self::Pipe => SyntaxKind::PIPE_TABLE,
            Self::Simple => SyntaxKind::SIMPLE_TABLE,
            Self::Multiline => SyntaxKind::MULTILINE_TABLE,
            Self::Grid => SyntaxKind::GRID_TABLE,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TableConversionError {
    UnsupportedSource,
    DisabledExtension,
    RaggedRows,
    MissingBody,
    SpanningCells,
    BlockContent,
    MultipleHeaders,
    Footer,
    HardLineBreak,
    MultilineLiteral,
    EastAsianLineBreak,
    Alignment,
    ContainerLayout,
    InvalidOutput,
}

impl std::fmt::Display for TableConversionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::UnsupportedSource => "The source table style is not supported",
            Self::DisabledExtension => "The destination table extension is disabled",
            Self::RaggedRows => "Rows do not all have the declared number of columns",
            Self::MissingBody => "The table has no body rows",
            Self::SpanningCells => "The destination cannot preserve merged cells",
            Self::BlockContent => "The conversion cannot preserve block structure inside cells",
            Self::MultipleHeaders => "The destination cannot preserve multiple header rows",
            Self::Footer => "The destination cannot preserve table footers",
            Self::HardLineBreak => "A cell contains a hard line break",
            Self::MultilineLiteral => "A cell contains an inline construct spanning multiple lines",
            Self::EastAsianLineBreak => "Conversion of East Asian line breaks is not supported yet",
            Self::Alignment => "The destination cannot preserve an explicit column alignment",
            Self::ContainerLayout => {
                "The destination cannot preserve this table's container layout"
            }
            Self::InvalidOutput => {
                "The conversion cannot preserve this table's structure and content"
            }
        })
    }
}

impl std::error::Error for TableConversionError {}

/// Breaks are permitted only between pieces, never inside an inline construct.
#[derive(Debug, Clone)]
struct Cell {
    pieces: Vec<String>,
    pipe_text: String,
    comparison: String,
}

impl Cell {
    fn parse_grid(text: &str, config: &Config) -> Result<Self, TableConversionError> {
        // The fragment parser does not recognize every Pandoc list marker yet.
        // Reject these forms before trusting its paragraph classification.
        if text
            .lines()
            .any(|line| unparsed_grid_list_marker(line, config))
        {
            return Err(TableConversionError::BlockContent);
        }
        // Grid extraction omits the final newline, but a terminal backslash
        // still creates a hard break in the cell's block content.
        let text = format!("{text}\n");
        if !text.trim().is_empty() {
            let options = config.parser_options();
            let tree = panache_parser::parser::Parser::new_fragment(&text, &options).parse();
            let blocks: Vec<_> = tree
                .children()
                .filter(|node| node.kind() != SyntaxKind::BLANK_LINE)
                .collect();
            if blocks.len() != 1 || blocks[0].kind() != SyntaxKind::PARAGRAPH {
                return Err(TableConversionError::BlockContent);
            }
            // The fragment parser can leave same-line HTML block interruptions
            // inside a paragraph. Pandoc ends the paragraph at these tags.
            if config.dialect() == panache_parser::Dialect::Pandoc
                && blocks[0]
                    .descendants()
                    .filter_map(InlineHtml::cast)
                    .any(|html| {
                        let raw = html.verbatim();
                        let tag = raw.trim_start_matches('<').trim_start_matches('/');
                        let end = tag
                            .find(|ch: char| !ch.is_ascii_alphanumeric() && ch != '-')
                            .unwrap_or(tag.len());
                        is_pandoc_block_tag_name(&tag[..end])
                    })
            {
                return Err(TableConversionError::BlockContent);
            }
        }
        Self::parse(&text, config)
    }

    fn parse(text: &str, config: &Config) -> Result<Self, TableConversionError> {
        // With this extension, replacing a soft break between wide characters
        // with a space changes content. Preserve the source until the converter
        // can represent that distinction in its inline pieces.
        if config.parser_extensions.east_asian_line_breaks
            && text.lines().zip(text.lines().skip(1)).any(|(left, right)| {
                left.trim_end()
                    .chars()
                    .next_back()
                    .and_then(UnicodeWidthChar::width)
                    == Some(2)
                    && right
                        .trim_start()
                        .chars()
                        .next()
                        .and_then(UnicodeWidthChar::width)
                        == Some(2)
            })
        {
            return Err(TableConversionError::EastAsianLineBreak);
        }
        let mut builder = GreenNodeBuilder::new();
        builder.start_node(SyntaxKind::TABLE_CELL.into());
        parse_inline_text_recursive(&mut builder, text, &config.parser_options(), false);
        builder.finish_node();
        let node = SyntaxNode::new_root(builder.finish());
        if node
            .descendants_with_tokens()
            .any(|el| el.kind() == SyntaxKind::HARD_LINE_BREAK)
        {
            return Err(TableConversionError::HardLineBreak);
        }
        // The parser does not yet include verbatim delimiters and content in
        // the command node. Escaping or reflowing those bytes can change TeX.
        if node
            .descendants()
            .filter_map(LatexCommand::cast)
            .any(|command| {
                command
                    .text()
                    .chars()
                    .skip(1)
                    .take_while(char::is_ascii_alphabetic)
                    .eq("verb".chars())
            })
        {
            return Err(TableConversionError::InvalidOutput);
        }
        Ok(Self {
            pieces: cell_pieces(&node, CellText::Source)?,
            pipe_text: cell_pieces(&node, CellText::Pipe)?.join(" "),
            comparison: cell_pieces(&node, CellText::Comparison)?.join(" "),
        })
    }

    fn text(&self) -> String {
        self.pieces.join(" ")
    }

    fn minimum_width(&self, measure: fn(&str) -> usize) -> usize {
        self.pieces.iter().map(|s| measure(s)).max().unwrap_or(0)
    }

    fn wrap(&self, width: usize, measure: fn(&str) -> usize) -> Vec<String> {
        let mut lines = Vec::new();
        let mut line = String::new();
        for piece in &self.pieces {
            if !line.is_empty() && measure(&line) + 1 + measure(piece) > width {
                lines.push(std::mem::take(&mut line));
            }
            if !line.is_empty() {
                line.push(' ');
            }
            line.push_str(piece);
        }
        if !line.is_empty() || lines.is_empty() {
            lines.push(line);
        }
        lines
    }
}

fn unparsed_grid_list_marker(line: &str, config: &Config) -> bool {
    if config.dialect() != panache_parser::Dialect::Pandoc {
        return false;
    }
    let marker = line
        .trim_start()
        .split_ascii_whitespace()
        .next()
        .unwrap_or("");
    if marker == "#." || (config.parser_extensions.fancy_lists && matches!(marker, "#)" | "(#)")) {
        return true;
    }
    config.parser_extensions.example_lists
        && marker.starts_with('@')
        && (marker.ends_with('.')
            || (config.parser_extensions.fancy_lists && marker.ends_with(')')))
}

fn cell_pieces(node: &SyntaxNode, mode: CellText) -> Result<Vec<String>, TableConversionError> {
    let mut pieces = Vec::new();
    let mut current = String::new();
    let mut pending_space = false;
    let mut literal_delimiter = false;
    for element in node.children_with_tokens() {
        match element {
            NodeOrToken::Token(token)
                if matches!(
                    token.kind(),
                    SyntaxKind::TEXT | SyntaxKind::NEWLINE | SyntaxKind::WHITESPACE
                ) =>
            {
                for ch in token.text().chars() {
                    if ch.is_ascii_whitespace() {
                        pending_space = true;
                    } else {
                        // Pandoc can pair literal emphasis markers across a
                        // newline even when they cannot pair across spaces.
                        let next_delimiter =
                            matches!(mode, CellText::Source) && matches!(ch, '*' | '_');
                        flush_cell_space(
                            &mut pieces,
                            &mut current,
                            &mut pending_space,
                            literal_delimiter || next_delimiter,
                        );
                        if ch == '|' && matches!(mode, CellText::Pipe) {
                            current.push('\\');
                        }
                        current.push(ch);
                        literal_delimiter = next_delimiter;
                    }
                }
            }
            element => {
                let text = match element {
                    NodeOrToken::Node(node) => inline_cell_text(&node, mode),
                    NodeOrToken::Token(token) => comparison_token_text(&token, mode),
                };
                if text.contains(['\n', '\r']) {
                    return Err(TableConversionError::MultilineLiteral);
                }
                flush_cell_space(
                    &mut pieces,
                    &mut current,
                    &mut pending_space,
                    literal_delimiter,
                );
                current.push_str(&text);
                literal_delimiter = false;
            }
        }
    }
    if !current.is_empty() {
        pieces.push(current);
    }
    Ok(pieces)
}

fn flush_cell_space(
    pieces: &mut Vec<String>,
    current: &mut String,
    pending_space: &mut bool,
    keep_together: bool,
) {
    if std::mem::take(pending_space) && !current.is_empty() {
        if keep_together {
            current.push(' ');
        } else {
            pieces.push(std::mem::take(current));
        }
    }
}

fn comparison_token_text(token: &crate::syntax::SyntaxToken, mode: CellText) -> String {
    if matches!(mode, CellText::Comparison)
        && token.kind() == SyntaxKind::ESCAPED_CHAR
        && token.text() == "\\|"
    {
        "|".to_string()
    } else {
        token.text().to_string()
    }
}

#[derive(Clone, Copy)]
enum CellText {
    Source,
    Pipe,
    Comparison,
}

fn inline_cell_text(node: &SyntaxNode, mode: CellText) -> String {
    if !matches!(
        node.kind(),
        SyntaxKind::EMPHASIS
            | SyntaxKind::STRONG
            | SyntaxKind::STRIKEOUT
            | SyntaxKind::LINK
            | SyntaxKind::LINK_TEXT
            | SyntaxKind::IMAGE_LINK
            | SyntaxKind::IMAGE_ALT
    ) {
        return node.text().to_string();
    }
    node.children_with_tokens()
        .map(|element| match element {
            NodeOrToken::Token(token)
                if matches!(
                    token.kind(),
                    SyntaxKind::TEXT | SyntaxKind::NEWLINE | SyntaxKind::WHITESPACE
                ) =>
            {
                let text = collapse_spaces(token.text());
                if matches!(mode, CellText::Pipe) {
                    text.replace('|', "\\|")
                } else {
                    text
                }
            }
            NodeOrToken::Node(node) => inline_cell_text(&node, mode),
            NodeOrToken::Token(token) => comparison_token_text(&token, mode),
        })
        .collect()
}

/// Shared with ordinary multiline formatting so a subsequent format cannot
/// split an inline construct that conversion deliberately kept intact.
pub(super) fn reflow_inline_cell(
    lines: &[String],
    width: usize,
    config: &Config,
    measure: fn(&str) -> usize,
) -> Option<Vec<String>> {
    Cell::parse(&lines.join("\n"), config)
        .ok()
        .map(|cell| cell.wrap(width, measure))
}

#[derive(Debug)]
struct LogicalTable {
    rows: Vec<Vec<Cell>>,
    alignments: Vec<TableAlignment>,
    has_header: bool,
    caption: Option<(bool, String)>,
}

fn columns(separator: &SyntaxNode) -> Vec<(usize, usize)> {
    let raw = text_without_line_prefixes(separator);
    let mut result = Vec::new();
    let mut start = None;
    for (offset, ch) in raw.char_indices() {
        if ch == '-' {
            start.get_or_insert(offset);
        } else if let Some(start) = start.take() {
            result.push((start, offset));
        }
    }
    if let Some(start) = start {
        result.push((start, raw.len()));
    }
    result
}

fn alignment(text: &str, width: usize) -> TableAlignment {
    let text = text.trim_end_matches([' ', '\t', '\r']);
    if text.is_empty() {
        return TableAlignment::Default;
    }
    match (text.starts_with([' ', '\t']), text.width() < width) {
        (false, false) => TableAlignment::Default,
        (false, true) => TableAlignment::Left,
        (true, false) => TableAlignment::Right,
        (true, true) => TableAlignment::Center,
    }
}

impl LogicalTable {
    fn read(table: &Table, config: &Config) -> Result<Self, TableConversionError> {
        let rows = table.rows();
        let mut has_header = rows.first().is_some_and(|row| row.is_header());
        if rows.len() <= usize::from(has_header) {
            return Err(TableConversionError::MissingBody);
        }
        let caption = table.caption().map(|caption| {
            let before =
                caption.syntax().text_range().start() < rows[0].syntax().text_range().start();
            (
                before,
                text_without_line_prefixes(caption.syntax())
                    .replace("\r\n", "\n")
                    .trim_end_matches('\n')
                    .to_string(),
            )
        });
        let (cell_texts, alignments) = if let Table::Grid(grid) = table {
            if table
                .syntax()
                .children()
                .any(|node| node.kind() == SyntaxKind::TABLE_FOOTER)
            {
                return Err(TableConversionError::Footer);
            }
            if rows.iter().filter(|row| row.is_header()).count() > 1 {
                return Err(TableConversionError::MultipleHeaders);
            }
            let layout = grid.layout().ok_or(TableConversionError::InvalidOutput)?;
            if layout
                .cells
                .iter()
                .any(|cell| cell.row_span != 1 || cell.col_span != 1)
            {
                return Err(TableConversionError::SpanningCells);
            }
            let count = layout.cols_pos.len() - 1;
            if layout.row_seps.len() != rows.len() + 1 || layout.cells.len() != rows.len() * count {
                return Err(TableConversionError::RaggedRows);
            }
            let mut result = vec![vec![String::new(); count]; rows.len()];
            for cell in layout.cells {
                result[cell.start_row][cell.start_col] = cell.content;
            }
            let alignments = grid.alignments();
            if alignments.len() != count {
                return Err(TableConversionError::RaggedRows);
            }
            (result, alignments)
        } else if let Table::Pipe(pipe) = table {
            let count = pipe
                .column_count()
                .ok_or(TableConversionError::RaggedRows)?;
            let mut result = Vec::new();
            for row in rows {
                let cells: Vec<_> = row
                    .cells()
                    .map(|cell| cell.syntax().text().to_string())
                    .collect();
                if cells.len() != count {
                    return Err(TableConversionError::RaggedRows);
                }
                result.push(cells);
            }
            (result, pipe.alignments())
        } else {
            let skip = usize::from(matches!(table, Table::Multiline(_)) && has_header);
            let separator = table
                .syntax()
                .children()
                .filter(|node| node.kind() == SyntaxKind::TABLE_SEPARATOR)
                .nth(skip)
                .ok_or(TableConversionError::InvalidOutput)?;
            let columns = columns(&separator);
            if columns.is_empty() {
                return Err(TableConversionError::InvalidOutput);
            }
            let reference = text_without_line_prefixes(rows[0].syntax());
            let reference = reference.lines().next().unwrap_or("");
            let alignments = columns
                .iter()
                .enumerate()
                .map(|(i, &(start, end))| {
                    alignment(
                        column_slice(
                            reference,
                            start,
                            columns.get(i + 1).map_or(usize::MAX, |c| c.0),
                        ),
                        end - start,
                    )
                })
                .collect();
            let mut result = Vec::new();
            for row in rows {
                let raw = text_without_line_prefixes(row.syntax());
                let mut cells = vec![Vec::new(); columns.len()];
                for line in raw.lines() {
                    for (i, &(start, _)) in columns.iter().enumerate() {
                        let end = columns.get(i + 1).map_or(usize::MAX, |c| c.0);
                        cells[i].push(
                            column_slice(line, start, end)
                                .trim_matches([' ', '\t', '\r'])
                                .to_string(),
                        );
                    }
                }
                result.push(cells.into_iter().map(|lines| lines.join("\n")).collect());
            }
            (result, alignments)
        };
        let mut rows: Vec<Vec<Cell>> = cell_texts
            .into_iter()
            .map(|row| {
                row.iter()
                    .map(|cell| {
                        if matches!(table, Table::Grid(_)) {
                            Cell::parse_grid(cell, config)
                        } else {
                            Cell::parse(cell, config)
                        }
                    })
                    .collect()
            })
            .collect::<Result<_, _>>()?;
        // Pandoc uses an empty pipe header as the spelling of a headerless
        // table. Keep that syntax in the CST, but compare its logical rows.
        if matches!(table, Table::Pipe(_))
            && config.dialect() == panache_parser::Dialect::Pandoc
            && has_header
            && rows[0].iter().all(|cell| cell.pieces.is_empty())
        {
            rows.remove(0);
            has_header = false;
        }
        Ok(Self {
            rows,
            alignments,
            has_header,
            caption,
        })
    }

    fn matches(&self, other: &Self) -> bool {
        self.rows.len() == other.rows.len()
            && self.rows.iter().zip(&other.rows).all(|(source, target)| {
                source.len() == target.len()
                    && source
                        .iter()
                        .zip(target)
                        .all(|(source, target)| source.comparison == target.comparison)
            })
            && self.has_header == other.has_header
            && self.caption == other.caption
            && self.alignments.len() == other.alignments.len()
            && self
                .alignments
                .iter()
                .zip(&other.alignments)
                .all(|(&source, &target)| {
                    source == target
                        || (source == TableAlignment::Default && target == TableAlignment::Left)
                })
    }

    fn render(
        &self,
        target: TableStyle,
        available_width: usize,
    ) -> Result<String, TableConversionError> {
        if target == TableStyle::Pipe {
            return Ok(self.render_pipe());
        }
        if target == TableStyle::Grid {
            return Ok(self.render_grid(available_width));
        }
        let cols = self.alignments.len();
        let mut widths = vec![2; cols];
        let mut minimum = vec![2; cols];
        for (row_idx, row) in self.rows.iter().enumerate() {
            for (i, cell) in row.iter().enumerate() {
                widths[i] = widths[i].max(cell.text().width() + 2);
                let min = if self.has_header && row_idx == 0 {
                    cell.text().width()
                } else {
                    cell.minimum_width(str::width)
                };
                minimum[i] = minimum[i].max(min + 2);
            }
        }
        if target == TableStyle::Multiline {
            let mut total = widths.iter().sum::<usize>() + cols.saturating_sub(1);
            while total > available_width {
                let Some(i) = (0..cols)
                    .filter(|&i| widths[i] > minimum[i])
                    .max_by_key(|&i| (widths[i], std::cmp::Reverse(i)))
                else {
                    break;
                };
                widths[i] -= 1;
                total -= 1;
            }
        }
        for (i, cell) in self.rows[0].iter().enumerate() {
            if cell.pieces.is_empty() && !matches!(self.alignments[i], TableAlignment::Default) {
                return Err(TableConversionError::Alignment);
            }
        }
        let separator = widths
            .iter()
            .map(|&width| "-".repeat(width))
            .collect::<Vec<_>>()
            .join(" ");
        let border = "-".repeat(separator.len());
        let mut out = String::new();
        if let Some((true, caption)) = &self.caption {
            out.push_str(caption.trim_end_matches('\n'));
            out.push_str("\n\n");
        }
        if target == TableStyle::Multiline || !self.has_header {
            out.push_str(if self.has_header { &border } else { &separator });
            out.push('\n');
        }
        for (row_idx, row) in self.rows.iter().enumerate() {
            let is_header = self.has_header && row_idx == 0;
            let cells: Vec<_> = row
                .iter()
                .enumerate()
                .map(|(i, cell)| {
                    if target == TableStyle::Simple || is_header {
                        vec![cell.text()]
                    } else {
                        cell.wrap(widths[i].saturating_sub(2), str::width)
                    }
                })
                .collect();
            for line_idx in 0..cells.iter().map(Vec::len).max().unwrap_or(1) {
                let line = cells
                    .iter()
                    .enumerate()
                    .map(|(i, lines)| {
                        let align = match self.alignments[i] {
                            TableAlignment::Default | TableAlignment::Left => Alignment::Left,
                            TableAlignment::Center => Alignment::Center,
                            TableAlignment::Right => Alignment::Right,
                        };
                        pad_simple_cell(
                            lines.get(line_idx).map_or("", String::as_str),
                            widths[i],
                            align,
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(" ");
                out.push_str(line.trim_end());
                out.push('\n');
            }
            if is_header {
                out.push_str(&separator);
                out.push('\n');
            } else if target == TableStyle::Multiline {
                // The final blank also disambiguates a short, single-row table.
                out.push('\n');
            }
        }
        out.push_str(if target == TableStyle::Multiline && self.has_header {
            &border
        } else {
            &separator
        });
        out.push('\n');
        if let Some((false, caption)) = &self.caption {
            out.push('\n');
            out.push_str(caption.trim_end_matches('\n'));
            out.push('\n');
        }
        Ok(out)
    }

    fn render_grid(&self, available_width: usize) -> String {
        let cols = self.alignments.len();
        let mut widths = vec![1; cols];
        let mut minimum = vec![1; cols];
        for row in &self.rows {
            for (i, cell) in row.iter().enumerate() {
                widths[i] = widths[i].max(grid_display_width(&cell.text()));
                minimum[i] = minimum[i].max(cell.minimum_width(grid_display_width));
            }
        }
        let mut total = widths.iter().sum::<usize>() + 3 * cols + 1;
        while total > available_width {
            let Some(i) = (0..cols)
                .filter(|&i| widths[i] > minimum[i])
                .max_by_key(|&i| (widths[i], std::cmp::Reverse(i)))
            else {
                break;
            };
            widths[i] -= 1;
            total -= 1;
        }
        let border = |fill: char, aligned: bool| {
            let mut line = String::from("+");
            for (i, &width) in widths.iter().enumerate() {
                let (left, right) = if aligned {
                    match self.alignments[i] {
                        TableAlignment::Default => (false, false),
                        TableAlignment::Left => (true, false),
                        TableAlignment::Center => (true, true),
                        TableAlignment::Right => (false, true),
                    }
                } else {
                    (false, false)
                };
                line.push(if left { ':' } else { fill });
                line.push_str(&fill.to_string().repeat(width));
                line.push(if right { ':' } else { fill });
                line.push('+');
            }
            line.push('\n');
            line
        };
        let mut out = String::new();
        if let Some((true, caption)) = &self.caption {
            out.push_str(caption);
            out.push_str("\n\n");
        }
        out.push_str(&border('-', !self.has_header));
        for (row_idx, row) in self.rows.iter().enumerate() {
            let cells: Vec<_> = row
                .iter()
                .enumerate()
                .map(|(i, cell)| cell.wrap(widths[i], grid_display_width))
                .collect();
            for line_idx in 0..cells.iter().map(Vec::len).max().unwrap_or(1) {
                out.push('|');
                for (i, cell) in cells.iter().enumerate() {
                    let text = cell.get(line_idx).map_or("", String::as_str);
                    out.push(' ');
                    out.push_str(text);
                    out.push_str(&" ".repeat(widths[i].saturating_sub(grid_display_width(text))));
                    out.push_str(" |");
                }
                out.push('\n');
            }
            let is_header = self.has_header && row_idx == 0;
            out.push_str(&border(if is_header { '=' } else { '-' }, is_header));
        }
        if let Some((false, caption)) = &self.caption {
            out.push('\n');
            out.push_str(caption);
            out.push('\n');
        }
        out
    }

    fn render_pipe(&self) -> String {
        let rows: Vec<Vec<String>> = self
            .rows
            .iter()
            .map(|row| row.iter().map(|cell| cell.pipe_text.clone()).collect())
            .collect();
        let widths = super::tables::calculate_column_widths(&rows);
        let mut out = String::new();
        if let Some((true, caption)) = &self.caption {
            out.push_str(caption);
            out.push_str("\n\n");
        }
        let empty_header = vec![String::new(); widths.len()];
        let header = if self.has_header {
            &rows[0]
        } else {
            &empty_header
        };
        let write_row = |out: &mut String, row: &[String]| {
            out.push('|');
            for (i, cell) in row.iter().enumerate() {
                let alignment = match self.alignments[i] {
                    TableAlignment::Default | TableAlignment::Left => Alignment::Left,
                    TableAlignment::Center => Alignment::Center,
                    TableAlignment::Right => Alignment::Right,
                };
                out.push(' ');
                out.push_str(&pad_simple_cell(cell, widths[i], alignment));
                out.push_str(" |");
            }
            out.push('\n');
        };
        write_row(&mut out, header);
        out.push('|');
        for (i, &width) in widths.iter().enumerate() {
            out.push(' ');
            let (left, right) = match self.alignments[i] {
                TableAlignment::Default => (false, false),
                TableAlignment::Left => (true, false),
                TableAlignment::Center => (true, true),
                TableAlignment::Right => (false, true),
            };
            if left {
                out.push(':');
            }
            out.push_str(&"-".repeat(width - usize::from(left) - usize::from(right)));
            if right {
                out.push(':');
            }
            out.push_str(" |");
        }
        out.push('\n');
        for row in rows.iter().skip(usize::from(self.has_header)) {
            write_row(&mut out, row);
        }
        if let Some((false, caption)) = &self.caption {
            out.push('\n');
            out.push_str(caption);
            out.push('\n');
        }
        out
    }
}

/// Convert a table while preserving its content, structure, and explicit alignment.
///
/// Column widths and wrapping may change, and default alignment may become
/// left alignment. Simple tables discard explicit column widths. Conversions
/// that cannot preserve the remaining table information return an error.
/// Pipe tables join wrapped prose onto one line per row and use an empty
/// header for headerless tables under the Pandoc dialect. Pipes in prose are
/// escaped; literal content in code and math is preserved.
/// Grid cells must be empty or contain one paragraph, without row or column
/// spans. Multiple header rows and footers cannot be converted to other styles.
/// The result has no container prefixes and uses LF line endings.
pub fn convert_table(
    table: &Table,
    target: TableStyle,
    config: &Config,
    available_width: usize,
) -> Result<String, TableConversionError> {
    let enabled = match target {
        TableStyle::Pipe => config.parser_extensions.pipe_tables,
        TableStyle::Simple => config.parser_extensions.simple_tables,
        TableStyle::Multiline => config.parser_extensions.multiline_tables,
        TableStyle::Grid => config.parser_extensions.grid_tables,
    };
    if !enabled {
        return Err(TableConversionError::DisabledExtension);
    }
    let model = LogicalTable::read(table, config)?;
    let output = model.render(target, available_width)?;
    let parsed = crate::parser::parse(&output, Some(config.parser_options()));
    let children: Vec<_> = parsed
        .children()
        .filter(|node| node.kind() != SyntaxKind::BLANK_LINE)
        .collect();
    if children.len() != 1 || children[0].kind() != target.syntax_kind() {
        return Err(TableConversionError::InvalidOutput);
    }
    let candidate = Table::cast(children[0].clone()).ok_or(TableConversionError::InvalidOutput)?;
    if !model.matches(&LogicalTable::read(&candidate, config)?) {
        return Err(TableConversionError::InvalidOutput);
    }
    Ok(output)
}

/// Check content, structure, and explicit alignment after restoring container prefixes.
///
/// Source column widths and wrapping are excluded from the conversion contract.
pub fn equivalent_tables(source: &Table, candidate: &Table, config: &Config) -> bool {
    match (
        LogicalTable::read(source, config),
        LogicalTable::read(candidate, config),
    ) {
        (Ok(source), Ok(candidate)) => source.matches(&candidate),
        _ => false,
    }
}
