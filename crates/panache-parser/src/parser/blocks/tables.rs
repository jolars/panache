//! Simple table parsing for Pandoc's simple_tables extension.

use crate::options::ParserOptions;
use crate::syntax::SyntaxKind;
use rowan::GreenNodeBuilder;
use unicode_width::UnicodeWidthChar;

use crate::parser::utils::attributes::{
    emit_attribute_node, try_parse_trailing_attributes_with_pos,
};
use crate::parser::utils::helpers::{emit_line_tokens, emit_separator_tokens, strip_newline};
use crate::parser::utils::inline_emission;

use super::container_prefix::StrippedLines;

/// Read-only indexed view over lines for table detection scans. Two
/// backings:
///
/// - `[&str]` — a raw, unstripped line buffer, used by callers that scan
///   the source directly (the block dispatcher's caption lookahead, list
///   and definition-list probes).
/// - [`StrippedLines`] / [`UniformStripView`] — a container-prefix-stripped
///   view that strips each line lazily on access via
///   [`StrippedLines::strip_at`]. Detection scans touch only a bounded
///   range (they stop at the first blank line), so this stays
///   O(scanned lines) rather than materializing the whole buffer. The old
///   `strip_all` collected `0..raw.len()` on every call, which was
///   quadratic when table detection runs at every block start inside a
///   large blockquote or list.
pub(crate) trait LineView {
    /// The line at absolute index `i`.
    fn line(&self, i: usize) -> &str;
    /// Total number of lines (absolute upper bound for indices).
    fn line_count(&self) -> usize;
}

impl LineView for [&str] {
    fn line(&self, i: usize) -> &str {
        self[i]
    }
    fn line_count(&self) -> usize {
        self.len()
    }
}

impl<'a, 'p> LineView for StrippedLines<'a, 'p> {
    fn line(&self, i: usize) -> &str {
        self.strip_at(i)
    }
    fn line_count(&self) -> usize {
        self.raw().len()
    }
}

/// A [`LineView`] over a [`StrippedLines`] window that strips *every* line —
/// including the dispatch line — with the full container strip rather than
/// the emission-safe line-0 strip. Grid-border detection needs this: a
/// `+---+` border sitting at column 0 of a list item's inner content must
/// not retain the list indent, or the strict column-0 check in
/// `try_parse_grid_separator` would reject it. Emission still goes through
/// the window, which preserves the indent bytes. This reproduces the old
/// grid path's `stripped[dispatch] = prefix.strip(...)` override, but
/// lazily.
pub(crate) struct UniformStripView<'s, 'a, 'p>(&'s StrippedLines<'a, 'p>);

impl<'s, 'a, 'p> LineView for UniformStripView<'s, 'a, 'p> {
    fn line(&self, i: usize) -> &str {
        self.0.prefix().strip(self.0.raw()[i])
    }
    fn line_count(&self) -> usize {
        self.0.raw().len()
    }
}

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
            '-' if !in_dashes => {
                col_start = i + offset;
                in_dashes = true;
            }
            ' ' if in_dashes => {
                columns.push(Column {
                    start: col_start,
                    end: i + offset,
                    alignment: Alignment::Default, // Will be determined later
                });
                in_dashes = false;
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

/// Parse a bare single dash run (`---`): at least two dashes, up to three
/// leading spaces, optional trailing whitespace, nothing else. This is the
/// shape a headerless single-column simple table's opener shares with a
/// horizontal rule, so callers must additionally require the closing dash
/// line (see `find_single_column_table_end`) before treating it as a table.
/// Multi-column runs (`--- ---`) are `try_parse_table_separator`'s domain.
fn parse_single_dash_run(line: &str) -> Option<Vec<Column>> {
    let (content, _) = strip_newline(line);
    let trimmed = content.trim_start();
    let leading_spaces = content.len() - trimmed.len();

    // Must have leading spaces <= 3 to not be a code block
    if leading_spaces > 3 {
        return None;
    }

    let dashes = trimmed.trim_end();
    if dashes.len() < 2 || !dashes.chars().all(|c| c == '-') {
        return None;
    }

    Some(vec![Column {
        start: leading_spaces,
        end: leading_spaces + dashes.len(),
        alignment: Alignment::Default,
    }])
}

/// Find the end of a headerless single-column simple table whose opener sits
/// at `start - 1`: rows run until a closing line of dashes (single run or
/// column separator), which pandoc requires before the first blank line —
/// without it the opener is a horizontal rule, not a table. Returns the
/// position just past the closer, which the caller emits as the final row
/// (the all-dashes `TABLE_ROW` convention headerless tables already use).
fn find_single_column_table_end(lines: &(impl LineView + ?Sized), start: usize) -> Option<usize> {
    let mut saw_row = false;
    for i in start..lines.line_count() {
        let line = lines.line(i);
        if line.trim().is_empty() {
            return None;
        }
        if try_parse_table_separator(line).is_some() || parse_single_dash_run(line).is_some() {
            return saw_row.then_some(i + 1);
        }
        saw_row = true;
    }
    None
}

/// Convert a display-column offset into a UTF-8 byte index for `line`.
///
/// Simple- and multiline-table column boundaries come from ASCII separator
/// lines whose dashes and spaces are each one display column wide, so the
/// `Column` offsets are effectively *display* columns. Header and data rows may
/// contain wide characters (CJK, fullwidth forms) that occupy two display
/// columns, or combining marks that occupy zero, so we must remap by
/// accumulated display width rather than character count. Pandoc aligns simple
/// tables the same way. The returned byte index is that of the first character
/// whose start reaches `offset`, matching how an author lays cells out visually.
fn column_offset_to_byte_index(line: &str, offset: usize) -> usize {
    let mut width = 0;
    for (byte_idx, ch) in line.char_indices() {
        if width >= offset {
            return byte_idx;
        }
        width += UnicodeWidthChar::width(ch).unwrap_or(0);
    }
    line.len()
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

fn line_is_fenced_div_fence(line: &str) -> bool {
    let trimmed = line.trim_start();
    let colon_count = trimmed.chars().take_while(|&c| c == ':').count();
    if colon_count < 3 {
        return false;
    }
    let rest = &trimmed[colon_count..];
    rest.is_empty() || rest.starts_with(char::is_whitespace)
}

fn is_valid_caption_start_before_table(lines: &(impl LineView + ?Sized), pos: usize) -> bool {
    if !is_table_caption_start(lines.line(pos)) {
        return false;
    }

    if is_bare_colon_caption_start(lines.line(pos))
        && bare_colon_caption_looks_like_definition_code_block(lines.line(pos))
    {
        return false;
    }

    // Avoid stealing definition-list definitions (":   ...") as table captions.
    if is_bare_colon_caption_start(lines.line(pos))
        && pos > 0
        && !lines.line(pos - 1).trim().is_empty()
        && !line_is_fenced_div_fence(lines.line(pos - 1))
    {
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
pub(crate) fn is_caption_followed_by_table(
    lines: &(impl LineView + ?Sized),
    caption_pos: usize,
) -> bool {
    if caption_pos >= lines.line_count() {
        return false;
    }

    // Caption must start with a caption prefix
    if !is_valid_caption_start_before_table(lines, caption_pos) {
        return false;
    }

    let mut pos = caption_pos + 1;

    // Skip continuation lines of caption (non-blank lines).
    // Stop at fenced-div fences (`:::`) — those close the enclosing div and
    // must not be folded into the caption.
    while pos < lines.line_count()
        && !lines.line(pos).trim().is_empty()
        && !line_is_fenced_div_fence(lines.line(pos))
    {
        // If we hit a table separator, we found a table
        if try_parse_table_separator(lines.line(pos)).is_some() {
            return true;
        }
        pos += 1;
    }

    // Skip one blank line
    if pos < lines.line_count() && lines.line(pos).trim().is_empty() {
        pos += 1;
    }

    // Check for a table grid at the next position.
    table_grid_starts_at(lines, pos)
}

/// Cheap lookahead: does any table kind's grid begin at absolute line `pos`?
///
/// This is the lightweight twin of the block dispatcher's `first_kind_at`,
/// which answers the same "is there a table here?" question by attempting a
/// full parse of each kind in turn. We deliberately do **not** call that from
/// the caption lookahead: caption detection runs at every block start, and a
/// full per-kind parse there would reintroduce the O(n²) blowup the bounded
/// separator probe exists to avoid. To keep the two predicates in agreement,
/// this calls the same primitive separator detectors the real parsers gate on
/// (`is_grid_table_start` → `try_parse_grid_separator`, `is_multiline_table_start`
/// → `try_parse_multiline_separator`/`is_column_separator`,
/// `try_parse_table_separator`, `try_parse_pipe_separator`).
fn table_grid_starts_at(lines: &(impl LineView + ?Sized), pos: usize) -> bool {
    if pos >= lines.line_count() {
        return false;
    }
    let line = lines.line(pos);

    // Grid table start (`+---+---+` or `+===+===+`).
    if is_grid_table_start(line) {
        return true;
    }

    // Multiline table start (`----` or `---- ---- ----`).
    if is_multiline_table_start(line) {
        return true;
    }

    // Bare dash run opening a headerless single-column simple table. Runs of
    // three or more dashes already matched the multiline check above; this
    // catches the two-dash opener pandoc also accepts. It is gated on the
    // closing dash line the real parser requires (a scan bounded by the first
    // blank line) so the probe stays in agreement with `try_parse_simple_table`.
    if parse_single_dash_run(line).is_some()
        && find_single_column_table_end(lines, pos + 1).is_some()
    {
        return true;
    }

    // Separator line (simple/pipe table, headerless).
    if try_parse_table_separator(line).is_some() {
        return true;
    }

    // Header line followed by a separator (simple/pipe table with header).
    if pos + 1 < lines.line_count() && !line.trim().is_empty() {
        let next_line = lines.line(pos + 1);
        if try_parse_table_separator(next_line).is_some()
            || try_parse_pipe_separator(next_line).is_some()
        {
            return true;
        }
    }

    false
}

fn caption_range_starting_at(
    lines: &(impl LineView + ?Sized),
    start: usize,
) -> Option<(usize, usize)> {
    if start >= lines.line_count() || !is_table_caption_start(lines.line(start)) {
        return None;
    }
    let mut end = start + 1;
    while end < lines.line_count()
        && !lines.line(end).trim().is_empty()
        && !line_is_fenced_div_fence(lines.line(end))
    {
        end += 1;
    }
    Some((start, end))
}

/// Find caption before table (if any).
/// Returns (caption_start, caption_end) positions, or None.
fn find_caption_before_table(
    lines: &(impl LineView + ?Sized),
    table_start: usize,
) -> Option<(usize, usize)> {
    if table_start == 0 {
        return None;
    }

    // Look backward for a caption
    // Caption must be immediately before table (with possible blank line between)
    let mut pos = table_start - 1;

    // Skip one blank line if present
    if lines.line(pos).trim().is_empty() {
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
            let line = lines.line(scan_pos);

            // If we hit a blank line or fenced-div fence, we've gone too far
            if line.trim().is_empty() || line_is_fenced_div_fence(line) {
                return None;
            }

            // If we find a caption start, this is the beginning of the multiline caption
            if is_valid_caption_start_before_table(lines, scan_pos) {
                if scan_pos > 0 && !lines.line(scan_pos - 1).trim().is_empty() {
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
        if pos > 0 && !lines.line(pos - 1).trim().is_empty() {
            return None;
        }
        if previous_nonblank_looks_like_table(lines, pos) {
            return None;
        }
        // This line is a caption start - return the range
        Some((pos, caption_end))
    }
}

fn previous_nonblank_looks_like_table(lines: &(impl LineView + ?Sized), pos: usize) -> bool {
    if pos == 0 {
        return false;
    }
    // Skip the blank gap directly above the caption candidate.
    let mut i = pos;
    while i > 0 && lines.line(i - 1).trim().is_empty() {
        i -= 1;
    }
    // Scan the contiguous non-blank block above for any table shape. A
    // simple/multiline table's dashed separator sits *above* its data rows
    // (which are plain text and don't look like table syntax on their own), so
    // we must walk the whole block, not just the nearest line, to recognize
    // that this caption is the caption-after of a preceding table rather than a
    // caption-before of the following one. Stop at the next blank line or a
    // fenced-div fence.
    while i > 0 {
        i -= 1;
        if lines.line(i).trim().is_empty() || line_is_fenced_div_fence(lines.line(i)) {
            break;
        }
        if line_looks_like_table_syntax(lines.line(i).trim()) {
            return true;
        }
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
fn find_caption_after_table(
    lines: &(impl LineView + ?Sized),
    table_end: usize,
) -> Option<(usize, usize)> {
    if table_end >= lines.line_count() {
        return None;
    }

    let mut pos = table_end;

    // Skip one blank line if present
    if pos < lines.line_count() && lines.line(pos).trim().is_empty() {
        pos += 1;
    }

    if pos >= lines.line_count() {
        return None;
    }

    // Check if this line is a caption
    if is_table_caption_start(lines.line(pos)) {
        let caption_start = pos;
        // Find end of caption (continues until blank line or fenced-div fence)
        let mut caption_end = caption_start + 1;
        while caption_end < lines.line_count()
            && !lines.line(caption_end).trim().is_empty()
            && !line_is_fenced_div_fence(lines.line(caption_end))
        {
            caption_end += 1;
        }
        Some((caption_start, caption_end))
    } else {
        None
    }
}

/// Emit a table caption node.
/// Emit caption text for a single line. If `lift_trailing_attrs` is set and
/// the text ends with a balanced `{...}` block, lift it into a structural
/// `ATTRIBUTE` node so `AttributeNode::cast` finds its id (matches Pandoc's
/// `+caption_attributes` behavior — `: caption {#tbl-id}` gives the table
/// the id).
fn emit_caption_line_text(
    builder: &mut GreenNodeBuilder<'static>,
    text_with_newline: &str,
    config: &ParserOptions,
    lift_trailing_attrs: bool,
) {
    let (text, newline_str) = strip_newline(text_with_newline);

    if lift_trailing_attrs
        && !text.is_empty()
        && let Some((_attrs, before_attrs, start_brace_pos)) =
            try_parse_trailing_attributes_with_pos(text)
    {
        let trimmed_len = text.trim_end().len();
        let space = &text[before_attrs.len()..start_brace_pos];
        let raw_attrs = &text[start_brace_pos..trimmed_len];
        let trailing_ws = &text[trimmed_len..];

        if !before_attrs.is_empty() {
            inline_emission::emit_inlines(builder, before_attrs, config, false);
        }
        if !space.is_empty() {
            builder.token(SyntaxKind::WHITESPACE.into(), space);
        }
        emit_attribute_node(builder, raw_attrs);
        if !trailing_ws.is_empty() {
            builder.token(SyntaxKind::WHITESPACE.into(), trailing_ws);
        }
        if !newline_str.is_empty() {
            builder.token(SyntaxKind::NEWLINE.into(), newline_str);
        }
        return;
    }

    if !text.is_empty() {
        inline_emission::emit_inlines(builder, text, config, false);
    }
    if !newline_str.is_empty() {
        builder.token(SyntaxKind::NEWLINE.into(), newline_str);
    }
}

/// Emit the blank (container-only) lines in the absolute range `[from, to)` as
/// `BLANK_LINE` nodes. Re-emits each line's container prefix as tokens via the
/// window, so a `>`-only blank line between a caption and its table inside a
/// blockquote round-trips losslessly. Mirrors the interior blank-row emitter in
/// `try_parse_multiline_table`. An empty range emits nothing.
fn emit_caption_blank_lines(
    builder: &mut GreenNodeBuilder<'static>,
    window: &StrippedLines<'_, '_>,
    from: usize,
    to: usize,
) {
    for abs in from..to {
        // `window.line` is the container-stripped view, so a `>`-only line reads
        // as blank.
        if window.line(abs).trim().is_empty() {
            builder.start_node(SyntaxKind::BLANK_LINE.into());
            let tail = window.emit_or_dispatch_tail(builder, abs);
            builder.token(SyntaxKind::BLANK_LINE.into(), tail);
            builder.finish_node();
        }
    }
}

fn emit_table_caption(
    builder: &mut GreenNodeBuilder<'static>,
    window: &StrippedLines<'_, '_>,
    start: usize,
    end: usize,
    config: &ParserOptions,
) {
    builder.start_node(SyntaxKind::TABLE_CAPTION.into());

    let last_idx = (end - start).saturating_sub(1);

    for (i, abs) in (start..end).enumerate() {
        let lift_attrs = i == last_idx;

        // Re-emit this caption line's container prefix (`>`/whitespace) as
        // tokens — except the dispatch line, whose prefix the core already
        // emitted — and operate on the stripped `tail`, so the caption prefix
        // (`Table:`/`:`) is recognized inside a blockquote or list rather than
        // swallowed into the caption text (which doubled the marker and broke
        // losslessness).
        let tail = window.emit_or_dispatch_tail(builder, abs);

        if i == 0 {
            // First line - parse and emit prefix separately
            let trimmed = tail.trim_start();
            let leading_ws_len = tail.len() - trimmed.len();

            // Emit leading whitespace if present
            if leading_ws_len > 0 {
                builder.token(SyntaxKind::WHITESPACE.into(), &tail[..leading_ws_len]);
            }

            // Check for caption prefix and emit separately
            // Calculate where the prefix ends (after trimmed content)
            let prefix_and_rest = if tail.ends_with('\n') {
                &tail[leading_ws_len..tail.len() - 1] // Exclude newline
            } else {
                &tail[leading_ws_len..]
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
                if rest_start < tail.len() {
                    emit_caption_line_text(builder, &tail[rest_start..], config, lift_attrs);
                }
            } else {
                // No recognized prefix, emit whole trimmed line
                emit_caption_line_text(builder, &tail[leading_ws_len..], config, lift_attrs);
            }
        } else {
            // Continuation lines - emit with inline parsing (attrs only on last line).
            emit_caption_line_text(builder, tail, config, lift_attrs);
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
        inline_emission::emit_inlines(builder, cell_text, config, false);
    }

    builder.finish_node(); // TABLE_CELL
}

/// Determine column alignments based on separator and optional header.
fn determine_alignments(columns: &mut [Column], separator_line: &str, header_line: Option<&str>) {
    for col in columns.iter_mut() {
        let sep_slice = &separator_line[col.start..col.end];

        if let Some(header) = header_line {
            let header_start = column_offset_to_byte_index(header, col.start);
            let header_end = column_offset_to_byte_index(header, col.end);

            // Extract header text for this column
            let header_text = if header_start < header_end {
                header[header_start..header_end].trim()
            } else if header_start < header.len() {
                header[header_start..].trim()
            } else {
                ""
            };

            if header_text.is_empty() {
                col.alignment = Alignment::Default;
                continue;
            }

            // Find where the header text starts and ends within the column
            let header_in_col = &header[header_start..header_end];
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
    window: &StrippedLines<'_, '_>,
    builder: &mut GreenNodeBuilder<'static>,
    config: &ParserOptions,
) -> Option<usize> {
    let lines = window.raw();
    let start_pos = window.pos();
    log::trace!("try_parse_simple_table at line {}", start_pos + 1);

    if start_pos >= lines.len() {
        return None;
    }

    // Cheap gate before the O(buffer) `strip_all` below: a simple table's
    // separator must sit on the dispatch line or the line just after it (see
    // `find_separator_line`). Table detection runs at every block start, so
    // stripping the whole line buffer for every prose/math paragraph that
    // can't be a table was quadratic on large documents. Peek just those one
    // or two lines via `strip_at` and bail before materializing the full view.
    let gate_first = window.strip_at(start_pos);
    let separator_here = try_parse_table_separator(gate_first).is_some();
    let separator_next = !separator_here
        && start_pos + 1 < lines.len()
        && !gate_first.trim().is_empty()
        && try_parse_table_separator(window.strip_at(start_pos + 1)).is_some();
    // A bare dash run can open a headerless *single-column* table (pandoc:
    // `---` / rows / `---`), but only on the dispatch line — on the line
    // after content it is a setext underline, which pandoc's header parser
    // claims before its table parser runs.
    let single_column_here =
        !separator_here && !separator_next && parse_single_dash_run(gate_first).is_some();
    if !separator_here && !separator_next && !single_column_here {
        return None;
    }

    // Detection scans read the container-prefix-stripped view lazily through
    // the window (see `LineView`): a table nested in `list → blockquote`
    // (e.g. `- >  a   b`) has its `  > ` prefix removed before the
    // separator/column-shape checks. With an empty prefix the stripped view
    // equals the raw lines. Scans stop at the first blank line, so only a
    // bounded range is ever stripped. Emission re-emits the prefix bytes as
    // tokens via the window; captions/blank lines still read raw `lines`.

    // Look for a separator line. The single-column shape is ambiguous with a
    // horizontal rule, so unlike the multi-column paths it also demands the
    // closing dash line up front (pandoc rejects the table without one).
    let (separator_pos, end_pos) = if single_column_here {
        let end_pos = find_single_column_table_end(window, start_pos + 1)?;
        (start_pos, end_pos)
    } else {
        let separator_pos = find_separator_line(window, start_pos)?;
        // Find table end (blank line or end of input)
        (separator_pos, find_table_end(window, separator_pos + 1))
    };
    log::trace!("  found separator at line {}", separator_pos + 1);

    let separator_line = window.line(separator_pos);
    let mut columns = if single_column_here {
        parse_single_dash_run(separator_line)?
    } else {
        try_parse_table_separator(separator_line)?
    };

    // Determine if there's a header (separator not at start)
    let has_header = separator_pos > start_pos;
    let header_line = if has_header {
        Some(window.line(separator_pos - 1))
    } else {
        None
    };

    // Determine alignments
    determine_alignments(&mut columns, separator_line, header_line);

    // Must have at least one data row (or it's just a separator)
    let data_rows = end_pos - separator_pos - 1;

    if data_rows == 0 {
        return None;
    }

    // Check for caption before table
    let caption_before = find_caption_before_table(window, start_pos);

    // Check for caption after table
    let caption_after = if caption_before.is_some() {
        None
    } else {
        find_caption_after_table(window, end_pos)
    };

    // Build the table
    builder.start_node(SyntaxKind::SIMPLE_TABLE.into());

    // Emit caption before if present
    if let Some((cap_start, cap_end)) = caption_before {
        emit_table_caption(builder, window, cap_start, cap_end, config);
        // Emit blank line between caption and table if present
        emit_caption_blank_lines(builder, window, cap_end, start_pos);
    }

    // Emit header if present. On the dispatch line the core already emitted
    // the container prefix; only continuation rows re-emit it (via the window
    // inside `emit_table_row`).
    if has_header {
        emit_table_row(
            builder,
            window,
            separator_pos - 1,
            &columns,
            SyntaxKind::TABLE_HEADER,
            config,
        );
    }

    // Emit separator, re-emitting any continuation-line container prefix
    // (`  > `) as WHITESPACE/BLOCK_QUOTE_MARKER tokens before the row text.
    builder.start_node(SyntaxKind::TABLE_SEPARATOR.into());
    let separator_tail = window.emit_or_dispatch_tail(builder, separator_pos);
    emit_separator_tokens(builder, separator_tail);
    builder.finish_node();

    // Emit data rows (always continuation lines)
    for idx in (separator_pos + 1)..end_pos {
        emit_table_row(
            builder,
            window,
            idx,
            &columns,
            SyntaxKind::TABLE_ROW,
            config,
        );
    }

    // Emit caption after if present
    if let Some((cap_start, cap_end)) = caption_after {
        // Emit blank line before caption if needed
        emit_caption_blank_lines(builder, window, end_pos, cap_start);
        emit_table_caption(builder, window, cap_start, cap_end, config);
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
fn find_separator_line(lines: &(impl LineView + ?Sized), start_pos: usize) -> Option<usize> {
    log::trace!("  find_separator_line from line {}", start_pos + 1);

    // Check first line
    log::trace!("    checking first line: {:?}", lines.line(start_pos));
    if try_parse_table_separator(lines.line(start_pos)).is_some() {
        log::trace!("    separator found at first line");
        return Some(start_pos);
    }

    // Check second line (for table with header)
    if start_pos + 1 < lines.line_count()
        && !lines.line(start_pos).trim().is_empty()
        && try_parse_table_separator(lines.line(start_pos + 1)).is_some()
    {
        return Some(start_pos + 1);
    }
    None
}

/// Find where the table ends (first blank line or end of input).
fn find_table_end(lines: &(impl LineView + ?Sized), start_pos: usize) -> usize {
    for i in start_pos..lines.line_count() {
        if lines.line(i).trim().is_empty() {
            return i;
        }
        // Check if this could be a closing separator
        if try_parse_table_separator(lines.line(i)).is_some() {
            // Check if next line is blank or end
            if i + 1 >= lines.line_count() || lines.line(i + 1).trim().is_empty() {
                return i + 1;
            }
        }
    }
    lines.line_count()
}

/// Emit a table row (header or data row) with inline-parsed cells for simple tables.
/// Uses column boundaries from the separator line to extract cells.
fn emit_table_row(
    builder: &mut GreenNodeBuilder<'static>,
    window: &StrippedLines<'_, '_>,
    abs_idx: usize,
    columns: &[Column],
    row_kind: SyntaxKind,
    config: &ParserOptions,
) {
    builder.start_node(row_kind.into());

    // On continuation lines the leading `  > ` prefix is re-emitted as
    // WHITESPACE/BLOCK_QUOTE_MARKER tokens inside the row node and the
    // stripped tail returned; the dispatch line just strips its (already
    // core-emitted) prefix. Empty prefix ⇒ the raw line.
    let line = window.emit_or_dispatch_tail(builder, abs_idx);

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
    for (i, col) in columns.iter().enumerate() {
        // Calculate actual positions in the trimmed line (accounting for leading whitespace)
        let cell_start = if col.start >= leading_ws_len {
            column_offset_to_byte_index(trimmed, col.start - leading_ws_len)
        } else {
            0
        };

        // A column spans from its own start to the start of the next column
        // (the inter-column gap belongs to the left column); the last column
        // runs to end-of-line. Ending the slice at the dash-run end instead
        // would split cell text that overruns a short dash run into the cell
        // plus a bogus WHITESPACE token.
        let end_offset = columns.get(i + 1).map_or(usize::MAX, |next| next.start);
        let cell_end = if end_offset == usize::MAX {
            trimmed.len()
        } else if end_offset >= leading_ws_len {
            column_offset_to_byte_index(trimmed, end_offset - leading_ws_len)
        } else {
            0
        };

        // Extract cell text from column bounds. When the column lies entirely
        // before the trimmed content (col.end <= leading_ws_len) both bounds
        // clamp to 0; treat that as an empty cell rather than re-emitting the
        // whole row.
        let cell_text = if cell_start < cell_end && cell_start < trimmed.len() {
            &trimmed[cell_start..cell_end]
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
    window: &StrippedLines<'_, '_>,
    abs_idx: usize,
    row_kind: SyntaxKind,
    config: &ParserOptions,
) {
    builder.start_node(row_kind.into());

    // On continuation lines (separator/data rows under a list+blockquote
    // container) the leading `  > ` prefix is not consumed by the core;
    // `emit_prefix_at` re-emits it as WHITESPACE/BLOCK_QUOTE_MARKER tokens
    // and returns the stripped tail. On the dispatch line the core already
    // emitted the prefix, so `dispatch_tail` just strips it from our view.
    // With an empty prefix (non-nested tables) both are no-ops returning
    // the raw line.
    let line = if abs_idx == window.dispatch_pos() {
        window.dispatch_tail()
    } else {
        window.emit_prefix_at(builder, abs_idx)
    };

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
    window: &StrippedLines<'_, '_>,
    builder: &mut GreenNodeBuilder<'static>,
    config: &ParserOptions,
) -> Option<usize> {
    let lines = window.raw();
    let start_pos = window.pos();
    if start_pos + 1 >= lines.len() {
        return None;
    }

    // Cheap gate: a pipe table's first line must contain a `|` (it is either
    // the header or, headerless, the delimiter row), unless this is a
    // caption-led table. Table detection runs at every block start, so doing
    // any per-line work for every prose/math paragraph was quadratic on large
    // documents. Peek the dispatch line and run the (bounded) caption probe on
    // the same stripped `window` the detection below uses, so the gate applies
    // inside containers (blockquote/list) too — not just at top level.
    if !window.strip_at(start_pos).contains('|') && !is_caption_followed_by_table(window, start_pos)
    {
        return None;
    }

    // Detection scans read the container-prefix-stripped view lazily through
    // the window (see `LineView`), so a table nested in `list → blockquote`
    // (e.g. `- > | a | b |`) has its `  > ` prefix removed before the
    // separator/cell shape checks. The dispatch line uses the emission-safe
    // line-0 strip (its prefix was consumed by the core); every other line
    // gets the full continuation strip. Scans stop at the first blank line, so
    // only a bounded range is stripped. Emission still reads raw `lines` so the
    // prefix bytes can be re-emitted as tokens.

    // Check if this line is a caption followed by a table
    // If so, the actual table starts after the caption and blank line
    let (actual_start, caption_before) = if is_caption_followed_by_table(window, start_pos) {
        let (cap_start, cap_end) = caption_range_starting_at(window, start_pos)?;
        let mut pos = cap_end;
        while pos < window.line_count() && window.line(pos).trim().is_empty() {
            pos += 1;
        }
        (pos, Some((cap_start, cap_end)))
    } else {
        (start_pos, None)
    };

    if actual_start + 1 >= lines.len() {
        return None;
    }

    // First line should have pipes (potential header)
    if !window.line(actual_start).contains('|') {
        return None;
    }

    // Second line should be separator
    let alignments = try_parse_pipe_separator(window.line(actual_start + 1))?;

    // Parse header cells
    let header_cells = parse_pipe_table_row(window.line(actual_start));

    // Number of columns should match (approximately - be lenient)
    if header_cells.len() != alignments.len() && !header_cells.is_empty() {
        // Only fail if very different
        if header_cells.len() < alignments.len() / 2 || header_cells.len() > alignments.len() * 2 {
            return None;
        }
    }

    // Find table end (first blank line or end of input)
    let mut end_pos = actual_start + 2;
    while end_pos < window.line_count() {
        let line = window.line(end_pos);
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
    let caption_before = caption_before.or_else(|| find_caption_before_table(window, actual_start));

    // Check for caption after table
    let caption_after = if caption_before.is_some() {
        None
    } else {
        find_caption_after_table(window, end_pos)
    };

    // Build the pipe table
    builder.start_node(SyntaxKind::PIPE_TABLE.into());

    // Emit caption before if present
    if let Some((cap_start, cap_end)) = caption_before {
        emit_table_caption(builder, window, cap_start, cap_end, config);
        // Emit blank line between caption and table if present
        emit_caption_blank_lines(builder, window, cap_end, actual_start);
    }

    // Emit header row with inline-parsed cells. On the dispatch line the
    // core already emitted the container prefix; only when the header is a
    // continuation line (e.g. it follows a caption-before line) do we emit
    // the prefix here.
    emit_pipe_table_row(
        builder,
        window,
        actual_start,
        SyntaxKind::TABLE_HEADER,
        config,
    );

    // Emit separator, re-emitting any continuation-line container prefix
    // (`  > `) as WHITESPACE/BLOCK_QUOTE_MARKER tokens before the row text.
    builder.start_node(SyntaxKind::TABLE_SEPARATOR.into());
    let sep_idx = actual_start + 1;
    let separator_tail = if sep_idx == window.dispatch_pos() {
        window.dispatch_tail()
    } else {
        window.emit_prefix_at(builder, sep_idx)
    };
    emit_separator_tokens(builder, separator_tail);
    builder.finish_node();

    // Emit data rows with inline-parsed cells (always continuation lines)
    for idx in (actual_start + 2)..end_pos {
        emit_pipe_table_row(builder, window, idx, SyntaxKind::TABLE_ROW, config);
    }

    // Emit caption after if present
    if let Some((cap_start, cap_end)) = caption_after {
        // Emit blank line before caption if needed
        emit_caption_blank_lines(builder, window, end_pos, cap_start);
        emit_table_caption(builder, window, cap_start, cap_end, config);
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
    use super::super::container_prefix::ContainerPrefix;
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
    fn column_offset_maps_by_display_width() {
        // Wide CJK characters occupy two display columns each. A column offset
        // of 4 display columns lands after two wide chars (6 bytes), not after
        // four chars.
        let line = "地號xy";
        assert_eq!(column_offset_to_byte_index(line, 4), 6);
        // ASCII stays byte-for-byte.
        assert_eq!(column_offset_to_byte_index("abcd", 2), 2);
    }

    #[test]
    fn simple_table_cjk_header_keeps_footnote_ref_intact() {
        // Regression for #411: wide (double-width) header text pushed the
        // footnote reference across a char-counted column boundary, splitting
        // `[^d]` so it never parsed as a FOOTNOTE_REFERENCE and the linter
        // flagged the definition as unused.
        let input = "\
 地號    地主    路段      總長[^d] 水利局舖的[^s]
------ -------   -------- --------- --------------
2976    Ralph    南段          64   33
";
        let tree = crate::parser::parse(input, None);
        let refs: Vec<_> = tree
            .descendants()
            .filter(|n| n.kind() == SyntaxKind::FOOTNOTE_REFERENCE)
            .map(|n| n.text().to_string())
            .collect();
        assert_eq!(refs, vec!["[^d]".to_string(), "[^s]".to_string()]);
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
        let prefix = ContainerPrefix::default();
        let window = StrippedLines::new(&input, 0, &prefix);
        let result = try_parse_simple_table(&window, &mut builder, &ParserOptions::default());

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
        let prefix = ContainerPrefix::default();
        let window = StrippedLines::new(&input, 0, &prefix);
        let result = try_parse_simple_table(&window, &mut builder, &ParserOptions::default());

        assert!(result.is_some());
        assert_eq!(result.unwrap(), 3); // sep + 2 rows
    }

    #[test]
    fn single_dash_run_detection() {
        assert!(parse_single_dash_run("---").is_some());
        assert!(parse_single_dash_run("--").is_some()); // pandoc accepts two dashes
        assert!(parse_single_dash_run("-").is_none()); // list marker territory
        assert!(parse_single_dash_run("  ----------  ").is_some());
        assert!(parse_single_dash_run("    ---").is_none()); // indented code
        assert!(parse_single_dash_run("--- ---").is_none()); // multi-column separator
        assert!(parse_single_dash_run("---x").is_none());
        assert!(parse_single_dash_run("- - -").is_none()); // spaced thematic break
    }

    #[test]
    fn headerless_single_column_simple_table() {
        // Pandoc parses a bare dash run followed by rows and a closing dash
        // run as a headerless single-column simple table (`---` / rows /
        // `---`), e.g. when the YAML metadata gate rejects a non-mapping
        // block.
        let input = vec!["---", "- a", "- b", "---", ""];

        let mut builder = GreenNodeBuilder::new();
        let prefix = ContainerPrefix::default();
        let window = StrippedLines::new(&input, 0, &prefix);
        let result = try_parse_simple_table(&window, &mut builder, &ParserOptions::default());

        assert_eq!(result, Some(4)); // opener + 2 rows + closer
    }

    #[test]
    fn headerless_single_column_requires_closer() {
        // Without a closing dash line before the first blank line the dash
        // run is a horizontal rule, not a table opener (matches pandoc).
        let input = vec!["---", "just prose", "", "after"];

        let mut builder = GreenNodeBuilder::new();
        let prefix = ContainerPrefix::default();
        let window = StrippedLines::new(&input, 0, &prefix);
        let result = try_parse_simple_table(&window, &mut builder, &ParserOptions::default());

        assert!(result.is_none());
    }

    #[test]
    fn headerless_single_column_requires_rows() {
        // Two adjacent dash runs are two horizontal rules, not an empty table.
        let input = vec!["---", "---", ""];

        let mut builder = GreenNodeBuilder::new();
        let prefix = ContainerPrefix::default();
        let window = StrippedLines::new(&input, 0, &prefix);
        let result = try_parse_simple_table(&window, &mut builder, &ParserOptions::default());

        assert!(result.is_none());
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
    fn table_grid_starts_at_matches_each_kind() {
        // Positives — one shape per table kind the real parsers accept.
        assert!(table_grid_starts_at(&["+---+---+"][..], 0)); // grid
        assert!(table_grid_starts_at(&["----------- -------"][..], 0)); // multiline
        assert!(table_grid_starts_at(&["--- --- ---"][..], 0)); // simple, headerless
        assert!(table_grid_starts_at(&["A | B", "| --- | --- |"][..], 0)); // pipe, header + sep
        assert!(table_grid_starts_at(&["A    B", "--- ---"][..], 0)); // simple, header + sep
        // A lone dash run is a multiline full-width separator under Pandoc (not a
        // thematic break), so the lookahead intentionally accepts it; the full
        // parser then rejects it if no rows follow.
        assert!(table_grid_starts_at(&["-------"][..], 0));

        // Negatives — shapes that must not read as a table start.
        assert!(!table_grid_starts_at(&["just some prose"][..], 0));
        assert!(!table_grid_starts_at(&["# Heading"][..], 0));
        assert!(!table_grid_starts_at(&["```", "code", "```"][..], 0)); // code fence
        assert!(!table_grid_starts_at(&["only one line"][..], 1)); // out of range
    }

    /// The cheap caption lookahead must agree with what the full parser does:
    /// when it says a table follows the caption, a table node really forms; when
    /// it says no table follows, none does. This guards against the lookahead
    /// (`table_grid_starts_at`) drifting from the real per-kind parsers.
    #[test]
    fn caption_lookahead_agrees_with_real_parse() {
        let with_table = ": Cap\n\n| A | B |\n|---|---|\n| 1 | 2 |\n";
        let lines: Vec<&str> = with_table.lines().collect();
        assert!(is_caption_followed_by_table(&lines[..], 0));
        assert!(format!("{:#?}", crate::parse(with_table, None)).contains("PIPE_TABLE"));

        let no_table = ": Cap\n\nplain paragraph\n";
        let lines: Vec<&str> = no_table.lines().collect();
        assert!(!is_caption_followed_by_table(&lines[..], 0));
        assert!(!format!("{:#?}", crate::parse(no_table, None)).contains("TABLE"));
    }

    /// Pandoc parses `table` before `orderedList` (but `bulletList` before
    /// `table`) in its `block` choice. So an ordered marker whose line is the
    /// header of a valid pipe table is NOT a list: the whole construct is a
    /// top-level table absorbing the marker as the first header cell. Bullets
    /// and a lone ordered marker (no delimiter) stay lists. Verified against
    /// pandoc 3.9 (`-f markdown -t native`).
    #[test]
    fn ordered_marker_on_pipe_table_line_is_top_level_table() {
        let input = "1. | a | b |\n   | - | - |\n   | 1 | 2 |\n";
        let tree = crate::parse(input, None);
        assert!(
            tree.descendants()
                .any(|n| n.kind() == SyntaxKind::PIPE_TABLE),
            "ordered marker + pipe table on the marker line should be a top-level table"
        );
        assert!(
            !tree.descendants().any(|n| n.kind() == SyntaxKind::LIST),
            "it must not nest under a list"
        );
        // Lossless: the marker and the overflow cell survive in the CST.
        let dump = format!("{tree:#?}");
        assert!(
            dump.contains("1."),
            "marker text preserved as a header cell"
        );
        assert!(dump.contains('b'), "overflow cell `b` preserved (lossless)");
    }

    #[test]
    fn lone_ordered_marker_pipe_line_is_a_list() {
        // No delimiter row → pandoc's `table` fails, `orderedList` catches it.
        let input = "1. | a | b |\n";
        let tree = crate::parse(input, None);
        assert!(
            tree.descendants().any(|n| n.kind() == SyntaxKind::LIST),
            "a lone ordered marker line stays a list"
        );
        assert!(
            !tree
                .descendants()
                .any(|n| n.kind() == SyntaxKind::PIPE_TABLE),
            "no table without a delimiter row"
        );
    }

    #[test]
    fn bullet_marker_on_pipe_table_line_stays_a_nested_table() {
        // Bullets already match pandoc (`BulletList -> Table`): regression guard.
        let input = "- | a | b |\n  | - | - |\n  | 1 | 2 |\n";
        let tree = crate::parse(input, None);
        assert!(
            tree.descendants().any(|n| n.kind() == SyntaxKind::LIST),
            "bullet marker keeps the list"
        );
        assert!(
            tree.descendants()
                .any(|n| n.kind() == SyntaxKind::PIPE_TABLE),
            "with the table nested inside the list item"
        );
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
    fn bare_colon_caption_after_div_opening_is_table_caption() {
        let input = "::: {#tbl:panel layout.nrow=\"1\"}\n  : My Caption {#tbl:foo-1}\n\n  | Col1 | Col2 | Col3 |\n  | ---- | ---- | ---- |\n  | A    | B    | C    |\n  | E    | F    | G    |\n  | A    | G    | G    |\n\n  : My Caption2 {#tbl:foo-2}\n\n  | Col1 | Col2 | Col3 |\n  | ---- | ---- | ---- |\n  | A    | B    | C    |\n  | E    | F    | G    |\n  | A    | G    | G    |\n\nCaption\n:::\n";
        let tree = crate::parse(input, None);

        let caption_count = tree
            .descendants()
            .filter(|node| node.kind() == SyntaxKind::TABLE_CAPTION)
            .count();
        assert_eq!(
            caption_count, 2,
            "expected both captions to attach to tables"
        );
        assert!(
            !tree
                .descendants()
                .any(|node| node.kind() == SyntaxKind::DEFINITION_LIST),
            "caption lines in this fenced div table layout should not parse as definition list"
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
        let prefix = ContainerPrefix::default();
        let window = StrippedLines::new(&input, 0, &prefix);
        let result = try_parse_simple_table(&window, &mut builder, &ParserOptions::default());

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
        let prefix = ContainerPrefix::default();
        let window = StrippedLines::new(&input, 2, &prefix);
        let result = try_parse_simple_table(&window, &mut builder, &ParserOptions::default());

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
        let prefix = ContainerPrefix::default();
        let window = StrippedLines::new(&input, 0, &prefix);
        let result = try_parse_simple_table(&window, &mut builder, &ParserOptions::default());

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
        let prefix = ContainerPrefix::default();
        let window = StrippedLines::new(&input, 0, &prefix);
        let result = try_parse_simple_table(&window, &mut builder, &ParserOptions::default());

        assert!(result.is_some());
        // Should consume through end of multi-line caption
        assert_eq!(result.unwrap(), 6);
    }

    #[test]
    fn test_simple_table_with_multibyte_cell_content() {
        let input = vec![
            "Name            Hex code     Hue     C, M, Y, K (%)   R, G, B (0-255)   R, G, B (%)",
            "--------------  ------------ ------- ---------------- ----------------- ------------",
            "        orange       #E69F00     41° 0, 50, 100, 0    230, 159, 0       90, 60, 0",
            "      sky blue       #56B4E9    202° 80, 0, 0, 0      86, 180, 233      35, 70, 90",
            "",
        ];

        let mut builder = GreenNodeBuilder::new();
        let prefix = ContainerPrefix::default();
        let window = StrippedLines::new(&input, 0, &prefix);
        let result = try_parse_simple_table(&window, &mut builder, &ParserOptions::default());

        assert!(result.is_some());
        assert_eq!(result.unwrap(), 4);
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
        let prefix = ContainerPrefix::default();
        let window = StrippedLines::new(&input, 1, &prefix);
        let result = try_parse_pipe_table(&window, &mut builder, &ParserOptions::default());

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
        let prefix = ContainerPrefix::default();
        let window = StrippedLines::new(&input, 1, &prefix);
        let result = try_parse_pipe_table(&window, &mut builder, &ParserOptions::default());

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
        let prefix = ContainerPrefix::default();
        let window = StrippedLines::new(&input, 1, &prefix);
        let result = try_parse_pipe_table(&window, &mut builder, &ParserOptions::default());

        assert!(result.is_some());
        assert_eq!(result.unwrap(), 5); // header + sep + row + blank + caption
    }

    #[test]
    fn test_pipe_table_with_multiline_caption_before() {
        let input = vec![
            ": (#tab:base) base R quoting",
            "functions",
            "",
            "| C | D |",
            "|---|---|",
            "| 3 | 4 |",
            "",
        ];

        let mut builder = GreenNodeBuilder::new();
        let prefix = ContainerPrefix::default();
        let window = StrippedLines::new(&input, 0, &prefix);
        let result = try_parse_pipe_table(&window, &mut builder, &ParserOptions::default());

        assert!(result.is_some());
        // caption(2) + blank(1) + header + sep + row
        assert_eq!(result.unwrap(), 6);
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

    // A grid border must begin at column 0 of its container content. Detection
    // runs on the container-prefix-stripped line (see `try_parse_grid_table`),
    // so any remaining leading whitespace means the border is indented relative
    // to its container -- pandoc parses that as a paragraph, not a grid table.
    if leading_spaces > 0 {
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

/// Emit a grid table row with inline-parsed cells.
/// Handles multi-line rows by emitting first line with TABLE_CELL nodes,
/// then continuation lines as raw TEXT for losslessness.
fn emit_grid_table_row(
    builder: &mut GreenNodeBuilder<'static>,
    window: &StrippedLines<'_, '_>,
    indices: &[usize],
    columns: &[GridColumn],
    row_kind: SyntaxKind,
    config: &ParserOptions,
) {
    if indices.is_empty() {
        return;
    }

    builder.start_node(row_kind.into());

    // Emit first line with TABLE_CELL nodes. The continuation-line container
    // prefix (`  > `) is re-emitted as WHITESPACE/BLOCK_QUOTE_MARKER tokens
    // inside the row node before the cell text; the returned tail is the
    // prefix-stripped line we slice cells from (empty prefix ⇒ raw line).
    // Grid table rows look like: "| Cell 1 | Cell 2 | Cell 3 |"
    let first_line = window.emit_or_dispatch_tail(builder, indices[0]);
    let cell_contents = extract_grid_cells_from_line(first_line, columns);
    let (line_without_newline, newline_str) = strip_newline(first_line);
    let trimmed = line_without_newline.trim();
    let expected_pipe_count = columns.len().saturating_add(1);
    let actual_pipe_count = trimmed.chars().filter(|&c| c == '|').count();

    // Rows that don't contain all expected column separators (spanning-style rows)
    // must be emitted verbatim for losslessness. The first line's prefix was
    // already consumed above; emit its tail and each continuation tail.
    if actual_pipe_count != expected_pipe_count {
        emit_line_tokens(builder, first_line);
        for &idx in &indices[1..] {
            let tail = window.emit_or_dispatch_tail(builder, idx);
            emit_line_tokens(builder, tail);
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

    // Emit continuation lines as TEXT for losslessness, re-emitting each
    // line's container prefix first.
    for &idx in &indices[1..] {
        let tail = window.emit_or_dispatch_tail(builder, idx);
        emit_line_tokens(builder, tail);
    }

    builder.finish_node();
}

/// Try to parse a grid table starting at the given position.
/// Returns the number of lines consumed if successful.
pub(crate) fn try_parse_grid_table(
    window: &StrippedLines<'_, '_>,
    builder: &mut GreenNodeBuilder<'static>,
    config: &ParserOptions,
) -> Option<usize> {
    let lines = window.raw();
    let start_pos = window.pos();
    if start_pos >= lines.len() {
        return None;
    }

    // Grid-border detection reads the stripped view through `UniformStripView`,
    // which strips *every* line — including the dispatch line — with the full
    // container strip. The strict column-0 check in `try_parse_grid_separator`
    // would otherwise reject a `+---+` border sitting at column 0 of a list
    // item's inner content if the dispatch line kept its list-indent. With an
    // empty prefix the stripped view equals the raw lines. Emission still goes
    // through `window.emit_or_dispatch_tail`, which preserves the indent bytes.
    // Scans stop at the first blank line, so only a bounded range is stripped.
    let view = UniformStripView(window);

    // Cheap gate: a grid table's first line is a grid separator (`+---+`/`+===+`),
    // unless this is a caption-led table. Table detection runs at every block
    // start, so any per-line work for every prose/math paragraph was quadratic
    // on large documents. Run the gate on the same `view` the detection uses, so
    // it applies inside containers (blockquote/list) too — not just at top level.
    if try_parse_grid_separator(view.line(start_pos)).is_none()
        && !is_caption_followed_by_table(&view, start_pos)
    {
        return None;
    }

    // Check if this line is a caption followed by a table
    // If so, the actual table starts after the caption and blank line
    let (actual_start, caption_before) = if is_caption_followed_by_table(&view, start_pos) {
        let (cap_start, cap_end) = caption_range_starting_at(&view, start_pos)?;
        let mut pos = cap_end;
        while pos < view.line_count() && view.line(pos).trim().is_empty() {
            pos += 1;
        }
        (pos, Some((cap_start, cap_end)))
    } else {
        (start_pos, None)
    };

    if actual_start >= lines.len() {
        return None;
    }

    // First line must be a grid separator
    let first_line = view.line(actual_start);
    let _columns = try_parse_grid_separator(first_line)?;

    // Track table structure
    let mut end_pos = actual_start + 1;
    let mut found_header_sep = false;
    let mut in_footer = false;

    // Scan table lines
    while end_pos < lines.len() {
        let line = view.line(end_pos);

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
    let caption_before = caption_before.or_else(|| find_caption_before_table(&view, actual_start));

    // Check for caption after table
    let caption_after = if caption_before.is_some() {
        None
    } else {
        find_caption_after_table(&view, end_pos)
    };

    // Build the grid table
    builder.start_node(SyntaxKind::GRID_TABLE.into());

    // Emit caption before if present
    if let Some((cap_start, cap_end)) = caption_before {
        emit_table_caption(builder, window, cap_start, cap_end, config);
        // Emit blank line between caption and table if present
        emit_caption_blank_lines(builder, window, cap_end, actual_start);
    }

    // Track whether we've passed the header separator
    let mut past_header_sep = false;
    let mut in_footer_section = false;
    // Accumulate ABSOLUTE indices of the lines making up a multi-line row, so
    // each line's container prefix can be re-emitted via the window.
    let mut current_row_indices: Vec<usize> = Vec::new();
    let mut current_row_kind = SyntaxKind::TABLE_HEADER;

    // Emit table rows - accumulate multi-line cells
    for idx in actual_start..end_pos {
        let line = view.line(idx);
        if let Some(sep_cols) = try_parse_grid_separator(line) {
            // Separator line - emit any accumulated row first
            if !current_row_indices.is_empty() {
                emit_grid_table_row(
                    builder,
                    window,
                    &current_row_indices,
                    &sep_cols,
                    current_row_kind,
                    config,
                );
                current_row_indices.clear();
            }

            let is_header_sep = sep_cols.iter().any(|c| c.is_header_separator);

            // Re-emit any continuation-line container prefix (`  > `) as
            // WHITESPACE/BLOCK_QUOTE_MARKER tokens before the separator text.
            if is_header_sep {
                if !past_header_sep {
                    // This is the header/body separator
                    builder.start_node(SyntaxKind::TABLE_SEPARATOR.into());
                    let tail = window.emit_or_dispatch_tail(builder, idx);
                    emit_separator_tokens(builder, tail);
                    builder.finish_node();
                    past_header_sep = true;
                } else {
                    // Footer separator
                    if !in_footer_section {
                        in_footer_section = true;
                    }
                    builder.start_node(SyntaxKind::TABLE_SEPARATOR.into());
                    let tail = window.emit_or_dispatch_tail(builder, idx);
                    emit_separator_tokens(builder, tail);
                    builder.finish_node();
                }
            } else {
                // Regular separator (row boundary)
                builder.start_node(SyntaxKind::TABLE_SEPARATOR.into());
                let tail = window.emit_or_dispatch_tail(builder, idx);
                emit_separator_tokens(builder, tail);
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

            current_row_indices.push(idx);
        }
    }

    // Emit any remaining accumulated row
    if !current_row_indices.is_empty() {
        // Use first separator's columns for cell boundaries
        if let Some(sep_cols) = try_parse_grid_separator(view.line(actual_start)) {
            emit_grid_table_row(
                builder,
                window,
                &current_row_indices,
                &sep_cols,
                current_row_kind,
                config,
            );
        }
    }

    // Emit caption after if present
    if let Some((cap_start, cap_end)) = caption_after {
        emit_caption_blank_lines(builder, window, end_pos, cap_start);
        emit_table_caption(builder, window, cap_start, cap_end, config);
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
    use super::super::container_prefix::ContainerPrefix;
    use super::*;

    #[test]
    fn test_grid_separator_detection() {
        assert!(try_parse_grid_separator("+---+---+").is_some());
        assert!(try_parse_grid_separator("+===+===+").is_some());
        assert!(try_parse_grid_separator("+---------------+---------------+").is_some());
        assert!(try_parse_grid_separator("+:---:+").is_some()); // center aligned
        assert!(try_parse_grid_separator("not a separator").is_none());
        assert!(try_parse_grid_separator("|---|---|").is_none()); // pipe table sep

        // A grid border must sit at column 0 of its container content; an
        // indented border is not a grid table (matches pandoc, which parses
        // an indented `+---+` as a paragraph). Detection runs on the
        // container-stripped line, so any remaining leading space disqualifies.
        assert!(try_parse_grid_separator(" +---+---+").is_none());
        assert!(try_parse_grid_separator("  +---+---+").is_none());
        assert!(try_parse_grid_separator("   +===+===+").is_none());
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
        let prefix = ContainerPrefix::default();
        let window = StrippedLines::new(&input, 0, &prefix);
        let result = try_parse_grid_table(&window, &mut builder, &ParserOptions::default());

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
        let prefix = ContainerPrefix::default();
        let window = StrippedLines::new(&input, 0, &prefix);
        let result = try_parse_grid_table(&window, &mut builder, &ParserOptions::default());

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
        let prefix = ContainerPrefix::default();
        let window = StrippedLines::new(&input, 0, &prefix);
        let result = try_parse_grid_table(&window, &mut builder, &ParserOptions::default());

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
        let prefix = ContainerPrefix::default();
        let window = StrippedLines::new(&input, 0, &prefix);
        let result = try_parse_grid_table(&window, &mut builder, &ParserOptions::default());

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
        let prefix = ContainerPrefix::default();
        let window = StrippedLines::new(&input, 2, &prefix);
        let result = try_parse_grid_table(&window, &mut builder, &ParserOptions::default());

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
        let prefix = ContainerPrefix::default();
        let window = StrippedLines::new(&input, 0, &prefix);
        let result = try_parse_grid_table(&window, &mut builder, &ParserOptions::default());

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
    lines: &(impl LineView + ?Sized),
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

    for i in (row_start + 1)..row_end {
        let (content, _) = strip_newline(lines.line(i));
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
    window: &StrippedLines<'_, '_>,
    builder: &mut GreenNodeBuilder<'static>,
    config: &ParserOptions,
) -> Option<usize> {
    let lines = window.raw();
    let start_pos = window.pos();
    if start_pos >= lines.len() {
        return None;
    }

    // Cheap gate: a multiline table's first line is either a full-width dash
    // separator or a column separator. Table detection runs at every block
    // start, so any per-line work for every paragraph that can't begin a
    // multiline table was quadratic on large documents. Peek just the dispatch
    // line via `strip_at` and bail before any further scanning.
    let first_line = window.strip_at(start_pos);

    // First line can be either:
    // 1. A full-width dash separator (for tables with headers)
    // 2. A column separator (for headerless tables)
    let is_full_width_start = try_parse_multiline_separator(first_line).is_some();
    let is_column_sep_start = !is_full_width_start && is_column_separator(first_line);
    if !is_full_width_start && !is_column_sep_start {
        return None;
    }

    // Detection scans read the container-prefix-stripped view lazily through the
    // window (see `LineView`) so a multiline table nested in `list → blockquote`
    // (e.g. `- > ----`) has its `  > ` prefix removed before the
    // separator/blank-row shape checks. The interior `>`-only row then strips to
    // `""` and registers as a blank row separator. With an empty prefix the
    // stripped view equals the raw lines. Scans stop at the first blank/closing
    // line, so only a bounded range is stripped. Emission re-emits the prefix
    // bytes as tokens via the window; captions read raw `lines`.
    let headerless_columns = if is_column_sep_start {
        try_parse_table_separator(window.line(start_pos))
    } else {
        None
    };

    // A headerless opening with at least one multi-dash column run is a genuine
    // table border (`------  ------`), as opposed to a spaced thematic break
    // whose "columns" are all single dashes (`- - - - -`). Only a genuine border
    // may close on a bare continuous dash run (the closer broadening below);
    // otherwise `- - - - -` would swallow following blocks up to the next
    // thematic break. A column-separator closer still works for either shape.
    let opening_has_wide_column = headerless_columns
        .as_deref()
        .is_some_and(|cols| cols.iter().any(|col| col.end - col.start >= 2));

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
        let line = window.line(pos);

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
                let next = window.line(pos);
                let is_valid_closer = if is_full_width_start {
                    try_parse_multiline_separator(next).is_some()
                } else {
                    // A headerless table may close with a column separator
                    // (`---- ----`) or a single continuous dash run (`--------`).
                    // The latter is rejected by `is_column_separator` (one dash
                    // group reads as a thematic rule), so accept it via the
                    // multiline-separator check too — but only for a genuine
                    // border (see `opening_has_wide_column`). Matches pandoc,
                    // which ends the table on either shape.
                    is_column_separator(next)
                        || (opening_has_wide_column
                            && try_parse_multiline_separator(next).is_some())
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

    // A blank line between rows is one way to tell a multiline table from a
    // simple one, but not the only one. A full-width top border (the
    // `is_full_width_start` case) already distinguishes a multiline table from
    // a simple table, so pandoc accepts it even when every row is a single line
    // with no interior blanks; the required column separator and closing border
    // (checked above and below) keep a bare thematic break from matching. Only
    // the headerless, column-separator-started shape still needs the
    // single-row guard.
    if !found_blank_line && is_column_sep_start {
        let columns = headerless_columns.as_deref()?;
        if !is_headerless_single_row_without_blank(window, start_pos + 1, pos - 1, columns) {
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
    let columns = try_parse_table_separator(window.line(column_sep_pos))
        .expect("Column separator must be valid");

    // Check for caption before table
    let caption_before = find_caption_before_table(window, start_pos);

    // Check for caption after table
    let caption_after = if caption_before.is_some() {
        None
    } else {
        find_caption_after_table(window, end_pos)
    };

    // Build the multiline table
    builder.start_node(SyntaxKind::MULTILINE_TABLE.into());

    // Emit caption before if present
    if let Some((cap_start, cap_end)) = caption_before {
        emit_table_caption(builder, window, cap_start, cap_end, config);
        // Emit blank line between caption and table if present
        emit_caption_blank_lines(builder, window, cap_end, start_pos);
    }

    // Emit opening separator. The dispatch line's prefix was already consumed
    // by core (`dispatch_tail`); a non-dispatch start (caption-before case)
    // re-emits its `  > ` prefix via `emit_prefix_at`.
    builder.start_node(SyntaxKind::TABLE_SEPARATOR.into());
    let tail = window.emit_or_dispatch_tail(builder, start_pos);
    emit_separator_tokens(builder, tail);
    builder.finish_node();

    // Track state for emitting. Accumulate ABSOLUTE indices of the lines making
    // up a multi-line row so each line's container prefix can be re-emitted via
    // the window.
    let mut in_header = has_header;
    let mut current_row_indices: Vec<usize> = Vec::new();

    for i in (start_pos + 1)..end_pos {
        let line = window.line(i);
        // Column separator (header/body divider)
        if i == column_sep_pos {
            // Emit any accumulated header lines
            if !current_row_indices.is_empty() {
                emit_multiline_table_row(
                    builder,
                    window,
                    &current_row_indices,
                    &columns,
                    SyntaxKind::TABLE_HEADER,
                    config,
                );
                current_row_indices.clear();
            }

            builder.start_node(SyntaxKind::TABLE_SEPARATOR.into());
            let tail = window.emit_or_dispatch_tail(builder, i);
            emit_separator_tokens(builder, tail);
            builder.finish_node();
            in_header = false;
            continue;
        }

        // Closing separator (full-width or column separator at end)
        if try_parse_multiline_separator(line).is_some() || is_column_separator(line) {
            // Emit any accumulated row lines
            if !current_row_indices.is_empty() {
                let kind = if in_header {
                    SyntaxKind::TABLE_HEADER
                } else {
                    SyntaxKind::TABLE_ROW
                };
                emit_multiline_table_row(
                    builder,
                    window,
                    &current_row_indices,
                    &columns,
                    kind,
                    config,
                );
                current_row_indices.clear();
            }

            builder.start_node(SyntaxKind::TABLE_SEPARATOR.into());
            let tail = window.emit_or_dispatch_tail(builder, i);
            emit_separator_tokens(builder, tail);
            builder.finish_node();
            continue;
        }

        // Blank line (row separator)
        if line.trim().is_empty() {
            // Emit accumulated row
            if !current_row_indices.is_empty() {
                let kind = if in_header {
                    SyntaxKind::TABLE_HEADER
                } else {
                    SyntaxKind::TABLE_ROW
                };
                emit_multiline_table_row(
                    builder,
                    window,
                    &current_row_indices,
                    &columns,
                    kind,
                    config,
                );
                current_row_indices.clear();
            }

            // Re-emit the interior `>`-only separator row's container prefix
            // (`  > `) inside the BLANK_LINE node so it round-trips losslessly.
            builder.start_node(SyntaxKind::BLANK_LINE.into());
            let tail = window.emit_or_dispatch_tail(builder, i);
            builder.token(SyntaxKind::BLANK_LINE.into(), tail);
            builder.finish_node();
            continue;
        }

        // Content line - accumulate for current row
        current_row_indices.push(i);
    }

    // Emit any remaining accumulated lines
    if !current_row_indices.is_empty() {
        let kind = if in_header {
            SyntaxKind::TABLE_HEADER
        } else {
            SyntaxKind::TABLE_ROW
        };
        emit_multiline_table_row(
            builder,
            window,
            &current_row_indices,
            &columns,
            kind,
            config,
        );
    }

    // Emit caption after if present
    if let Some((cap_start, cap_end)) = caption_after {
        emit_caption_blank_lines(builder, window, end_pos, cap_start);
        emit_table_caption(builder, window, cap_start, cap_end, config);
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
        let column_start = column_offset_to_byte_index(line_content, column.start);
        let column_end = column_offset_to_byte_index(line_content, column.end);

        // Extract FULL text for this column (including whitespace)
        let cell_text = if column_start < column_end {
            &line_content[column_start..column_end]
        } else if column_start < line_content.len() {
            &line_content[column_start..]
        } else {
            ""
        };

        cells.push(cell_text.to_string());
    }

    cells
}

/// Emit a multiline table row with inline parsing (Phase 7.1).
///
/// `indices` are ABSOLUTE line indices into the window's raw buffer; each
/// physical line re-emits its container prefix (`  > `) via the window before
/// its content. With an empty prefix the tails equal the raw lines, so emission
/// is byte-identical to the pre-window path.
fn emit_multiline_table_row(
    builder: &mut GreenNodeBuilder<'static>,
    window: &StrippedLines<'_, '_>,
    indices: &[usize],
    columns: &[Column],
    kind: SyntaxKind,
    config: &ParserOptions,
) {
    if indices.is_empty() {
        return;
    }

    builder.start_node(kind.into());

    // Emit the first line's container prefix as tokens, then slice cells from
    // the prefix-stripped tail (for CST losslessness, only the first physical
    // line is parsed into cells; continuation lines stay verbatim TEXT).
    let first_line = window.emit_or_dispatch_tail(builder, indices[0]);
    let cell_contents = extract_first_line_cell_contents(first_line, columns);
    let (trimmed, newline_str) = strip_newline(first_line);
    let mut current_pos = 0;

    for (col_idx, column) in columns.iter().enumerate() {
        let cell_text = &cell_contents[col_idx];
        let cell_start = column_offset_to_byte_index(trimmed, column.start);
        let cell_end = column_offset_to_byte_index(trimmed, column.end);

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

    // Emit continuation lines as TEXT to preserve exact line structure,
    // re-emitting each line's container prefix first.
    for &idx in &indices[1..] {
        let tail = window.emit_or_dispatch_tail(builder, idx);
        emit_line_tokens(builder, tail);
    }

    builder.finish_node();
}

#[cfg(test)]
mod multiline_table_tests {
    use super::super::container_prefix::ContainerPrefix;
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
        let prefix = ContainerPrefix::default();
        let window = StrippedLines::new(&input, 0, &prefix);
        let result = try_parse_multiline_table(&window, &mut builder, &ParserOptions::default());

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
        let prefix = ContainerPrefix::default();
        let window = StrippedLines::new(&input, 0, &prefix);
        let result = try_parse_multiline_table(&window, &mut builder, &ParserOptions::default());

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
        let prefix = ContainerPrefix::default();
        let window = StrippedLines::new(&input, 0, &prefix);
        let result = try_parse_multiline_table(&window, &mut builder, &ParserOptions::default());

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
        let prefix = ContainerPrefix::default();
        let window = StrippedLines::new(&input, 0, &prefix);
        let result = try_parse_multiline_table(&window, &mut builder, &ParserOptions::default());

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
        let prefix = ContainerPrefix::default();
        let window = StrippedLines::new(&input, 0, &prefix);
        let result = try_parse_multiline_table(&window, &mut builder, &ParserOptions::default());

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
        let prefix = ContainerPrefix::default();
        let window = StrippedLines::new(&input, 0, &prefix);
        let result = try_parse_multiline_table(&window, &mut builder, &ParserOptions::default());

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
        let prefix = ContainerPrefix::default();
        let window = StrippedLines::new(&input, 0, &prefix);
        let result = try_parse_multiline_table(&window, &mut builder, &ParserOptions::default());

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
        let prefix = ContainerPrefix::default();
        let window = StrippedLines::new(&input, 0, &prefix);
        let result = try_parse_multiline_table(&window, &mut builder, &ParserOptions::default());

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
