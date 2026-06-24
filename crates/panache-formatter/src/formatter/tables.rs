use crate::config::{Config, WrapMode};
use crate::formatter::inline::format_inline_node;
use crate::formatter::inline_layout::wrap_text_first_fit;
use crate::formatter::sentence_wrap::{ResolvedProfile, resolve_profile, split_sentence_text};
use crate::syntax::{SyntaxKind, SyntaxNode, SyntaxToken};
use panache_parser::analyze_grid;
use rowan::NodeOrToken;
use std::collections::BTreeSet;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

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
    // Group consecutive non-blank lines into paragraphs, dropping blank runs.
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
            // Preserve the paragraph break between reflowed paragraphs.
            out.push(String::new());
        }
        let joined = paragraph.join(" ");
        if width == 0 {
            // Degenerate column: keep the text rather than wrapping to nothing.
            out.push(joined);
        } else {
            out.extend(wrap_text_first_fit(&joined, width));
        }
    }
    out
}

/// Whether a grid cell's content is plain prose that can be safely reflowed.
///
/// Grid cells can hold arbitrary block content (lists, code, blockquotes,
/// headings) and hard line breaks (a trailing `\`). Reflowing those as plain
/// text would corrupt them, so we only reflow cells whose every non-blank line
/// is ordinary inline text. Block-bearing cells are kept verbatim (their
/// leading/trailing blank padding is still trimmed in `reflow_or_trim_grid_cell`).
fn grid_cell_is_reflowable(lines: &[String]) -> bool {
    let mut has_content = false;
    for line in lines {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        has_content = true;
        // A trailing backslash is a pandoc hard line break -- keep the geometry.
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

    // Bullet list: "-", "*", or "+" followed by content.
    if matches!(first, "-" | "*" | "+") && trimmed.len() > first.len() {
        return true;
    }
    // Ordered list: digits then '.' or ')' (e.g. "1.", "2)") followed by content.
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

/// Whether `token` is an ordered-list marker like `1.` or `2)`.
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
        // `reflow_cell_lines` already drops leading/trailing/internal blank runs.
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

    // Reflow to the width the renderer will actually use for each column: the
    // widest existing content line, floored at the load-bearing source width.
    // Using the bare source width would wrap a cell whose content already
    // exceeds it (the renderer expands such a column instead of shrinking it),
    // and would not be idempotent. The widest line always reproduces at its own
    // width, so this target is stable across passes.
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

        // Reflow/trim each column across the group's physical lines.
        let mut cols: Vec<Vec<String>> = Vec::with_capacity(num_cols);
        for (col, &target) in targets.iter().enumerate() {
            let lines: Vec<String> = (start..end)
                .map(|r| table_data.rows[r].get(col).cloned().unwrap_or_default())
                .collect();
            cols.push(reflow_or_trim_grid_cell(&lines, target));
        }

        // Redistribute the per-column lines back into physical rows; keep at
        // least one line so an all-empty group still renders a row.
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
        // A caption is collapsed to a single logical line, so there are no soft
        // breaks for `Semantic` to preserve — it degenerates to `Sentence`.
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
    // Captions inside a blockquote carry BLOCK_QUOTE_MARKER tokens for
    // losslessness; the blockquote formatter re-adds the prefix dynamically, so
    // drop the marker (and the whitespace that follows it) here.
    let mut skip_next_whitespace = false;

    for caption_child in caption_node.children_with_tokens() {
        match caption_child {
            rowan::NodeOrToken::Token(token) if token.kind() == SyntaxKind::BLOCK_QUOTE_MARKER => {
                skip_next_whitespace = true;
            }
            rowan::NodeOrToken::Token(token)
                if token.kind() == SyntaxKind::WHITESPACE && skip_next_whitespace =>
            {
                skip_next_whitespace = false;
            }
            rowan::NodeOrToken::Token(token)
                if token.kind() == SyntaxKind::TABLE_CAPTION_PREFIX =>
            {
                // Skip the original prefix
                skip_next_whitespace = false;
            }
            rowan::NodeOrToken::Token(token) => {
                skip_next_whitespace = false;
                caption_body.push_str(token.text());
            }
            rowan::NodeOrToken::Node(node) => {
                skip_next_whitespace = false;
                caption_body.push_str(&node.text().to_string());
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

/// Format cell content, handling both TEXT tokens and inline elements
fn format_cell_content(node: &SyntaxNode, config: &Config) -> String {
    let mut result = String::new();

    for child in node.children_with_tokens() {
        match child {
            NodeOrToken::Token(token) => {
                if token.kind() == SyntaxKind::TEXT
                    || token.kind() == SyntaxKind::NEWLINE
                    || token.kind() == SyntaxKind::ESCAPED_CHAR
                {
                    result.push_str(token.text());
                }
            }
            NodeOrToken::Node(node) => {
                // Handle inline elements (emphasis, code, links, etc.)
                result.push_str(&format_inline_node(&node, config));
            }
        }
    }

    result
}

/// Extract cell contents from TABLE_CELL nodes if present, otherwise fall back to text splitting
fn extract_row_cells(row_node: &SyntaxNode, config: &Config) -> Vec<String> {
    let mut cells = Vec::new();

    // Check if this row has TABLE_CELL children
    let has_table_cells = row_node
        .children()
        .any(|child| child.kind() == SyntaxKind::TABLE_CELL);

    if has_table_cells {
        // New approach: extract from TABLE_CELL nodes
        for child in row_node.children() {
            if child.kind() == SyntaxKind::TABLE_CELL {
                cells.push(format_cell_content(&child, config));
            }
        }
    }

    cells
}

/// Extract alignments from separator line (e.g., "|:---|---:|:---:|")
/// The separator-marker tokens (`TABLE_SEP_*`) of a `TABLE_SEPARATOR` node,
/// in order. Skips the container prefix (`WHITESPACE` / blockquote markers)
/// and the trailing `NEWLINE`.
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

/// Split a separator's marker tokens into delimiter-separated segments
/// (`split('|')` / `split('+')` over the marker stream).
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
        // Skip empty cells (whitespace-only, incl. those from leading/trailing
        // and doubled pipes) — matches the old trim + skip-empty behavior.
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

/// Split a row into cells, handling leading/trailing pipes
fn split_row(row_text: &str) -> Vec<String> {
    let trimmed = row_text.trim();
    let cells: Vec<&str> = trimmed.split('|').collect();

    cells
        .iter()
        .enumerate()
        .filter_map(|(i, cell)| {
            let cell = cell.trim();
            // Skip first and last if they're empty (from leading/trailing pipes)
            if (i == 0 || i == cells.len() - 1) && cell.is_empty() {
                None
            } else {
                Some(cell.to_string())
            }
        })
        .collect()
}

/// Extract structured data from pipe table AST node
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
                // Prefer the structured TABLE_CELL nodes: the parser already
                // resolved cell boundaries with escape awareness, so an escaped
                // `\|` stays inside its cell. Re-rendering the row and splitting
                // on `|` (as `split_row` does) is escape-blind: it re-tokenizes
                // the `\|` as a delimiter and invents a phantom column.
                let cells = extract_row_cells(&child, config);
                let cells = if cells.is_empty() {
                    split_row(&format_cell_content(&child, config))
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
                // Use unicode display width instead of byte length
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
                // Use unicode display width instead of byte length
                widths[col_idx] = widths[col_idx].max(cell.width());
            }
        }
    }

    widths
}

/// Format a pipe table with consistent alignment and padding
pub fn format_pipe_table(node: &SyntaxNode, config: &Config, indent: usize) -> String {
    let table_data = extract_pipe_table_data(node, config);
    let mut output = String::new();

    // Early return if no rows
    if table_data.rows.is_empty() {
        return node.text().to_string();
    }

    let widths = calculate_column_widths(&table_data.rows);

    // Format rows
    for (row_idx, row) in table_data.rows.iter().enumerate() {
        output.push('|');

        for (col_idx, cell) in row.iter().enumerate() {
            let width = widths.get(col_idx).copied().unwrap_or(3);
            let alignment = table_data
                .alignments
                .get(col_idx)
                .copied()
                .unwrap_or(Alignment::Default);

            // Add padding
            output.push(' ');

            // Apply alignment using unicode display width
            let cell_width = cell.width();
            let total_padding = width.saturating_sub(cell_width);

            let padded_cell = if row_idx == 0 {
                // Header row: always left-align
                format!("{}{}", cell, " ".repeat(total_padding))
            } else {
                // Data rows: respect alignment
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

        // Insert separator after first row (header)
        if row_idx == 0 {
            output.push('|');

            for (col_idx, width) in widths.iter().enumerate() {
                let alignment = table_data
                    .alignments
                    .get(col_idx)
                    .copied()
                    .unwrap_or(Alignment::Default);

                output.push(' ');

                // Create separator with alignment markers
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
    // A top-level pipe table self-indents by the configured `table-indent`
    // (default 2). Nested tables always honor the container indent threaded in.
    let block_indent = if indent == 0 {
        config.table_indent
    } else {
        indent
    };
    indent_table_block(&output, block_indent)
}

// Grid Table Formatting
// ============================================================================

/// Extract alignments from grid table separator line (e.g., "+:---+---:+:---:+")
/// Token segments strictly between consecutive `+` delimiters (the grid
/// columns). Mirrors `split('+')` then `skip(1).take(len - 2)`.
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
        // Grid segments carry no interior whitespace; match the old untrimmed
        // first/last-char colon check, skipping empty (`++`) segments.
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

/// Split a grid table row into cells (e.g., "| A | B |" -> ["A", "B"])
fn split_grid_row(row_text: &str) -> Vec<String> {
    let trimmed = row_text.trim();

    // Split by | and filter
    let cells: Vec<&str> = trimmed.split('|').collect();

    cells
        .iter()
        .enumerate()
        .filter_map(|(i, cell)| {
            let cell = cell.trim();
            // Skip first and last if they're empty (from leading/trailing pipes)
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

    // Peel a leading/trailing caption line off the table body.
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

    // Set of separator physical-line indices.
    let sep_lines: BTreeSet<usize> = row_seps.iter().copied().collect();

    // Per-line marker skeleton: the canonical-boundary index of every `+`/`|` on
    // the line, with its char. `None` if a marker doesn't land on a boundary or
    // an outer border is missing -- the table is misaligned and is preserved
    // verbatim rather than laid out on borders that don't line up.
    let line_markers = |line: &str| -> Option<Vec<(usize, char)>> {
        let mut markers = Vec::new();
        // Use char positions (not byte offsets): `cols_pos` is char-indexed, and
        // a multibyte char (e.g. `°`) before a marker would otherwise mismap.
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

    // A segment (between two markers) is a cell edge (`-`/`=`/`:`), a
    // rowspan-interior blank (only spaces), or content.
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

    // Column widths. The floor is the column's source border width (preserved,
    // not shrunk to content): pandoc derives relative column widths from grid
    // border widths and propagates them to output formats, so a grid column
    // carries layout meaning -- see #323. The canonical boundary spacing encodes
    // that width: between adjacent `+`s sit one pad space, the interior, one pad
    // space, so interior = gap - 3. Content then raises the floor (single-column
    // cells directly, multi-column colspans by growing their span's deficit,
    // shorter spans first). Sized from the source rather than the cell tiling,
    // so it stays robust where the tiling can't model a construct (e.g. a
    // rowspan cell whose text sits on a sub-row separator). Idempotent: after one
    // pass the borders already match.
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

    // Alignment from the alignment-bearing separator: header `===` line if any,
    // else the first separator. Default (left) where no colon.
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

    // Emit every line on the canonical grid. The marker skeleton (which
    // boundaries carry a `+`/`|`) and each segment's role are read from the
    // source line, recomputing only segment lengths from `widths`:
    //   * a segment of only `-`/`=`/`:`(/spaces) is a cell edge -> dash run;
    //   * a segment of only spaces is a rowspan interior -> kept blank;
    //   * anything else is a content cell -> trimmed and re-padded.
    // Reading the skeleton from the source (rather than deriving it from the
    // cell tiling) is what lets a colspan boundary the alignment row still
    // marks with `+` differ from one a rowspan line leaves blank, and handles
    // hybrid lines that carry rowspan-cell text *and* a sub-row separator
    // (e.g. `| Temperature +----+----+`).
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

/// Extract structured data from grid table AST node
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
                // Grid column widths encode relative output widths (pandoc maps
                // them to <col style="width:X%">), so take the per-column max
                // across every separator to preserve the source widths later.
                let widths = grid_separator_widths_cst(&child);
                if separator_widths.len() < widths.len() {
                    separator_widths.resize(widths.len(), 0);
                }
                for (col_idx, w) in widths.into_iter().enumerate() {
                    separator_widths[col_idx] = separator_widths[col_idx].max(w);
                }

                // Extract alignments from separators that have them
                // Grid tables have alignments in the first separator (headerless)
                // or header separator (tables with headers)
                // Priority: extract from any separator with colons, otherwise keep Default
                let extracted = extract_grid_alignments(&child);
                if !extracted.is_empty() && extracted.iter().any(|a| *a != Alignment::Default) {
                    // Found a separator with alignment info, use it
                    alignments = extracted;
                } else if alignments.is_empty() && !extracted.is_empty() {
                    // No alignments yet, save these (even if all Default)
                    alignments = extracted;
                }
            }
            SyntaxKind::TABLE_HEADER | SyntaxKind::TABLE_ROW | SyntaxKind::TABLE_FOOTER => {
                let section = match child.kind() {
                    SyntaxKind::TABLE_HEADER => GridRowSection::Header,
                    SyntaxKind::TABLE_FOOTER => GridRowSection::Footer,
                    _ => GridRowSection::Body,
                };

                let cells = extract_row_cells(&child, config);
                let has_parsed_cells = !cells.is_empty();
                let mut seeded_from_plain_line = false;
                if !has_parsed_cells {
                    let row_text = child.text().to_string();
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

                // Continuation lines are emitted as raw text in CST rows; include
                // them for width calculation and output structure.
                let mut seen_first_content_line = false;
                let row_text = child.text().to_string();
                for line in row_text.lines() {
                    let trimmed_start = line.trim_start();
                    let trimmed_end = line.trim_end();
                    if !(trimmed_start.starts_with('|') && trimmed_end.ends_with('|')) {
                        continue;
                    }
                    // Spanning-style boundary lines contain embedded '+' separators.
                    // Keep them attached to the row text via parser losslessness, but
                    // don't treat them as independent logical rows for column sizing/output.
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

    // Measure each line's markers relative to its OWN leading indent, not a
    // global minimum. A grid table nested as a list item's first block has its
    // first line flush (the marker supplies that indent) while continuation
    // lines are indented; a global indent would leave those offset and fabricate
    // a span. Per-line normalization aligns them, so only genuine colspans (a
    // row missing an interior marker the rest of the table observes) trip this.
    let per_line: Vec<BTreeSet<usize>> = grid_lines
        .iter()
        .map(|line| {
            let indent = line.chars().take_while(|c| *c == ' ').count();
            grid_marker_columns(line, indent)
        })
        .collect();
    let union: BTreeSet<usize> = per_line.iter().flatten().copied().collect();

    // A span exists when some line is missing a marker that other lines place
    // strictly between this line's own outer markers -- i.e. a cell crosses a
    // boundary the rest of the table observes.
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

/// Render one separator segment (between two `+`) of `interior` dashes, using
/// `fill` (`-` or `=`) and applying alignment colons only when the segment sits
/// on the alignment-bearing separator.
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

/// Pad `text` to `interior` display columns under `align`.
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

/// Alignment of each segment (between `+`) of a separator line, read from the
/// leading/trailing `:` of its dash run.
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
    let raw_table = node.text().to_string();
    let mut extra_abbreviations = Vec::new();
    let profile = resolve_profile(node, config, &mut extra_abbreviations);

    // Spanning grid tables -- row spans (a `|` content row carrying a `+`) or
    // column spans (a cell straddling a boundary the rest of the table
    // observes) -- can't be reflowed by the structured path, which assumes a
    // uniform column count and would truncate/pad spanning rows. Lay them out
    // span-aware on the canonical grid instead. See #323 (rowspan) and #359
    // (colspan).
    let is_spanning = raw_table
        .lines()
        .any(|line| line.trim_start().starts_with('|') && line.contains('+'))
        || grid_table_has_column_spans(&raw_table);
    if is_spanning {
        return format_unified_spanning_grid_table(&raw_table, config, profile, indent);
    }

    let mut table_data = extract_grid_table_data(node, config);
    let mut output = String::new();

    // Early return if no rows
    if table_data.rows.is_empty() {
        return node.text().to_string();
    }

    // Reflow plain-prose body cells to their fixed column width and drop blank
    // padding lines, unless wrapping is disabled. Column widths are preserved
    // (pandoc maps them to relative output widths); cells carrying block content
    // or hard line breaks stay verbatim. See `reflow_grid_table_cells`.
    let wrap_mode = config.wrap.clone().unwrap_or(WrapMode::Reflow);
    if wrap_mode != WrapMode::Preserve {
        reflow_grid_table_cells(&mut table_data);
    }

    // Use the source separator widths as a floor: grid column widths are
    // load-bearing (pandoc maps them to relative output widths), so preserve
    // them rather than shrinking to content. Only expand when formatted content
    // genuinely exceeds the source column. This mirrors the spanning-grid path
    // and stays idempotent (after one pass the source width is >= content).
    let mut widths = calculate_grid_column_widths(&table_data.rows);
    for (col_idx, width) in widths.iter_mut().enumerate() {
        *width = (*width).max(table_data.column_widths.get(col_idx).copied().unwrap_or(0));
    }

    // Helper to create separator line
    let make_separator = |fill_char: char, with_alignment_markers: bool| -> String {
        let mut line = String::from("+");

        for (col_idx, width) in widths.iter().enumerate() {
            let alignment = table_data
                .alignments
                .get(col_idx)
                .copied()
                .unwrap_or(Alignment::Default);

            // Create separator with optional alignment markers
            // Per Pandoc spec: alignment colons go in header separator ONLY, not row separators
            let segment = if with_alignment_markers {
                // Header separator: include alignment colons if specified
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
                // Row separator: no alignment colons
                fill_char.to_string().repeat(width + 2)
            };

            line.push_str(&segment);
            line.push('+');
        }

        line.push('\n');
        line
    };

    // Top border
    // Headerless grid tables encode alignment markers in the first separator,
    // so preserve markers there when no explicit header rows are present.
    let has_header_rows = table_data.row_sections.contains(&GridRowSection::Header);
    output.push_str(&make_separator('-', !has_header_rows));

    // Format rows
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

            // Apply alignment using unicode display width
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

        // Insert section-aware separator.
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
    // Grid tables honor the threaded container indent (0 at the top level) so
    // the `+---+` border sits at column 0 -- pandoc rejects an indented border.
    indent_table_block(&output, indent)
}

// Simple Table Formatting
// ============================================================================

/// Column information for simple tables (extracted from separator line)
#[derive(Debug, Clone)]
struct SimpleColumn {
    /// Start position (byte index) in the line
    start: usize,
    /// End position (byte index) in the line
    end: usize,
    /// Column alignment
    alignment: Alignment,
}

/// Extract column positions from a simple table separator line.
/// Returns column boundaries and default alignments.
fn extract_simple_table_columns(separator: &SyntaxNode) -> Vec<SimpleColumn> {
    // One column per dash run, byte offsets relative to the node start (= line
    // start, leading whitespace/prefix included) so they line up with the
    // header line the alignment pass indexes into. End is exclusive.
    let node_start = u32::from(separator.text_range().start());
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
fn determine_simple_alignments(
    columns: &mut [SimpleColumn],
    _separator_line: &str,
    header_line: Option<&str>,
) {
    if let Some(header) = header_line {
        for col in columns.iter_mut() {
            if col.end > header.len() {
                col.alignment = Alignment::Default;
                continue;
            }

            // Extract header text for this column
            let header_text = if col.end <= header.len() {
                header[col.start..col.end].trim()
            } else if col.start < header.len() {
                header[col.start..].trim()
            } else {
                ""
            };

            if header_text.is_empty() {
                col.alignment = Alignment::Default;
                continue;
            }

            // Find where the header text starts and ends within the column
            let header_in_col = &header[col.start..col.end.min(header.len())];
            let text_start = header_in_col.len() - header_in_col.trim_start().len();
            // text_end is the position AFTER the last non-whitespace character
            let trimmed_text = header_in_col.trim();
            let text_end = text_start + trimmed_text.len();

            // Column width is separator length
            let col_width = col.end - col.start;

            let flush_left = text_start == 0;
            let flush_right = text_end == col_width;

            col.alignment = match (flush_left, flush_right) {
                (true, true) => Alignment::Default,
                (true, false) => Alignment::Left,
                (false, true) => Alignment::Right,
                (false, false) => Alignment::Center,
            };
        }
    }
}

/// Split a simple table row into cells using column boundaries
fn split_simple_table_row(row_text: &str, columns: &[SimpleColumn]) -> Vec<String> {
    let mut cells = Vec::new();

    // Strip newline from row
    let row = if let Some(stripped) = row_text.strip_suffix("\r\n") {
        stripped
    } else if let Some(stripped) = row_text.strip_suffix('\n') {
        stripped
    } else {
        row_text
    };

    for (i, col) in columns.iter().enumerate() {
        // A column spans to the start of the next column (the gap belongs to
        // the left column); the last column runs to end-of-line. Ending at the
        // dash-run end instead would truncate cell text wider than its dashes.
        let end = columns
            .get(i + 1)
            .map_or(row.len(), |next| next.start.min(row.len()));
        let cell_text = if col.start < end {
            row[col.start..end].trim()
        } else {
            ""
        };
        cells.push(cell_text.to_string());
    }

    cells
}

/// Extract structured data from simple table AST node
fn extract_simple_table_data(node: &SyntaxNode, config: &Config) -> TableData {
    let mut rows = Vec::new();
    let mut columns: Vec<SimpleColumn> = Vec::new();
    let mut caption = None;
    let mut separator_line = String::new();
    let mut header_line: Option<String> = None;
    let mut header_cells: Option<Vec<String>> = None;
    // Raw text of the first non-separator data row. Headerless simple tables
    // carry no header, so pandoc derives column alignment from this row's
    // position relative to the dash runs (see `determine_simple_alignments`).
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
                separator_line = child.text().to_string();

                // Extract column positions
                columns = extract_simple_table_columns(&child);
            }
            SyntaxKind::TABLE_HEADER => {
                // Always preserve RAW text for alignment detection
                let raw_text = child.text().to_string();
                header_line = Some(raw_text);

                // Try to extract from TABLE_CELL nodes for content
                let cells = extract_row_cells(&child, config);
                if !cells.is_empty() {
                    header_cells = Some(cells);
                } else {
                    header_cells = None;
                }
            }
            SyntaxKind::TABLE_ROW => {
                // Data rows come after separator
                if !columns.is_empty() {
                    // Remember the first real data row's raw text for headerless
                    // alignment detection. Skip the closing dash separator that a
                    // headerless table emits as an all-dashes TABLE_ROW.
                    let raw_row = child.text().to_string();
                    let row_is_separator = raw_row
                        .trim()
                        .chars()
                        .all(|c| c == '-' || c.is_whitespace());
                    if !row_is_separator && first_data_row_line.is_none() {
                        first_data_row_line = Some(raw_row);
                    }

                    // Try to extract from TABLE_CELL nodes first
                    let cells = extract_row_cells(&child, config);

                    if !cells.is_empty() {
                        // Check if this is actually a separator line (all cells are dashes/whitespace)
                        let is_separator = cells
                            .iter()
                            .all(|cell| cell.trim().chars().all(|c| c == '-'));

                        if !is_separator {
                            // Successfully extracted from TABLE_CELL nodes
                            rows.push(cells);
                        }
                    } else {
                        // Fall back to old approach (for backwards compatibility)
                        let row_content = format_cell_content(&child, config);

                        // Skip rows that are actually separator lines (for headerless tables)
                        let is_separator = row_content
                            .trim()
                            .chars()
                            .all(|c| c == '-' || c.is_whitespace());

                        if !is_separator {
                            let cells = split_simple_table_row(&row_content, &columns);
                            rows.push(cells);
                        }
                    }
                }
            }
            _ => {}
        }
    }

    // Determine alignments from the header if present, else (headerless) from
    // the first data row — pandoc reads alignment off whichever line sits
    // against the dash runs.
    if !columns.is_empty() {
        let alignment_line = header_line.as_deref().or(first_data_row_line.as_deref());
        determine_simple_alignments(&mut columns, &separator_line, alignment_line);
    }

    // Track if we have a header before potentially consuming header_line
    let has_header = header_line.is_some() || header_cells.is_some();

    // Add header row to rows if present
    if let Some(cells) = header_cells {
        // Already extracted from TABLE_CELL nodes
        rows.insert(0, cells);
    } else if let Some(header) = header_line {
        // Fall back to old text splitting approach
        let header_cells = split_simple_table_row(&header, &columns);
        rows.insert(0, header_cells);
    }

    let alignments = columns.iter().map(|c| c.alignment).collect();

    // Output geometry is recomputed from content in `format_simple_table`
    // (pandoc-style), so we don't carry separator-derived widths/positions.
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
pub fn format_simple_table(node: &SyntaxNode, config: &Config) -> String {
    if !node.text().to_string().is_ascii() {
        return node.text().to_string();
    }

    let table_data = extract_simple_table_data(node, config);

    // Early return if no rows
    if table_data.rows.is_empty() {
        return node.text().to_string();
    }

    let has_header = table_data.has_header;
    let alignments = &table_data.alignments;

    // True per-column content widths (no minimum), then pandoc's `+2` padding.
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
    } else {
        // Headerless simple tables are delimited by a separator above and below.
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
    indent_table_block(&output, config.table_indent)
}

/// Extract column information from a multiline table separator. One
/// `(start, end)` per dash run, byte offsets relative to the node start
/// (leading whitespace preserved, as the old line-relative offsets were),
/// end exclusive.
fn extract_multiline_columns(separator: &SyntaxNode) -> Vec<(usize, usize)> {
    let node_start = u32::from(separator.text_range().start());
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

/// Derive slice/render geometry from raw `(start, dash_end)` dash runs.
fn multiline_columns(raw: &[(usize, usize)]) -> MultilineColumns {
    let n = raw.len();
    let mut slice = Vec::with_capacity(n);
    let mut render = Vec::with_capacity(n);
    for (i, &(start, dash_end)) in raw.iter().enumerate() {
        if i + 1 < n {
            let next = raw[i + 1].0;
            slice.push((start, next));
            // Reserve a single-space gutter before the next column's start.
            let width = next.saturating_sub(start).saturating_sub(1);
            render.push((start, start + width));
        } else {
            // Last column: slice to end-of-line; render keeps its dash run
            // (callers widen this to fit content if needed).
            slice.push((start, usize::MAX));
            render.push((start, dash_end));
        }
    }
    MultilineColumns { slice, render }
}

/// Determine alignment for a column based on header text position
fn determine_multiline_alignment(header_text: &str, col_start: usize, col_end: usize) -> Alignment {
    if header_text.is_empty() {
        return Alignment::Default;
    }

    // Use first non-empty line of header to determine alignment
    let first_line = header_text
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("");

    // Extract text within this column using original line (not normalized)
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

/// Represents a multiline table with cells that can span multiple lines
struct MultilineTableData {
    /// Rows of cells, where each cell is a vector of lines
    rows: Vec<Vec<Vec<String>>>,
    alignments: Vec<Alignment>,
    caption: Option<String>,
    column_positions: Vec<(usize, usize)>,
    has_header: bool,
}

/// Extract multiline cell content from a text block  
fn extract_multiline_cells(text: &str, column_positions: &[(usize, usize)]) -> Vec<Vec<String>> {
    let lines: Vec<&str> = text.lines().collect();
    let num_cols = column_positions.len();

    // Initialize cells - each cell is a vec of lines
    let mut cells: Vec<Vec<String>> = vec![Vec::new(); num_cols];

    for line in lines {
        // Keep line as-is without normalization - column positions should work on original text
        for (col_idx, &(col_start, col_end)) in column_positions.iter().enumerate() {
            let cell_line = if col_end <= line.len() {
                &line[col_start..col_end]
            } else if col_start < line.len() {
                &line[col_start..]
            } else {
                ""
            };
            // Trim the cell line to normalize spacing - this ensures idempotency
            // We trim both leading and trailing whitespace because alignment will be
            // recalculated based on column positions
            cells[col_idx].push(cell_line.trim().to_string());
        }
    }

    cells
}

/// Extract cells from TABLE_CELL nodes and continuation TEXT (Phase 7.1)
fn extract_cells_from_table_cell_nodes(
    row: &SyntaxNode,
    config: &Config,
    column_positions: &[(usize, usize)],
) -> Vec<Vec<String>> {
    // Format TABLE_CELL inline content, then extract multi-line text
    let mut formatted_text = String::new();

    for child in row.children_with_tokens() {
        match child {
            rowan::NodeOrToken::Token(token) => {
                formatted_text.push_str(token.text());
            }
            rowan::NodeOrToken::Node(node) => {
                if node.kind() == SyntaxKind::TABLE_CELL {
                    // Format the inline content within the cell
                    formatted_text.push_str(&format_cell_content(&node, config));
                } else {
                    // Other nodes (shouldn't happen in well-formed CST)
                    formatted_text.push_str(&node.text().to_string());
                }
            }
        }
    }

    extract_multiline_cells(&formatted_text, column_positions)
}

/// Extract structured data from multiline table AST node
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

                // For headerless tables: first separator defines columns
                // For tables with headers: second separator (after header) defines columns
                // We extract from first separator and will overwrite if we see a second one
                if separator_count == 1 || (separator_count == 2 && has_header) {
                    raw_columns = extract_multiline_columns(&child);
                }
            }
            SyntaxKind::TABLE_HEADER => {
                has_header = true;
                // Always use raw text for alignment detection - it preserves original spacing
                header_text = child.text().to_string();
            }
            SyntaxKind::TABLE_ROW => {
                // Slice cells on the pandoc column spans (gap text belongs to
                // the left column) so wide cells are never truncated.
                let slice = multiline_columns(&raw_columns).slice;
                // Check if row has TABLE_CELL nodes (Phase 7.1)
                if child.children().any(|c| c.kind() == SyntaxKind::TABLE_CELL) {
                    let cells = extract_cells_from_table_cell_nodes(&child, config, &slice);
                    rows.push(cells);
                } else {
                    // Old style: format cell content and split into cells
                    let row_content = format_cell_content(&child, config);
                    let cells = extract_multiline_cells(&row_content, &slice);
                    rows.push(cells);
                }
            }
            _ => {}
        }
    }

    let slice = multiline_columns(&raw_columns).slice;

    // Add header as first row if present
    if has_header && !raw_columns.is_empty() {
        let header_node = node
            .children()
            .find(|c| c.kind() == SyntaxKind::TABLE_HEADER);

        let header_cells = if let Some(hdr) = header_node {
            if hdr.children().any(|c| c.kind() == SyntaxKind::TABLE_CELL) {
                // New style: extract from TABLE_CELL nodes + continuation text
                extract_cells_from_table_cell_nodes(&hdr, config, &slice)
            } else {
                // Old style: extract from text
                extract_multiline_cells(&header_text, &slice)
            }
        } else {
            extract_multiline_cells(&header_text, &slice)
        };

        rows.insert(0, header_cells);
    }

    // Render geometry: keep column starts fixed, widen the last column to fit
    // its content (its dash run may be shorter than the text it holds).
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

    // Determine alignments from header, else from the first body row.
    if has_header && !column_positions.is_empty() {
        for &(col_start, col_end) in &column_positions {
            alignments.push(determine_multiline_alignment(
                &header_text,
                col_start,
                col_end,
            ));
        }
    } else if !rows.is_empty() && !column_positions.is_empty() {
        // No header - determine alignment from first body row (per Pandoc spec)
        let first_row_node = node
            .children()
            .find(|c| c.kind() == SyntaxKind::TABLE_ROW)
            .unwrap();
        // Use raw text to preserve original spacing for alignment detection
        let first_row_text = first_row_node.text().to_string();
        for &(col_start, col_end) in &column_positions {
            alignments.push(determine_multiline_alignment(
                &first_row_text,
                col_start,
                col_end,
            ));
        }
    } else {
        // Fallback - use default alignment
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
pub fn format_multiline_table(node: &SyntaxNode, config: &Config) -> String {
    if !node.text().to_string().is_ascii() {
        return node.text().to_string();
    }

    let mut table_data = extract_multiline_table_data(node, config);
    let mut output = String::new();

    // Early return if no rows or no column info
    if table_data.rows.is_empty() || table_data.column_positions.is_empty() {
        return node.text().to_string();
    }

    // Reflow each body cell to its (fixed) column width unless wrapping is
    // disabled. Column widths are preserved; we only re-pack the cell text to
    // use the existing width more tightly and drop blank padding lines.
    //
    // The header row is intentionally left untouched: column alignment is
    // detected from the header's text geometry, and packing a header so it
    // fills the column would erase its leading pad and flip a centered column to
    // left on the next pass (breaking idempotency). Headers are short anyway.
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

    // Calculate total table width
    let last_col_end = positions.last().map(|(_, end)| *end).unwrap_or(0);

    // Emit opening separator
    if table_data.has_header {
        // With header: opening separator is full-width dashes
        output.push_str(&"-".repeat(last_col_end));
        output.push('\n');
    } else {
        // Headerless: opening separator shows column boundaries
        let mut sep_chars: Vec<char> = vec![' '; last_col_end];
        for &(col_start, col_end) in &positions {
            for item in sep_chars.iter_mut().take(col_end).skip(col_start) {
                *item = '-';
            }
        }
        output.push_str(&sep_chars.iter().collect::<String>());
        output.push('\n');
    }

    // Emit header if present
    if table_data.has_header && !table_data.rows.is_empty() {
        let header_row = &table_data.rows[0];

        // Determine max number of lines across all header cells
        let max_lines = header_row.iter().map(|cell| cell.len()).max().unwrap_or(0);

        // Emit each line of the header
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

                    // Calculate text start position based on alignment
                    let text_start_in_col = match alignment {
                        Alignment::Left | Alignment::Default => 0,
                        Alignment::Right => total_padding,
                        Alignment::Center => total_padding / 2,
                    };

                    // Place characters
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

        // Emit column separator (no indent)
        let mut sep_chars: Vec<char> = vec![' '; last_col_end];
        for &(col_start, col_end) in &positions {
            for item in sep_chars.iter_mut().take(col_end).skip(col_start) {
                *item = '-';
            }
        }
        output.push_str(&sep_chars.iter().collect::<String>());
        output.push('\n');
    }

    // Emit body rows
    let start_row = if table_data.has_header { 1 } else { 0 };
    for (row_idx, row) in table_data.rows.iter().enumerate().skip(start_row) {
        // Determine max number of lines across all cells in this row
        let max_lines = row.iter().map(|cell| cell.len()).max().unwrap_or(0);

        // Emit each line of the row
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

                    // Calculate text start position based on alignment
                    let text_start_in_col = match alignment {
                        Alignment::Left | Alignment::Default => 0,
                        Alignment::Right => total_padding,
                        Alignment::Center => total_padding / 2,
                    };

                    // Place characters
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

        // Emit blank line between rows
        if row_idx < table_data.rows.len() - 1 {
            output.push('\n');
        }
    }

    // Emit closing separator
    if table_data.has_header {
        // With header: closing separator is full-width dashes
        output.push_str(&"-".repeat(last_col_end));
        output.push('\n');
    } else {
        // Headerless: closing separator shows column boundaries
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
    indent_table_block(&output, config.table_indent)
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
        // Trailing backslash is a pandoc hard line break.
        assert!(!grid_cell_is_reflowable(&lines("Population\\\n(in 2018)")));
    }

    #[test]
    fn empty_or_blank_only_cells_are_not_reflowable() {
        assert!(!grid_cell_is_reflowable(&[]));
        assert!(!grid_cell_is_reflowable(&lines("\n   \n")));
    }

    #[test]
    fn reflow_packs_prose_and_drops_trailing_blank() {
        // "Lorem ipsum dolor sit" packed into width 18.
        let out = reflow_or_trim_grid_cell(&lines("Lorem ipsum\ndolor sit\n"), 18);
        assert_eq!(out, vec!["Lorem ipsum dolor", "sit"]);
    }

    #[test]
    fn trim_only_keeps_block_content_but_drops_blank_edges() {
        let out = reflow_or_trim_grid_cell(&lines("\n- item one\n- item two\n"), 18);
        assert_eq!(out, vec!["- item one", "- item two"]);
    }
}
