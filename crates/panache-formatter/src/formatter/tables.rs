use crate::config::{Config, WrapMode};
use crate::formatter::Formatter;
use crate::formatter::inline::format_inline_node;
use crate::formatter::inline_layout::wrap_text_first_fit;
use crate::formatter::sentence_wrap::{ResolvedProfile, resolve_profile, split_sentence_text};
use crate::syntax::{SyntaxKind, SyntaxNode, SyntaxToken, text_without_line_prefixes};
use panache_parser::analyze_grid;
use rowan::NodeOrToken;
use std::collections::BTreeSet;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

impl Formatter {
    pub(super) fn format_table(&mut self, node: &SyntaxNode, indent: usize) {
        match node.kind() {
            SyntaxKind::SIMPLE_TABLE => {
                log::trace!("Formatting simple table");
                self.output
                    .push_str(&format_simple_table(node, &self.config, indent));

                if let Some(next) = node.next_sibling()
                    && super::utils::is_block_element(next.kind())
                    && !self.output.ends_with("\n\n")
                {
                    self.output.push('\n');
                }
            }
            SyntaxKind::MULTILINE_TABLE => {
                self.output
                    .push_str(&format_multiline_table(node, &self.config, indent))
            }
            SyntaxKind::PIPE_TABLE => {
                self.output
                    .push_str(&format_pipe_table(node, &self.config, indent))
            }
            SyntaxKind::GRID_TABLE => {
                if let Some(next) = node.next_sibling()
                    && self.is_grid_table_continuation_paragraph(&next)
                {
                    self.output.push_str(&node.text().to_string());
                    if !self.output.ends_with('\n') {
                        self.output.push('\n');
                    }
                    return;
                }
                self.output
                    .push_str(&format_grid_table(node, &self.config, indent));
            }
            _ => unreachable!("format_table received a non-table node"),
        }
    }
}

/// Indent (in columns) assumed for table types that self-indent at the top
/// level (pipe, simple, multiline) when budgeting caption wrap width. The
/// actual block indent is `config.table_indent` (default 2, range 0--3); grid
/// tables instead honor the container indent threaded from the dispatcher so a
/// top-level grid sits at column 0 -- pandoc rejects an indented `+---+`
/// border. See `format_grid_table`.
const TABLE_BLOCK_INDENT: usize = 2;

fn indent_table_block(block: &str, indent: usize) -> String {
    if indent == 0 {
        return block.to_string();
    }
    let prefix = " ".repeat(indent);

    let already_indented = block
        .lines()
        .filter(|line| !line.is_empty())
        .all(|line| line.starts_with(&prefix));
    if already_indented {
        return block.to_string();
    }

    let mut output = String::with_capacity(block.len() + indent + 32);
    let mut line_start = 0;

    for (idx, ch) in block.char_indices() {
        if ch == '\n' {
            let line = &block[line_start..idx];
            if !line.is_empty() {
                output.push_str(&prefix);
            }
            output.push_str(line);
            output.push('\n');
            line_start = idx + 1;
        }
    }

    if line_start < block.len() {
        let line = &block[line_start..];
        if !line.is_empty() {
            output.push_str(&prefix);
        }
        output.push_str(line);
    }

    output
}

fn normalize_table_caption(caption_body: &str) -> String {
    let normalized_body = caption_body
        .lines()
        .map(str::trim)
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string();

    if normalized_body.is_empty() {
        ":".to_string()
    } else {
        format!(": {normalized_body}")
    }
}

fn collapse_ascii_whitespace(text: &str) -> String {
    text.split_ascii_whitespace().collect::<Vec<_>>().join(" ")
}

fn wrap_words_with_widths(words: &[&str], first_width: usize, rest_width: usize) -> Vec<String> {
    if words.is_empty() {
        return Vec::new();
    }

    let mut out = Vec::new();
    let mut current = String::new();
    let mut current_width = 0usize;
    let mut line_width = first_width.max(1);

    for word in words {
        let word_width = word.width();
        if current.is_empty() {
            current.push_str(word);
            current_width = word_width;
            continue;
        }

        if current_width + 1 + word_width > line_width {
            out.push(current);
            current = (*word).to_string();
            current_width = word_width;
            line_width = rest_width.max(1);
            continue;
        }

        current.push(' ');
        current.push_str(word);
        current_width += 1 + word_width;
    }

    if !current.is_empty() {
        out.push(current);
    }

    out
}

/// Reflow a multi-line table cell's lines to fit a fixed column width.
///
/// Column widths in grid/multiline tables are load-bearing (pandoc maps them to
/// relative output widths), so we never resize the column -- we only re-pack the
/// cell's text to use the existing width more tightly. Leading/trailing blank
/// lines are dropped (pandoc discards them); runs of blank lines split the cell
/// into paragraphs (an internal blank line in a grid cell is a paragraph break),
/// each reflowed independently and rejoined with a single blank line. Multiline
/// table cells never contain internal blanks, so this reduces to one paragraph.
fn reflow_cell_lines(lines: &[String], width: usize) -> Vec<String> {
    let mut paragraphs: Vec<Vec<&str>> = Vec::new();
    let mut current: Vec<&str> = Vec::new();
    for line in lines {
        if line.trim().is_empty() {
            if !current.is_empty() {
                paragraphs.push(std::mem::take(&mut current));
            }
        } else {
            current.push(line.trim());
        }
    }
    if !current.is_empty() {
        paragraphs.push(current);
    }

    let mut out = Vec::new();
    for paragraph in paragraphs {
        if !out.is_empty() {
            out.push(String::new());
        }
        let joined = paragraph.join(" ");
        if width == 0 {
            out.push(joined);
        } else {
            out.extend(wrap_text_first_fit(&joined, width));
        }
    }
    out
}

fn grid_cell_is_reflowable(lines: &[String]) -> bool {
    let mut has_content = false;
    for line in lines {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        has_content = true;
        if trimmed.ends_with('\\') {
            return false;
        }
        if grid_cell_line_is_block_marker(trimmed) {
            return false;
        }
    }
    has_content
}

/// Detect a leading block-level marker that must not be reflowed into a
/// paragraph: a list bullet/number, blockquote, ATX heading, code fence, or a
/// nested pipe/grid line. `trimmed` must already be whitespace-trimmed.
fn grid_cell_line_is_block_marker(trimmed: &str) -> bool {
    let first = trimmed.split_whitespace().next().unwrap_or("");

    if matches!(first, "-" | "*" | "+") && trimmed.len() > first.len() {
        return true;
    }
    if is_ordered_list_marker(first) && trimmed.len() > first.len() {
        return true;
    }

    trimmed.starts_with('>')
        || trimmed.starts_with('#')
        || trimmed.starts_with("```")
        || trimmed.starts_with("~~~")
        || trimmed.starts_with('|')
        || trimmed.starts_with('+')
}

fn is_ordered_list_marker(token: &str) -> bool {
    let bytes = token.as_bytes();
    let Some((last, digits)) = bytes.split_last() else {
        return false;
    };
    !digits.is_empty() && digits.iter().all(u8::is_ascii_digit) && matches!(last, b'.' | b')')
}

/// Reflow a single grid cell (its lines across one row group) to `width`, or --
/// when the content carries block structure -- keep it verbatim after dropping
/// leading/trailing blank lines. Column widths are load-bearing, so `width` is a
/// fixed target, never a resize.
fn reflow_or_trim_grid_cell(lines: &[String], width: usize) -> Vec<String> {
    if width > 0 && grid_cell_is_reflowable(lines) {
        reflow_cell_lines(lines, width)
    } else {
        let first = lines.iter().position(|l| !l.trim().is_empty());
        let last = lines.iter().rposition(|l| !l.trim().is_empty());
        match (first, last) {
            (Some(f), Some(l)) => lines[f..=l].to_vec(),
            _ => Vec::new(),
        }
    }
}

/// Re-pack grid table cells within each row group: drop blank padding lines and
/// reflow plain-prose cells to their fixed column width.
///
/// The line-per-row grid model stores each physical `| ... |` line as its own
/// logical row, so a multi-line cell is spread across several rows sharing one
/// `row_groups` id. Here we regroup those physical lines into per-column cells,
/// reflow/trim each cell, then redistribute the result back into physical lines.
/// Column widths are never resized (pandoc maps grid widths to relative output
/// widths); cells with block content or hard line breaks stay verbatim.
fn reflow_grid_table_cells(table_data: &mut GridTableData) {
    let num_cols = table_data
        .column_widths
        .len()
        .max(table_data.rows.iter().map(Vec::len).max().unwrap_or(0));
    if num_cols == 0 {
        return;
    }

    let content_widths = calculate_grid_column_widths(&table_data.rows);
    let targets: Vec<usize> = (0..num_cols)
        .map(|col| {
            content_widths
                .get(col)
                .copied()
                .unwrap_or(0)
                .max(table_data.column_widths.get(col).copied().unwrap_or(0))
        })
        .collect();

    let mut new_rows: Vec<Vec<String>> = Vec::new();
    let mut new_sections: Vec<GridRowSection> = Vec::new();
    let mut new_groups: Vec<usize> = Vec::new();

    let mut start = 0;
    while start < table_data.rows.len() {
        let group = table_data.row_groups.get(start).copied();
        let section = table_data
            .row_sections
            .get(start)
            .copied()
            .unwrap_or(GridRowSection::Body);
        let mut end = start;
        while end < table_data.rows.len() && table_data.row_groups.get(end).copied() == group {
            end += 1;
        }

        let mut cols: Vec<Vec<String>> = Vec::with_capacity(num_cols);
        for (col, &target) in targets.iter().enumerate() {
            let lines: Vec<String> = (start..end)
                .map(|r| table_data.rows[r].get(col).cloned().unwrap_or_default())
                .collect();
            cols.push(reflow_or_trim_grid_cell(&lines, target));
        }

        let line_count = cols.iter().map(Vec::len).max().unwrap_or(0).max(1);
        let group_id = group.unwrap_or(0);
        for line_idx in 0..line_count {
            let row: Vec<String> = (0..num_cols)
                .map(|col| cols[col].get(line_idx).cloned().unwrap_or_default())
                .collect();
            new_rows.push(row);
            new_sections.push(section);
            new_groups.push(group_id);
        }

        start = end;
    }

    table_data.rows = new_rows;
    table_data.row_sections = new_sections;
    table_data.row_groups = new_groups;
}

fn split_sentences(text: &str, profile: ResolvedProfile<'_>) -> Vec<String> {
    split_sentence_text(text, profile)
}

fn format_table_caption_with_language(
    caption_text: &str,
    config: &Config,
    profile: ResolvedProfile<'_>,
) -> String {
    const CAPTION_PREFIX: &str = ": ";
    const CAPTION_HANGING_INDENT: &str = "  ";

    let Some(rest) = caption_text
        .strip_prefix(':')
        .or_else(|| caption_text.strip_prefix("Table:"))
        .or_else(|| caption_text.strip_prefix("table:"))
    else {
        return caption_text.to_string();
    };
    let body = rest.trim();
    if body.is_empty() {
        return ":".to_string();
    }

    let wrap_mode = config.wrap.clone().unwrap_or(WrapMode::Reflow);
    let available_width = config.line_width.saturating_sub(TABLE_BLOCK_INDENT).max(1);

    match wrap_mode {
        WrapMode::Preserve => format!(": {body}"),
        WrapMode::Reflow => {
            let normalized = collapse_ascii_whitespace(body);
            let words: Vec<&str> = normalized.split_ascii_whitespace().collect();
            let first_width = available_width
                .saturating_sub(CAPTION_PREFIX.width())
                .max(1);
            let rest_width = available_width
                .saturating_sub(CAPTION_HANGING_INDENT.width())
                .max(1);
            let wrapped = wrap_words_with_widths(&words, first_width, rest_width);
            if wrapped.is_empty() {
                ":".to_string()
            } else {
                let mut out = String::new();
                out.push_str(CAPTION_PREFIX);
                out.push_str(&wrapped[0]);
                for line in wrapped.iter().skip(1) {
                    out.push('\n');
                    out.push_str(CAPTION_HANGING_INDENT);
                    out.push_str(line);
                }
                out
            }
        }
        WrapMode::Sentence | WrapMode::Semantic => {
            let normalized = collapse_ascii_whitespace(body);
            let lines = split_sentences(&normalized, profile);
            if lines.is_empty() {
                ":".to_string()
            } else {
                let mut out = String::new();
                out.push_str(CAPTION_PREFIX);
                out.push_str(&lines[0]);
                for line in lines.iter().skip(1) {
                    out.push('\n');
                    out.push_str(CAPTION_HANGING_INDENT);
                    out.push_str(line);
                }
                out
            }
        }
    }
}

fn format_table_caption(caption_text: &str, config: &Config, node: &SyntaxNode) -> String {
    let mut extra_abbreviations = Vec::new();
    let profile = resolve_profile(node, config, &mut extra_abbreviations);
    format_table_caption_with_language(caption_text, config, profile)
}

fn extract_table_caption_content(caption_node: &SyntaxNode) -> String {
    let mut caption_body = String::new();
    for caption_child in caption_node.children_with_tokens() {
        match caption_child {
            rowan::NodeOrToken::Token(token)
                if matches!(
                    token.kind(),
                    SyntaxKind::LINE_PREFIX | SyntaxKind::TABLE_CAPTION_PREFIX
                ) => {}
            rowan::NodeOrToken::Token(token) => {
                caption_body.push_str(token.text());
            }
            rowan::NodeOrToken::Node(node) => {
                caption_body.push_str(&text_without_line_prefixes(&node));
            }
        }
    }

    normalize_table_caption(&caption_body)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Alignment {
    Left,
    Right,
    Center,
    Default,
}

struct TableData {
    rows: Vec<Vec<String>>,     // All rows including header
    alignments: Vec<Alignment>, // Column alignments
    caption: Option<String>,    // Optional caption text
    has_header: bool,           // True if table has a header row
}

fn collapse_cell_ws_runs(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut in_ws = false;
    for ch in text.chars() {
        if ch == ' ' || ch == '\t' {
            if !in_ws {
                result.push(' ');
            }
            in_ws = true;
        } else {
            result.push(ch);
            in_ws = false;
        }
    }
    result
}

/// Format cell content, handling both TEXT tokens and inline elements.
///
/// With `collapse_ws`, whitespace runs inside `TEXT` tokens collapse to a
/// single space, matching pandoc's reader (intra-cell whitespace is a single
/// `Space` inline). Inline nodes are untouched, so code-span content keeps its
/// runs. The simple- and pipe-table paths opt in; the multiline paths slice
/// cells by byte offsets against the source geometry (collapsing would skew
/// the columns) and collapse later instead, when cells reflow.
fn format_cell_content(node: &SyntaxNode, config: &Config, collapse_ws: bool) -> String {
    let mut result = String::new();

    for child in node.children_with_tokens() {
        match child {
            NodeOrToken::Token(token) => {
                if token.kind() == SyntaxKind::TEXT
                    || token.kind() == SyntaxKind::NEWLINE
                    || token.kind() == SyntaxKind::ESCAPED_CHAR
                {
                    if collapse_ws && token.kind() == SyntaxKind::TEXT {
                        result.push_str(&collapse_cell_ws_runs(token.text()));
                    } else {
                        result.push_str(token.text());
                    }
                }
            }
            NodeOrToken::Node(node) => {
                result.push_str(&format_inline_node(&node, config));
            }
        }
    }

    result
}

/// Extract cell contents from TABLE_CELL nodes if present, otherwise fall back to text splitting
fn extract_row_cells(row_node: &SyntaxNode, config: &Config, collapse_ws: bool) -> Vec<String> {
    let mut cells = Vec::new();

    let has_table_cells = row_node
        .children()
        .any(|child| child.kind() == SyntaxKind::TABLE_CELL);

    if has_table_cells {
        for child in row_node.children() {
            if child.kind() == SyntaxKind::TABLE_CELL {
                cells.push(format_cell_content(&child, config, collapse_ws));
            }
        }
    }

    cells
}

/// Byte length of the container prefix at the start of `node` — its
/// leading run of `LINE_PREFIX` tokens.
///
/// A table nested in a container carries its own prefix on every line it
/// owns, but the *first* line's prefix belongs to the enclosing container
/// and so sits outside the table node. Column geometry is byte offsets into
/// these lines, so the header would otherwise be measured from a different
/// origin than the separator and rows.
fn container_prefix_len(node: &SyntaxNode) -> usize {
    node.children_with_tokens()
        .map_while(|el| {
            el.into_token()
                .filter(|t| t.kind() == SyntaxKind::LINE_PREFIX)
                .map(|t| t.text().len())
        })
        .sum()
}

fn text_without_prefixes(node: &SyntaxNode, render_node: impl Fn(&SyntaxNode) -> String) -> String {
    let mut out = String::new();
    for element in node.children_with_tokens() {
        match element {
            NodeOrToken::Token(token) => {
                if token.kind() != SyntaxKind::LINE_PREFIX {
                    out.push_str(token.text());
                }
            }
            NodeOrToken::Node(child) => {
                out.push_str(&render_node(&child));
            }
        }
    }
    out
}

fn raw_text_without_prefixes(node: &SyntaxNode) -> String {
    text_without_prefixes(node, |child| child.text().to_string())
}

fn separator_marker_tokens(separator: &SyntaxNode) -> impl Iterator<Item = SyntaxToken> {
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

fn separator_segments(separator: &SyntaxNode) -> Vec<Vec<SyntaxToken>> {
    let mut segs: Vec<Vec<SyntaxToken>> = vec![Vec::new()];
    for t in separator_marker_tokens(separator) {
        if t.kind() == SyntaxKind::TABLE_SEP_DELIM {
            segs.push(Vec::new());
        } else {
            segs.last_mut().unwrap().push(t);
        }
    }
    segs
}

fn extract_alignments(separator: &SyntaxNode) -> Vec<Alignment> {
    let mut alignments = Vec::new();

    for seg in separator_segments(separator) {
        let non_ws = |t: &&SyntaxToken| t.kind() != SyntaxKind::TABLE_SEP_WHITESPACE;
        let Some(first) = seg.iter().find(non_ws) else {
            continue;
        };
        let last = seg.iter().rev().find(non_ws).unwrap();

        let starts_colon = first.kind() == SyntaxKind::TABLE_SEP_COLON;
        let ends_colon = last.kind() == SyntaxKind::TABLE_SEP_COLON;

        let alignment = match (starts_colon, ends_colon) {
            (true, true) => Alignment::Center,
            (true, false) => Alignment::Left,
            (false, true) => Alignment::Right,
            (false, false) => Alignment::Default,
        };

        alignments.push(alignment);
    }

    alignments
}

fn split_row(row_text: &str) -> Vec<String> {
    let trimmed = row_text.trim();
    let cells: Vec<&str> = trimmed.split('|').collect();

    cells
        .iter()
        .enumerate()
        .filter_map(|(i, cell)| {
            let cell = cell.trim();
            if (i == 0 || i == cells.len() - 1) && cell.is_empty() {
                None
            } else {
                Some(cell.to_string())
            }
        })
        .collect()
}

fn extract_pipe_table_data(node: &SyntaxNode, config: &Config) -> TableData {
    let mut rows = Vec::new();
    let mut alignments = Vec::new();
    let mut caption = None;

    for child in node.children() {
        match child.kind() {
            SyntaxKind::TABLE_CAPTION => {
                let caption_text = extract_table_caption_content(&child);
                if caption.is_none() {
                    caption = Some(caption_text);
                }
            }
            SyntaxKind::TABLE_SEPARATOR => {
                alignments = extract_alignments(&child);
            }
            SyntaxKind::TABLE_HEADER | SyntaxKind::TABLE_ROW => {
                let cells = extract_row_cells(&child, config, true);
                let cells = if cells.is_empty() {
                    split_row(&format_cell_content(&child, config, true))
                } else {
                    cells
                };
                rows.push(cells);
            }
            _ => {}
        }
    }

    TableData {
        rows,
        alignments,
        caption,
        has_header: true, // Pipe tables always have headers
    }
}

/// Calculate the maximum width needed for each column
fn calculate_column_widths(rows: &[Vec<String>]) -> Vec<usize> {
    if rows.is_empty() {
        return Vec::new();
    }

    let num_cols = rows.iter().map(|r| r.len()).max().unwrap_or(0);
    let mut widths = vec![3; num_cols]; // Minimum width of 3 for "---"

    for row in rows {
        for (col_idx, cell) in row.iter().enumerate() {
            if col_idx < num_cols {
                widths[col_idx] = widths[col_idx].max(cell.width());
            }
        }
    }

    widths
}

/// Calculate the maximum width needed for each column (grid tables)
/// Grid tables don't have a minimum width constraint
fn calculate_grid_column_widths(rows: &[Vec<String>]) -> Vec<usize> {
    if rows.is_empty() {
        return Vec::new();
    }

    let num_cols = rows.iter().map(|r| r.len()).max().unwrap_or(0);
    let mut widths = vec![0; num_cols];

    for row in rows {
        for (col_idx, cell) in row.iter().enumerate() {
            if col_idx < num_cols {
                widths[col_idx] = widths[col_idx].max(cell.width());
            }
        }
    }

    widths
}

/// Format a pipe table with consistent alignment and padding
pub fn format_pipe_table(node: &SyntaxNode, config: &Config, indent: usize) -> String {
    let mut table_data = extract_pipe_table_data(node, config);
    let mut output = String::new();

    if table_data.rows.is_empty() {
        return indent_table_block(&text_without_line_prefixes(node), indent);
    }

    let cols = table_data.alignments.len();
    if cols > 0 {
        if table_data.rows.iter().any(|row| row.len() > cols) {
            return indent_table_block(&text_without_line_prefixes(node), indent);
        }
        for row in &mut table_data.rows {
            row.resize(cols, String::new());
        }
    }

    let widths = calculate_column_widths(&table_data.rows);

    for (row_idx, row) in table_data.rows.iter().enumerate() {
        output.push('|');

        for (col_idx, cell) in row.iter().enumerate() {
            let width = widths.get(col_idx).copied().unwrap_or(3);
            let alignment = table_data
                .alignments
                .get(col_idx)
                .copied()
                .unwrap_or(Alignment::Default);

            output.push(' ');

            let cell_width = cell.width();
            let total_padding = width.saturating_sub(cell_width);

            let padded_cell = if row_idx == 0 {
                format!("{}{}", cell, " ".repeat(total_padding))
            } else {
                match alignment {
                    Alignment::Left | Alignment::Default => {
                        format!("{}{}", cell, " ".repeat(total_padding))
                    }
                    Alignment::Right => {
                        format!("{}{}", " ".repeat(total_padding), cell)
                    }
                    Alignment::Center => {
                        let left_padding = total_padding / 2;
                        let right_padding = total_padding - left_padding;
                        format!(
                            "{}{}{}",
                            " ".repeat(left_padding),
                            cell,
                            " ".repeat(right_padding)
                        )
                    }
                }
            };

            output.push_str(&padded_cell);
            output.push_str(" |");
        }

        output.push('\n');

        if row_idx == 0 {
            output.push('|');

            for (col_idx, width) in widths.iter().enumerate() {
                let alignment = table_data
                    .alignments
                    .get(col_idx)
                    .copied()
                    .unwrap_or(Alignment::Default);

                output.push(' ');

                let separator = match alignment {
                    Alignment::Left => format!(":{:-<width$}", "", width = width - 1),
                    Alignment::Right => format!("{:->width$}:", "", width = width - 1),
                    Alignment::Center => format!(":{:-<width$}:", "", width = width - 2),
                    Alignment::Default => format!("{:-<width$}", "", width = width),
                };

                output.push_str(&separator);
                output.push_str(" |");
            }

            output.push('\n');
        }
    }

    if let Some(ref caption_text) = table_data.caption {
        output.push('\n');
        let formatted_caption = format_table_caption(caption_text, config, node);
        output.push_str(&formatted_caption);
        output.push('\n');
    }
    let block_indent = if indent == 0 {
        config.table_indent
    } else {
        indent
    };
    indent_table_block(&output, block_indent)
}

fn grid_inner_segments(separator: &SyntaxNode) -> Vec<Vec<SyntaxToken>> {
    let mut segs: Vec<Vec<SyntaxToken>> = Vec::new();
    let mut cur: Option<Vec<SyntaxToken>> = None;
    for t in separator_marker_tokens(separator) {
        if t.kind() == SyntaxKind::TABLE_SEP_DELIM {
            if let Some(seg) = cur.take() {
                segs.push(seg);
            }
            cur = Some(Vec::new());
        } else if let Some(seg) = cur.as_mut() {
            seg.push(t);
        }
    }
    segs
}

fn extract_grid_alignments(separator: &SyntaxNode) -> Vec<Alignment> {
    let mut alignments = Vec::new();

    for segment in grid_inner_segments(separator) {
        if segment.is_empty() {
            continue;
        }

        let starts_colon = segment.first().unwrap().kind() == SyntaxKind::TABLE_SEP_COLON;
        let ends_colon = segment.last().unwrap().kind() == SyntaxKind::TABLE_SEP_COLON;

        let alignment = match (starts_colon, ends_colon) {
            (true, true) => Alignment::Center,
            (true, false) => Alignment::Left,
            (false, true) => Alignment::Right,
            (false, false) => Alignment::Default,
        };

        alignments.push(alignment);
    }

    alignments
}

/// Grid column widths (chars between `+`, minus 2) read from the separator's
/// CST tokens. The raw spanning-grid path has no CST node and uses the
/// string-based [`grid_separator_widths`] instead.
fn grid_separator_widths_cst(separator: &SyntaxNode) -> Vec<usize> {
    grid_inner_segments(separator)
        .iter()
        .map(|seg| {
            seg.iter()
                .map(|t| t.text().len())
                .sum::<usize>()
                .saturating_sub(2)
        })
        .collect()
}

fn split_grid_row(row_text: &str) -> Vec<String> {
    let trimmed = row_text.trim();

    let cells: Vec<&str> = trimmed.split('|').collect();

    cells
        .iter()
        .enumerate()
        .filter_map(|(i, cell)| {
            let cell = cell.trim();
            if (i == 0 || i == cells.len() - 1) && cell.is_empty() {
                None
            } else {
                Some(cell.to_string())
            }
        })
        .collect()
}

/// Format a grid table with row spans and/or column spans in one canonical
/// layout pass. Replaces both the old rowspan passthrough (which emitted
/// separators verbatim and *guessed* alignment with data-specific hacks) and
/// the line-by-line colspan engine.
///
/// The canonical column/row grid comes from the shared `analyze_grid` pass —
/// the same one the pandoc-native projector uses, so both agree on where the
/// boundaries are. Each source line is then re-emitted on that grid: its marker
/// skeleton (`+`/`|` per boundary) and each segment's role (a `-`/`=`/`:` cell
/// edge, a rowspan-interior blank, or a content cell) are read from the source
/// and only the lengths/padding are recomputed from `widths`. Reading the
/// skeleton from the source — rather than reconstructing it from the cell
/// tiling — keeps colspan dash runs continuous, rowspan interiors blank, and
/// `|` vertical edges intact, and it handles hybrid lines that carry
/// rowspan-cell text alongside a sub-row separator. Column widths floor to the
/// source border widths (preserved, not shrunk to content -- grid widths carry
/// relative-width meaning, see #323) and grow only to fit content, and alignment
/// is read from the alignment-bearing separator's colons rather than guessed.
fn format_unified_spanning_grid_table(
    raw_table: &str,
    config: &Config,
    profile: ResolvedProfile<'_>,
    indent: usize,
) -> String {
    let mut raw_lines: Vec<&str> = raw_table.lines().collect();
    while raw_lines.last().is_some_and(|l| l.trim().is_empty()) {
        raw_lines.pop();
    }

    let mut caption: Option<String> = None;
    let take_caption = |line: &str| -> Option<String> {
        let t = line.trim_start();
        t.strip_prefix(':')
            .or_else(|| t.strip_prefix("Table:"))
            .or_else(|| t.strip_prefix("table:"))
            .map(|rest| format!(": {}", rest.trim()))
    };
    if let Some(first) = raw_lines.first().copied()
        && let Some(cap) = take_caption(first)
    {
        caption = Some(cap);
        raw_lines.remove(0);
        while raw_lines.first().is_some_and(|l| l.trim().is_empty()) {
            raw_lines.remove(0);
        }
    }
    if caption.is_none()
        && let Some(last) = raw_lines.last().copied()
        && let Some(cap) = take_caption(last)
    {
        caption = Some(cap);
        raw_lines.pop();
        while raw_lines.last().is_some_and(|l| l.trim().is_empty()) {
            raw_lines.pop();
        }
    }

    if raw_lines.is_empty() {
        return indent_table_block(raw_table, indent);
    }

    let common_indent = raw_lines
        .iter()
        .filter(|l| !l.trim().is_empty())
        .map(|line| line.chars().take_while(|c| *c == ' ').count())
        .min()
        .unwrap_or(0);
    let lines: Vec<&str> = raw_lines
        .iter()
        .map(|l| dedent_line(l, common_indent))
        .collect();

    let Some(layout) = analyze_grid(&lines) else {
        return colspan_verbatim(&raw_lines, common_indent, indent);
    };
    let cols_pos = &layout.cols_pos;
    let row_seps = &layout.row_seps;
    let ncols = cols_pos.len() - 1;
    let nrows = row_seps.len() - 1;
    if ncols == 0 || nrows == 0 {
        return colspan_verbatim(&raw_lines, common_indent, indent);
    }
    let idx_of = |pos: usize| -> Option<usize> { cols_pos.iter().position(|&b| b == pos) };

    let sep_lines: BTreeSet<usize> = layout.full_seps.iter().copied().collect();

    let line_markers = |line: &str| -> Option<Vec<(usize, char)>> {
        let mut markers = Vec::new();
        for (ci, ch) in line.chars().enumerate() {
            if ch == '+' || ch == '|' {
                markers.push((idx_of(ci)?, ch));
            }
        }
        if markers.first().map(|&(k, _)| k) != Some(0)
            || markers.last().map(|&(k, _)| k) != Some(ncols)
        {
            return None;
        }
        Some(markers)
    };

    enum Seg {
        Dash,
        Blank,
        Content,
    }
    let seg_role = |seg: &str| -> Seg {
        if seg.chars().all(|c| matches!(c, '-' | '=' | ':' | ' ')) {
            if seg.chars().any(|c| matches!(c, '-' | '=' | ':')) {
                Seg::Dash
            } else {
                Seg::Blank
            }
        } else {
            Seg::Content
        }
    };

    let mut widths: Vec<usize> = (0..ncols)
        .map(|c| {
            cols_pos[c + 1]
                .saturating_sub(cols_pos[c])
                .saturating_sub(3)
        })
        .collect();
    let mut spanning: Vec<(usize, usize, usize)> = Vec::new();
    for line in &lines {
        let chars: Vec<char> = line.chars().collect();
        let Some(markers) = line_markers(line) else {
            return colspan_verbatim(&raw_lines, common_indent, indent);
        };
        for win in markers.windows(2) {
            let (ka, _) = win[0];
            let (kb, _) = win[1];
            let seg: String = chars[cols_pos[ka] + 1..cols_pos[kb]].iter().collect();
            if !matches!(seg_role(&seg), Seg::Content) {
                continue;
            }
            let w = UnicodeWidthStr::width(seg.trim());
            if kb - ka == 1 {
                widths[ka] = widths[ka].max(w);
            } else {
                spanning.push((ka, kb, w));
            }
        }
    }
    spanning.sort_by_key(|&(s, e, _)| e - s);
    for (s, e, need) in spanning {
        let span = e - s;
        let cap = colspan_interior(&widths[s..e]);
        if need > cap {
            let deficit = need - cap;
            let per = deficit / span;
            let rem = deficit % span;
            for (k, w) in widths[s..e].iter_mut().enumerate() {
                *w += per + usize::from(k < rem);
            }
        }
    }

    let align_phys = lines
        .iter()
        .enumerate()
        .find(|(p, l)| sep_lines.contains(p) && l.contains('='))
        .or_else(|| {
            lines
                .iter()
                .enumerate()
                .find(|(p, _)| sep_lines.contains(p))
        })
        .map(|(p, _)| p);
    let mut alignments = vec![Alignment::Default; ncols];
    if let Some(p) = align_phys {
        let body = lines[p];
        let segs = colspan_separator_segments(body);
        let marker_idx: Vec<usize> = body
            .chars()
            .enumerate()
            .filter(|&(_, c)| c == '+')
            .filter_map(|(i, _)| idx_of(i))
            .collect();
        for (seg, win) in segs.iter().zip(marker_idx.windows(2)) {
            for slot in &mut alignments[win[0]..win[1]] {
                *slot = *seg;
            }
        }
    }

    let mut out = String::new();
    for (p, line) in lines.iter().enumerate() {
        let chars: Vec<char> = line.chars().collect();
        let Some(markers) = line_markers(line) else {
            return colspan_verbatim(&raw_lines, common_indent, indent);
        };
        let is_header = line.contains('=');
        let fill = if is_header { '=' } else { '-' };
        let emit_align = align_phys == Some(p);
        out.push(markers[0].1);
        for win in markers.windows(2) {
            let (ka, _) = win[0];
            let (kb, cb) = win[1];
            let interior = colspan_interior(&widths[ka..kb]);
            let seg: String = chars[cols_pos[ka] + 1..cols_pos[kb]].iter().collect();
            match seg_role(&seg) {
                Seg::Dash => out.push_str(&render_separator_segment(
                    interior,
                    fill,
                    alignments[ka],
                    emit_align,
                )),
                Seg::Blank => out.push_str(&" ".repeat(interior + 2)),
                Seg::Content => {
                    let padded = pad_colspan_cell(seg.trim(), interior, alignments[ka]);
                    out.push(' ');
                    out.push_str(&padded);
                    out.push(' ');
                }
            }
            out.push(cb);
        }
        out.push('\n');
    }

    if let Some(caption) = caption {
        let caption = format_table_caption_with_language(&caption, config, profile);
        out.push('\n');
        out.push_str(&caption);
        out.push('\n');
    }
    indent_table_block(&out, indent)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GridRowSection {
    Header,
    Body,
    Footer,
}

struct GridTableData {
    rows: Vec<Vec<String>>,
    row_sections: Vec<GridRowSection>,
    row_groups: Vec<usize>,
    alignments: Vec<Alignment>,
    caption: Option<String>,
    /// Per-column content widths derived from the source `+---+` separators.
    /// Grid column widths are load-bearing (pandoc maps them to relative output
    /// widths), so they are preserved as a floor rather than recomputed from
    /// content. See `format_grid_table`.
    column_widths: Vec<usize>,
}

fn extract_grid_table_data(node: &SyntaxNode, config: &Config) -> GridTableData {
    let mut rows = Vec::new();
    let mut row_sections = Vec::new();
    let mut row_groups = Vec::new();
    let mut alignments = Vec::new();
    let mut caption = None;
    let mut row_group_index = 0usize;
    let mut separator_widths: Vec<usize> = Vec::new();

    for child in node.children() {
        match child.kind() {
            SyntaxKind::TABLE_CAPTION => {
                let caption_text = extract_table_caption_content(&child);
                if caption.is_none() {
                    caption = Some(caption_text);
                }
            }
            SyntaxKind::TABLE_SEPARATOR => {
                let widths = grid_separator_widths_cst(&child);
                if separator_widths.len() < widths.len() {
                    separator_widths.resize(widths.len(), 0);
                }
                for (col_idx, w) in widths.into_iter().enumerate() {
                    separator_widths[col_idx] = separator_widths[col_idx].max(w);
                }

                let extracted = extract_grid_alignments(&child);
                if !extracted.is_empty()
                    && (alignments.is_empty() || extracted.iter().any(|a| *a != Alignment::Default))
                {
                    alignments = extracted;
                }
            }
            SyntaxKind::TABLE_HEADER | SyntaxKind::TABLE_ROW | SyntaxKind::TABLE_FOOTER => {
                let section = match child.kind() {
                    SyntaxKind::TABLE_HEADER => GridRowSection::Header,
                    SyntaxKind::TABLE_FOOTER => GridRowSection::Footer,
                    _ => GridRowSection::Body,
                };

                let cells = extract_row_cells(&child, config, false);
                let has_parsed_cells = !cells.is_empty();
                let mut seeded_from_plain_line = false;
                if !has_parsed_cells {
                    let row_text = text_without_line_prefixes(&child);
                    for line in row_text.lines() {
                        let trimmed_start = line.trim_start();
                        let trimmed_end = line.trim_end();
                        if !(trimmed_start.starts_with('|')
                            && trimmed_end.ends_with('|')
                            && !trimmed_start.contains('+'))
                        {
                            continue;
                        }
                        let parsed = split_grid_row(line);
                        if !parsed.is_empty() {
                            rows.push(parsed);
                            row_sections.push(section);
                            row_groups.push(row_group_index);
                            seeded_from_plain_line = true;
                        }
                        break;
                    }
                } else {
                    rows.push(cells);
                    row_sections.push(section);
                    row_groups.push(row_group_index);
                }

                let mut seen_first_content_line = false;
                let row_text = text_without_line_prefixes(&child);
                for line in row_text.lines() {
                    let trimmed_start = line.trim_start();
                    let trimmed_end = line.trim_end();
                    if !(trimmed_start.starts_with('|') && trimmed_end.ends_with('|')) {
                        continue;
                    }
                    if trimmed_start.contains('+') {
                        continue;
                    }
                    if !seen_first_content_line {
                        seen_first_content_line = true;
                        if has_parsed_cells || seeded_from_plain_line {
                            continue;
                        }
                    }
                    let parsed = split_grid_row(line);
                    if !parsed.is_empty() {
                        rows.push(parsed);
                        row_sections.push(section);
                        row_groups.push(row_group_index);
                    }
                }
                row_group_index += 1;
            }
            _ => {}
        }
    }

    let target_cols = if !alignments.is_empty() {
        alignments.len()
    } else {
        rows.iter().map(|r| r.len()).max().unwrap_or(0)
    };

    if target_cols > 0 {
        for row in &mut rows {
            if row.len() > target_cols {
                row.truncate(target_cols);
            } else if row.len() < target_cols {
                row.resize(target_cols, String::new());
            }
        }
        separator_widths.resize(target_cols, 0);
    }

    GridTableData {
        rows,
        row_sections,
        row_groups,
        alignments,
        caption,
        column_widths: separator_widths,
    }
}

/// Display-column positions of every `+`/`|` grid marker on a line, measured
/// after stripping `common_indent` leading spaces. Grid markers line up by
/// display column (not byte/char index), so wide characters are accounted for.
fn grid_marker_columns(line: &str, common_indent: usize) -> BTreeSet<usize> {
    let body = line
        .char_indices()
        .nth(common_indent)
        .map(|(i, _)| &line[i..])
        .unwrap_or("")
        .trim_end();
    let mut cols = BTreeSet::new();
    let mut col = 0usize;
    for ch in body.chars() {
        if ch == '+' || ch == '|' {
            cols.insert(col);
        }
        col += UnicodeWidthChar::width(ch).unwrap_or(0);
    }
    cols
}

/// Detect column-spanning grid tables: cells that straddle a column boundary
/// present elsewhere in the table (the canonical pandoc colspan, written by
/// omitting the `|`/`+` at that boundary on the spanning line). The structured
/// formatter assumes every row carries the full set of columns and would
/// truncate or pad spanning rows, dropping content. Such tables are preserved
/// verbatim instead. Rowspan-style lines (a `|` row containing `+`) are handled
/// earlier by `format_spanning_grid_table_raw`, so they never reach here.
fn grid_table_has_column_spans(raw_table: &str) -> bool {
    let grid_lines: Vec<&str> = raw_table
        .lines()
        .filter(|line| {
            let t = line.trim_start();
            let te = line.trim_end();
            t.starts_with('+') || (t.starts_with('|') && te.ends_with('|'))
        })
        .collect();
    if grid_lines.len() < 2 {
        return false;
    }

    let per_line: Vec<BTreeSet<usize>> = grid_lines
        .iter()
        .map(|line| {
            let indent = line.chars().take_while(|c| *c == ' ').count();
            grid_marker_columns(line, indent)
        })
        .collect();
    let union: BTreeSet<usize> = per_line.iter().flatten().copied().collect();

    per_line.iter().any(|cols| {
        let (Some(&min), Some(&max)) = (cols.iter().next(), cols.iter().next_back()) else {
            return false;
        };
        union
            .iter()
            .any(|&b| b > min && b < max && !cols.contains(&b))
    })
}

/// Interior width (excluding the single padding space on each side) of a cell
/// occupying `span` fine columns whose individual content widths are `widths`.
/// Merging n columns reclaims the n-1 internal `+`/`|` markers and their
/// flanking padding: `sum(widths) + 3*(n-1)`.
fn colspan_interior(widths: &[usize]) -> usize {
    let sum: usize = widths.iter().sum();
    sum + 3 * widths.len().saturating_sub(1)
}

fn render_separator_segment(
    interior: usize,
    fill: char,
    align: Alignment,
    on_align_line: bool,
) -> String {
    let mut seg: Vec<char> = std::iter::repeat_n(fill, interior + 2).collect();
    if on_align_line && seg.len() >= 2 {
        match align {
            Alignment::Center => {
                *seg.first_mut().unwrap() = ':';
                *seg.last_mut().unwrap() = ':';
            }
            Alignment::Left => *seg.first_mut().unwrap() = ':',
            Alignment::Right => *seg.last_mut().unwrap() = ':',
            Alignment::Default => {}
        }
    }
    seg.into_iter().collect()
}

fn pad_colspan_cell(text: &str, interior: usize, align: Alignment) -> String {
    let pad = interior.saturating_sub(text.width());
    match align {
        Alignment::Right => format!("{}{}", " ".repeat(pad), text),
        Alignment::Center => {
            let left = pad / 2;
            format!("{}{}{}", " ".repeat(left), text, " ".repeat(pad - left))
        }
        _ => format!("{}{}", text, " ".repeat(pad)),
    }
}

/// Lossless fallback: re-emit the (already de-captioned) table lines with only
/// the common block indent stripped, then re-apply `indent`. Used when a table
/// doesn't fit the colspan model we can lay out cleanly.
fn colspan_verbatim(lines: &[&str], common_indent: usize, indent: usize) -> String {
    let mut out = String::new();
    for line in lines {
        out.push_str(dedent_line(line, common_indent));
        out.push('\n');
    }
    indent_table_block(&out, indent)
}

fn dedent_line(line: &str, common_indent: usize) -> &str {
    line.char_indices()
        .nth(common_indent)
        .map(|(i, _)| &line[i..])
        .unwrap_or("")
        .trim_end()
}

/// A partial (rowspan) row separator: a `+`-leading boundary line where at
/// least one column's segment is blank because its cell continues into the
/// next row (`+   +---+`, `+---+   |`). The structured path would rebuild
/// it as a full separator, destroying the span, so such a table must take
/// the span-aware canonical-grid path.
pub(crate) fn is_partial_grid_separator(line: &str) -> bool {
    let trimmed = line.trim();
    if !trimmed.starts_with('+') || !trimmed.ends_with(['+', '|']) {
        return false;
    }
    let mut has_dashes = false;
    let mut has_blank = false;
    for segment in trimmed.split(['+', '|']) {
        if segment.is_empty() {
            continue;
        }
        if segment.chars().all(|c| c == ' ') {
            has_blank = true;
        } else if segment.chars().all(|c| matches!(c, '-' | '=' | ':')) {
            has_dashes = true;
        } else {
            return false;
        }
    }
    has_dashes && has_blank
}

fn colspan_separator_segments(separator: &str) -> Vec<Alignment> {
    let trimmed = separator.trim();
    trimmed
        .split('+')
        .skip(1)
        .take(trimmed.matches('+').count().saturating_sub(1))
        .map(|seg| {
            let starts = seg.starts_with(':');
            let ends = seg.ends_with(':');
            match (starts, ends) {
                (true, true) => Alignment::Center,
                (true, false) => Alignment::Left,
                (false, true) => Alignment::Right,
                (false, false) => Alignment::Default,
            }
        })
        .collect()
}

/// Format a grid table with consistent alignment and padding
pub fn format_grid_table(node: &SyntaxNode, config: &Config, indent: usize) -> String {
    let raw_table = text_without_line_prefixes(node);
    let mut extra_abbreviations = Vec::new();
    let profile = resolve_profile(node, config, &mut extra_abbreviations);

    let is_spanning = raw_table.lines().any(|line| {
        (line.trim_start().starts_with('|') && line.contains('+'))
            || is_partial_grid_separator(line)
    }) || grid_table_has_column_spans(&raw_table);
    if is_spanning {
        return format_unified_spanning_grid_table(&raw_table, config, profile, indent);
    }

    let mut table_data = extract_grid_table_data(node, config);
    let mut output = String::new();

    if table_data.rows.is_empty() {
        return raw_table;
    }

    let wrap_mode = config.wrap.clone().unwrap_or(WrapMode::Reflow);
    if wrap_mode != WrapMode::Preserve {
        reflow_grid_table_cells(&mut table_data);
    }

    let mut widths = calculate_grid_column_widths(&table_data.rows);
    for (col_idx, width) in widths.iter_mut().enumerate() {
        *width = (*width).max(table_data.column_widths.get(col_idx).copied().unwrap_or(0));
    }

    let make_separator = |fill_char: char, with_alignment_markers: bool| -> String {
        let mut line = String::from("+");

        for (col_idx, width) in widths.iter().enumerate() {
            let alignment = table_data
                .alignments
                .get(col_idx)
                .copied()
                .unwrap_or(Alignment::Default);

            let segment = if with_alignment_markers {
                match alignment {
                    Alignment::Left => {
                        let mut s = String::from(":");
                        s.push_str(&fill_char.to_string().repeat(width + 1));
                        s
                    }
                    Alignment::Right => {
                        let mut s = String::new();
                        s.push_str(&fill_char.to_string().repeat(width + 1));
                        s.push(':');
                        s
                    }
                    Alignment::Center => {
                        let mut s = String::from(":");
                        s.push_str(&fill_char.to_string().repeat(*width));
                        s.push(':');
                        s
                    }
                    Alignment::Default => fill_char.to_string().repeat(width + 2),
                }
            } else {
                fill_char.to_string().repeat(width + 2)
            };

            line.push_str(&segment);
            line.push('+');
        }

        line.push('\n');
        line
    };

    let has_header_rows = table_data.row_sections.contains(&GridRowSection::Header);
    output.push_str(&make_separator('-', !has_header_rows));

    for (row_idx, row) in table_data.rows.iter().enumerate() {
        let current_section = table_data
            .row_sections
            .get(row_idx)
            .copied()
            .unwrap_or(GridRowSection::Body);
        output.push('|');

        for (col_idx, _) in widths.iter().enumerate() {
            let cell = row.get(col_idx).map_or("", String::as_str);
            let width = widths.get(col_idx).copied().unwrap_or(3);
            let alignment = table_data
                .alignments
                .get(col_idx)
                .copied()
                .unwrap_or(Alignment::Default);

            output.push(' ');

            let cell_width = cell.width();
            let total_padding = width.saturating_sub(cell_width);
            let effective_alignment = if current_section == GridRowSection::Header {
                match alignment {
                    Alignment::Center => Alignment::Center,
                    _ => Alignment::Left,
                }
            } else {
                alignment
            };

            let padded_cell = match effective_alignment {
                Alignment::Left | Alignment::Default => {
                    format!("{}{}", cell, " ".repeat(total_padding))
                }
                Alignment::Right => {
                    format!("{}{}", " ".repeat(total_padding), cell)
                }
                Alignment::Center => {
                    let left_padding = total_padding / 2;
                    let right_padding = total_padding - left_padding;
                    format!(
                        "{}{}{}",
                        " ".repeat(left_padding),
                        cell,
                        " ".repeat(right_padding)
                    )
                }
            };

            output.push_str(&padded_cell);
            output.push_str(" |");
        }

        output.push('\n');

        let next_section = table_data.row_sections.get(row_idx + 1).copied();
        let current_group = table_data.row_groups.get(row_idx).copied();
        let next_group = table_data.row_groups.get(row_idx + 1).copied();

        if current_group.is_some() && current_group == next_group {
            continue;
        }

        let separator = match (current_section, next_section) {
            (GridRowSection::Header, Some(GridRowSection::Header)) => make_separator('-', false),
            (GridRowSection::Header, _) => make_separator('=', true),
            (GridRowSection::Body, Some(GridRowSection::Footer)) => make_separator('=', false),
            (GridRowSection::Footer, _) => make_separator('=', false),
            (_, _) => make_separator('-', false),
        };
        output.push_str(&separator);
    }

    if let Some(ref caption_text) = table_data.caption {
        output.push('\n');
        let formatted_caption = format_table_caption(caption_text, config, node);
        output.push_str(&formatted_caption);
        output.push('\n');
    }
    indent_table_block(&output, indent)
}

#[derive(Debug, Clone)]
struct SimpleColumn {
    /// Start position (byte index) in the line
    start: usize,
    /// End position (byte index) in the line
    end: usize,
    /// Column alignment
    alignment: Alignment,
}

fn extract_simple_table_columns(separator: &SyntaxNode) -> Vec<SimpleColumn> {
    let node_start =
        u32::from(separator.text_range().start()) + container_prefix_len(separator) as u32;
    separator_marker_tokens(separator)
        .filter(|t| t.kind() == SyntaxKind::TABLE_SEP_DASHES)
        .map(|t| {
            let start = (u32::from(t.text_range().start()) - node_start) as usize;
            SimpleColumn {
                start,
                end: start + t.text().len(),
                alignment: Alignment::Default,
            }
        })
        .collect()
}

/// Determine column alignments from a reference line's text position relative
/// to the separator dash runs. The reference line is the header when present,
/// or (for headerless tables) the first data row — both sit against the dash
/// runs, so the same flushness rule applies.
///
/// This restates pandoc's `alignType` (`Readers/Markdown.hs`), which slices the
/// line at the *column starts* — a cell owns the gap to the next column, and
/// the last one owns the rest of the line — then asks two questions of the
/// right-trimmed slice: does it begin past the dash run's first character
/// (`leftSpace`), and does it stop strictly before the run's last one
/// (`rightSpace`)? The same rule lives in the pandoc-AST projector
/// (`simple_table_aligns`), which is what the conformance suite pins.
///
/// Both bounds matter for idempotency, because the emitted dash run is
/// `content width + 2` of the *widest* cell in the column, which may sit in a
/// body row well past the end of the header line. Truncating the slice at the
/// dash-run end, or bailing to `Default` once the run outran the header, made
/// the verdict depend on the emitted line width, so a centered column flipped
/// to `Default` on the second pass and the table wobbled between two layouts.
fn determine_simple_alignments(
    columns: &mut [SimpleColumn],
    _separator_line: &str,
    header_line: Option<&str>,
) {
    let Some(header) = header_line else {
        return;
    };
    let header = header.trim_end_matches(['\n', '\r']);
    let starts: Vec<usize> = columns.iter().map(|col| col.start).collect();

    for (idx, col) in columns.iter_mut().enumerate() {
        let slice_end = starts
            .get(idx + 1)
            .copied()
            .unwrap_or(header.len())
            .min(header.len());
        let slice = header.get(col.start..slice_end).unwrap_or("");
        let right_trimmed = slice.trim_end_matches([' ', '\t']);
        if right_trimmed.is_empty() {
            col.alignment = Alignment::Default;
            continue;
        }

        let leading = right_trimmed.len() - right_trimmed.trim_start_matches([' ', '\t']).len();
        let col_width = col.end - col.start;
        let left_space = leading > 0;
        let right_space = right_trimmed.len() < col_width;

        col.alignment = match (left_space, right_space) {
            (false, false) => Alignment::Default,
            (false, true) => Alignment::Left,
            (true, false) => Alignment::Right,
            (true, true) => Alignment::Center,
        };
    }
}

fn split_simple_table_row(row_text: &str, columns: &[SimpleColumn]) -> Vec<String> {
    let mut cells = Vec::new();

    let row = if let Some(stripped) = row_text.strip_suffix("\r\n") {
        stripped
    } else if let Some(stripped) = row_text.strip_suffix('\n') {
        stripped
    } else {
        row_text
    };

    for (i, col) in columns.iter().enumerate() {
        let end = columns
            .get(i + 1)
            .map_or(row.len(), |next| next.start.min(row.len()));
        let cell_text = if col.start < end {
            row[col.start..end].trim()
        } else {
            ""
        };
        cells.push(collapse_cell_ws_runs(cell_text));
    }

    cells
}

fn extract_simple_table_data(node: &SyntaxNode, config: &Config) -> TableData {
    let mut rows = Vec::new();
    let mut columns: Vec<SimpleColumn> = Vec::new();
    let mut caption = None;
    let mut separator_line = String::new();
    let mut header_line: Option<String> = None;
    let mut header_cells: Option<Vec<String>> = None;
    let mut first_data_row_line: Option<String> = None;

    for child in node.children() {
        match child.kind() {
            SyntaxKind::TABLE_CAPTION => {
                let caption_text = extract_table_caption_content(&child);
                if caption.is_none() {
                    caption = Some(caption_text);
                }
            }
            SyntaxKind::TABLE_SEPARATOR => {
                if columns.is_empty() {
                    separator_line = raw_text_without_prefixes(&child);

                    columns = extract_simple_table_columns(&child);
                }
            }
            SyntaxKind::TABLE_HEADER => {
                header_line = Some(raw_text_without_prefixes(&child));

                let cells = extract_row_cells(&child, config, true);
                if !cells.is_empty() {
                    header_cells = Some(cells);
                } else {
                    header_cells = None;
                }
            }
            SyntaxKind::TABLE_ROW if !columns.is_empty() => {
                if first_data_row_line.is_none() {
                    first_data_row_line = Some(raw_text_without_prefixes(&child));
                }

                let cells = extract_row_cells(&child, config, true);

                if !cells.is_empty() {
                    rows.push(cells);
                } else {
                    let row_content = format_cell_content(&child, config, false);
                    let cells = split_simple_table_row(&row_content, &columns);
                    rows.push(cells);
                }
            }
            _ => {}
        }
    }

    if !columns.is_empty() {
        let alignment_line = header_line.as_deref().or(first_data_row_line.as_deref());
        determine_simple_alignments(&mut columns, &separator_line, alignment_line);
    }

    let has_header = header_line.is_some() || header_cells.is_some();

    if let Some(cells) = header_cells {
        rows.insert(0, cells);
    } else if let Some(header) = header_line {
        let header_cells = split_simple_table_row(&header, &columns);
        rows.insert(0, header_cells);
    }

    let alignments = columns.iter().map(|c| c.alignment).collect();

    TableData {
        rows,
        alignments,
        caption,
        has_header, // Simple tables may or may not have headers
    }
}

/// Pad a single simple-table cell to `width` according to its alignment.
fn pad_simple_cell(cell: &str, width: usize, alignment: Alignment) -> String {
    let total_padding = width.saturating_sub(cell.width());
    match alignment {
        Alignment::Left | Alignment::Default => format!("{cell}{}", " ".repeat(total_padding)),
        Alignment::Right => format!("{}{cell}", " ".repeat(total_padding)),
        Alignment::Center => {
            let left = total_padding / 2;
            let right = total_padding - left;
            format!("{}{cell}{}", " ".repeat(left), " ".repeat(right))
        }
    }
}

/// Format a simple table the way pandoc normalizes them.
///
/// Output geometry is recomputed purely from cell content: each column's dash
/// run (and field width) is `max-content-width + 2`, columns are separated by a
/// single space, and cell text is aligned within its field. This makes the
/// result independent of the incoming column spacing, so two documents that
/// parse to the same table format identically and the output is idempotent.
pub fn format_simple_table(node: &SyntaxNode, config: &Config, indent: usize) -> String {
    let raw_table = text_without_line_prefixes(node);
    if !raw_table.is_ascii() {
        return indent_table_block(&raw_table, indent);
    }

    let table_data = extract_simple_table_data(node, config);

    if table_data.rows.is_empty() {
        return indent_table_block(&raw_table, indent);
    }

    let has_header = table_data.has_header;
    let alignments = &table_data.alignments;

    let num_cols = table_data.rows.iter().map(|r| r.len()).max().unwrap_or(0);
    let mut content_widths = vec![0usize; num_cols];
    for row in &table_data.rows {
        for (col_idx, cell) in row.iter().enumerate() {
            content_widths[col_idx] = content_widths[col_idx].max(cell.width());
        }
    }
    let field_widths: Vec<usize> = content_widths.iter().map(|w| w + 2).collect();

    let push_separator = |output: &mut String| {
        for (col_idx, width) in field_widths.iter().enumerate() {
            if col_idx > 0 {
                output.push(' ');
            }
            output.push_str(&"-".repeat(*width));
        }
        output.push('\n');
    };

    let push_row = |output: &mut String, cells: &[String]| {
        let mut line = String::new();
        for (col_idx, width) in field_widths.iter().enumerate() {
            if col_idx > 0 {
                line.push(' ');
            }
            let cell = cells.get(col_idx).map(String::as_str).unwrap_or("");
            let alignment = alignments
                .get(col_idx)
                .copied()
                .unwrap_or(Alignment::Default);
            line.push_str(&pad_simple_cell(cell, *width, alignment));
        }
        output.push_str(line.trim_end());
        output.push('\n');
    };

    let mut output = String::new();

    if has_header {
        push_row(&mut output, &table_data.rows[0]);
        push_separator(&mut output);
        for row in table_data.rows.iter().skip(1) {
            push_row(&mut output, row);
        }
        let has_closer = node
            .children()
            .filter(|c| c.kind() == SyntaxKind::TABLE_SEPARATOR)
            .count()
            >= 2;
        let blank_follows = match node.next_sibling() {
            Some(next) => next.kind() == SyntaxKind::BLANK_LINE,
            None => node
                .parent()
                .is_none_or(|p| p.kind() == SyntaxKind::DOCUMENT),
        };
        if has_closer && !blank_follows {
            push_separator(&mut output);
        }
    } else {
        push_separator(&mut output);
        for row in &table_data.rows {
            push_row(&mut output, row);
        }
        push_separator(&mut output);
    }

    if let Some(ref caption_text) = table_data.caption {
        output.push('\n');
        let formatted_caption = format_table_caption(caption_text, config, node);
        output.push_str(&formatted_caption);
        output.push('\n');
    }
    indent_table_block(&output, config.table_indent + indent)
}

fn extract_multiline_columns(separator: &SyntaxNode) -> Vec<(usize, usize)> {
    let node_start =
        u32::from(separator.text_range().start()) + container_prefix_len(separator) as u32;
    separator_marker_tokens(separator)
        .filter(|t| t.kind() == SyntaxKind::TABLE_SEP_DASHES)
        .map(|t| {
            let start = (u32::from(t.text_range().start()) - node_start) as usize;
            (start, start + t.text().len())
        })
        .collect()
}

/// Column geometry for a multiline table, derived from the separator dash runs.
///
/// Pandoc spans a column from the start of its dash run to the start of the
/// *next* run (the inter-run gap belongs to the left column); the last column
/// runs to end-of-line. We therefore slice cell content on the wider `slice`
/// span (so gap text is never truncated) but render each non-last column one
/// space narrower than its span, reserving a single-space gutter so the
/// re-emitted separator and rows keep columns separated. Keeping every column
/// *start* fixed preserves the relative widths pandoc derives from the dash
/// geometry, so the table renders identically.
struct MultilineColumns {
    /// `(start, exclusive end)` used to slice cell text. The last column's end
    /// is `usize::MAX` so it captures everything to end-of-line.
    slice: Vec<(usize, usize)>,
    /// `(start, exclusive end)` used for rendering: dash runs, padding, reflow.
    render: Vec<(usize, usize)>,
}

fn multiline_columns(raw: &[(usize, usize)]) -> MultilineColumns {
    let n = raw.len();
    let mut slice = Vec::with_capacity(n);
    let mut render = Vec::with_capacity(n);
    for (i, &(start, dash_end)) in raw.iter().enumerate() {
        if i + 1 < n {
            let next = raw[i + 1].0;
            slice.push((start, next));
            let width = next.saturating_sub(start).saturating_sub(1);
            render.push((start, start + width));
        } else {
            slice.push((start, usize::MAX));
            render.push((start, dash_end));
        }
    }
    MultilineColumns { slice, render }
}

fn determine_multiline_alignment(header_text: &str, col_start: usize, col_end: usize) -> Alignment {
    if header_text.is_empty() {
        return Alignment::Default;
    }

    let first_line = header_text
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("");

    let header_in_col = if col_end <= first_line.len() {
        &first_line[col_start..col_end]
    } else if col_start < first_line.len() {
        &first_line[col_start..]
    } else {
        return Alignment::Default;
    };

    let text_start = header_in_col.len() - header_in_col.trim_start().len();
    let trimmed_text = header_in_col.trim();
    let text_end = text_start + trimmed_text.len();

    let col_width = col_end - col_start;
    let flush_left = text_start == 0;
    let flush_right = text_end == col_width;

    match (flush_left, flush_right) {
        (true, true) => Alignment::Default,
        (true, false) => Alignment::Left,
        (false, true) => Alignment::Right,
        (false, false) => Alignment::Center,
    }
}

struct MultilineTableData {
    /// Rows of cells, where each cell is a vector of lines
    rows: Vec<Vec<Vec<String>>>,
    alignments: Vec<Alignment>,
    caption: Option<String>,
    column_positions: Vec<(usize, usize)>,
    has_header: bool,
}

fn extract_multiline_cells(text: &str, column_positions: &[(usize, usize)]) -> Vec<Vec<String>> {
    let lines: Vec<&str> = text.lines().collect();
    let num_cols = column_positions.len();

    let mut cells: Vec<Vec<String>> = vec![Vec::new(); num_cols];

    for line in lines {
        for (col_idx, &(col_start, col_end)) in column_positions.iter().enumerate() {
            let cell_line = if col_end <= line.len() {
                &line[col_start..col_end]
            } else if col_start < line.len() {
                &line[col_start..]
            } else {
                ""
            };
            cells[col_idx].push(cell_line.trim().to_string());
        }
    }

    cells
}

fn extract_cells_from_table_cell_nodes(
    row: &SyntaxNode,
    config: &Config,
    column_positions: &[(usize, usize)],
) -> Vec<Vec<String>> {
    let formatted_text = text_without_prefixes(row, |node| {
        if node.kind() == SyntaxKind::TABLE_CELL {
            format_cell_content(node, config, false)
        } else {
            node.text().to_string()
        }
    });

    extract_multiline_cells(&formatted_text, column_positions)
}

fn extract_multiline_table_data(node: &SyntaxNode, config: &Config) -> MultilineTableData {
    let mut rows: Vec<Vec<Vec<String>>> = Vec::new();
    let mut raw_columns: Vec<(usize, usize)> = Vec::new();
    let mut alignments = Vec::new();
    let mut caption = None;
    let mut has_header = false;
    let mut header_text = String::new();
    let mut separator_count = 0;

    for child in node.children() {
        match child.kind() {
            SyntaxKind::TABLE_CAPTION => {
                let caption_text = extract_table_caption_content(&child);
                if caption.is_none() {
                    caption = Some(caption_text);
                }
            }
            SyntaxKind::TABLE_SEPARATOR => {
                separator_count += 1;

                if separator_count == 1 || (separator_count == 2 && has_header) {
                    raw_columns = extract_multiline_columns(&child);
                }
            }
            SyntaxKind::TABLE_HEADER => {
                has_header = true;
                header_text = raw_text_without_prefixes(&child);
            }
            SyntaxKind::TABLE_ROW => {
                let slice = multiline_columns(&raw_columns).slice;
                if child.children().any(|c| c.kind() == SyntaxKind::TABLE_CELL) {
                    let cells = extract_cells_from_table_cell_nodes(&child, config, &slice);
                    rows.push(cells);
                } else {
                    let row_content = format_cell_content(&child, config, false);
                    let cells = extract_multiline_cells(&row_content, &slice);
                    rows.push(cells);
                }
            }
            _ => {}
        }
    }

    let slice = multiline_columns(&raw_columns).slice;

    if has_header && !raw_columns.is_empty() {
        let header_node = node
            .children()
            .find(|c| c.kind() == SyntaxKind::TABLE_HEADER);

        let header_cells = if let Some(hdr) = header_node {
            if hdr.children().any(|c| c.kind() == SyntaxKind::TABLE_CELL) {
                extract_cells_from_table_cell_nodes(&hdr, config, &slice)
            } else {
                extract_multiline_cells(&header_text, &slice)
            }
        } else {
            extract_multiline_cells(&header_text, &slice)
        };

        rows.insert(0, header_cells);
    }

    let mut column_positions = multiline_columns(&raw_columns).render;
    if let Some(&(last_start, last_end)) = column_positions.last() {
        let last_idx = column_positions.len() - 1;
        let content_width = rows
            .iter()
            .filter_map(|row| row.get(last_idx))
            .flat_map(|cell| cell.iter())
            .map(|line| line.trim_end().width())
            .max()
            .unwrap_or(0);
        let width = (last_end - last_start).max(content_width);
        column_positions[last_idx] = (last_start, last_start + width);
    }

    if has_header && !column_positions.is_empty() {
        for &(col_start, col_end) in &column_positions {
            alignments.push(determine_multiline_alignment(
                &header_text,
                col_start,
                col_end,
            ));
        }
    } else if !rows.is_empty() && !column_positions.is_empty() {
        let first_row_node = node
            .children()
            .find(|c| c.kind() == SyntaxKind::TABLE_ROW)
            .unwrap();
        let first_row_text = raw_text_without_prefixes(&first_row_node);
        for &(col_start, col_end) in &column_positions {
            alignments.push(determine_multiline_alignment(
                &first_row_text,
                col_start,
                col_end,
            ));
        }
    } else {
        alignments = vec![Alignment::Default; column_positions.len()];
    }

    MultilineTableData {
        rows,
        alignments,
        caption,
        column_positions,
        has_header,
    }
}

/// Format a multiline table preserving column widths and structure
pub fn format_multiline_table(node: &SyntaxNode, config: &Config, indent: usize) -> String {
    let raw_table = text_without_line_prefixes(node);
    if !raw_table.is_ascii() {
        return indent_table_block(&raw_table, indent);
    }

    let mut table_data = extract_multiline_table_data(node, config);
    let mut output = String::new();

    if table_data.rows.is_empty() || table_data.column_positions.is_empty() {
        return indent_table_block(&raw_table, indent);
    }

    let wrap_mode = config.wrap.clone().unwrap_or(WrapMode::Reflow);
    if wrap_mode != WrapMode::Preserve {
        let col_widths: Vec<usize> = table_data
            .column_positions
            .iter()
            .map(|(start, end)| end.saturating_sub(*start))
            .collect();
        let body_start = usize::from(table_data.has_header);
        for row in table_data.rows.iter_mut().skip(body_start) {
            for (col_idx, cell) in row.iter_mut().enumerate() {
                let width = col_widths.get(col_idx).copied().unwrap_or(0);
                *cell = reflow_cell_lines(cell, width);
            }
        }
    }

    let base_offset = table_data
        .column_positions
        .first()
        .map(|(start, _)| *start)
        .unwrap_or(0);
    let positions: Vec<(usize, usize)> = table_data
        .column_positions
        .iter()
        .map(|(start, end)| {
            (
                start.saturating_sub(base_offset),
                end.saturating_sub(base_offset),
            )
        })
        .collect();

    let last_col_end = positions.last().map(|(_, end)| *end).unwrap_or(0);

    if table_data.has_header {
        output.push_str(&"-".repeat(last_col_end));
        output.push('\n');
    } else {
        let mut sep_chars: Vec<char> = vec![' '; last_col_end];
        for &(col_start, col_end) in &positions {
            for item in sep_chars.iter_mut().take(col_end).skip(col_start) {
                *item = '-';
            }
        }
        output.push_str(&sep_chars.iter().collect::<String>());
        output.push('\n');
    }

    if table_data.has_header && !table_data.rows.is_empty() {
        let header_row = &table_data.rows[0];

        let max_lines = header_row.iter().map(|cell| cell.len()).max().unwrap_or(0);

        for line_idx in 0..max_lines {
            let mut line_chars: Vec<char> = vec![' '; last_col_end];

            for (col_idx, cell_lines) in header_row.iter().enumerate() {
                if let Some(&(col_start, col_end)) = positions.get(col_idx) {
                    let cell_text = cell_lines.get(line_idx).map(|s| s.as_str()).unwrap_or("");
                    let alignment = table_data
                        .alignments
                        .get(col_idx)
                        .copied()
                        .unwrap_or(Alignment::Default);

                    let col_width = col_end - col_start;
                    let cell_width = cell_text.trim_end().width();
                    let total_padding = col_width.saturating_sub(cell_width);

                    let text_start_in_col = match alignment {
                        Alignment::Left | Alignment::Default => 0,
                        Alignment::Right => total_padding,
                        Alignment::Center => total_padding / 2,
                    };

                    for (i, ch) in cell_text.trim_end().chars().enumerate() {
                        let target_pos = col_start + text_start_in_col + i;
                        if target_pos < line_chars.len() {
                            line_chars[target_pos] = ch;
                        }
                    }
                }
            }

            output.push_str(line_chars.iter().collect::<String>().trim_end());
            output.push('\n');
        }

        let mut sep_chars: Vec<char> = vec![' '; last_col_end];
        for &(col_start, col_end) in &positions {
            for item in sep_chars.iter_mut().take(col_end).skip(col_start) {
                *item = '-';
            }
        }
        output.push_str(&sep_chars.iter().collect::<String>());
        output.push('\n');
    }

    let start_row = if table_data.has_header { 1 } else { 0 };
    for (row_idx, row) in table_data.rows.iter().enumerate().skip(start_row) {
        let max_lines = row.iter().map(|cell| cell.len()).max().unwrap_or(0);

        for line_idx in 0..max_lines {
            let mut line_chars: Vec<char> = vec![' '; last_col_end];

            for (col_idx, cell_lines) in row.iter().enumerate() {
                if let Some(&(col_start, col_end)) = positions.get(col_idx) {
                    let cell_text = cell_lines.get(line_idx).map(|s| s.as_str()).unwrap_or("");
                    let alignment = table_data
                        .alignments
                        .get(col_idx)
                        .copied()
                        .unwrap_or(Alignment::Default);

                    let col_width = col_end - col_start;
                    let cell_width = cell_text.trim_end().width();
                    let total_padding = col_width.saturating_sub(cell_width);

                    let text_start_in_col = match alignment {
                        Alignment::Left | Alignment::Default => 0,
                        Alignment::Right => total_padding,
                        Alignment::Center => total_padding / 2,
                    };

                    for (i, ch) in cell_text.trim_end().chars().enumerate() {
                        let target_pos = col_start + text_start_in_col + i;
                        if target_pos < line_chars.len() {
                            line_chars[target_pos] = ch;
                        }
                    }
                }
            }

            output.push_str(line_chars.iter().collect::<String>().trim_end());
            output.push('\n');
        }

        if row_idx < table_data.rows.len() - 1 {
            output.push('\n');
        }
    }

    if table_data.has_header {
        output.push_str(&"-".repeat(last_col_end));
        output.push('\n');
    } else {
        let mut sep_chars: Vec<char> = vec![' '; last_col_end];
        for &(col_start, col_end) in &positions {
            for item in sep_chars.iter_mut().take(col_end).skip(col_start) {
                *item = '-';
            }
        }
        output.push_str(&sep_chars.iter().collect::<String>());
        output.push('\n');
    }

    if let Some(ref caption_text) = table_data.caption {
        output.push('\n');
        let formatted_caption = format_table_caption(caption_text, config, node);
        output.push_str(&formatted_caption);
        output.push('\n');
    }
    indent_table_block(&output, config.table_indent + indent)
}

#[cfg(test)]
mod grid_reflow_tests {
    use super::*;

    fn lines(text: &str) -> Vec<String> {
        text.lines().map(str::to_string).collect()
    }

    #[test]
    fn ordered_list_marker_distinguishes_numbers_from_markers() {
        assert!(is_ordered_list_marker("1."));
        assert!(is_ordered_list_marker("2)"));
        assert!(is_ordered_list_marker("42."));
        assert!(!is_ordered_list_marker("1,234"));
        assert!(!is_ordered_list_marker("v2.0"));
        assert!(!is_ordered_list_marker("1"));
        assert!(!is_ordered_list_marker("."));
    }

    #[test]
    fn plain_prose_cells_are_reflowable() {
        assert!(grid_cell_is_reflowable(&lines("Lorem ipsum\ndolor sit")));
        assert!(grid_cell_is_reflowable(&lines(
            "A fairly long\ndescription"
        )));
    }

    #[test]
    fn block_and_hard_break_cells_are_not_reflowable() {
        assert!(!grid_cell_is_reflowable(&lines("- item one\n- item two")));
        assert!(!grid_cell_is_reflowable(&lines("1. first\n2. second")));
        assert!(!grid_cell_is_reflowable(&lines("> quote")));
        assert!(!grid_cell_is_reflowable(&lines("# heading")));
        assert!(!grid_cell_is_reflowable(&lines("```\ncode\n```")));
        assert!(!grid_cell_is_reflowable(&lines("Population\\\n(in 2018)")));
    }

    #[test]
    fn empty_or_blank_only_cells_are_not_reflowable() {
        assert!(!grid_cell_is_reflowable(&[]));
        assert!(!grid_cell_is_reflowable(&lines("\n   \n")));
    }

    #[test]
    fn reflow_packs_prose_and_drops_trailing_blank() {
        let out = reflow_or_trim_grid_cell(&lines("Lorem ipsum\ndolor sit\n"), 18);
        assert_eq!(out, vec!["Lorem ipsum dolor", "sit"]);
    }

    #[test]
    fn trim_only_keeps_block_content_but_drops_blank_edges() {
        let out = reflow_or_trim_grid_cell(&lines("\n- item one\n- item two\n"), 18);
        assert_eq!(out, vec!["- item one", "- item two"]);
    }
}
