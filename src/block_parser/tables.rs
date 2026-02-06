//! Simple table parsing for Pandoc's simple_tables extension.

use crate::syntax::SyntaxKind;
use rowan::GreenNodeBuilder;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Alignment {
    Left,
    Right,
    Center,
    Default,
}

/// Column information extracted from the separator line.
#[derive(Debug, Clone)]
pub(crate) struct Column {
    /// Start position (byte index) in the line
    start: usize,
    /// End position (byte index) in the line
    end: usize,
    /// Column alignment
    alignment: Alignment,
}

/// Try to detect if a line is a table separator line.
/// Returns Some(column positions) if it's a valid separator.
pub(crate) fn try_parse_table_separator(line: &str) -> Option<Vec<Column>> {
    let trimmed = line.trim_start();
    let leading_spaces = line.len() - trimmed.len();

    // Must have leading spaces <= 3 to not be a code block
    if leading_spaces > 3 {
        return None;
    }

    // Must contain at least one dash
    if !trimmed.contains('-') {
        return None;
    }

    // A separator line consists of dashes and spaces
    if !trimmed.chars().all(|c| c == '-' || c == ' ') {
        return None;
    }

    // Must not be a horizontal rule (needs spaces between dash groups)
    // Horizontal rules are continuous dashes (possibly with leading spaces)
    if trimmed.chars().filter(|&c| c == '-').count() >= 3
        && !trimmed.contains("  ") // no double spaces = likely horizontal rule
        && trimmed.chars().all(|c| c == '-' || c == ' ')
    {
        // Could be horizontal rule, check if there are clear column separations
        let dash_groups: Vec<_> = trimmed.split(' ').filter(|s| !s.is_empty()).collect();

        // If only one group of dashes, it's a horizontal rule
        if dash_groups.len() == 1 {
            return None;
        }
    }

    // Extract column positions from dash groups
    let columns = extract_columns(trimmed, leading_spaces);

    if columns.is_empty() {
        return None;
    }

    Some(columns)
}

/// Extract column positions from a separator line.
fn extract_columns(separator: &str, offset: usize) -> Vec<Column> {
    let mut columns = Vec::new();
    let mut in_dashes = false;
    let mut col_start = 0;

    for (i, ch) in separator.char_indices() {
        match ch {
            '-' => {
                if !in_dashes {
                    col_start = i + offset;
                    in_dashes = true;
                }
            }
            ' ' => {
                if in_dashes {
                    columns.push(Column {
                        start: col_start,
                        end: i + offset,
                        alignment: Alignment::Default, // Will be determined later
                    });
                    in_dashes = false;
                }
            }
            _ => {}
        }
    }

    // Handle last column
    if in_dashes {
        columns.push(Column {
            start: col_start,
            end: separator.len() + offset,
            alignment: Alignment::Default,
        });
    }

    columns
}

/// Determine column alignments based on separator and optional header.
fn determine_alignments(columns: &mut [Column], separator_line: &str, header_line: Option<&str>) {
    for col in columns.iter_mut() {
        let sep_slice = &separator_line[col.start..col.end];

        if let Some(header) = header_line {
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
            let text_end = header_in_col.trim_end().len() + text_start;

            // Check dash alignment relative to text
            let dashes_start = 0; // Dashes start at beginning of sep_slice
            let dashes_end = sep_slice.len();

            let flush_left = dashes_start == text_start;
            let flush_right = dashes_end == text_end;

            col.alignment = match (flush_left, flush_right) {
                (true, true) => Alignment::Default,
                (true, false) => Alignment::Left,
                (false, true) => Alignment::Right,
                (false, false) => Alignment::Center,
            };
        } else {
            // Without header, alignment based on first row (we'll handle this later)
            col.alignment = Alignment::Default;
        }
    }
}

/// Try to parse a simple table starting at the given position.
/// Returns the number of lines consumed if successful.
pub(crate) fn try_parse_simple_table(
    lines: &[&str],
    start_pos: usize,
    builder: &mut GreenNodeBuilder<'static>,
) -> Option<usize> {
    if start_pos >= lines.len() {
        return None;
    }

    // Look for a separator line
    let separator_pos = find_separator_line(lines, start_pos)?;
    let separator_line = lines[separator_pos];
    let mut columns = try_parse_table_separator(separator_line)?;

    // Determine if there's a header (separator not at start)
    let has_header = separator_pos > start_pos;
    let header_line = if has_header {
        Some(lines[separator_pos - 1])
    } else {
        None
    };

    // Determine alignments
    determine_alignments(&mut columns, separator_line, header_line);

    // Find table end (blank line or end of input)
    let end_pos = find_table_end(lines, separator_pos + 1);

    // Must have at least one data row (or it's just a separator)
    let data_rows = end_pos - separator_pos - 1;

    if data_rows == 0 {
        return None;
    }

    // Build the table
    builder.start_node(SyntaxKind::SimpleTable.into());

    // Emit header if present
    if has_header {
        emit_table_row(
            builder,
            lines[separator_pos - 1],
            &columns,
            SyntaxKind::TableHeader,
        );
    }

    // Emit separator
    builder.start_node(SyntaxKind::TableSeparator.into());
    builder.token(SyntaxKind::TEXT.into(), separator_line);
    builder.token(SyntaxKind::NEWLINE.into(), "\n");
    builder.finish_node();

    // Emit data rows
    for line in lines.iter().take(end_pos).skip(separator_pos + 1) {
        emit_table_row(builder, line, &columns, SyntaxKind::TableRow);
    }

    builder.finish_node(); // SimpleTable

    // Calculate lines consumed
    let lines_consumed = if has_header {
        end_pos - (separator_pos - 1)
    } else {
        end_pos - separator_pos
    };

    Some(lines_consumed)
}

/// Find the position of a separator line starting from pos.
fn find_separator_line(lines: &[&str], start_pos: usize) -> Option<usize> {
    // Check first line
    if try_parse_table_separator(lines[start_pos]).is_some() {
        return Some(start_pos);
    }

    // Check second line (for table with header)
    if start_pos + 1 < lines.len()
        && !lines[start_pos].trim().is_empty()
        && try_parse_table_separator(lines[start_pos + 1]).is_some()
    {
        return Some(start_pos + 1);
    }

    None
}

/// Find where the table ends (first blank line or end of input).
fn find_table_end(lines: &[&str], start_pos: usize) -> usize {
    for i in start_pos..lines.len() {
        if lines[i].trim().is_empty() {
            return i;
        }
        // Check if this could be a closing separator
        if try_parse_table_separator(lines[i]).is_some() {
            // Check if next line is blank or end
            if i + 1 >= lines.len() || lines[i + 1].trim().is_empty() {
                return i + 1;
            }
        }
    }
    lines.len()
}

/// Emit a table row (header or data row).
fn emit_table_row(
    builder: &mut GreenNodeBuilder<'static>,
    line: &str,
    _columns: &[Column],
    row_kind: SyntaxKind,
) {
    builder.start_node(row_kind.into());

    // For now, just emit the whole line as text since formatting is deferred
    // In the future, we can parse individual cells here
    builder.token(SyntaxKind::TEXT.into(), line);
    builder.token(SyntaxKind::NEWLINE.into(), "\n");

    builder.finish_node();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_separator_detection() {
        assert!(try_parse_table_separator("------- ------ ----------   -------").is_some());
        assert!(try_parse_table_separator("  ---  ---  ---").is_some());
        assert!(try_parse_table_separator("-------").is_none()); // horizontal rule
        assert!(try_parse_table_separator("--- --- ---").is_some()); // table separator
    }

    #[test]
    fn test_column_extraction() {
        let line = "-------     ------ ----------   -------";
        let columns = extract_columns(line, 0);
        assert_eq!(columns.len(), 4);
    }

    #[test]
    fn test_simple_table_with_header() {
        let input = vec![
            "  Right     Left     Center     Default",
            "-------     ------ ----------   -------",
            "     12     12        12            12",
            "    123     123       123          123",
            "",
        ];

        let mut builder = GreenNodeBuilder::new();
        let result = try_parse_simple_table(&input, 0, &mut builder);

        assert!(result.is_some());
        assert_eq!(result.unwrap(), 4); // header + sep + 2 rows
    }

    #[test]
    fn test_headerless_table() {
        let input = vec![
            "-------     ------ ----------   -------",
            "     12     12        12            12",
            "    123     123       123          123",
            "",
        ];

        let mut builder = GreenNodeBuilder::new();
        let result = try_parse_simple_table(&input, 0, &mut builder);

        assert!(result.is_some());
        assert_eq!(result.unwrap(), 3); // sep + 2 rows
    }
}
