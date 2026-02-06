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

/// Try to parse a table caption from a line.
/// Returns Some((prefix_len, caption_text)) if it's a caption.
fn try_parse_caption_prefix(line: &str) -> Option<(usize, &str)> {
    let trimmed = line.trim_start();
    let leading_spaces = line.len() - trimmed.len();

    // Must have leading spaces <= 3 to not be a code block
    if leading_spaces > 3 {
        return None;
    }

    // Check for "Table:" or "table:" or just ":"
    if let Some(rest) = trimmed.strip_prefix("Table:") {
        Some((leading_spaces + 6, rest))
    } else if let Some(rest) = trimmed.strip_prefix("table:") {
        Some((leading_spaces + 6, rest))
    } else if let Some(rest) = trimmed.strip_prefix(':') {
        // Just ":" - but need to be careful not to match definition list markers
        // A caption with just ":" should have content or be followed by content
        if !rest.trim().is_empty() || rest.starts_with(' ') {
            Some((leading_spaces + 1, rest))
        } else {
            None
        }
    } else {
        None
    }
}

/// Check if a line could be the start of a table caption.
fn is_table_caption_start(line: &str) -> bool {
    try_parse_caption_prefix(line).is_some()
}

/// Check if there's a table following a potential caption at this position.
/// This is used to avoid parsing a caption as a paragraph when it belongs to a table.
pub(crate) fn is_caption_followed_by_table(lines: &[&str], caption_pos: usize) -> bool {
    if caption_pos >= lines.len() {
        return false;
    }

    // Caption must start with a caption prefix
    if !is_table_caption_start(lines[caption_pos]) {
        return false;
    }

    let mut pos = caption_pos + 1;

    // Skip continuation lines of caption (non-blank lines)
    while pos < lines.len() && !lines[pos].trim().is_empty() {
        // If we hit a table separator, we found a table
        if try_parse_table_separator(lines[pos]).is_some() {
            return true;
        }
        pos += 1;
    }

    // Skip one blank line
    if pos < lines.len() && lines[pos].trim().is_empty() {
        pos += 1;
    }

    // Check for table at next position
    if pos < lines.len() {
        // Could be a separator line (headerless table)
        if try_parse_table_separator(lines[pos]).is_some() {
            return true;
        }
        // Or could be a header line followed by separator
        if pos + 1 < lines.len()
            && !lines[pos].trim().is_empty()
            && try_parse_table_separator(lines[pos + 1]).is_some()
        {
            return true;
        }
    }

    false
}

/// Find caption before table (if any).
/// Returns the position where caption starts, or None.
fn find_caption_before_table(lines: &[&str], table_start: usize) -> Option<usize> {
    if table_start == 0 {
        return None;
    }

    // Look backward for a caption
    // Caption must be immediately before table (with possible blank line between)
    let mut pos = table_start - 1;

    // Skip one blank line if present
    if lines[pos].trim().is_empty() {
        if pos == 0 {
            return None;
        }
        pos -= 1;
    }

    // Check if this line is a caption
    if is_table_caption_start(lines[pos]) {
        // For now, assume caption is single line
        // Multi-line captions are more complex to detect backwards
        Some(pos)
    } else {
        None
    }
}

/// Find caption after table (if any).
/// Returns (caption_start, caption_end) positions, or None.
fn find_caption_after_table(lines: &[&str], table_end: usize) -> Option<(usize, usize)> {
    if table_end >= lines.len() {
        return None;
    }

    let mut pos = table_end;

    // Skip one blank line if present
    if pos < lines.len() && lines[pos].trim().is_empty() {
        pos += 1;
    }

    if pos >= lines.len() {
        return None;
    }

    // Check if this line is a caption
    if is_table_caption_start(lines[pos]) {
        let caption_start = pos;
        // Find end of caption (continues until blank line)
        let mut caption_end = caption_start + 1;
        while caption_end < lines.len() && !lines[caption_end].trim().is_empty() {
            caption_end += 1;
        }
        Some((caption_start, caption_end))
    } else {
        None
    }
}

/// Emit a table caption node.
fn emit_table_caption(
    builder: &mut GreenNodeBuilder<'static>,
    lines: &[&str],
    start: usize,
    end: usize,
) {
    builder.start_node(SyntaxKind::TableCaption.into());

    for (i, line) in lines[start..end].iter().enumerate() {
        if i == 0 {
            // First line - strip the caption prefix
            if let Some((_prefix_len, caption_text)) = try_parse_caption_prefix(line) {
                let content = caption_text.trim();
                if !content.is_empty() {
                    builder.token(SyntaxKind::TEXT.into(), content);
                }
            }
        } else {
            // Continuation lines
            let content = line.trim();
            if !content.is_empty() {
                if i > 0 {
                    builder.token(SyntaxKind::TEXT.into(), " ");
                }
                builder.token(SyntaxKind::TEXT.into(), content);
            }
        }
    }

    builder.token(SyntaxKind::NEWLINE.into(), "\n");
    builder.finish_node();
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

    // Check for caption before table
    let caption_before = find_caption_before_table(lines, start_pos);

    // Check for caption after table
    let caption_after = find_caption_after_table(lines, end_pos);

    // Build the table
    builder.start_node(SyntaxKind::SimpleTable.into());

    // Emit caption before if present
    if let Some(caption_pos) = caption_before {
        emit_table_caption(builder, lines, caption_pos, caption_pos + 1);
    }

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

    // Emit caption after if present
    if let Some((cap_start, cap_end)) = caption_after {
        // Emit blank line before caption if needed
        if cap_start > end_pos {
            builder.start_node(SyntaxKind::BlankLine.into());
            builder.token(SyntaxKind::BlankLine.into(), "");
            builder.token(SyntaxKind::NEWLINE.into(), "\n");
            builder.finish_node();
        }
        emit_table_caption(builder, lines, cap_start, cap_end);
    }

    builder.finish_node(); // SimpleTable

    // Calculate lines consumed (including captions)
    let table_start = caption_before.unwrap_or(if has_header {
        separator_pos - 1
    } else {
        separator_pos
    });

    let table_end = if let Some((_, cap_end)) = caption_after {
        cap_end
    } else {
        end_pos
    };

    let lines_consumed = table_end - table_start;

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

// ============================================================================
// Pipe Table Parsing
// ============================================================================

/// Check if a line is a pipe table separator line.
/// Returns the column alignments if it's a valid separator.
fn try_parse_pipe_separator(line: &str) -> Option<Vec<Alignment>> {
    let trimmed = line.trim();

    // Must contain at least one pipe
    if !trimmed.contains('|') && !trimmed.contains('+') {
        return None;
    }

    // Split by pipes (or + for orgtbl variant)
    let cells: Vec<&str> = if trimmed.contains('+') {
        // Orgtbl variant: use + as separator in separator line
        trimmed.split(['|', '+']).collect()
    } else {
        trimmed.split('|').collect()
    };

    let mut alignments = Vec::new();

    for cell in cells {
        let cell = cell.trim();

        // Skip empty cells (from leading/trailing pipes)
        if cell.is_empty() {
            continue;
        }

        // Must be dashes with optional colons
        let starts_colon = cell.starts_with(':');
        let ends_colon = cell.ends_with(':');

        // Remove colons to check if rest is all dashes
        let without_colons = cell.trim_start_matches(':').trim_end_matches(':');

        // Must have at least one dash
        if without_colons.is_empty() || !without_colons.chars().all(|c| c == '-') {
            return None;
        }

        // Determine alignment from colon positions
        let alignment = match (starts_colon, ends_colon) {
            (true, true) => Alignment::Center,
            (true, false) => Alignment::Left,
            (false, true) => Alignment::Right,
            (false, false) => Alignment::Default,
        };

        alignments.push(alignment);
    }

    // Must have at least one column
    if alignments.is_empty() {
        None
    } else {
        Some(alignments)
    }
}

/// Split a pipe table row into cells.
fn split_pipe_row(line: &str) -> Vec<String> {
    let trimmed = line.trim();

    // Handle escaped pipes: \|
    // For now, simple split - in future handle escapes properly
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

/// Try to parse a pipe table starting at the given position.
/// Returns the number of lines consumed if successful.
pub(crate) fn try_parse_pipe_table(
    lines: &[&str],
    start_pos: usize,
    builder: &mut GreenNodeBuilder<'static>,
) -> Option<usize> {
    if start_pos + 1 >= lines.len() {
        return None;
    }

    // First line should have pipes (potential header)
    let header_line = lines[start_pos];
    if !header_line.contains('|') {
        return None;
    }

    // Second line should be separator
    let separator_line = lines[start_pos + 1];
    let alignments = try_parse_pipe_separator(separator_line)?;

    // Parse header cells
    let header_cells = split_pipe_row(header_line);

    // Number of columns should match (approximately - be lenient)
    if header_cells.len() != alignments.len() && !header_cells.is_empty() {
        // Only fail if very different
        if header_cells.len() < alignments.len() / 2 || header_cells.len() > alignments.len() * 2 {
            return None;
        }
    }

    // Find table end (first blank line or end of input)
    let mut end_pos = start_pos + 2;
    while end_pos < lines.len() {
        let line = lines[end_pos];
        if line.trim().is_empty() {
            break;
        }
        // Row should have pipes
        if !line.contains('|') {
            break;
        }
        end_pos += 1;
    }

    // Must have at least one data row
    if end_pos <= start_pos + 2 {
        return None;
    }

    // Check for caption before table
    let caption_before = find_caption_before_table(lines, start_pos);

    // Check for caption after table
    let caption_after = find_caption_after_table(lines, end_pos);

    // Build the pipe table
    builder.start_node(SyntaxKind::PipeTable.into());

    // Emit caption before if present
    if let Some(caption_pos) = caption_before {
        emit_table_caption(builder, lines, caption_pos, caption_pos + 1);
    }

    // Emit header row
    builder.start_node(SyntaxKind::TableHeader.into());
    builder.token(SyntaxKind::TEXT.into(), header_line);
    builder.token(SyntaxKind::NEWLINE.into(), "\n");
    builder.finish_node();

    // Emit separator
    builder.start_node(SyntaxKind::TableSeparator.into());
    builder.token(SyntaxKind::TEXT.into(), separator_line);
    builder.token(SyntaxKind::NEWLINE.into(), "\n");
    builder.finish_node();

    // Emit data rows
    for line in lines.iter().take(end_pos).skip(start_pos + 2) {
        builder.start_node(SyntaxKind::TableRow.into());
        builder.token(SyntaxKind::TEXT.into(), line);
        builder.token(SyntaxKind::NEWLINE.into(), "\n");
        builder.finish_node();
    }

    // Emit caption after if present
    if let Some((cap_start, cap_end)) = caption_after {
        // Emit blank line before caption if needed
        if cap_start > end_pos {
            builder.start_node(SyntaxKind::BlankLine.into());
            builder.token(SyntaxKind::BlankLine.into(), "");
            builder.token(SyntaxKind::NEWLINE.into(), "\n");
            builder.finish_node();
        }
        emit_table_caption(builder, lines, cap_start, cap_end);
    }

    builder.finish_node(); // PipeTable

    // Calculate lines consumed
    let table_start = caption_before.unwrap_or(start_pos);
    let table_end = if let Some((_, cap_end)) = caption_after {
        cap_end
    } else {
        end_pos
    };

    Some(table_end - table_start)
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

    #[test]
    fn test_caption_prefix_detection() {
        assert!(try_parse_caption_prefix("Table: My caption").is_some());
        assert!(try_parse_caption_prefix("table: My caption").is_some());
        assert!(try_parse_caption_prefix(": My caption").is_some());
        assert!(try_parse_caption_prefix(":").is_none()); // Just colon, no content
        assert!(try_parse_caption_prefix("Not a caption").is_none());
    }

    #[test]
    fn test_table_with_caption_after() {
        let input = vec![
            "  Right     Left     Center     Default",
            "-------     ------ ----------   -------",
            "     12     12        12            12",
            "    123     123       123          123",
            "",
            "Table: Demonstration of simple table syntax.",
            "",
        ];

        let mut builder = GreenNodeBuilder::new();
        let result = try_parse_simple_table(&input, 0, &mut builder);

        assert!(result.is_some());
        // Should consume: header + sep + 2 rows + blank + caption
        assert_eq!(result.unwrap(), 6);
    }

    #[test]
    fn test_table_with_caption_before() {
        let input = vec![
            "Table: Demonstration of simple table syntax.",
            "",
            "  Right     Left     Center     Default",
            "-------     ------ ----------   -------",
            "     12     12        12            12",
            "    123     123       123          123",
            "",
        ];

        let mut builder = GreenNodeBuilder::new();
        let result = try_parse_simple_table(&input, 2, &mut builder);

        assert!(result.is_some());
        // Should consume: caption + blank + header + sep + 2 rows
        assert_eq!(result.unwrap(), 6);
    }

    #[test]
    fn test_caption_with_colon_prefix() {
        let input = vec![
            "  Right     Left",
            "-------     ------",
            "     12     12",
            "",
            ": Short caption",
            "",
        ];

        let mut builder = GreenNodeBuilder::new();
        let result = try_parse_simple_table(&input, 0, &mut builder);

        assert!(result.is_some());
        assert_eq!(result.unwrap(), 5); // header + sep + row + blank + caption
    }

    #[test]
    fn test_multiline_caption() {
        let input = vec![
            "  Right     Left",
            "-------     ------",
            "     12     12",
            "",
            "Table: This is a longer caption",
            "that spans multiple lines.",
            "",
        ];

        let mut builder = GreenNodeBuilder::new();
        let result = try_parse_simple_table(&input, 0, &mut builder);

        assert!(result.is_some());
        // Should consume through end of multi-line caption
        assert_eq!(result.unwrap(), 6);
    }

    // Pipe table tests
    #[test]
    fn test_pipe_separator_detection() {
        assert!(try_parse_pipe_separator("|------:|:-----|---------|:------:|").is_some());
        assert!(try_parse_pipe_separator("|---|---|").is_some());
        assert!(try_parse_pipe_separator("-----|-----:").is_some()); // No leading pipe
        assert!(try_parse_pipe_separator("|-----+-------|").is_some()); // Orgtbl variant
        assert!(try_parse_pipe_separator("not a separator").is_none());
    }

    #[test]
    fn test_pipe_alignments() {
        let aligns = try_parse_pipe_separator("|------:|:-----|---------|:------:|").unwrap();
        assert_eq!(aligns.len(), 4);
        assert_eq!(aligns[0], Alignment::Right);
        assert_eq!(aligns[1], Alignment::Left);
        assert_eq!(aligns[2], Alignment::Default);
        assert_eq!(aligns[3], Alignment::Center);
    }

    #[test]
    fn test_split_pipe_row() {
        let cells = split_pipe_row("| Right | Left | Center |");
        assert_eq!(cells.len(), 3);
        assert_eq!(cells[0], "Right");
        assert_eq!(cells[1], "Left");
        assert_eq!(cells[2], "Center");

        // Without leading/trailing pipes
        let cells2 = split_pipe_row("Right | Left | Center");
        assert_eq!(cells2.len(), 3);
    }

    #[test]
    fn test_basic_pipe_table() {
        let input = vec![
            "",
            "| Right | Left | Center |",
            "|------:|:-----|:------:|",
            "|   12  |  12  |   12   |",
            "|  123  |  123 |  123   |",
            "",
        ];

        let mut builder = GreenNodeBuilder::new();
        let result = try_parse_pipe_table(&input, 1, &mut builder);

        assert!(result.is_some());
        assert_eq!(result.unwrap(), 4); // header + sep + 2 rows
    }

    #[test]
    fn test_pipe_table_no_edge_pipes() {
        let input = vec![
            "",
            "fruit| price",
            "-----|-----:",
            "apple|2.05",
            "pear|1.37",
            "",
        ];

        let mut builder = GreenNodeBuilder::new();
        let result = try_parse_pipe_table(&input, 1, &mut builder);

        assert!(result.is_some());
        assert_eq!(result.unwrap(), 4);
    }

    #[test]
    fn test_pipe_table_with_caption() {
        let input = vec![
            "",
            "| Col1 | Col2 |",
            "|------|------|",
            "| A    | B    |",
            "",
            "Table: My pipe table",
            "",
        ];

        let mut builder = GreenNodeBuilder::new();
        let result = try_parse_pipe_table(&input, 1, &mut builder);

        assert!(result.is_some());
        assert_eq!(result.unwrap(), 5); // header + sep + row + blank + caption
    }
}
