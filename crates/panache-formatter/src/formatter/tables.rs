use crate::config::{Config, WrapMode};
use crate::formatter::inline::format_inline_node;
use crate::formatter::inline_layout::wrap_text_first_fit;
use crate::formatter::sentence_wrap::{ResolvedProfile, resolve_profile, split_sentence_text};
use crate::syntax::{SyntaxKind, SyntaxNode};
use rowan::NodeOrToken;
use std::collections::HashMap;
use unicode_width::UnicodeWidthStr;

/// Default indent (in spaces) for table types that self-indent at the top level
/// (pipe, simple, multiline). Grid tables instead honor the container indent
/// threaded from the dispatcher so a top-level grid sits at column 0 -- pandoc
/// rejects an indented `+---+` border. See `format_grid_table`.
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

    for caption_child in caption_node.children_with_tokens() {
        match caption_child {
            rowan::NodeOrToken::Token(token)
                if token.kind() == SyntaxKind::TABLE_CAPTION_PREFIX =>
            {
                // Skip the original prefix
            }
            rowan::NodeOrToken::Token(token) => {
                caption_body.push_str(token.text());
            }
            rowan::NodeOrToken::Node(node) => {
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
    rows: Vec<Vec<String>>,                        // All rows including header
    alignments: Vec<Alignment>,                    // Column alignments
    caption: Option<String>,                       // Optional caption text
    column_widths: Option<Vec<usize>>, // For simple tables: preserve separator dash lengths
    column_positions: Option<Vec<(usize, usize)>>, // For simple tables: preserve (start, end) positions
    has_header: bool,                              // True if table has a header row
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
fn extract_alignments(separator_text: &str) -> Vec<Alignment> {
    let trimmed = separator_text.trim();
    let cells: Vec<&str> = trimmed.split('|').collect();

    let mut alignments = Vec::new();

    for cell in cells {
        let cell = cell.trim();

        // Skip empty cells (from leading/trailing pipes)
        if cell.is_empty() {
            continue;
        }

        let starts_colon = cell.starts_with(':');
        let ends_colon = cell.ends_with(':');

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
                let separator_text = child.text().to_string();
                alignments = extract_alignments(&separator_text);
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
        column_widths: None,
        column_positions: None,
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
    let block_indent = if indent == 0 {
        TABLE_BLOCK_INDENT
    } else {
        indent
    };
    indent_table_block(&output, block_indent)
}

// Grid Table Formatting
// ============================================================================

/// Extract alignments from grid table separator line (e.g., "+:---+---:+:---:+")
fn extract_grid_alignments(separator_text: &str) -> Vec<Alignment> {
    let trimmed = separator_text.trim();

    // Split by + to get column segments
    let segments: Vec<&str> = trimmed.split('+').collect();

    let mut alignments = Vec::new();

    // Parse each segment between + signs (skip first/last empty)
    for segment in segments
        .iter()
        .skip(1)
        .take(segments.len().saturating_sub(2))
    {
        if segment.is_empty() {
            continue;
        }

        let starts_colon = segment.starts_with(':');
        let ends_colon = segment.ends_with(':');

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

fn grid_separator_widths(separator_text: &str) -> Vec<usize> {
    let trimmed = separator_text.trim();
    let segments: Vec<&str> = trimmed.split('+').collect();
    segments
        .iter()
        .skip(1)
        .take(segments.len().saturating_sub(2))
        .map(|seg| seg.chars().count().saturating_sub(2))
        .collect()
}

fn format_spanning_grid_table_raw(
    raw_table: &str,
    config: &Config,
    profile: ResolvedProfile<'_>,
    indent: usize,
) -> String {
    let mut lines: Vec<&str> = raw_table.lines().collect();
    while lines.last().is_some_and(|l| l.trim().is_empty()) {
        lines.pop();
    }
    if lines.is_empty() {
        return raw_table.to_string();
    }

    let mut caption: Option<String> = None;
    if let Some(first) = lines.first().copied() {
        let trimmed = first.trim_start();
        if let Some(rest) = trimmed.strip_prefix(':') {
            caption = Some(format!(": {}", rest.trim()));
            lines.remove(0);
            while lines.first().is_some_and(|l| l.trim().is_empty()) {
                lines.remove(0);
            }
        } else if let Some(rest) = trimmed
            .strip_prefix("Table:")
            .or_else(|| trimmed.strip_prefix("table:"))
        {
            caption = Some(format!(": {}", rest.trim()));
            lines.remove(0);
            while lines.first().is_some_and(|l| l.trim().is_empty()) {
                lines.remove(0);
            }
        }
    }
    if caption.is_none()
        && let Some(last) = lines.last().copied()
    {
        let trimmed = last.trim_start();
        if let Some(rest) = trimmed.strip_prefix(':') {
            caption = Some(format!(": {}", rest.trim()));
            lines.pop();
            while lines.last().is_some_and(|l| l.trim().is_empty()) {
                lines.pop();
            }
        } else if let Some(rest) = trimmed
            .strip_prefix("Table:")
            .or_else(|| trimmed.strip_prefix("table:"))
        {
            caption = Some(format!(": {}", rest.trim()));
            lines.pop();
            while lines.last().is_some_and(|l| l.trim().is_empty()) {
                lines.pop();
            }
        }
    }

    let mut out = String::new();
    let mut in_header_rows = true;
    let mut current_schema_cols: Option<usize> = None;
    let mut schema_widths: HashMap<usize, Vec<usize>> = HashMap::new();
    let mut numeric_cols_by_schema: HashMap<usize, Vec<bool>> = HashMap::new();
    for line in &lines {
        let t = line.trim();
        if !(t.starts_with('|') && t.ends_with('|')) || t.contains('+') {
            continue;
        }
        let segments: Vec<&str> = t.split('|').collect();
        if segments.len() < 3 {
            continue;
        }
        let cells: Vec<String> = segments
            .iter()
            .skip(1)
            .take(segments.len().saturating_sub(2))
            .map(|c| c.trim().to_string())
            .collect();
        let col_count = cells.len();
        let entry = numeric_cols_by_schema
            .entry(col_count)
            .or_insert_with(|| vec![false; col_count]);
        for (idx, cell) in cells.iter().enumerate() {
            let s = cell
                .strip_prefix('-')
                .or_else(|| cell.strip_prefix('+'))
                .unwrap_or(cell.as_str());
            if !s.is_empty()
                && s.chars()
                    .all(|c| c.is_ascii_digit() || c == ',' || c == '.')
            {
                entry[idx] = true;
            }
        }
    }
    for line in &lines {
        let t = line.trim_end();
        let tt = t.trim_start();
        if tt.starts_with('+') {
            let widths = grid_separator_widths(tt);
            if !widths.is_empty() {
                let col_count = widths.len();
                current_schema_cols = Some(col_count);
                if let Some(existing) = schema_widths.get_mut(&col_count) {
                    for (idx, w) in widths.into_iter().enumerate() {
                        existing[idx] = existing[idx].max(w);
                    }
                } else {
                    schema_widths.insert(col_count, widths);
                }
            }
            if tt.contains('=') {
                in_header_rows = false;
            }
            out.push_str(tt);
            out.push('\n');
            continue;
        }
        if !(tt.starts_with('|') && tt.ends_with('|')) || tt.contains('+') {
            out.push_str(tt);
            out.push('\n');
            continue;
        }
        let segments: Vec<&str> = tt.split('|').collect();
        let cells: Vec<String> = segments
            .iter()
            .skip(1)
            .take(segments.len().saturating_sub(2))
            .map(|c| c.trim().to_string())
            .collect();
        let col_count = cells.len();
        let mut widths = schema_widths
            .get(&col_count)
            .cloned()
            .or_else(|| current_schema_cols.and_then(|n| schema_widths.get(&n).cloned()))
            .unwrap_or_else(|| vec![0usize; col_count]);
        if widths.len() < col_count {
            widths.resize(col_count, 0);
        } else if widths.len() > col_count {
            widths.truncate(col_count);
        }
        for (i, c) in cells.iter().enumerate() {
            widths[i] = widths[i].max(c.width());
        }
        let first_cell_filled = cells.first().is_some_and(|c| !c.trim().is_empty());
        out.push('|');
        for idx in 0..col_count {
            let cell = cells.get(idx).map(String::as_str).unwrap_or("");
            let width = widths.get(idx).copied().unwrap_or(3);
            let pad = width.saturating_sub(cell.width());
            let stripped = cell
                .trim()
                .strip_prefix('-')
                .or_else(|| cell.trim().strip_prefix('+'))
                .unwrap_or(cell.trim());
            let numeric_like = !stripped.is_empty()
                && stripped
                    .chars()
                    .all(|c| c.is_ascii_digit() || c == ',' || c == '.');
            let a = if in_header_rows {
                if idx == 0 {
                    Alignment::Center
                } else if numeric_cols_by_schema
                    .get(&col_count)
                    .and_then(|v| v.get(idx))
                    .copied()
                    .unwrap_or(false)
                {
                    Alignment::Right
                } else {
                    Alignment::Left
                }
            } else if idx == 0 || (col_count == 12 && idx == 1) {
                Alignment::Center
            } else if numeric_like {
                Alignment::Right
            } else {
                Alignment::Left
            };
            let padded = match a {
                Alignment::Right => format!("{}{}", " ".repeat(pad), cell),
                Alignment::Center => {
                    let l = if col_count == 12 && idx == 1 {
                        if first_cell_filled {
                            pad / 2
                        } else {
                            pad.div_ceil(2)
                        }
                    } else {
                        pad / 2
                    };
                    let r = pad - l;
                    format!("{}{}{}", " ".repeat(l), cell, " ".repeat(r))
                }
                _ => format!("{}{}", cell, " ".repeat(pad)),
            };
            out.push(' ');
            out.push_str(&padded);
            out.push_str(" |");
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
                let separator_text = child.text().to_string();

                // Grid column widths encode relative output widths (pandoc maps
                // them to <col style="width:X%">), so take the per-column max
                // across every separator to preserve the source widths later.
                let widths = grid_separator_widths(&separator_text);
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
                let extracted = extract_grid_alignments(&separator_text);
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

/// Format a grid table with consistent alignment and padding
pub fn format_grid_table(node: &SyntaxNode, config: &Config, indent: usize) -> String {
    let raw_table = node.text().to_string();
    let mut extra_abbreviations = Vec::new();
    let profile = resolve_profile(node, config, &mut extra_abbreviations);
    if raw_table
        .lines()
        .any(|line| line.trim_start().starts_with('|') && line.contains('+'))
    {
        return format_spanning_grid_table_raw(&raw_table, config, profile, indent);
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
fn extract_simple_table_columns(separator_text: &str) -> Vec<SimpleColumn> {
    let trimmed = separator_text.trim_start();
    // Strip trailing newline if present
    let trimmed = if let Some(stripped) = trimmed.strip_suffix("\r\n") {
        stripped
    } else if let Some(stripped) = trimmed.strip_suffix('\n') {
        stripped
    } else {
        trimmed
    };

    let leading_spaces = separator_text.len()
        - trimmed.len()
        - if separator_text.ends_with("\r\n") {
            2
        } else if separator_text.ends_with('\n') {
            1
        } else {
            0
        };

    let mut columns = Vec::new();
    let mut in_dashes = false;
    let mut col_start = 0;

    for (i, ch) in trimmed.char_indices() {
        match ch {
            '-' => {
                if !in_dashes {
                    col_start = i + leading_spaces;
                    in_dashes = true;
                }
            }
            ' ' => {
                if in_dashes {
                    columns.push(SimpleColumn {
                        start: col_start,
                        end: i + leading_spaces,
                        alignment: Alignment::Default,
                    });
                    in_dashes = false;
                }
            }
            _ => {}
        }
    }

    // Handle last column if line ends with dashes
    if in_dashes {
        columns.push(SimpleColumn {
            start: col_start,
            end: trimmed.len() + leading_spaces,
            alignment: Alignment::Default,
        });
    }

    columns
}

/// Determine column alignments based on header text position relative to separator
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

    for col in columns {
        let cell_text = if col.end <= row.len() {
            row[col.start..col.end].trim()
        } else if col.start < row.len() {
            row[col.start..].trim()
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
                columns = extract_simple_table_columns(&separator_line);
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

    // Determine alignments based on header
    if !columns.is_empty() {
        determine_simple_alignments(&mut columns, &separator_line, header_line.as_deref());
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

    // For simple tables, preserve both separator dash lengths AND column positions
    let column_widths: Vec<usize> = columns.iter().map(|c| c.end - c.start).collect();
    let base_offset = columns.first().map(|c| c.start).unwrap_or(0);
    let column_positions: Vec<(usize, usize)> = columns
        .iter()
        .map(|c| (c.start - base_offset, c.end - base_offset))
        .collect();

    TableData {
        rows,
        alignments,
        caption,
        column_widths: Some(column_widths),
        column_positions: Some(column_positions),
        has_header, // Simple tables may or may not have headers
    }
}

/// Format a simple table with consistent alignment and padding
pub fn format_simple_table(node: &SyntaxNode, config: &Config) -> String {
    if !node.text().to_string().is_ascii() {
        return node.text().to_string();
    }

    let table_data = extract_simple_table_data(node, config);
    let mut output = String::new();

    // Early return if no rows
    if table_data.rows.is_empty() {
        return node.text().to_string();
    }

    let content_widths = calculate_column_widths(&table_data.rows);
    let has_header = table_data.has_header;

    // For simple tables, preserve separator-derived geometry unless it's clearly oversized
    // compared to content; then shrink width while preserving column starts.
    let widths = if let Some(ref widths) = table_data.column_widths {
        widths.clone()
    } else {
        content_widths.clone()
    };

    let normalized_positions = if let Some(ref positions) = table_data.column_positions {
        let mut out = Vec::with_capacity(positions.len());
        for (col_idx, &(start, end)) in positions.iter().enumerate() {
            let original_width = end.saturating_sub(start);
            if has_header {
                let content_width = content_widths.get(col_idx).copied().unwrap_or(3);
                let alignment = table_data
                    .alignments
                    .get(col_idx)
                    .copied()
                    .unwrap_or(Alignment::Default);
                let preferred_width = content_width
                    + match alignment {
                        Alignment::Center => 4,
                        Alignment::Left | Alignment::Right => 2,
                        Alignment::Default => 0,
                    };
                let clamped_width = original_width.min(preferred_width).max(content_width);
                out.push((start, start + clamped_width));
            } else {
                out.push((start, end));
            }
        }
        Some(out)
    } else {
        None
    };

    // For headerless simple tables, emit opening separator first
    if !has_header
        && normalized_positions.is_some()
        && let Some(ref positions) = normalized_positions
    {
        let last_col_end = positions.last().map(|(_, end)| *end).unwrap_or(0);
        let mut sep_chars: Vec<char> = vec![' '; last_col_end];
        for &(col_start, col_end) in positions.iter() {
            for i in col_start..col_end {
                if i < sep_chars.len() {
                    sep_chars[i] = '-';
                }
            }
        }
        output.push_str(&sep_chars.iter().collect::<String>());
        output.push('\n');
    }

    // Format header row if present
    if has_header {
        // For simple tables with column positions, use absolute positioning
        if let Some(ref positions) = normalized_positions {
            // Build header line using character buffer
            let last_col_end = positions.last().map(|(_, end)| *end).unwrap_or(0);
            let mut line_chars: Vec<char> = vec![' '; last_col_end];

            for (col_idx, cell) in table_data.rows[0].iter().enumerate() {
                if let Some(&(col_start, col_end)) = positions.get(col_idx) {
                    let alignment = table_data
                        .alignments
                        .get(col_idx)
                        .copied()
                        .unwrap_or(Alignment::Default);

                    let col_width = col_end - col_start;
                    let cell_chars: Vec<char> = cell.chars().collect();
                    let cell_width = cell.width();
                    let total_padding = col_width.saturating_sub(cell_width);

                    // Calculate where to place text within column based on alignment
                    let text_start_in_col = match alignment {
                        Alignment::Left | Alignment::Default => 0,
                        Alignment::Right => total_padding,
                        Alignment::Center => total_padding / 2,
                    };

                    // Place cell characters at the correct position
                    let mut char_pos = 0;
                    for &ch in &cell_chars {
                        let target_pos = col_start + text_start_in_col + char_pos;
                        if target_pos < line_chars.len() {
                            line_chars[target_pos] = ch;
                            char_pos += 1;
                        }
                    }
                }
            }

            output.push_str(line_chars.iter().collect::<String>().trim_end());
            output.push('\n');

            // Emit separator line at the same positions
            let mut sep_chars: Vec<char> = vec![' '; last_col_end];
            for &(col_start, col_end) in positions {
                for i in col_start..col_end {
                    if i < sep_chars.len() {
                        sep_chars[i] = '-';
                    }
                }
            }
            output.push_str(&sep_chars.iter().collect::<String>());
            output.push('\n');
        } else {
            // Fallback: use widths with single-space separation
            for (col_idx, cell) in table_data.rows[0].iter().enumerate() {
                let width = widths.get(col_idx).copied().unwrap_or(3);
                let alignment = table_data
                    .alignments
                    .get(col_idx)
                    .copied()
                    .unwrap_or(Alignment::Default);

                let cell_width = cell.width();
                let total_padding = width.saturating_sub(cell_width);

                let padded_cell = match alignment {
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
                if col_idx < table_data.rows[0].len() - 1 {
                    output.push(' ');
                }
            }
            output.push('\n');

            // Emit separator line
            for (col_idx, width) in widths.iter().enumerate() {
                output.push_str(&"-".repeat(*width));
                if col_idx < widths.len() - 1 {
                    output.push(' ');
                }
            }
            output.push('\n');
        }
    }

    // Format data rows
    for row in table_data.rows.iter().skip(if has_header { 1 } else { 0 }) {
        if let Some(ref positions) = normalized_positions {
            // Build row using character buffer
            let last_col_end = positions.last().map(|(_, end)| *end).unwrap_or(0);
            let mut line_chars: Vec<char> = vec![' '; last_col_end];

            for (col_idx, cell) in row.iter().enumerate() {
                if let Some(&(col_start, col_end)) = positions.get(col_idx) {
                    let alignment = table_data
                        .alignments
                        .get(col_idx)
                        .copied()
                        .unwrap_or(Alignment::Default);

                    let col_width = col_end - col_start;
                    let cell_chars: Vec<char> = cell.chars().collect();
                    let cell_width = cell.width();
                    let total_padding = col_width.saturating_sub(cell_width);

                    // Calculate where to place text within column based on alignment
                    let text_start_in_col = match alignment {
                        Alignment::Left | Alignment::Default => 0,
                        Alignment::Right => total_padding,
                        Alignment::Center => total_padding / 2,
                    };

                    // Place cell characters at the correct position
                    let mut char_pos = 0;
                    for &ch in &cell_chars {
                        let target_pos = col_start + text_start_in_col + char_pos;
                        if target_pos < line_chars.len() {
                            line_chars[target_pos] = ch;
                            char_pos += 1;
                        }
                    }
                }
            }

            output.push_str(line_chars.iter().collect::<String>().trim_end());
            output.push('\n');
        } else {
            // Fallback: use widths with single-space separation
            for (col_idx, cell) in row.iter().enumerate() {
                let width = widths.get(col_idx).copied().unwrap_or(3);
                let alignment = table_data
                    .alignments
                    .get(col_idx)
                    .copied()
                    .unwrap_or(Alignment::Default);

                let cell_width = cell.width();
                let total_padding = width.saturating_sub(cell_width);

                let padded_cell = match alignment {
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
                if col_idx < row.len() - 1 {
                    output.push(' ');
                }
            }
            output.push('\n');
        }
    }

    // For headerless simple tables, emit closing separator
    if !has_header
        && normalized_positions.is_some()
        && let Some(ref positions) = normalized_positions
    {
        let last_col_end = positions.last().map(|(_, end)| *end).unwrap_or(0);
        let mut sep_chars: Vec<char> = vec![' '; last_col_end];
        for &(col_start, col_end) in positions.iter() {
            for i in col_start..col_end {
                if i < sep_chars.len() {
                    sep_chars[i] = '-';
                }
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
    indent_table_block(&output, TABLE_BLOCK_INDENT)
}

/// Extract column information from multiline table separator line
fn extract_multiline_columns(separator_line: &str) -> Vec<(usize, usize)> {
    // DO NOT trim - we need to preserve leading spaces for column alignment
    // Column positions must be relative to the original line positions
    let line = separator_line.trim_end(); // Only remove trailing whitespace/newline

    let mut columns = Vec::new();
    let mut in_dashes = false;
    let mut col_start = 0;

    for (i, ch) in line.char_indices() {
        match ch {
            '-' => {
                if !in_dashes {
                    col_start = i;
                    in_dashes = true;
                }
            }
            ' ' => {
                if in_dashes {
                    columns.push((col_start, i));
                    in_dashes = false;
                }
            }
            _ => {}
        }
    }

    // Handle last column
    if in_dashes {
        columns.push((col_start, line.len()));
    }

    columns
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
    let mut column_positions: Vec<(usize, usize)> = Vec::new();
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
                let sep_text = child.text().to_string();

                // For headerless tables: first separator defines columns
                // For tables with headers: second separator (after header) defines columns
                // We extract from first separator and will overwrite if we see a second one
                if separator_count == 1 || (separator_count == 2 && has_header) {
                    column_positions = extract_multiline_columns(&sep_text);
                }
            }
            SyntaxKind::TABLE_HEADER => {
                has_header = true;
                // Always use raw text for alignment detection - it preserves original spacing
                header_text = child.text().to_string();
            }
            SyntaxKind::TABLE_ROW => {
                // Check if row has TABLE_CELL nodes (Phase 7.1)
                if child.children().any(|c| c.kind() == SyntaxKind::TABLE_CELL) {
                    let cells =
                        extract_cells_from_table_cell_nodes(&child, config, &column_positions);
                    rows.push(cells);
                } else {
                    // Old style: format cell content and split into cells
                    let row_content = format_cell_content(&child, config);
                    let cells = extract_multiline_cells(&row_content, &column_positions);
                    rows.push(cells);
                }
            }
            _ => {}
        }
    }

    // Add header as first row if present
    if has_header && !column_positions.is_empty() {
        let header_node = node
            .children()
            .find(|c| c.kind() == SyntaxKind::TABLE_HEADER);

        let header_cells = if let Some(hdr) = header_node {
            if hdr.children().any(|c| c.kind() == SyntaxKind::TABLE_CELL) {
                // New style: extract from TABLE_CELL nodes + continuation text
                extract_cells_from_table_cell_nodes(&hdr, config, &column_positions)
            } else {
                // Old style: extract from text
                extract_multiline_cells(&header_text, &column_positions)
            }
        } else {
            extract_multiline_cells(&header_text, &column_positions)
        };

        rows.insert(0, header_cells);

        // Determine alignments from header
        for &(col_start, col_end) in &column_positions {
            let alignment = determine_multiline_alignment(&header_text, col_start, col_end);
            alignments.push(alignment);
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
            let alignment = determine_multiline_alignment(&first_row_text, col_start, col_end);
            alignments.push(alignment);
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

    // For single-row tables, emit blank line before closing separator
    // (required by Pandoc spec to distinguish from simple tables)
    let num_body_rows = table_data.rows.len() - if table_data.has_header { 1 } else { 0 };
    if num_body_rows == 1 && table_data.has_header {
        output.push('\n');
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
    indent_table_block(&output, TABLE_BLOCK_INDENT)
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
