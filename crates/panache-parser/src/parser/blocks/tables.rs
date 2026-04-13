//! Simple table parsing for Pandoc's simple_tables extension.

use crate::config::ParserOptions;
use crate::syntax::SyntaxKind;
use rowan::GreenNodeBuilder;
use unicode_width::UnicodeWidthChar;

use crate::parser::utils::helpers::{emit_line_tokens, strip_newline};
use crate::parser::utils::inline_emission;

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
    // Strip trailing newline if present (CRLF or LF)
    let (trimmed, newline_str) = strip_newline(trimmed);
    let leading_spaces = line.len() - trimmed.len() - newline_str.len();

    // Must have leading spaces <= 3 to not be a code block
    if leading_spaces > 3 {
        return None;
    }

    // Simple tables only use dashed separators.
    if trimmed.contains('*') || trimmed.contains('_') {
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

    // Must not be a horizontal rule.
    let dash_groups: Vec<_> = trimmed.split(' ').filter(|s| !s.is_empty()).collect();
    if dash_groups.len() <= 1 {
        return None;
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

    // Check for "Table:" or "table:" or just ":".
    if let Some(rest) = trimmed.strip_prefix("Table:") {
        Some((leading_spaces + 6, rest))
    } else if let Some(rest) = trimmed.strip_prefix("table:") {
        Some((leading_spaces + 6, rest))
    } else if let Some(rest) = trimmed.strip_prefix(':') {
        // Just ":" caption markers must be followed by whitespace (Pandoc-style).
        // This avoids accidentally treating constructs like fenced div fences ":::" as captions.
        if rest.starts_with(|c: char| c.is_whitespace()) {
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

fn is_bare_colon_caption_start(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with(':') && !trimmed.starts_with("::") && !trimmed.starts_with(":::")
}

fn bare_colon_caption_looks_like_definition_code_block(line: &str) -> bool {
    let Some((_, rest)) = try_parse_caption_prefix(line) else {
        return false;
    };
    let trimmed = rest.trim_start();
    trimmed.starts_with("```") || trimmed.starts_with("~~~")
}

fn is_valid_caption_start_before_table(lines: &[&str], pos: usize) -> bool {
    if !is_table_caption_start(lines[pos]) {
        return false;
    }

    if is_bare_colon_caption_start(lines[pos])
        && bare_colon_caption_looks_like_definition_code_block(lines[pos])
    {
        return false;
    }

    // Avoid stealing definition-list definitions (":   ...") as table captions.
    if is_bare_colon_caption_start(lines[pos]) && pos > 0 && !lines[pos - 1].trim().is_empty() {
        return false;
    }
    true
}

/// Check if a line could be the start of a grid table.
/// Grid tables start with a separator line like +---+---+ or +===+===+
fn is_grid_table_start(line: &str) -> bool {
    try_parse_grid_separator(line).is_some()
}

/// Check if a line could be the start of a multiline table.
/// Multiline tables start with either:
/// - A full-width dash separator (----)
/// - A column separator with dashes and spaces (---- ---- ----)
fn is_multiline_table_start(line: &str) -> bool {
    try_parse_multiline_separator(line).is_some() || is_column_separator(line)
}

/// Check if there's a table following a potential caption at this position.
/// This is used to avoid parsing a caption as a paragraph when it belongs to a table.
pub(crate) fn is_caption_followed_by_table(lines: &[&str], caption_pos: usize) -> bool {
    if caption_pos >= lines.len() {
        return false;
    }

    // Caption must start with a caption prefix
    if !is_valid_caption_start_before_table(lines, caption_pos) {
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
        let line = lines[pos];

        // Check for grid table start (+---+---+ or +===+===+)
        if is_grid_table_start(line) {
            return true;
        }

        // Check for multiline table start (---- or ---- ---- ----)
        if is_multiline_table_start(line) {
            return true;
        }

        // Could be a separator line (simple/pipe table, headerless)
        if try_parse_table_separator(line).is_some() {
            return true;
        }

        // Or could be a header line followed by separator (simple/pipe table with header)
        if pos + 1 < lines.len() && !line.trim().is_empty() {
            let next_line = lines[pos + 1];
            if try_parse_table_separator(next_line).is_some()
                || try_parse_pipe_separator(next_line).is_some()
            {
                return true;
            }
        }
    }

    false
}

/// Find caption before table (if any).
/// Returns (caption_start, caption_end) positions, or None.
fn find_caption_before_table(lines: &[&str], table_start: usize) -> Option<(usize, usize)> {
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

    // Now pos points to the last non-blank line before the table
    // This could be the last line of a multiline caption, or a single-line caption
    let caption_end = pos + 1; // End is exclusive

    // If this line is NOT a caption start, it might be a continuation line
    // Scan backward through non-blank lines to find the caption start
    if !is_valid_caption_start_before_table(lines, pos) {
        // Not a caption start - check if there's a caption start above
        let mut scan_pos = pos;
        while scan_pos > 0 {
            scan_pos -= 1;
            let line = lines[scan_pos];

            // If we hit a blank line, we've gone too far
            if line.trim().is_empty() {
                return None;
            }

            // If we find a caption start, this is the beginning of the multiline caption
            if is_valid_caption_start_before_table(lines, scan_pos) {
                if scan_pos > 0 && !lines[scan_pos - 1].trim().is_empty() {
                    return None;
                }
                if previous_nonblank_looks_like_table(lines, scan_pos) {
                    return None;
                }
                return Some((scan_pos, caption_end));
            }
        }
        // Scanned to beginning without finding caption start
        None
    } else {
        if pos > 0 && !lines[pos - 1].trim().is_empty() {
            return None;
        }
        if previous_nonblank_looks_like_table(lines, pos) {
            return None;
        }
        // This line is a caption start - return the range
        Some((pos, caption_end))
    }
}

fn previous_nonblank_looks_like_table(lines: &[&str], pos: usize) -> bool {
    if pos == 0 {
        return false;
    }
    let mut i = pos;
    while i > 0 {
        i -= 1;
        let line = lines[i].trim();
        if line.is_empty() {
            continue;
        }
        return line_looks_like_table_syntax(line);
    }
    false
}

fn line_looks_like_table_syntax(line: &str) -> bool {
    if line.starts_with('|') && line.matches('|').count() >= 2 {
        return true;
    }
    if line.starts_with('+') && line.ends_with('+') && (line.contains('-') || line.contains('=')) {
        return true;
    }
    try_parse_table_separator(line).is_some()
        || try_parse_pipe_separator(line).is_some()
        || try_parse_grid_separator(line).is_some()
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
    config: &ParserOptions,
) {
    builder.start_node(SyntaxKind::TABLE_CAPTION.into());

    for (i, line) in lines[start..end].iter().enumerate() {
        if i == 0 {
            // First line - parse and emit prefix separately
            let trimmed = line.trim_start();
            let leading_ws_len = line.len() - trimmed.len();

            // Emit leading whitespace if present
            if leading_ws_len > 0 {
                builder.token(SyntaxKind::WHITESPACE.into(), &line[..leading_ws_len]);
            }

            // Check for caption prefix and emit separately
            // Calculate where the prefix ends (after trimmed content)
            let prefix_and_rest = if line.ends_with('\n') {
                &line[leading_ws_len..line.len() - 1] // Exclude newline
            } else {
                &line[leading_ws_len..]
            };

            let (prefix_len, prefix_text) = if prefix_and_rest.starts_with("Table: ") {
                (7, "Table: ")
            } else if prefix_and_rest.starts_with("table: ") {
                (7, "table: ")
            } else if prefix_and_rest.starts_with(": ") {
                (2, ": ")
            } else if prefix_and_rest.starts_with(':') {
                (1, ":")
            } else {
                (0, "")
            };

            if prefix_len > 0 {
                builder.token(SyntaxKind::TABLE_CAPTION_PREFIX.into(), prefix_text);

                // Emit rest of line after prefix
                let rest_start = leading_ws_len + prefix_len;
                if rest_start < line.len() {
                    // Get the caption text (excluding newline)
                    let (caption_text, newline_str) = strip_newline(&line[rest_start..]);

                    if !caption_text.is_empty() {
                        inline_emission::emit_inlines(builder, caption_text, config);
                    }

                    if !newline_str.is_empty() {
                        builder.token(SyntaxKind::NEWLINE.into(), newline_str);
                    }
                }
            } else {
                // No recognized prefix, emit whole trimmed line
                let (text, newline_str) = strip_newline(&line[leading_ws_len..]);

                if !text.is_empty() {
                    inline_emission::emit_inlines(builder, text, config);
                }

                if !newline_str.is_empty() {
                    builder.token(SyntaxKind::NEWLINE.into(), newline_str);
                }
            }
        } else {
            // Continuation lines - emit with inline parsing
            let (text, newline_str) = strip_newline(line);

            if !text.is_empty() {
                inline_emission::emit_inlines(builder, text, config);
            }

            if !newline_str.is_empty() {
                builder.token(SyntaxKind::NEWLINE.into(), newline_str);
            }
        }
    }

    builder.finish_node(); // TABLE_CAPTION
}

/// Emit a table cell with inline content parsing.
/// This is the core helper for Phase 7.1 table inline parsing migration.
fn emit_table_cell(
    builder: &mut GreenNodeBuilder<'static>,
    cell_text: &str,
    config: &ParserOptions,
) {
    builder.start_node(SyntaxKind::TABLE_CELL.into());

    // Parse inline content within the cell
    if !cell_text.is_empty() {
        inline_emission::emit_inlines(builder, cell_text, config);
    }

    builder.finish_node(); // TABLE_CELL
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
    config: &ParserOptions,
) -> Option<usize> {
    log::debug!("try_parse_simple_table at line {}", start_pos + 1);

    if start_pos >= lines.len() {
        return None;
    }

    // Look for a separator line
    let separator_pos = find_separator_line(lines, start_pos)?;
    log::debug!("  found separator at line {}", separator_pos + 1);

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
    builder.start_node(SyntaxKind::SIMPLE_TABLE.into());

    // Emit caption before if present
    if let Some((cap_start, cap_end)) = caption_before {
        emit_table_caption(builder, lines, cap_start, cap_end, config);
    }

    // Emit header if present
    if has_header {
        emit_table_row(
            builder,
            lines[separator_pos - 1],
            &columns,
            SyntaxKind::TABLE_HEADER,
            config,
        );
    }

    // Emit separator
    builder.start_node(SyntaxKind::TABLE_SEPARATOR.into());
    emit_line_tokens(builder, separator_line);
    builder.finish_node();

    // Emit data rows
    for line in lines.iter().take(end_pos).skip(separator_pos + 1) {
        emit_table_row(builder, line, &columns, SyntaxKind::TABLE_ROW, config);
    }

    // Emit caption after if present
    if let Some((cap_start, cap_end)) = caption_after {
        // Emit blank line before caption if needed
        if cap_start > end_pos {
            builder.start_node(SyntaxKind::BLANK_LINE.into());
            builder.token(SyntaxKind::BLANK_LINE.into(), "\n");
            builder.finish_node();
        }
        emit_table_caption(builder, lines, cap_start, cap_end, config);
    }

    builder.finish_node(); // SimpleTable

    // Calculate lines consumed (including captions)
    let table_start = if let Some((cap_start, _)) = caption_before {
        cap_start
    } else if has_header {
        separator_pos - 1
    } else {
        separator_pos
    };

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
    log::debug!("  find_separator_line from line {}", start_pos + 1);

    // Check first line
    log::debug!("    checking first line: {:?}", lines[start_pos]);
    if try_parse_table_separator(lines[start_pos]).is_some() {
        log::debug!("    separator found at first line");
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

/// Emit a table row (header or data row) with inline-parsed cells for simple tables.
/// Uses column boundaries from the separator line to extract cells.
fn emit_table_row(
    builder: &mut GreenNodeBuilder<'static>,
    line: &str,
    columns: &[Column],
    row_kind: SyntaxKind,
    config: &ParserOptions,
) {
    builder.start_node(row_kind.into());

    let (line_without_newline, newline_str) = strip_newline(line);

    // Emit leading whitespace if present
    let trimmed = line_without_newline.trim_start();
    let leading_ws_len = line_without_newline.len() - line_without_newline.trim_start().len();
    if leading_ws_len > 0 {
        builder.token(
            SyntaxKind::WHITESPACE.into(),
            &line_without_newline[..leading_ws_len],
        );
    }

    // Track where we are in the line (for losslessness)
    let mut current_pos = 0;

    // Extract and emit cells based on column boundaries
    for col in columns.iter() {
        // Calculate actual positions in the trimmed line (accounting for leading whitespace)
        let cell_start = if col.start >= leading_ws_len {
            (col.start - leading_ws_len).min(trimmed.len())
        } else {
            0
        };

        let cell_end = if col.end >= leading_ws_len {
            (col.end - leading_ws_len).min(trimmed.len())
        } else {
            0
        };

        // Extract cell text from column bounds
        let cell_text = if cell_start < cell_end && cell_start < trimmed.len() {
            &trimmed[cell_start..cell_end]
        } else if cell_start < trimmed.len() {
            &trimmed[cell_start..]
        } else {
            ""
        };

        let cell_content = cell_text.trim();
        let cell_content_start = cell_text.len() - cell_text.trim_start().len();

        // Emit any whitespace from current position to start of cell content
        let content_abs_pos = (cell_start + cell_content_start).min(trimmed.len());
        if current_pos < content_abs_pos {
            builder.token(
                SyntaxKind::WHITESPACE.into(),
                &trimmed[current_pos..content_abs_pos],
            );
        }

        // Emit cell with inline parsing
        emit_table_cell(builder, cell_content, config);

        // Update current position to end of cell content
        current_pos = content_abs_pos + cell_content.len();
    }

    // Emit any remaining whitespace after last cell
    if current_pos < trimmed.len() {
        builder.token(SyntaxKind::WHITESPACE.into(), &trimmed[current_pos..]);
    }

    // Emit newline if present
    if !newline_str.is_empty() {
        builder.token(SyntaxKind::NEWLINE.into(), newline_str);
    }

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
/// Handles escaped pipes (\|) properly by not splitting on them.
fn parse_pipe_table_row(line: &str) -> Vec<String> {
    let trimmed = line.trim();

    let mut cells = Vec::new();
    let mut current_cell = String::new();
    let mut chars = trimmed.chars().peekable();
    let mut char_count = 0;

    while let Some(ch) = chars.next() {
        char_count += 1;
        match ch {
            '\\' => {
                // Check if next char is a pipe - if so, it's an escaped pipe
                if let Some(&'|') = chars.peek() {
                    current_cell.push('\\');
                    current_cell.push('|');
                    chars.next(); // consume the pipe
                } else {
                    current_cell.push(ch);
                }
            }
            '|' => {
                // Check if this is the leading pipe (first character)
                if char_count == 1 {
                    continue; // Skip leading pipe
                }

                // End current cell, start new one
                cells.push(current_cell.trim().to_string());
                current_cell.clear();
            }
            _ => {
                current_cell.push(ch);
            }
        }
    }

    // Add last cell if it's not empty (it would be empty if line ended with pipe)
    let trimmed_cell = current_cell.trim().to_string();
    if !trimmed_cell.is_empty() {
        cells.push(trimmed_cell);
    }

    cells
}

/// Emit a pipe table row with inline-parsed cells.
/// Preserves losslessness by emitting exact byte representation while parsing cell content inline.
fn emit_pipe_table_row(
    builder: &mut GreenNodeBuilder<'static>,
    line: &str,
    row_kind: SyntaxKind,
    config: &ParserOptions,
) {
    builder.start_node(row_kind.into());

    let (line_without_newline, newline_str) = strip_newline(line);
    let trimmed = line_without_newline.trim();

    // Parse cell boundaries
    let mut cell_starts = Vec::new();
    let mut cell_ends = Vec::new();
    let mut in_escape = false;

    // Find all pipe positions (excluding escaped ones)
    let mut pipe_positions = Vec::new();
    for (i, ch) in trimmed.char_indices() {
        if in_escape {
            in_escape = false;
            continue;
        }
        if ch == '\\' {
            in_escape = true;
            continue;
        }
        if ch == '|' {
            pipe_positions.push(i);
        }
    }

    // Determine cell boundaries based on pipe positions
    if pipe_positions.is_empty() {
        // No pipes - treat entire line as one cell (shouldn't happen for valid pipe tables)
        cell_starts.push(0);
        cell_ends.push(trimmed.len());
    } else {
        // Check if line starts with pipe
        let start_pipe = pipe_positions.first() == Some(&0);
        // Check if line ends with pipe
        let end_pipe = pipe_positions.last() == Some(&(trimmed.len() - 1));

        if start_pipe {
            // Skip first pipe
            for i in 1..pipe_positions.len() {
                cell_starts.push(pipe_positions[i - 1] + 1);
                cell_ends.push(pipe_positions[i]);
            }
            // Add last cell if there's no trailing pipe
            if !end_pipe {
                cell_starts.push(*pipe_positions.last().unwrap() + 1);
                cell_ends.push(trimmed.len());
            }
        } else {
            // No leading pipe
            cell_starts.push(0);
            cell_ends.push(pipe_positions[0]);

            for i in 1..pipe_positions.len() {
                cell_starts.push(pipe_positions[i - 1] + 1);
                cell_ends.push(pipe_positions[i]);
            }

            // Add last cell if there's no trailing pipe
            if !end_pipe {
                cell_starts.push(*pipe_positions.last().unwrap() + 1);
                cell_ends.push(trimmed.len());
            }
        }
    }

    // Emit leading whitespace if present (before trim)
    let leading_ws_len = line_without_newline.len() - line_without_newline.trim_start().len();
    if leading_ws_len > 0 {
        builder.token(
            SyntaxKind::WHITESPACE.into(),
            &line_without_newline[..leading_ws_len],
        );
    }

    // Emit cells with pipes
    for (idx, (start, end)) in cell_starts.iter().zip(cell_ends.iter()).enumerate() {
        // Emit pipe before cell (except for first cell if no leading pipe)
        if *start > 0 {
            builder.token(SyntaxKind::TEXT.into(), "|");
        } else if idx == 0 && trimmed.starts_with('|') {
            // Leading pipe
            builder.token(SyntaxKind::TEXT.into(), "|");
        }

        // Get cell content with its whitespace
        let cell_with_ws = &trimmed[*start..*end];
        let cell_content = cell_with_ws.trim();

        // Emit leading whitespace within cell
        let cell_leading_ws = &cell_with_ws[..cell_with_ws.len() - cell_with_ws.trim_start().len()];
        if !cell_leading_ws.is_empty() {
            builder.token(SyntaxKind::WHITESPACE.into(), cell_leading_ws);
        }

        // Emit cell with inline parsing
        emit_table_cell(builder, cell_content, config);

        // Emit trailing whitespace within cell
        let cell_trailing_ws_start = cell_leading_ws.len() + cell_content.len();
        if cell_trailing_ws_start < cell_with_ws.len() {
            builder.token(
                SyntaxKind::WHITESPACE.into(),
                &cell_with_ws[cell_trailing_ws_start..],
            );
        }
    }

    // Emit trailing pipe if present
    if !pipe_positions.is_empty() && trimmed.ends_with('|') {
        builder.token(SyntaxKind::TEXT.into(), "|");
    }

    // Emit trailing whitespace after trim (before newline)
    let trailing_ws_start = leading_ws_len + trimmed.len();
    if trailing_ws_start < line_without_newline.len() {
        builder.token(
            SyntaxKind::WHITESPACE.into(),
            &line_without_newline[trailing_ws_start..],
        );
    }

    // Emit newline
    if !newline_str.is_empty() {
        builder.token(SyntaxKind::NEWLINE.into(), newline_str);
    }

    builder.finish_node();
}

/// Try to parse a pipe table starting at the given position.
/// Returns the number of lines consumed if successful.
pub(crate) fn try_parse_pipe_table(
    lines: &[&str],
    start_pos: usize,
    builder: &mut GreenNodeBuilder<'static>,
    config: &ParserOptions,
) -> Option<usize> {
    if start_pos + 1 >= lines.len() {
        return None;
    }

    // Check if this line is a caption followed by a table
    // If so, the actual table starts after the caption and blank line
    let (actual_start, has_caption_before) = if is_caption_followed_by_table(lines, start_pos) {
        // Skip caption line
        let mut pos = start_pos + 1;
        // Skip blank line if present
        while pos < lines.len() && lines[pos].trim().is_empty() {
            pos += 1;
        }
        (pos, true)
    } else {
        (start_pos, false)
    };

    if actual_start + 1 >= lines.len() {
        return None;
    }

    // First line should have pipes (potential header)
    let header_line = lines[actual_start];
    if !header_line.contains('|') {
        return None;
    }

    // Second line should be separator
    let separator_line = lines[actual_start + 1];
    let alignments = try_parse_pipe_separator(separator_line)?;

    // Parse header cells
    let header_cells = parse_pipe_table_row(header_line);

    // Number of columns should match (approximately - be lenient)
    if header_cells.len() != alignments.len() && !header_cells.is_empty() {
        // Only fail if very different
        if header_cells.len() < alignments.len() / 2 || header_cells.len() > alignments.len() * 2 {
            return None;
        }
    }

    // Find table end (first blank line or end of input)
    let mut end_pos = actual_start + 2;
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
    if end_pos <= actual_start + 2 {
        return None;
    }

    // Check for caption before table (only if we didn't already detect it)
    let caption_before = if has_caption_before {
        Some((start_pos, start_pos + 1)) // Single-line caption detected earlier
    } else {
        find_caption_before_table(lines, actual_start)
    };

    // Check for caption after table
    let caption_after = find_caption_after_table(lines, end_pos);

    // Build the pipe table
    builder.start_node(SyntaxKind::PIPE_TABLE.into());

    // Emit caption before if present
    if let Some((cap_start, cap_end)) = caption_before {
        emit_table_caption(builder, lines, cap_start, cap_end, config);
        // Emit blank line between caption and table if present
        if cap_end < actual_start {
            for line in lines.iter().take(actual_start).skip(cap_end) {
                if line.trim().is_empty() {
                    builder.start_node(SyntaxKind::BLANK_LINE.into());
                    builder.token(SyntaxKind::BLANK_LINE.into(), line);
                    builder.finish_node();
                }
            }
        }
    }

    // Emit header row with inline-parsed cells
    emit_pipe_table_row(builder, header_line, SyntaxKind::TABLE_HEADER, config);

    // Emit separator
    builder.start_node(SyntaxKind::TABLE_SEPARATOR.into());
    emit_line_tokens(builder, separator_line);
    builder.finish_node();

    // Emit data rows with inline-parsed cells
    for line in lines.iter().take(end_pos).skip(actual_start + 2) {
        emit_pipe_table_row(builder, line, SyntaxKind::TABLE_ROW, config);
    }

    // Emit caption after if present
    if let Some((cap_start, cap_end)) = caption_after {
        // Emit blank line before caption if needed
        if cap_start > end_pos {
            builder.start_node(SyntaxKind::BLANK_LINE.into());
            builder.token(SyntaxKind::BLANK_LINE.into(), "\n");
            builder.finish_node();
        }
        emit_table_caption(builder, lines, cap_start, cap_end, config);
    }

    builder.finish_node(); // PipeTable

    // Calculate lines consumed
    let table_start = caption_before
        .map(|(start, _)| start)
        .unwrap_or(actual_start);
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
        let result = try_parse_simple_table(&input, 0, &mut builder, &ParserOptions::default());

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
        let result = try_parse_simple_table(&input, 0, &mut builder, &ParserOptions::default());

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
    fn bare_colon_fenced_code_is_not_table_caption() {
        let input = "Term\n: ```\n  code\n  ```\n";
        let tree = crate::parse(input, None);

        assert!(
            tree.descendants()
                .any(|node| node.kind() == SyntaxKind::DEFINITION_LIST),
            "should parse as definition list"
        );
        assert!(
            tree.descendants()
                .any(|node| node.kind() == SyntaxKind::CODE_BLOCK),
            "definition should preserve fenced code block"
        );
        assert!(
            !tree
                .descendants()
                .any(|node| node.kind() == SyntaxKind::TABLE_CAPTION),
            "fenced code definition should not be parsed as table caption"
        );
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
        let result = try_parse_simple_table(&input, 0, &mut builder, &ParserOptions::default());

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
        let result = try_parse_simple_table(&input, 2, &mut builder, &ParserOptions::default());

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
        let result = try_parse_simple_table(&input, 0, &mut builder, &ParserOptions::default());

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
        let result = try_parse_simple_table(&input, 0, &mut builder, &ParserOptions::default());

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
    fn test_parse_pipe_table_row() {
        let cells = parse_pipe_table_row("| Right | Left | Center |");
        assert_eq!(cells.len(), 3);
        assert_eq!(cells[0], "Right");
        assert_eq!(cells[1], "Left");
        assert_eq!(cells[2], "Center");

        // Without leading/trailing pipes
        let cells2 = parse_pipe_table_row("Right | Left | Center");
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
        let result = try_parse_pipe_table(&input, 1, &mut builder, &ParserOptions::default());

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
        let result = try_parse_pipe_table(&input, 1, &mut builder, &ParserOptions::default());

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
        let result = try_parse_pipe_table(&input, 1, &mut builder, &ParserOptions::default());

        assert!(result.is_some());
        assert_eq!(result.unwrap(), 5); // header + sep + row + blank + caption
    }
}

// ============================================================================
// Grid Table Parsing
// ============================================================================

/// Check if a line is a grid table row separator (starts with +, contains -, ends with +).
/// Returns Some(vec of column info) if valid, None otherwise.
fn try_parse_grid_separator(line: &str) -> Option<Vec<GridColumn>> {
    let trimmed = line.trim_start();
    let leading_spaces = line.len() - trimmed.len();

    // Must have leading spaces <= 3 to not be a code block
    if leading_spaces > 3 {
        return None;
    }

    // Must start with + and end with +
    if !trimmed.starts_with('+') || !trimmed.trim_end().ends_with('+') {
        return None;
    }

    // Split by + to get column segments
    let trimmed = trimmed.trim_end();
    let segments: Vec<&str> = trimmed.split('+').collect();

    // Need at least 3 parts: empty before first +, column(s), empty after last +
    if segments.len() < 3 {
        return None;
    }

    let mut columns = Vec::new();

    // Parse each segment between + signs
    for segment in segments.iter().skip(1).take(segments.len() - 2) {
        if segment.is_empty() {
            continue;
        }

        // Segment must be dashes/equals with optional colons for alignment
        let seg_trimmed = *segment;

        // Get the fill character (after removing colons)
        let inner = seg_trimmed.trim_start_matches(':').trim_end_matches(':');

        // Must be all dashes or all equals
        if inner.is_empty() {
            return None;
        }

        let first_char = inner.chars().next().unwrap();
        if first_char != '-' && first_char != '=' {
            return None;
        }

        if !inner.chars().all(|c| c == first_char) {
            return None;
        }

        let is_header_sep = first_char == '=';

        columns.push(GridColumn {
            is_header_separator: is_header_sep,
            width: seg_trimmed.chars().count(),
        });
    }

    if columns.is_empty() {
        None
    } else {
        Some(columns)
    }
}

/// Column information for grid tables.
#[derive(Debug, Clone)]
struct GridColumn {
    is_header_separator: bool,
    width: usize,
}

fn slice_cell_by_display_width(line: &str, start_byte: usize, width: usize) -> (usize, usize) {
    let mut end_byte = start_byte;
    let mut display_cols = 0usize;

    for (offset, ch) in line[start_byte..].char_indices() {
        if ch == '|' {
            let sep_byte = start_byte + offset;
            return (sep_byte, sep_byte + 1);
        }
        let ch_width = UnicodeWidthChar::width(ch).unwrap_or(0);
        if display_cols + ch_width > width {
            break;
        }
        display_cols += ch_width;
        end_byte = start_byte + offset + ch.len_utf8();
        if display_cols >= width {
            break;
        }
    }

    // If the width budget is exhausted before seeing a separator (for example
    // because of padding/layout drift), advance to the next literal separator
    // to keep row slicing aligned and preserve losslessness.
    let mut sep_byte = end_byte;
    while sep_byte < line.len() {
        let mut chars = line[sep_byte..].chars();
        let Some(ch) = chars.next() else {
            break;
        };
        if ch == '|' {
            return (sep_byte, sep_byte + 1);
        }
        sep_byte += ch.len_utf8();
    }

    (end_byte, end_byte)
}

/// Check if a line is a grid table content row.
/// Accepts normal rows ending with `|` and spanning-style continuation lines ending with `+`.
fn is_grid_content_row(line: &str) -> bool {
    let trimmed = line.trim_start();
    let leading_spaces = line.len() - trimmed.len();

    if leading_spaces > 3 {
        return false;
    }

    let trimmed = trimmed.trim_end();
    trimmed.starts_with('|') && (trimmed.ends_with('|') || trimmed.ends_with('+'))
}

/// Extract cell contents from a single grid table row line.
/// Returns a vector of cell contents (trimmed) based on column boundaries.
/// Grid table rows look like: "| Cell 1 | Cell 2 | Cell 3 |"
fn extract_grid_cells_from_line(line: &str, _columns: &[GridColumn]) -> Vec<String> {
    let (line_content, _) = strip_newline(line);
    let line_trimmed = line_content.trim();

    if !line_trimmed.starts_with('|') || !line_trimmed.ends_with('|') {
        return vec![String::new(); _columns.len()];
    }

    let mut cells = Vec::with_capacity(_columns.len());
    let mut pos_byte = 1; // Skip leading pipe

    for col in _columns {
        let col_idx = cells.len();
        if pos_byte >= line_trimmed.len() {
            cells.push(String::new());
            continue;
        }

        let start_byte = pos_byte;
        let end_byte = if col_idx + 1 == _columns.len() {
            line_trimmed.len().saturating_sub(1) // consume to trailing pipe for last column
        } else {
            let (end, next_start) = slice_cell_by_display_width(line_trimmed, pos_byte, col.width);
            pos_byte = next_start;
            end
        };
        cells.push(line_trimmed[start_byte..end_byte].trim().to_string());
        if col_idx + 1 == _columns.len() {
            pos_byte = line_trimmed.len();
        }
    }

    cells
}

/// Extract cell contents from multiple grid table row lines (for multi-line cells).
/// Concatenates cell contents across lines with newlines, then trims.
fn extract_grid_cells_multiline(lines: &[&str], columns: &[GridColumn]) -> Vec<String> {
    if lines.is_empty() {
        return vec![String::new(); columns.len()];
    }

    extract_grid_cells_from_line(lines[0], columns)
}

/// Emit a grid table row with inline-parsed cells.
/// Handles multi-line rows by emitting first line with TABLE_CELL nodes,
/// then continuation lines as raw TEXT for losslessness.
fn emit_grid_table_row(
    builder: &mut GreenNodeBuilder<'static>,
    lines: &[&str],
    columns: &[GridColumn],
    row_kind: SyntaxKind,
    config: &ParserOptions,
) {
    if lines.is_empty() {
        return;
    }

    // Extract cell contents from the first line.
    let cell_contents = extract_grid_cells_multiline(lines, columns);

    builder.start_node(row_kind.into());

    // Emit first line with TABLE_CELL nodes
    // Grid table rows look like: "| Cell 1 | Cell 2 | Cell 3 |"
    let first_line = lines[0];
    let (line_without_newline, newline_str) = strip_newline(first_line);
    let trimmed = line_without_newline.trim();
    let expected_pipe_count = columns.len().saturating_add(1);
    let actual_pipe_count = trimmed.chars().filter(|&c| c == '|').count();

    // Rows that don't contain all expected column separators (spanning-style rows)
    // must be emitted verbatim for losslessness.
    if actual_pipe_count != expected_pipe_count {
        emit_line_tokens(builder, first_line);
        for line in lines.iter().skip(1) {
            emit_line_tokens(builder, line);
        }
        builder.finish_node();
        return;
    }

    // Emit leading whitespace
    let leading_ws_len = line_without_newline.len() - line_without_newline.trim_start().len();
    if leading_ws_len > 0 {
        builder.token(
            SyntaxKind::WHITESPACE.into(),
            &line_without_newline[..leading_ws_len],
        );
    }

    // Emit leading pipe
    if trimmed.starts_with('|') {
        builder.token(SyntaxKind::TEXT.into(), "|");
    }

    // Emit each cell based on fixed column widths from separators
    let mut pos_byte = 1usize; // after leading pipe
    for (idx, cell_content) in cell_contents.iter().enumerate() {
        let part = if idx < columns.len() && pos_byte <= trimmed.len() {
            let start_byte = pos_byte;
            let end_byte = if idx + 1 == columns.len() && !trimmed.is_empty() {
                trimmed.len().saturating_sub(1) // consume to trailing pipe for last column
            } else {
                let (end, next_start) =
                    slice_cell_by_display_width(trimmed, pos_byte, columns[idx].width);
                pos_byte = next_start;
                end
            };
            let slice = &trimmed[start_byte..end_byte];
            if idx + 1 == columns.len() {
                pos_byte = trimmed.len();
            }
            slice
        } else {
            ""
        };

        // Emit leading whitespace in cell
        let cell_trimmed = part.trim();
        let ws_start_len = part.len() - part.trim_start().len();
        if ws_start_len > 0 {
            builder.token(SyntaxKind::WHITESPACE.into(), &part[..ws_start_len]);
        }

        // Emit TABLE_CELL with inline parsing
        emit_table_cell(builder, cell_content, config);

        // Emit trailing whitespace in cell
        let ws_end_start = ws_start_len + cell_trimmed.len();
        if ws_end_start < part.len() {
            builder.token(SyntaxKind::WHITESPACE.into(), &part[ws_end_start..]);
        }

        // Emit pipe separator (unless this is the last cell and line doesn't end with |)
        if idx < cell_contents.len() - 1 || trimmed.ends_with('|') {
            builder.token(SyntaxKind::TEXT.into(), "|");
        }
    }

    // Emit trailing whitespace before newline
    let trailing_ws_start = leading_ws_len + trimmed.len();
    if trailing_ws_start < line_without_newline.len() {
        builder.token(
            SyntaxKind::WHITESPACE.into(),
            &line_without_newline[trailing_ws_start..],
        );
    }

    // Emit newline
    if !newline_str.is_empty() {
        builder.token(SyntaxKind::NEWLINE.into(), newline_str);
    }

    // Emit continuation lines as TEXT for losslessness
    for line in lines.iter().skip(1) {
        emit_line_tokens(builder, line);
    }

    builder.finish_node();
}

/// Try to parse a grid table starting at the given position.
/// Returns the number of lines consumed if successful.
pub(crate) fn try_parse_grid_table(
    lines: &[&str],
    start_pos: usize,
    builder: &mut GreenNodeBuilder<'static>,
    config: &ParserOptions,
) -> Option<usize> {
    if start_pos >= lines.len() {
        return None;
    }

    // Check if this line is a caption followed by a table
    // If so, the actual table starts after the caption and blank line
    let (actual_start, has_caption_before) = if is_caption_followed_by_table(lines, start_pos) {
        // Skip caption line
        let mut pos = start_pos + 1;
        // Skip blank line if present
        while pos < lines.len() && lines[pos].trim().is_empty() {
            pos += 1;
        }
        (pos, true)
    } else {
        (start_pos, false)
    };

    if actual_start >= lines.len() {
        return None;
    }

    // First line must be a grid separator
    let first_line = lines[actual_start];
    let _columns = try_parse_grid_separator(first_line)?;

    // Track table structure
    let mut end_pos = actual_start + 1;
    let mut found_header_sep = false;
    let mut in_footer = false;

    // Scan table lines
    while end_pos < lines.len() {
        let line = lines[end_pos];

        // Check for blank line (table ends)
        if line.trim().is_empty() {
            break;
        }

        // Check for separator line
        if let Some(sep_cols) = try_parse_grid_separator(line) {
            // Check if this is a header separator (=)
            if sep_cols.iter().any(|c| c.is_header_separator) {
                if !found_header_sep {
                    found_header_sep = true;
                } else if !in_footer {
                    // Second = separator starts footer
                    in_footer = true;
                }
            }
            end_pos += 1;
            continue;
        }

        // Check for content row
        if is_grid_content_row(line) {
            end_pos += 1;
            continue;
        }

        // Not a valid grid table line - table ends
        break;
    }

    // Must have consumed at least 3 lines (top separator, content, bottom separator)
    // Or just top + content rows that end with a separator
    if end_pos <= actual_start + 1 {
        return None;
    }

    // Last consumed line should be a separator for a well-formed table
    // But we'll be lenient and accept tables ending with content rows

    // Check for caption before table (only if we didn't already detected it)
    let caption_before = if has_caption_before {
        Some((start_pos, start_pos + 1)) // Single-line caption detected earlier
    } else {
        find_caption_before_table(lines, actual_start)
    };

    // Check for caption after table
    let caption_after = find_caption_after_table(lines, end_pos);

    // Build the grid table
    builder.start_node(SyntaxKind::GRID_TABLE.into());

    // Emit caption before if present
    if let Some((cap_start, cap_end)) = caption_before {
        emit_table_caption(builder, lines, cap_start, cap_end, config);
        // Emit blank line between caption and table if present
        if cap_end < actual_start {
            for line in lines.iter().take(actual_start).skip(cap_end) {
                if line.trim().is_empty() {
                    builder.start_node(SyntaxKind::BLANK_LINE.into());
                    builder.token(SyntaxKind::BLANK_LINE.into(), line);
                    builder.finish_node();
                }
            }
        }
    }

    // Track whether we've passed the header separator
    let mut past_header_sep = false;
    let mut in_footer_section = false;
    let mut current_row_lines: Vec<&str> = Vec::new();
    let mut current_row_kind = SyntaxKind::TABLE_HEADER;

    // Emit table rows - accumulate multi-line cells
    for line in lines.iter().take(end_pos).skip(actual_start) {
        if let Some(sep_cols) = try_parse_grid_separator(line) {
            // Separator line - emit any accumulated row first
            if !current_row_lines.is_empty() {
                emit_grid_table_row(
                    builder,
                    &current_row_lines,
                    &sep_cols,
                    current_row_kind,
                    config,
                );
                current_row_lines.clear();
            }

            let is_header_sep = sep_cols.iter().any(|c| c.is_header_separator);

            if is_header_sep {
                if !past_header_sep {
                    // This is the header/body separator
                    builder.start_node(SyntaxKind::TABLE_SEPARATOR.into());
                    emit_line_tokens(builder, line);
                    builder.finish_node();
                    past_header_sep = true;
                } else {
                    // Footer separator
                    if !in_footer_section {
                        in_footer_section = true;
                    }
                    builder.start_node(SyntaxKind::TABLE_SEPARATOR.into());
                    emit_line_tokens(builder, line);
                    builder.finish_node();
                }
            } else {
                // Regular separator (row boundary)
                builder.start_node(SyntaxKind::TABLE_SEPARATOR.into());
                emit_line_tokens(builder, line);
                builder.finish_node();
            }
        } else if is_grid_content_row(line) {
            // Content row - accumulate for multi-line cells
            current_row_kind = if !past_header_sep && found_header_sep {
                SyntaxKind::TABLE_HEADER
            } else if in_footer_section {
                SyntaxKind::TABLE_FOOTER
            } else {
                SyntaxKind::TABLE_ROW
            };

            current_row_lines.push(line);
        }
    }

    // Emit any remaining accumulated row
    if !current_row_lines.is_empty() {
        // Use first separator's columns for cell boundaries
        if let Some(sep_cols) = try_parse_grid_separator(lines[actual_start]) {
            emit_grid_table_row(
                builder,
                &current_row_lines,
                &sep_cols,
                current_row_kind,
                config,
            );
        }
    }

    // Emit caption after if present
    if let Some((cap_start, cap_end)) = caption_after {
        if cap_start > end_pos {
            builder.start_node(SyntaxKind::BLANK_LINE.into());
            builder.token(SyntaxKind::BLANK_LINE.into(), "\n");
            builder.finish_node();
        }
        emit_table_caption(builder, lines, cap_start, cap_end, config);
    }

    builder.finish_node(); // GRID_TABLE

    // Calculate lines consumed
    let table_start = caption_before
        .map(|(start, _)| start)
        .unwrap_or(actual_start);
    let table_end = if let Some((_, cap_end)) = caption_after {
        cap_end
    } else {
        end_pos
    };

    Some(table_end - table_start)
}

#[cfg(test)]
mod grid_table_tests {
    use super::*;

    #[test]
    fn test_grid_separator_detection() {
        assert!(try_parse_grid_separator("+---+---+").is_some());
        assert!(try_parse_grid_separator("+===+===+").is_some());
        assert!(try_parse_grid_separator("+---------------+---------------+").is_some());
        assert!(try_parse_grid_separator("+:---:+").is_some()); // center aligned
        assert!(try_parse_grid_separator("not a separator").is_none());
        assert!(try_parse_grid_separator("|---|---|").is_none()); // pipe table sep
    }

    #[test]
    fn test_grid_header_separator() {
        let cols = try_parse_grid_separator("+===+===+").unwrap();
        assert!(cols.iter().all(|c| c.is_header_separator));

        let cols2 = try_parse_grid_separator("+---+---+").unwrap();
        assert!(cols2.iter().all(|c| !c.is_header_separator));
    }

    #[test]
    fn test_grid_content_row_detection() {
        assert!(is_grid_content_row("| content | content |"));
        assert!(is_grid_content_row("|  |  |"));
        assert!(is_grid_content_row("| content +------+"));
        assert!(!is_grid_content_row("+---+---+")); // separator, not content
        assert!(!is_grid_content_row("no pipes here"));
    }

    #[test]
    fn test_basic_grid_table() {
        let input = vec![
            "+-------+-------+",
            "| Col1  | Col2  |",
            "+=======+=======+",
            "| A     | B     |",
            "+-------+-------+",
            "",
        ];

        let mut builder = GreenNodeBuilder::new();
        let result = try_parse_grid_table(&input, 0, &mut builder, &ParserOptions::default());

        assert!(result.is_some());
        assert_eq!(result.unwrap(), 5);
    }

    #[test]
    fn test_grid_table_multirow() {
        let input = vec![
            "+---------------+---------------+",
            "| Fruit         | Advantages    |",
            "+===============+===============+",
            "| Bananas       | - wrapper     |",
            "|               | - color       |",
            "+---------------+---------------+",
            "| Oranges       | - scurvy      |",
            "|               | - tasty       |",
            "+---------------+---------------+",
            "",
        ];

        let mut builder = GreenNodeBuilder::new();
        let result = try_parse_grid_table(&input, 0, &mut builder, &ParserOptions::default());

        assert!(result.is_some());
        assert_eq!(result.unwrap(), 9);
    }

    #[test]
    fn test_grid_table_with_footer() {
        let input = vec![
            "+-------+-------+",
            "| Fruit | Price |",
            "+=======+=======+",
            "| Apple | $1.00 |",
            "+-------+-------+",
            "| Pear  | $1.50 |",
            "+=======+=======+",
            "| Total | $2.50 |",
            "+=======+=======+",
            "",
        ];

        let mut builder = GreenNodeBuilder::new();
        let result = try_parse_grid_table(&input, 0, &mut builder, &ParserOptions::default());

        assert!(result.is_some());
        assert_eq!(result.unwrap(), 9);
    }

    #[test]
    fn test_grid_table_headerless() {
        let input = vec![
            "+-------+-------+",
            "| A     | B     |",
            "+-------+-------+",
            "| C     | D     |",
            "+-------+-------+",
            "",
        ];

        let mut builder = GreenNodeBuilder::new();
        let result = try_parse_grid_table(&input, 0, &mut builder, &ParserOptions::default());

        assert!(result.is_some());
        assert_eq!(result.unwrap(), 5);
    }

    #[test]
    fn test_grid_table_with_caption_before() {
        let input = vec![
            ": Sample table",
            "",
            "+-------+-------+",
            "| A     | B     |",
            "+=======+=======+",
            "| C     | D     |",
            "+-------+-------+",
            "",
        ];

        let mut builder = GreenNodeBuilder::new();
        let result = try_parse_grid_table(&input, 2, &mut builder, &ParserOptions::default());

        assert!(result.is_some());
        // Should include caption + blank + table
        assert_eq!(result.unwrap(), 7);
    }

    #[test]
    fn test_grid_table_with_caption_after() {
        let input = vec![
            "+-------+-------+",
            "| A     | B     |",
            "+=======+=======+",
            "| C     | D     |",
            "+-------+-------+",
            "",
            "Table: My grid table",
            "",
        ];

        let mut builder = GreenNodeBuilder::new();
        let result = try_parse_grid_table(&input, 0, &mut builder, &ParserOptions::default());

        assert!(result.is_some());
        // table + blank + caption
        assert_eq!(result.unwrap(), 7);
    }
}

// ============================================================================
// Multiline Table Parsing
// ============================================================================

/// Check if a line is a multiline table separator (continuous dashes).
/// Multiline table separators span the full width and are all dashes.
/// Returns Some(columns) if valid, None otherwise.
fn try_parse_multiline_separator(line: &str) -> Option<Vec<Column>> {
    let trimmed = line.trim_start();
    let leading_spaces = line.len() - trimmed.len();

    // Must have leading spaces <= 3 to not be a code block
    if leading_spaces > 3 {
        return None;
    }

    let trimmed = trimmed.trim_end();

    // Must be all dashes (continuous line of dashes)
    if trimmed.is_empty() || !trimmed.chars().all(|c| c == '-') {
        return None;
    }

    // Must have at least 3 dashes
    if trimmed.len() < 3 {
        return None;
    }

    // This is a full-width separator - columns will be determined by column separator lines
    Some(vec![Column {
        start: leading_spaces,
        end: leading_spaces + trimmed.len(),
        alignment: Alignment::Default,
    }])
}

/// Check if a line is a column separator line for multiline tables.
/// Column separators have dashes with spaces between them to define columns.
fn is_column_separator(line: &str) -> bool {
    try_parse_table_separator(line).is_some() && !line.contains('*') && !line.contains('_')
}

fn is_headerless_single_row_without_blank(
    lines: &[&str],
    row_start: usize,
    row_end: usize,
    columns: &[Column],
) -> bool {
    if row_start >= row_end {
        return false;
    }

    if row_end - row_start == 1 {
        return false;
    }

    let Some(last_col) = columns.last() else {
        return false;
    };

    for line in lines.iter().take(row_end).skip(row_start + 1) {
        let (content, _) = strip_newline(line);
        let prefix_end = last_col.start.min(content.len());
        if !content[..prefix_end].trim().is_empty() {
            return false;
        }
    }

    true
}

/// Try to parse a multiline table starting at the given position.
/// Returns the number of lines consumed if successful.
pub(crate) fn try_parse_multiline_table(
    lines: &[&str],
    start_pos: usize,
    builder: &mut GreenNodeBuilder<'static>,
    config: &ParserOptions,
) -> Option<usize> {
    if start_pos >= lines.len() {
        return None;
    }

    let first_line = lines[start_pos];

    // First line can be either:
    // 1. A full-width dash separator (for tables with headers)
    // 2. A column separator (for headerless tables)
    let is_full_width_start = try_parse_multiline_separator(first_line).is_some();
    let is_column_sep_start = !is_full_width_start && is_column_separator(first_line);
    let headerless_columns = if is_column_sep_start {
        try_parse_table_separator(first_line)
    } else {
        None
    };

    if !is_full_width_start && !is_column_sep_start {
        return None;
    }

    // Look ahead to find the structure
    let mut pos = start_pos + 1;
    let mut found_column_sep = is_column_sep_start; // Already found if headerless
    let mut column_sep_pos = if is_column_sep_start { start_pos } else { 0 };
    let mut has_header = false;
    let mut found_blank_line = false;
    let mut found_closing_sep = false;
    let mut content_line_count = 0usize;

    // Scan for header section and column separator
    while pos < lines.len() {
        let line = lines[pos];

        // Check for column separator (defines columns) - only if we started with full-width
        if is_full_width_start && is_column_separator(line) && !found_column_sep {
            found_column_sep = true;
            column_sep_pos = pos;
            has_header = pos > start_pos + 1; // Has header if there's content before column sep
            pos += 1;
            continue;
        }

        // Check for blank line (row separator in body)
        if line.trim().is_empty() {
            found_blank_line = true;
            pos += 1;
            // Check if next line is a valid closing separator for this table shape.
            if pos < lines.len() {
                let next = lines[pos];
                let is_valid_closer = if is_full_width_start {
                    try_parse_multiline_separator(next).is_some()
                } else {
                    is_column_separator(next)
                };
                if is_valid_closer {
                    found_closing_sep = true;
                    pos += 1; // Include the closing separator
                    break;
                }
            }
            continue;
        }

        // Check for closing full-width dashes (only for full-width-start tables).
        if is_full_width_start && try_parse_multiline_separator(line).is_some() {
            found_closing_sep = true;
            pos += 1;
            break;
        }

        // Check for closing column separator (for headerless tables)
        if is_column_sep_start && is_column_separator(line) && content_line_count > 0 {
            found_closing_sep = true;
            pos += 1;
            break;
        }

        // Content row
        content_line_count += 1;
        pos += 1;
    }

    // Must have found a column separator to be a valid multiline table
    if !found_column_sep {
        return None;
    }

    // Must have had at least one blank line between rows (distinguishes from simple tables)
    if !found_blank_line {
        if !is_column_sep_start {
            return None;
        }
        let columns = headerless_columns.as_deref()?;
        if !is_headerless_single_row_without_blank(lines, start_pos + 1, pos - 1, columns) {
            return None;
        }
    }

    // Must have a closing separator
    if !found_closing_sep {
        return None;
    }

    // Must have consumed more than just the opening separator
    if pos <= start_pos + 2 {
        return None;
    }

    let end_pos = pos;

    // Extract column boundaries from the separator line
    let columns =
        try_parse_table_separator(lines[column_sep_pos]).expect("Column separator must be valid");

    // Check for caption before table
    let caption_before = find_caption_before_table(lines, start_pos);

    // Check for caption after table
    let caption_after = find_caption_after_table(lines, end_pos);

    // Build the multiline table
    builder.start_node(SyntaxKind::MULTILINE_TABLE.into());

    // Emit caption before if present
    if let Some((cap_start, cap_end)) = caption_before {
        emit_table_caption(builder, lines, cap_start, cap_end, config);

        // Emit blank line between caption and table if present
        if cap_end < start_pos {
            for line in lines.iter().take(start_pos).skip(cap_end) {
                if line.trim().is_empty() {
                    builder.start_node(SyntaxKind::BLANK_LINE.into());
                    builder.token(SyntaxKind::BLANK_LINE.into(), line);
                    builder.finish_node();
                }
            }
        }
    }

    // Emit opening separator
    builder.start_node(SyntaxKind::TABLE_SEPARATOR.into());
    emit_line_tokens(builder, lines[start_pos]);
    builder.finish_node();

    // Track state for emitting
    let mut in_header = has_header;
    let mut current_row_lines: Vec<&str> = Vec::new();

    for (i, line) in lines.iter().enumerate().take(end_pos).skip(start_pos + 1) {
        // Column separator (header/body divider)
        if i == column_sep_pos {
            // Emit any accumulated header lines
            if !current_row_lines.is_empty() {
                emit_multiline_table_row(
                    builder,
                    &current_row_lines,
                    &columns,
                    SyntaxKind::TABLE_HEADER,
                    config,
                );
                current_row_lines.clear();
            }

            builder.start_node(SyntaxKind::TABLE_SEPARATOR.into());
            emit_line_tokens(builder, line);
            builder.finish_node();
            in_header = false;
            continue;
        }

        // Closing separator (full-width or column separator at end)
        if try_parse_multiline_separator(line).is_some() || is_column_separator(line) {
            // Emit any accumulated row lines
            if !current_row_lines.is_empty() {
                let kind = if in_header {
                    SyntaxKind::TABLE_HEADER
                } else {
                    SyntaxKind::TABLE_ROW
                };
                emit_multiline_table_row(builder, &current_row_lines, &columns, kind, config);
                current_row_lines.clear();
            }

            builder.start_node(SyntaxKind::TABLE_SEPARATOR.into());
            emit_line_tokens(builder, line);
            builder.finish_node();
            continue;
        }

        // Blank line (row separator)
        if line.trim().is_empty() {
            // Emit accumulated row
            if !current_row_lines.is_empty() {
                let kind = if in_header {
                    SyntaxKind::TABLE_HEADER
                } else {
                    SyntaxKind::TABLE_ROW
                };
                emit_multiline_table_row(builder, &current_row_lines, &columns, kind, config);
                current_row_lines.clear();
            }

            builder.start_node(SyntaxKind::BLANK_LINE.into());
            builder.token(SyntaxKind::BLANK_LINE.into(), "\n");
            builder.finish_node();
            continue;
        }

        // Content line - accumulate for current row
        current_row_lines.push(line);
    }

    // Emit any remaining accumulated lines
    if !current_row_lines.is_empty() {
        let kind = if in_header {
            SyntaxKind::TABLE_HEADER
        } else {
            SyntaxKind::TABLE_ROW
        };
        emit_multiline_table_row(builder, &current_row_lines, &columns, kind, config);
    }

    // Emit caption after if present
    if let Some((cap_start, cap_end)) = caption_after {
        if cap_start > end_pos {
            builder.start_node(SyntaxKind::BLANK_LINE.into());
            builder.token(SyntaxKind::BLANK_LINE.into(), "\n");
            builder.finish_node();
        }
        emit_table_caption(builder, lines, cap_start, cap_end, config);
    }

    builder.finish_node(); // MultilineTable

    // Calculate lines consumed
    let table_start = caption_before.map(|(start, _)| start).unwrap_or(start_pos);
    let table_end = if let Some((_, cap_end)) = caption_after {
        cap_end
    } else {
        end_pos
    };

    Some(table_end - table_start)
}

/// Extract cell contents from first line only (for CST emission).
/// Multi-line content will be in continuation TEXT tokens.
fn extract_first_line_cell_contents(line: &str, columns: &[Column]) -> Vec<String> {
    let (line_content, _) = strip_newline(line);
    let mut cells = Vec::new();

    for column in columns.iter() {
        // Extract FULL text for this column (including whitespace)
        let cell_text = if column.end <= line_content.len() {
            &line_content[column.start..column.end]
        } else if column.start < line_content.len() {
            &line_content[column.start..]
        } else {
            ""
        };

        cells.push(cell_text.to_string());
    }

    cells
}

/// Emit a multiline table row with inline parsing (Phase 7.1).
fn emit_multiline_table_row(
    builder: &mut GreenNodeBuilder<'static>,
    lines: &[&str],
    columns: &[Column],
    kind: SyntaxKind,
    config: &ParserOptions,
) {
    if lines.is_empty() {
        return;
    }

    // Extract cell contents from first line only (for CST losslessness)
    let first_line = lines[0];
    let cell_contents = extract_first_line_cell_contents(first_line, columns);

    builder.start_node(kind.into());

    // Emit first line with TABLE_CELL nodes
    let (trimmed, newline_str) = strip_newline(first_line);
    let mut current_pos = 0;

    for (col_idx, column) in columns.iter().enumerate() {
        let cell_text = &cell_contents[col_idx];
        let cell_start = column.start.min(trimmed.len());
        let cell_end = column.end.min(trimmed.len());

        // Emit whitespace before cell
        if current_pos < cell_start {
            builder.token(
                SyntaxKind::WHITESPACE.into(),
                &trimmed[current_pos..cell_start],
            );
        }

        // Emit cell with inline parsing (first line content only)
        emit_table_cell(builder, cell_text, config);

        current_pos = cell_end;
    }

    // Emit trailing whitespace
    if current_pos < trimmed.len() {
        builder.token(SyntaxKind::WHITESPACE.into(), &trimmed[current_pos..]);
    }

    // Emit newline
    if !newline_str.is_empty() {
        builder.token(SyntaxKind::NEWLINE.into(), newline_str);
    }

    // Emit continuation lines as TEXT to preserve exact line structure
    for line in lines.iter().skip(1) {
        emit_line_tokens(builder, line);
    }

    builder.finish_node();
}

#[cfg(test)]
mod multiline_table_tests {
    use super::*;
    use crate::syntax::SyntaxNode;

    #[test]
    fn test_multiline_separator_detection() {
        assert!(
            try_parse_multiline_separator(
                "-------------------------------------------------------------"
            )
            .is_some()
        );
        assert!(try_parse_multiline_separator("---").is_some());
        assert!(try_parse_multiline_separator("  -----").is_some()); // with leading spaces
        assert!(try_parse_multiline_separator("--").is_none()); // too short
        assert!(try_parse_multiline_separator("--- ---").is_none()); // has spaces
        assert!(try_parse_multiline_separator("+---+").is_none()); // grid separator
    }

    #[test]
    fn test_basic_multiline_table() {
        let input = vec![
            "-------------------------------------------------------------",
            " Centered   Default           Right Left",
            "  Header    Aligned         Aligned Aligned",
            "----------- ------- --------------- -------------------------",
            "   First    row                12.0 Example of a row that",
            "                                    spans multiple lines.",
            "",
            "  Second    row                 5.0 Here's another one.",
            "-------------------------------------------------------------",
            "",
        ];

        let mut builder = GreenNodeBuilder::new();
        let result = try_parse_multiline_table(&input, 0, &mut builder, &ParserOptions::default());

        assert!(result.is_some());
        assert_eq!(result.unwrap(), 9);
    }

    #[test]
    fn test_multiline_table_headerless() {
        let input = vec![
            "----------- ------- --------------- -------------------------",
            "   First    row                12.0 Example of a row that",
            "                                    spans multiple lines.",
            "",
            "  Second    row                 5.0 Here's another one.",
            "----------- ------- --------------- -------------------------",
            "",
        ];

        let mut builder = GreenNodeBuilder::new();
        let result = try_parse_multiline_table(&input, 0, &mut builder, &ParserOptions::default());

        assert!(result.is_some());
        assert_eq!(result.unwrap(), 6);
    }

    #[test]
    fn test_multiline_table_headerless_single_line_is_not_multiline() {
        let input = vec![
            "-------     ------ ----------   -------",
            "     12     12        12             12",
            "-------     ------ ----------   -------",
            "",
            "Not part of table.",
            "",
        ];

        let mut builder = GreenNodeBuilder::new();
        let result = try_parse_multiline_table(&input, 0, &mut builder, &ParserOptions::default());

        assert!(result.is_none());
    }

    #[test]
    fn test_multiline_table_headerless_single_row_continuation_without_blank_line() {
        let input = vec![
            "----------  ---------  -----------  ---------------------------",
            "   First    row               12.0  Example of a row that spans",
            "                                    multiple lines.",
            "----------  ---------  -----------  ---------------------------",
            "",
        ];

        let mut builder = GreenNodeBuilder::new();
        let result = try_parse_multiline_table(&input, 0, &mut builder, &ParserOptions::default());

        assert!(result.is_some());
        assert_eq!(result.unwrap(), 4);
    }

    #[test]
    fn test_multiline_table_with_caption() {
        let input = vec![
            "-------------------------------------------------------------",
            " Col1       Col2",
            "----------- -------",
            "   A        B",
            "",
            "-------------------------------------------------------------",
            "",
            "Table: Here's the caption.",
            "",
        ];

        let mut builder = GreenNodeBuilder::new();
        let result = try_parse_multiline_table(&input, 0, &mut builder, &ParserOptions::default());

        assert!(result.is_some());
        // table (6 lines) + blank + caption
        assert_eq!(result.unwrap(), 8);
    }

    #[test]
    fn test_multiline_table_single_row() {
        let input = vec![
            "---------------------------------------------",
            " Header1    Header2",
            "----------- -----------",
            "   Data     More data",
            "",
            "---------------------------------------------",
            "",
        ];

        let mut builder = GreenNodeBuilder::new();
        let result = try_parse_multiline_table(&input, 0, &mut builder, &ParserOptions::default());

        assert!(result.is_some());
        assert_eq!(result.unwrap(), 6);
    }

    #[test]
    fn test_headerless_multiline_table_does_not_close_on_full_width_rule() {
        let input = vec![
            "- - - - -",
            "Third section with underscores.",
            "",
            "_____",
            "",
            "> Quote before rule",
            ">",
            "> ***",
            ">",
            "> Quote after rule",
            "",
            "Final paragraph.",
            "",
            "Here's a horizontal rule:",
            "",
            "---",
            "Text directly after the horizontal rule.",
            "",
        ];

        let mut builder = GreenNodeBuilder::new();
        let result = try_parse_multiline_table(&input, 0, &mut builder, &ParserOptions::default());

        assert!(result.is_none());
    }

    #[test]
    fn test_not_multiline_table() {
        // Simple table should not be parsed as multiline
        let input = vec![
            "  Right     Left     Center     Default",
            "-------     ------ ----------   -------",
            "     12     12        12            12",
            "",
        ];

        let mut builder = GreenNodeBuilder::new();
        let result = try_parse_multiline_table(&input, 0, &mut builder, &ParserOptions::default());

        // Should not parse because first line isn't a full-width separator
        assert!(result.is_none());
    }

    // Phase 7.1: Unit tests for emit_table_cell() helper
    #[test]
    fn test_emit_table_cell_plain_text() {
        let mut builder = GreenNodeBuilder::new();
        emit_table_cell(&mut builder, "Cell", &ParserOptions::default());
        let green = builder.finish();
        let node = SyntaxNode::new_root(green);

        assert_eq!(node.kind(), SyntaxKind::TABLE_CELL);
        assert_eq!(node.text(), "Cell");

        // Should have TEXT child
        let children: Vec<_> = node.children_with_tokens().collect();
        assert_eq!(children.len(), 1);
        assert_eq!(children[0].kind(), SyntaxKind::TEXT);
    }

    #[test]
    fn test_emit_table_cell_with_emphasis() {
        let mut builder = GreenNodeBuilder::new();
        emit_table_cell(&mut builder, "*italic*", &ParserOptions::default());
        let green = builder.finish();
        let node = SyntaxNode::new_root(green);

        assert_eq!(node.kind(), SyntaxKind::TABLE_CELL);
        assert_eq!(node.text(), "*italic*");

        // Should have EMPHASIS child
        let children: Vec<_> = node.children().collect();
        assert_eq!(children.len(), 1);
        assert_eq!(children[0].kind(), SyntaxKind::EMPHASIS);
    }

    #[test]
    fn test_emit_table_cell_with_code() {
        let mut builder = GreenNodeBuilder::new();
        emit_table_cell(&mut builder, "`code`", &ParserOptions::default());
        let green = builder.finish();
        let node = SyntaxNode::new_root(green);

        assert_eq!(node.kind(), SyntaxKind::TABLE_CELL);
        assert_eq!(node.text(), "`code`");

        // Should have CODE_SPAN child
        let children: Vec<_> = node.children().collect();
        assert_eq!(children.len(), 1);
        assert_eq!(children[0].kind(), SyntaxKind::INLINE_CODE);
    }

    #[test]
    fn test_emit_table_cell_with_link() {
        let mut builder = GreenNodeBuilder::new();
        emit_table_cell(&mut builder, "[text](url)", &ParserOptions::default());
        let green = builder.finish();
        let node = SyntaxNode::new_root(green);

        assert_eq!(node.kind(), SyntaxKind::TABLE_CELL);
        assert_eq!(node.text(), "[text](url)");

        // Should have LINK child
        let children: Vec<_> = node.children().collect();
        assert_eq!(children.len(), 1);
        assert_eq!(children[0].kind(), SyntaxKind::LINK);
    }

    #[test]
    fn test_emit_table_cell_with_strong() {
        let mut builder = GreenNodeBuilder::new();
        emit_table_cell(&mut builder, "**bold**", &ParserOptions::default());
        let green = builder.finish();
        let node = SyntaxNode::new_root(green);

        assert_eq!(node.kind(), SyntaxKind::TABLE_CELL);
        assert_eq!(node.text(), "**bold**");

        // Should have STRONG child
        let children: Vec<_> = node.children().collect();
        assert_eq!(children.len(), 1);
        assert_eq!(children[0].kind(), SyntaxKind::STRONG);
    }

    #[test]
    fn test_emit_table_cell_mixed_inline() {
        let mut builder = GreenNodeBuilder::new();
        emit_table_cell(
            &mut builder,
            "Text **bold** and `code`",
            &ParserOptions::default(),
        );
        let green = builder.finish();
        let node = SyntaxNode::new_root(green);

        assert_eq!(node.kind(), SyntaxKind::TABLE_CELL);
        assert_eq!(node.text(), "Text **bold** and `code`");

        // Should have multiple children: TEXT, STRONG, TEXT, CODE_SPAN
        let children: Vec<_> = node.children_with_tokens().collect();
        assert!(children.len() >= 4);

        // Check some expected types
        assert_eq!(children[0].kind(), SyntaxKind::TEXT);
        assert_eq!(children[1].kind(), SyntaxKind::STRONG);
    }

    #[test]
    fn test_emit_table_cell_empty() {
        let mut builder = GreenNodeBuilder::new();
        emit_table_cell(&mut builder, "", &ParserOptions::default());
        let green = builder.finish();
        let node = SyntaxNode::new_root(green);

        assert_eq!(node.kind(), SyntaxKind::TABLE_CELL);
        assert_eq!(node.text(), "");

        // Empty cell should have no children
        let children: Vec<_> = node.children_with_tokens().collect();
        assert_eq!(children.len(), 0);
    }

    #[test]
    fn test_emit_table_cell_escaped_pipe() {
        let mut builder = GreenNodeBuilder::new();
        emit_table_cell(&mut builder, r"A \| B", &ParserOptions::default());
        let green = builder.finish();
        let node = SyntaxNode::new_root(green);

        assert_eq!(node.kind(), SyntaxKind::TABLE_CELL);
        // The escaped pipe should be preserved
        assert_eq!(node.text(), r"A \| B");
    }
}
