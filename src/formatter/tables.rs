use crate::config::Config;
use crate::formatter::inline::format_inline_node;
use crate::syntax::{SyntaxKind, SyntaxNode};
use rowan::NodeOrToken;

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
            SyntaxKind::TableCaption => {
                // Build normalized caption: "Table: " + caption text (without prefix)
                let mut caption_text = String::from("Table: ");

                for caption_child in child.children_with_tokens() {
                    match caption_child {
                        rowan::NodeOrToken::Token(token)
                            if token.kind() == SyntaxKind::TableCaptionPrefix =>
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
            SyntaxKind::TableSeparator => {
                let separator_text = child.text().to_string();
                alignments = extract_alignments(&separator_text);
                seen_separator = true;
            }
            SyntaxKind::TableHeader | SyntaxKind::TableRow => {
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
                widths[col_idx] = widths[col_idx].max(cell.len());
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

            // Apply alignment (only to data rows, not header)
            let padded_cell = if row_idx == 0 {
                // Header row: always left-align
                format!("{:<width$}", cell, width = width)
            } else {
                // Data rows: respect alignment
                match alignment {
                    Alignment::Left | Alignment::Default => {
                        format!("{:<width$}", cell, width = width)
                    }
                    Alignment::Right => {
                        format!("{:>width$}", cell, width = width)
                    }
                    Alignment::Center => {
                        let total_padding = width.saturating_sub(cell.len());
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
