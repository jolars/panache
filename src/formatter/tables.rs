use crate::config::Config;
use crate::formatter::inline::format_inline_node;
use crate::syntax::{SyntaxKind, SyntaxNode};
use rowan::NodeOrToken;
use unicode_width::UnicodeWidthStr;

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
    caption_after: bool,                           // True if caption comes after table
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
                if token.kind() == SyntaxKind::TEXT || token.kind() == SyntaxKind::NEWLINE {
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
    let mut caption_after = false;
    let mut seen_separator = false;

    for child in node.children() {
        match child.kind() {
            SyntaxKind::TABLE_CAPTION => {
                // Build normalized caption: "Table: " + caption text (without prefix)
                let mut caption_text = String::from("Table: ");

                for caption_child in child.children_with_tokens() {
                    match caption_child {
                        rowan::NodeOrToken::Token(token)
                            if token.kind() == SyntaxKind::TABLE_CAPTION_PREFIX =>
                        {
                            // Skip the original prefix - we're adding normalized "Table: " above
                        }
                        rowan::NodeOrToken::Token(token) => {
                            caption_text.push_str(token.text());
                        }
                        rowan::NodeOrToken::Node(node) => {
                            caption_text.push_str(&node.text().to_string());
                        }
                    }
                }

                caption = Some(caption_text.trim().to_string());
                caption_after = seen_separator; // After if we've seen separator/rows
            }
            SyntaxKind::TABLE_SEPARATOR => {
                let separator_text = child.text().to_string();
                alignments = extract_alignments(&separator_text);
                seen_separator = true;
            }
            SyntaxKind::TABLE_HEADER | SyntaxKind::TABLE_ROW => {
                let row_content = format_cell_content(&child, config);
                let cells = split_row(&row_content);
                rows.push(cells);
            }
            _ => {}
        }
    }

    TableData {
        rows,
        alignments,
        caption,
        caption_after,
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
pub fn format_pipe_table(node: &SyntaxNode, config: &Config) -> String {
    let table_data = extract_pipe_table_data(node, config);
    let mut output = String::new();

    // Early return if no rows
    if table_data.rows.is_empty() {
        return node.text().to_string();
    }

    let widths = calculate_column_widths(&table_data.rows);

    // Emit caption before if present
    if let Some(ref caption_text) = table_data.caption
        && !table_data.caption_after
    {
        // Caption text now includes the prefix (e.g., "Table: " or ": "),
        // so just output it as-is
        output.push_str(caption_text);
        output.push_str("\n\n"); // Blank line between caption and table
    }

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

    // Emit caption after if present
    if let Some(ref caption_text) = table_data.caption
        && table_data.caption_after
    {
        output.push('\n');
        // Caption text now includes the prefix, so output as-is
        output.push_str(caption_text);
        output.push('\n');
    }

    output
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

/// Extract structured data from grid table AST node
fn extract_grid_table_data(node: &SyntaxNode, config: &Config) -> TableData {
    let mut rows = Vec::new();
    let mut alignments = Vec::new();
    let mut caption = None;
    let mut caption_after = false;
    let mut seen_header = false;

    for child in node.children() {
        match child.kind() {
            SyntaxKind::TABLE_CAPTION => {
                // Build normalized caption: "Table: " + caption text (without prefix)
                let mut caption_text = String::from("Table: ");

                for caption_child in child.children_with_tokens() {
                    match caption_child {
                        rowan::NodeOrToken::Token(token)
                            if token.kind() == SyntaxKind::TABLE_CAPTION_PREFIX =>
                        {
                            // Skip the original prefix
                        }
                        rowan::NodeOrToken::Token(token) => {
                            caption_text.push_str(token.text());
                        }
                        rowan::NodeOrToken::Node(node) => {
                            caption_text.push_str(&node.text().to_string());
                        }
                    }
                }

                caption = Some(caption_text.trim().to_string());
                caption_after = seen_header; // After if we've seen table content
            }
            SyntaxKind::TABLE_SEPARATOR => {
                let separator_text = child.text().to_string();

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

                // Check if this is a header separator (contains =)
                if separator_text.contains('=') {
                    seen_header = true;
                }
            }
            SyntaxKind::TABLE_HEADER | SyntaxKind::TABLE_ROW => {
                let row_content = format_cell_content(&child, config);
                let cells = split_grid_row(&row_content);
                rows.push(cells);
            }
            _ => {}
        }
    }

    TableData {
        rows,
        alignments,
        caption,
        caption_after,
        column_widths: None,
        column_positions: None,
        has_header: true, // Grid tables always have headers
    }
}

/// Format a grid table with consistent alignment and padding
pub fn format_grid_table(node: &SyntaxNode, config: &Config) -> String {
    let table_data = extract_grid_table_data(node, config);
    let mut output = String::new();

    // Early return if no rows
    if table_data.rows.is_empty() {
        return node.text().to_string();
    }

    let widths = calculate_grid_column_widths(&table_data.rows);

    // Emit caption before if present
    if let Some(ref caption_text) = table_data.caption
        && !table_data.caption_after
    {
        output.push_str(caption_text);
        output.push_str("\n\n");
    }

    // Helper to create separator line
    let make_separator = |is_header: bool| -> String {
        let mut line = String::from("+");

        for (col_idx, width) in widths.iter().enumerate() {
            let alignment = table_data
                .alignments
                .get(col_idx)
                .copied()
                .unwrap_or(Alignment::Default);

            let fill_char = if is_header { '=' } else { '-' };

            // Create separator with alignment markers
            // Per Pandoc spec: alignment colons go in header separator ONLY, not row separators
            let segment = if is_header {
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
    output.push_str(&make_separator(false));

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

        // Insert separator after first row (header) or after each row
        if row_idx == 0 {
            // Header separator with =
            output.push_str(&make_separator(true));
        } else {
            // Row separator with -
            output.push_str(&make_separator(false));
        }
    }

    // Emit caption after if present
    if let Some(ref caption_text) = table_data.caption
        && table_data.caption_after
    {
        output.push('\n');
        output.push_str(caption_text);
        output.push('\n');
    }

    output
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
    let mut caption_after = false;
    let mut separator_line = String::new();
    let mut header_line: Option<String> = None;
    let mut seen_separator = false;

    for child in node.children() {
        match child.kind() {
            SyntaxKind::TABLE_CAPTION => {
                // Build normalized caption: "Table: " + caption text (without prefix)
                let mut caption_text = String::from("Table: ");

                for caption_child in child.children_with_tokens() {
                    match caption_child {
                        rowan::NodeOrToken::Token(token)
                            if token.kind() == SyntaxKind::TABLE_CAPTION_PREFIX =>
                        {
                            // Skip the original prefix
                        }
                        rowan::NodeOrToken::Token(token) => {
                            caption_text.push_str(token.text());
                        }
                        rowan::NodeOrToken::Node(node) => {
                            caption_text.push_str(&node.text().to_string());
                        }
                    }
                }

                caption = Some(caption_text.trim().to_string());
                caption_after = seen_separator;
            }
            SyntaxKind::TABLE_SEPARATOR => {
                separator_line = child.text().to_string();
                seen_separator = true;

                // Extract column positions
                columns = extract_simple_table_columns(&separator_line);
            }
            SyntaxKind::TABLE_HEADER => {
                // Get RAW text for alignment detection (preserve exact spacing)
                let raw_text = child.text().to_string();
                header_line = Some(raw_text);
            }
            SyntaxKind::TABLE_ROW => {
                // Data rows come after separator
                if !columns.is_empty() {
                    let row_content = format_cell_content(&child, config);

                    // Skip rows that are actually separator lines (for headerless tables)
                    // Separator lines are all dashes and spaces (and newlines)
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
            _ => {}
        }
    }

    // Determine alignments based on header
    if !columns.is_empty() {
        determine_simple_alignments(&mut columns, &separator_line, header_line.as_deref());
    }

    // Track if we have a header before potentially consuming header_line
    let has_header = header_line.is_some();

    // Add header row to rows if present
    if let Some(header) = header_line {
        let header_cells = split_simple_table_row(&header, &columns);
        rows.insert(0, header_cells);
    }

    let alignments = columns.iter().map(|c| c.alignment).collect();

    // For simple tables, preserve both separator dash lengths AND column positions
    let column_widths: Vec<usize> = columns.iter().map(|c| c.end - c.start).collect();
    let column_positions: Vec<(usize, usize)> = columns.iter().map(|c| (c.start, c.end)).collect();

    TableData {
        rows,
        alignments,
        caption,
        caption_after,
        column_widths: Some(column_widths),
        column_positions: Some(column_positions),
        has_header, // Simple tables may or may not have headers
    }
}

/// Format a simple table with consistent alignment and padding
pub fn format_simple_table(node: &SyntaxNode, config: &Config) -> String {
    let table_data = extract_simple_table_data(node, config);
    let mut output = String::new();

    // Early return if no rows
    if table_data.rows.is_empty() {
        return node.text().to_string();
    }

    // For simple tables, use separator dash lengths; for other tables, calculate from content
    let widths = if let Some(ref widths) = table_data.column_widths {
        widths.clone()
    } else {
        calculate_column_widths(&table_data.rows)
    };

    // Emit caption before if present
    if let Some(ref caption_text) = table_data.caption
        && !table_data.caption_after
    {
        output.push_str(caption_text);
        output.push_str("\n\n");
    }

    // Check if we have a header (from TableData, not just first row)
    let has_header = table_data.has_header;

    // For headerless simple tables, emit opening separator first
    if !has_header
        && table_data.column_positions.is_some()
        && let Some(ref positions) = table_data.column_positions
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
        if let Some(ref positions) = table_data.column_positions {
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
        if let Some(ref positions) = table_data.column_positions {
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
        && table_data.column_positions.is_some()
        && let Some(ref positions) = table_data.column_positions
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

    // Emit caption after if present
    if let Some(ref caption_text) = table_data.caption
        && table_data.caption_after
    {
        output.push('\n');
        output.push_str(caption_text);
        output.push('\n');
    }

    output
}

/// Extract column information from multiline table separator line
fn extract_multiline_columns(separator_line: &str) -> Vec<(usize, usize)> {
    // Strip leading/trailing whitespace - column positions are relative to trimmed line
    let trimmed = separator_line.trim();

    let mut columns = Vec::new();
    let mut in_dashes = false;
    let mut col_start = 0;

    for (i, ch) in trimmed.char_indices() {
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
        columns.push((col_start, trimmed.len()));
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
                line[col_start..col_end].trim()
            } else if col_start < line.len() {
                line[col_start..].trim()
            } else {
                ""
            };
            cells[col_idx].push(cell_line.to_string());
        }
    }

    cells
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
                // Build normalized caption: "Table: " + caption text (without prefix)
                let mut caption_text = String::from("Table: ");

                for caption_child in child.children_with_tokens() {
                    match caption_child {
                        rowan::NodeOrToken::Token(token)
                            if token.kind() == SyntaxKind::TABLE_CAPTION_PREFIX =>
                        {
                            // Skip the original prefix - we're using ": " prefix
                        }
                        rowan::NodeOrToken::Token(token) => {
                            caption_text.push_str(token.text());
                        }
                        rowan::NodeOrToken::Node(node) => {
                            caption_text.push_str(&node.text().to_string());
                        }
                    }
                }

                caption = Some(caption_text.trim().to_string());
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
                // Collect all header text (may span multiple lines)
                header_text = child.text().to_string();
            }
            SyntaxKind::TABLE_ROW => {
                // Format cell content and split into cells
                let row_content = format_cell_content(&child, config);

                // Extract multiline cells
                let cells = extract_multiline_cells(&row_content, &column_positions);
                rows.push(cells);
            }
            _ => {}
        }
    }

    // Add header as first row if present
    if has_header && !column_positions.is_empty() {
        let header_cells = extract_multiline_cells(&header_text, &column_positions);
        rows.insert(0, header_cells);

        // Determine alignments from header
        for &(col_start, col_end) in &column_positions {
            let alignment = determine_multiline_alignment(&header_text, col_start, col_end);
            alignments.push(alignment);
        }
    } else {
        // No header - use default alignment
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
    let table_data = extract_multiline_table_data(node, config);
    let mut output = String::new();

    // Early return if no rows or no column info
    if table_data.rows.is_empty() || table_data.column_positions.is_empty() {
        return node.text().to_string();
    }

    let positions = &table_data.column_positions;

    // Calculate total table width
    let last_col_end = positions.last().map(|(_, end)| *end).unwrap_or(0);

    // Emit caption before if present
    if let Some(ref caption_text) = table_data.caption {
        output.push_str(caption_text);
        output.push_str("\n\n"); // Blank line between caption and table
    }

    // Emit opening separator
    if table_data.has_header {
        // With header: opening separator is full-width dashes
        output.push_str(&"-".repeat(last_col_end));
        output.push('\n');
    } else {
        // Headerless: opening separator shows column boundaries
        let mut sep_chars: Vec<char> = vec![' '; last_col_end];
        for &(col_start, col_end) in positions {
            for i in col_start..col_end {
                sep_chars[i] = '-';
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

            output.push_str(&line_chars.iter().collect::<String>().trim_end());
            output.push('\n');
        }

        // Emit column separator (no indent)
        let mut sep_chars: Vec<char> = vec![' '; last_col_end];
        for &(col_start, col_end) in positions {
            for i in col_start..col_end {
                sep_chars[i] = '-';
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

            output.push_str(&line_chars.iter().collect::<String>().trim_end());
            output.push('\n');
        }

        // Emit blank line between rows (except after last row)
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
        for &(col_start, col_end) in positions {
            for i in col_start..col_end {
                sep_chars[i] = '-';
            }
        }
        output.push_str(&sep_chars.iter().collect::<String>());
        output.push('\n');
    }

    output
}
