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
    rows: Vec<Vec<String>>,     // All rows including header
    alignments: Vec<Alignment>, // Column alignments
    caption: Option<String>,    // Optional caption text
    caption_after: bool,        // True if caption comes after table
}

/// Format cell content, handling both TEXT tokens and inline elements
fn format_cell_content(node: &SyntaxNode, config: &Config) -> String {
    let mut result = String::new();

    for child in node.children_with_tokens() {
        match child {
            NodeOrToken::Token(token) => {
                if token.kind() == SyntaxKind::TEXT {
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

                // Always extract alignments from separators (if not already set)
                // Grid tables have alignments in the first separator or header separator
                if alignments.is_empty() {
                    let extracted = extract_grid_alignments(&separator_text);
                    if !extracted.is_empty() {
                        alignments = extracted;
                    }
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
            // Note: Header separators (=) don't use alignment colons
            let segment = if is_header {
                // Header separator: no colons, just equals signs
                fill_char.to_string().repeat(width + 2)
            } else {
                // Regular separator: include alignment colons if specified
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
