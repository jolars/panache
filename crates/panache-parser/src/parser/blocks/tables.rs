//! Simple table parsing for Pandoc's simple_tables extension.

use crate::options::{Dialect, ParserOptions};
use crate::syntax::SyntaxKind;
use rowan::GreenNodeBuilder;
use unicode_width::UnicodeWidthChar;

use crate::parser::utils::attributes::{
    emit_attribute_node, try_parse_trailing_attributes_with_pos,
};
use crate::parser::utils::helpers::{emit_line_tokens, emit_separator_tokens, strip_newline};
use crate::parser::utils::inline_emission;

use super::container_prefix::{ContainerPrefix, FrameVerdict, StrippedLines};
use super::reference_links::try_parse_footnote_marker;

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
    fn line(&self, i: usize) -> &str;
    fn line_count(&self) -> usize;
    fn frame_verdict(&self, i: usize) -> FrameVerdict<'_> {
        FrameVerdict::Inside { rest: self.line(i) }
    }

    /// Whether line `i` ends the enclosing container's line run — the
    /// second half of the container bound, for the boundary the frame
    /// verdict cannot see.
    ///
    /// [`Self::frame_verdict`] bounds a scan by *indent*, which settles
    /// every dedent that follows a blank line. It says nothing about a
    /// bare `:::`, which carries no indent yet closes the div it sits
    /// in: pandoc collects a list item's / definition body's /
    /// blockquote's raw lines with `notFollowedByDivCloser`, so the run
    /// stops there and the block the scan was tracking cannot continue
    /// past it. Without this bound a table swallows the fence as a row
    /// and the div never closes.
    ///
    /// Defaults to `false`: a raw line slice has no container to end.
    fn ends_container_lines(&self, _i: usize) -> bool {
        false
    }

    /// For a line that ends the container's line run (see
    /// [`Self::ends_container_lines`]): whether the terminated run
    /// supplies, on reparse, the trailing blank line pandoc's headered
    /// simple-table footer (`blanklines`) needs. A terminator
    /// contiguous with the table's last row cuts the collected raw off
    /// without a blank; the footer then fails and the lines reparse as
    /// a paragraph — unless a blockquote or footnote body in the
    /// terminated chain restores the blank (pandoc reparses a quote's
    /// raw with `"\n\n"` appended and a note's with `"\n"`). See the
    /// `*_supplies_blank` fields on `ContainerPrefix`.
    ///
    /// Defaults to `true` (blank-like): meaningful only where
    /// [`Self::ends_container_lines`] can fire.
    fn run_end_supplies_blank(&self, _i: usize) -> bool {
        true
    }
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
    fn frame_verdict(&self, i: usize) -> FrameVerdict<'_> {
        if i == self.dispatch_pos() {
            FrameVerdict::Inside {
                rest: self.strip_at(i),
            }
        } else {
            self.prefix().resolve(self.raw()[i])
        }
    }
    fn ends_container_lines(&self, i: usize) -> bool {
        let prefix = self.prefix();
        (prefix.div_closer_ends_lines() && line_is_fenced_div_closer(self.raw()[i]))
            || note_marker_ends_lines(prefix, self.raw()[i])
            || list_start_ends_lines(prefix, self.raw()[i])
            || html_closer_ends_lines(prefix, self.raw()[i])
    }
    fn run_end_supplies_blank(&self, i: usize) -> bool {
        let prefix = self.prefix();
        let raw = self.raw()[i];
        if prefix.div_closer_ends_lines() && line_is_fenced_div_closer(raw) {
            return prefix.div_closer_supplies_blank();
        }
        if note_marker_ends_lines(prefix, raw) {
            return true;
        }
        if list_start_ends_lines(prefix, raw) {
            return prefix.list_start_supplies_blank();
        }
        if html_closer_ends_lines(prefix, raw) {
            return prefix.html_closer_supplies_blank();
        }
        true
    }
}

/// Pandoc's `noteBlock` stops collecting at a new note marker with at
/// most 3 spaces in the note's outer frame — a verdict failing at or
/// before the note's own op. A failure past it means the line reached
/// the note's frame, so the marker is note content (see
/// `note_marker_op_bound`). The marker test runs on the resolved tail:
/// inside a blockquote the raw line still carries its `>` prefix.
fn note_marker_ends_lines(prefix: &ContainerPrefix, raw: &str) -> bool {
    let Some(bound) = prefix.note_marker_op_bound() else {
        return false;
    };
    match prefix.resolve(raw) {
        FrameVerdict::Dedented { rest, op_index, .. }
        | FrameVerdict::FakedIndent { rest, op_index }
            if op_index <= bound =>
        {
            rest.starts_with("[^") && try_parse_footnote_marker(rest).is_some()
        }
        _ => false,
    }
}

/// Inside markdown-in-html, pandoc guards its line collectors with
/// `notFollowedByHtmlCloser`: a line opening with the close form of
/// the surrounding HTML block's tag ends the run — the closer belongs
/// to the HTML block, not to whatever block the container's content
/// was in the middle of. Armed by `ContainerPrefix::html_closer_tag`
/// (ordering-gated at the tag-holding list item; see the field).
///
/// The closer's indent tolerance is the tag-holding item's content
/// column: pandoc's item gobble eats at most that many columns before
/// the guard's `htmlTag` runs, and `htmlTag` skips no whitespace — so
/// `  </div>` at the content column ends the run while one extra space
/// keeps it lazy content (both probed). A line still carrying a `>`
/// marker after its indent fails the tag parse and stays ordinary
/// content.
pub(crate) fn html_closer_ends_lines(prefix: &ContainerPrefix, raw: &str) -> bool {
    use super::super::utils::container_stack::leading_indent;
    use super::html_blocks::{HtmlBlockType, try_parse_html_block_start};

    let Some((tag, content_col)) = prefix.html_closer_tag() else {
        return false;
    };
    let (cols, bytes) = leading_indent(raw);
    if cols > content_col as usize {
        return false;
    }
    matches!(
        try_parse_html_block_start(&raw[bytes..], false),
        Some(HtmlBlockType::BlockTag {
            tag_name,
            is_closing: true,
            ..
        }) if tag_name.eq_ignore_ascii_case(tag)
    )
}

/// Pandoc collects a list item's continuation lines with
/// `listContinuationLine`, which stops at a `listStart` reached by at
/// most 3 spaces in the frame the list was parsed in — for a nested
/// list, the enclosing item's content reparse, so the tolerance is 3
/// columns past the start of the marker's ladder band (see
/// `ContainerPrefix::list_start_band_start`). A three-way split, all
/// verified against `pandoc -f markdown -t native`: a marker at or
/// past the item's content column is nested-list content (the full
/// resolve stays inside the frame); one past its band's tolerance but
/// short of the content column is a lazy continuation; one within the
/// tolerance ends the run.
fn list_start_ends_lines(prefix: &ContainerPrefix, raw: &str) -> bool {
    use super::super::utils::container_stack::leading_indent;
    use super::container_prefix::StripOp;
    use super::lists::{OpenListHint, try_parse_list_marker_with};

    let Some(detect) = prefix.list_start_detect() else {
        return false;
    };
    let Some(la_index) = prefix
        .ops()
        .iter()
        .rposition(|op| matches!(op, StripOp::ListAdvance(_)))
    else {
        return false;
    };
    match prefix.resolve(raw) {
        FrameVerdict::Dedented { op_index, .. } | FrameVerdict::FakedIndent { op_index, .. }
            if op_index <= la_index => {}
        _ => return false,
    }
    match prefix.without_innermost_list_advance().resolve(raw) {
        FrameVerdict::Inside { rest } => {
            let indent = leading_indent(rest).0;
            indent <= prefix.list_start_band_start(indent) + 3
                && try_parse_list_marker_with(rest, detect, OpenListHint::None).is_some()
        }
        _ => false,
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
        if i == self.0.dispatch_pos() {
            self.0.prefix().strip_dispatch_line(self.0.raw()[i])
        } else {
            self.0.prefix().strip(self.0.raw()[i])
        }
    }
    fn line_count(&self) -> usize {
        self.0.raw().len()
    }
    fn ends_container_lines(&self, i: usize) -> bool {
        self.0.ends_container_lines(i)
    }
    fn run_end_supplies_blank(&self, i: usize) -> bool {
        self.0.run_end_supplies_blank(i)
    }
}

/// A [`LineView`] that strips `content_col` columns of leading whitespace
/// from every line of an inner view, on top of whatever the inner view
/// strips. The first-content-line term lookahead reads captions through
/// this: a footnote or definition body's content column is 4, so its
/// `    : cap` line fails [`try_parse_caption_prefix`]'s <= 3 indent gate
/// when probed raw, and a caption-led table in the body would wrongly
/// promote the body's first line to a definition term.
///
/// `frame_verdict` composes: a line must reach the inner view's frame
/// *and* the content column to count as inside. Blank lines fall out as
/// `Dedented`, which the caption probe already exempts.
pub(crate) struct ContentColStripView<'v, V: LineView + ?Sized> {
    pub inner: &'v V,
    pub content_col: usize,
}

impl<V: LineView + ?Sized> LineView for ContentColStripView<'_, V> {
    fn line(&self, i: usize) -> &str {
        super::container_prefix::strip_list_indent(self.inner.line(i), self.content_col)
    }
    fn line_count(&self) -> usize {
        self.inner.line_count()
    }
    fn frame_verdict(&self, i: usize) -> FrameVerdict<'_> {
        let inner_verdict = self.inner.frame_verdict(i);
        if !inner_verdict.reaches_frame() {
            return inner_verdict;
        }
        super::container_prefix::resolve_content_indent(self.inner.line(i), self.content_col)
    }
    fn ends_container_lines(&self, i: usize) -> bool {
        self.inner.ends_container_lines(i)
    }
    fn run_end_supplies_blank(&self, i: usize) -> bool {
        self.inner.run_end_supplies_blank(i)
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
    let (trimmed, newline_str) = strip_newline(trimmed);
    let leading_spaces = line.len() - trimmed.len() - newline_str.len();

    if leading_spaces > 3 {
        return None;
    }

    if trimmed.contains('*') || trimmed.contains('_') {
        return None;
    }

    if !trimmed.contains('-') {
        return None;
    }

    if !trimmed.chars().all(|c| c == '-' || c == ' ') {
        return None;
    }

    let dash_groups: Vec<_> = trimmed.split(' ').filter(|s| !s.is_empty()).collect();
    if dash_groups.len() <= 1 {
        return None;
    }

    let columns = extract_columns(trimmed, leading_spaces);

    if columns.is_empty() {
        return None;
    }

    Some(columns)
}

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
/// position just past the closer, which the caller emits as a closing
/// `TABLE_SEPARATOR`.
fn find_single_column_table_end(lines: &(impl LineView + ?Sized), start: usize) -> Option<usize> {
    let mut saw_row = false;
    for i in start..lines.line_count() {
        let line = lines.line(i);
        if line.trim().is_empty() {
            return None;
        }
        if lines.ends_container_lines(i) {
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

fn try_parse_caption_prefix(line: &str) -> Option<(usize, &str)> {
    let trimmed = line.trim_start();
    let leading_spaces = line.len() - trimmed.len();

    if leading_spaces > 3 {
        return None;
    }

    if let Some(rest) = trimmed.strip_prefix("Table:") {
        Some((leading_spaces + 6, rest))
    } else if let Some(rest) = trimmed.strip_prefix("table:") {
        Some((leading_spaces + 6, rest))
    } else if let Some(rest) = trimmed.strip_prefix(':') {
        if rest.starts_with(|c: char| c.is_whitespace()) {
            Some((leading_spaces + 1, rest))
        } else {
            None
        }
    } else {
        None
    }
}

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

fn line_is_fenced_div_closer(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.len() >= 3 && trimmed.chars().all(|c| c == ':')
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

    if is_bare_colon_caption_start(lines.line(pos))
        && pos > 0
        && !lines.line(pos - 1).trim().is_empty()
        && !line_is_fenced_div_fence(lines.line(pos - 1))
    {
        return false;
    }
    true
}

fn is_grid_table_start(line: &str) -> bool {
    try_parse_grid_separator(line).is_some()
}

fn is_multiline_table_start(line: &str) -> bool {
    is_multiline_border(line)
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

    if !is_valid_caption_start_before_table(lines, caption_pos) {
        return false;
    }

    let mut pos = caption_pos + 1;

    let inside_container = |i: usize| {
        let line = lines.line(i);
        line.trim().is_empty() || lines.frame_verdict(i).reaches_frame()
    };

    while pos < lines.line_count()
        && !lines.line(pos).trim().is_empty()
        && !lines.ends_container_lines(pos)
    {
        if !inside_container(pos) {
            return false;
        }
        if try_parse_table_separator(lines.line(pos)).is_some() {
            return true;
        }
        pos += 1;
    }

    if pos < lines.line_count() && lines.line(pos).trim().is_empty() {
        pos += 1;
    }

    if pos < lines.line_count() && !inside_container(pos) {
        return false;
    }

    if pos < lines.line_count() && lines.ends_container_lines(pos) {
        return false;
    }

    table_grid_starts_at(lines, pos)
}

fn table_grid_starts_at(lines: &(impl LineView + ?Sized), pos: usize) -> bool {
    if pos >= lines.line_count() {
        return false;
    }
    let line = lines.line(pos);

    if is_grid_table_start(line) {
        return true;
    }

    if is_multiline_table_start(line) {
        return true;
    }

    if try_parse_table_separator(line).is_some() {
        return true;
    }

    if pos + 1 < lines.line_count()
        && !line.trim().is_empty()
        && !lines.ends_container_lines(pos + 1)
    {
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
        && !lines.ends_container_lines(end)
    {
        end += 1;
    }
    Some((start, end))
}

/// Find caption before table (if any).
/// Returns (caption_start, caption_end) positions, or None.
///
/// `dispatch` bounds the backward scan: a caption-before is only ever
/// legitimate when the core dispatched the table *at* the caption line (the
/// dispatcher's caption lookahead anchors dispatch there), so a caption
/// candidate above `dispatch` is a line the core already parsed and emitted
/// as other content — e.g. a definition body's own `:   a` marker line —
/// and claiming it would duplicate its bytes (pandoc reads such tables with
/// an empty caption).
fn find_caption_before_table(
    lines: &(impl LineView + ?Sized),
    table_start: usize,
    dispatch: usize,
) -> Option<(usize, usize)> {
    if table_start == 0 {
        return None;
    }

    let mut pos = table_start - 1;

    if lines.line(pos).trim().is_empty() {
        if pos == 0 {
            return None;
        }
        pos -= 1;
    }

    let caption_end = pos + 1; // End is exclusive

    let starts_its_block = |p: usize| {
        p == 0 || lines.line(p - 1).trim().is_empty() || line_is_fenced_div_fence(lines.line(p - 1))
    };

    if !is_valid_caption_start_before_table(lines, pos) {
        let mut scan_pos = pos;
        while scan_pos > 0 {
            scan_pos -= 1;
            let line = lines.line(scan_pos);

            if line.trim().is_empty() || lines.ends_container_lines(scan_pos) {
                return None;
            }

            if is_valid_caption_start_before_table(lines, scan_pos) {
                if scan_pos < dispatch {
                    return None;
                }
                if !starts_its_block(scan_pos) {
                    return None;
                }
                if previous_nonblank_looks_like_table(lines, scan_pos) {
                    return None;
                }
                return Some((scan_pos, caption_end));
            }
        }
        None
    } else {
        if pos < dispatch {
            return None;
        }
        if !starts_its_block(pos) {
            return None;
        }
        if previous_nonblank_looks_like_table(lines, pos) {
            return None;
        }
        Some((pos, caption_end))
    }
}

fn previous_nonblank_looks_like_table(lines: &(impl LineView + ?Sized), pos: usize) -> bool {
    if pos == 0 {
        return false;
    }
    let mut i = pos;
    while i > 0 && lines.line(i - 1).trim().is_empty() {
        i -= 1;
    }
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

fn find_caption_after_table(
    lines: &(impl LineView + ?Sized),
    table_end: usize,
) -> Option<(usize, usize)> {
    if table_end >= lines.line_count() {
        return None;
    }

    let mut pos = table_end;

    if pos < lines.line_count() && lines.line(pos).trim().is_empty() {
        pos += 1;
    }

    if pos >= lines.line_count() || lines.ends_container_lines(pos) {
        return None;
    }

    if is_table_caption_start(lines.line(pos)) {
        let caption_start = pos;
        let mut caption_end = caption_start + 1;
        while caption_end < lines.line_count()
            && !lines.line(caption_end).trim().is_empty()
            && !lines.ends_container_lines(caption_end)
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

        let tail = window.emit_or_dispatch_tail(builder, abs);

        if i == 0 {
            let trimmed = tail.trim_start();
            let leading_ws_len = tail.len() - trimmed.len();

            if leading_ws_len > 0 {
                builder.token(SyntaxKind::WHITESPACE.into(), &tail[..leading_ws_len]);
            }

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

                let rest_start = leading_ws_len + prefix_len;
                if rest_start < tail.len() {
                    emit_caption_line_text(builder, &tail[rest_start..], config, lift_attrs);
                }
            } else {
                emit_caption_line_text(builder, &tail[leading_ws_len..], config, lift_attrs);
            }
        } else {
            emit_caption_line_text(builder, tail, config, lift_attrs);
        }
    }

    builder.finish_node(); // TABLE_CAPTION
}

fn emit_table_cell(
    builder: &mut GreenNodeBuilder<'static>,
    cell_text: &str,
    config: &ParserOptions,
) {
    builder.start_node(SyntaxKind::TABLE_CELL.into());

    if !cell_text.is_empty() {
        inline_emission::emit_inlines(builder, cell_text, config, false);
    }

    builder.finish_node(); // TABLE_CELL
}

fn determine_alignments(columns: &mut [Column], separator_line: &str, header_line: Option<&str>) {
    for col in columns.iter_mut() {
        let sep_slice = &separator_line[col.start..col.end];

        if let Some(header) = header_line {
            let header_start = column_offset_to_byte_index(header, col.start);
            let header_end = column_offset_to_byte_index(header, col.end);

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

            let header_in_col = &header[header_start..header_end];
            let text_start = header_in_col.len() - header_in_col.trim_start().len();
            let text_end = header_in_col.trim_end().len() + text_start;

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

    let view = UniformStripView(window);

    let gate_first = view.line(start_pos);
    let separator_here = try_parse_table_separator(gate_first).is_some();
    let separator_next = !separator_here
        && start_pos + 1 < lines.len()
        && !gate_first.trim().is_empty()
        && try_parse_table_separator(view.line(start_pos + 1)).is_some();
    let single_column_here =
        !separator_here && !separator_next && parse_single_dash_run(gate_first).is_some();
    if !separator_here && !separator_next && !single_column_here {
        return None;
    }

    let (separator_pos, end_pos, has_closer) = if single_column_here {
        let end_pos = find_single_column_table_end(&view, start_pos + 1)?;
        (start_pos, end_pos, true)
    } else {
        let separator_pos = find_separator_line(&view, start_pos)?;
        let headerless = separator_pos == start_pos;
        let (end_pos, has_closer) = find_table_end(&view, separator_pos + 1, headerless)?;
        (separator_pos, end_pos, has_closer)
    };
    log::trace!("  found separator at line {}", separator_pos + 1);

    let separator_line = view.line(separator_pos);
    let mut columns = if single_column_here {
        parse_single_dash_run(separator_line)?
    } else {
        try_parse_table_separator(separator_line)?
    };

    let has_header = separator_pos > start_pos;
    let header_line = if has_header {
        Some(view.line(separator_pos - 1))
    } else {
        None
    };

    determine_alignments(&mut columns, separator_line, header_line);

    let data_rows = end_pos - separator_pos - 1 - usize::from(has_closer);

    if data_rows == 0 {
        return None;
    }

    let caption_before = find_caption_before_table(&view, start_pos, window.dispatch_pos());

    let caption_after = if caption_before.is_some() {
        None
    } else {
        find_caption_after_table(&view, end_pos)
    };

    builder.start_node(SyntaxKind::SIMPLE_TABLE.into());

    if let Some((cap_start, cap_end)) = caption_before {
        emit_table_caption(builder, window, cap_start, cap_end, config);
        emit_caption_blank_lines(builder, window, cap_end, start_pos);
    }

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

    builder.start_node(SyntaxKind::TABLE_SEPARATOR.into());
    let separator_tail = window.emit_or_dispatch_tail(builder, separator_pos);
    emit_separator_tokens(builder, separator_tail);
    builder.finish_node();

    let rows_end = if has_closer { end_pos - 1 } else { end_pos };
    for idx in (separator_pos + 1)..rows_end {
        emit_table_row(
            builder,
            window,
            idx,
            &columns,
            SyntaxKind::TABLE_ROW,
            config,
        );
    }

    if has_closer {
        builder.start_node(SyntaxKind::TABLE_SEPARATOR.into());
        let closer_tail = window.emit_or_dispatch_tail(builder, end_pos - 1);
        emit_separator_tokens(builder, closer_tail);
        builder.finish_node();
    }

    if let Some((cap_start, cap_end)) = caption_after {
        emit_caption_blank_lines(builder, window, end_pos, cap_start);
        emit_table_caption(builder, window, cap_start, cap_end, config);
    }

    builder.finish_node(); // SimpleTable

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

fn find_separator_line(lines: &(impl LineView + ?Sized), start_pos: usize) -> Option<usize> {
    log::trace!("  find_separator_line from line {}", start_pos + 1);

    log::trace!("    checking first line: {:?}", lines.line(start_pos));
    if try_parse_table_separator(lines.line(start_pos)).is_some() {
        log::trace!("    separator found at first line");
        return Some(start_pos);
    }

    if start_pos + 1 < lines.line_count()
        && !lines.line(start_pos).trim().is_empty()
        && try_parse_table_separator(lines.line(start_pos + 1)).is_some()
    {
        return Some(start_pos + 1);
    }
    None
}

/// Find where the table ends: a closing separator line, or (unless
/// `require_closer`) the first blank line or end of input. Headerless tables
/// pass `require_closer` because pandoc rejects them without the closing
/// dash line (the opener is a horizontal rule instead). Returns
/// `(end_pos, has_closer)` where `has_closer` marks that `end_pos - 1` is a
/// closing dash line the caller must emit as a `TABLE_SEPARATOR`.
fn find_table_end(
    lines: &(impl LineView + ?Sized),
    start_pos: usize,
    require_closer: bool,
) -> Option<(usize, bool)> {
    let mut saw_row = false;
    for i in start_pos..lines.line_count() {
        if lines.line(i).trim().is_empty() {
            return (!require_closer).then_some((i, false));
        }
        if lines.ends_container_lines(i) {
            return (!require_closer && lines.run_end_supplies_blank(i)).then_some((i, false));
        }
        if try_parse_table_separator(lines.line(i)).is_some()
            && (i + 1 >= lines.line_count()
                || lines.line(i + 1).trim().is_empty()
                || lines.ends_container_lines(i + 1))
        {
            if require_closer && !saw_row {
                return None;
            }
            return Some((i + 1, true));
        }
        saw_row = true;
    }
    (!require_closer).then_some((lines.line_count(), false))
}

fn emit_table_row(
    builder: &mut GreenNodeBuilder<'static>,
    window: &StrippedLines<'_, '_>,
    abs_idx: usize,
    columns: &[Column],
    row_kind: SyntaxKind,
    config: &ParserOptions,
) {
    builder.start_node(row_kind.into());

    let line = window.emit_or_dispatch_tail(builder, abs_idx);

    let line = if abs_idx == window.dispatch_pos() {
        let uniform = window.prefix().strip(window.raw()[abs_idx]);
        let retained = line.len().saturating_sub(uniform.len());
        if retained > 0
            && line.ends_with(uniform)
            && line[..retained].chars().all(|c| c == ' ' || c == '\t')
        {
            builder.token(SyntaxKind::WHITESPACE.into(), &line[..retained]);
            uniform
        } else {
            line
        }
    } else {
        line
    };

    let (line_without_newline, newline_str) = strip_newline(line);

    let trimmed = line_without_newline.trim_start();
    let leading_ws_len = line_without_newline.len() - line_without_newline.trim_start().len();
    if leading_ws_len > 0 {
        builder.token(
            SyntaxKind::WHITESPACE.into(),
            &line_without_newline[..leading_ws_len],
        );
    }

    let mut current_pos = 0;

    for (i, col) in columns.iter().enumerate() {
        let cell_start = if col.start >= leading_ws_len {
            column_offset_to_byte_index(trimmed, col.start - leading_ws_len)
        } else {
            0
        };

        let end_offset = columns.get(i + 1).map_or(usize::MAX, |next| next.start);
        let cell_end = if end_offset == usize::MAX {
            trimmed.len()
        } else if end_offset >= leading_ws_len {
            column_offset_to_byte_index(trimmed, end_offset - leading_ws_len)
        } else {
            0
        };

        let cell_text = if cell_start < cell_end && cell_start < trimmed.len() {
            &trimmed[cell_start..cell_end]
        } else {
            ""
        };

        let cell_content = cell_text.trim();
        let cell_content_start = cell_text.len() - cell_text.trim_start().len();

        let content_abs_pos = (cell_start + cell_content_start).min(trimmed.len());
        if current_pos < content_abs_pos {
            builder.token(
                SyntaxKind::WHITESPACE.into(),
                &trimmed[current_pos..content_abs_pos],
            );
        }

        emit_table_cell(builder, cell_content, config);

        current_pos = content_abs_pos + cell_content.len();
    }

    if current_pos < trimmed.len() {
        builder.token(SyntaxKind::WHITESPACE.into(), &trimmed[current_pos..]);
    }

    if !newline_str.is_empty() {
        builder.token(SyntaxKind::NEWLINE.into(), newline_str);
    }

    builder.finish_node();
}

/// Does `line` start within pandoc's `nonindentSpaces` (at most 3 columns,
/// tabs expanded to a 4-column stop)? The line must already be stripped of
/// its container prefix: pandoc measures the indent in the frame the block
/// is parsed in, not in the source.
fn within_nonindent_spaces(line: &str) -> bool {
    use super::super::utils::container_stack::leading_indent;

    leading_indent(line).0 <= 3
}

/// Check if a line is a pipe table separator line.
/// Returns the column alignments if it's a valid separator.
pub(crate) fn try_parse_pipe_separator(line: &str) -> Option<Vec<Alignment>> {
    let trimmed = line.trim();

    if !trimmed.contains('|') && !trimmed.contains('+') {
        return None;
    }

    let cells: Vec<&str> = if trimmed.contains('+') {
        trimmed.split(['|', '+']).collect()
    } else {
        trimmed.split('|').collect()
    };

    let mut alignments = Vec::new();

    for cell in cells {
        let cell = cell.trim();

        if cell.is_empty() {
            continue;
        }

        let starts_colon = cell.starts_with(':');
        let ends_colon = cell.ends_with(':');

        let without_colons = cell.trim_start_matches(':').trim_end_matches(':');

        if without_colons.is_empty() || !without_colons.chars().all(|c| c == '-') {
            return None;
        }

        let alignment = match (starts_colon, ends_colon) {
            (true, true) => Alignment::Center,
            (true, false) => Alignment::Left,
            (false, true) => Alignment::Right,
            (false, false) => Alignment::Default,
        };

        alignments.push(alignment);
    }

    if alignments.is_empty() {
        None
    } else {
        Some(alignments)
    }
}

/// Find cell boundaries before inline emission. Pandoc protects literal code
/// and math, whereas CommonMark table delimiters take precedence over inlines.
fn pipe_delimiters(line: &str, config: &ParserOptions) -> Vec<usize> {
    use crate::parser::inlines::{
        code_spans::try_parse_code_span, hugo::HugoIndex, inline_ir::try_pandoc_math_opaque,
        raw_inline::is_raw_inline,
    };

    let hugo = config
        .extensions
        .hugo_shortcodes
        .then(|| HugoIndex::new(line));
    let mut positions = Vec::new();
    let mut pos = 0;
    while pos < line.len() {
        let rest = &line[pos..];
        if let Some(hugo) = &hugo
            && let Some(span) = hugo.scan_at(line, pos)
            && span.opening.complete
        {
            pos += span.end;
            continue;
        }
        if config.dialect == Dialect::Pandoc {
            if rest.starts_with('`')
                && let Some((len, content, backticks, attributes)) = try_parse_code_span(rest)
            {
                let raw_attribute = config.extensions.raw_attribute
                    && attributes
                        .as_ref()
                        .is_some_and(|(attrs, _)| is_raw_inline(attrs).is_some())
                    && !(backticks == 1
                        && crate::syntax::inline_execution_parts(content, &config.extensions)
                            .is_some());
                pos += if config.extensions.inline_code_attributes || raw_attribute {
                    len
                } else {
                    content.len() + backticks * 2
                };
                continue;
            }
            if let Some(len) = try_pandoc_math_opaque(line, pos, line.len(), config) {
                pos += len;
                continue;
            }
        }
        let ch = rest.chars().next().unwrap();
        if ch == '\\' {
            pos += 1 + rest[1..].chars().next().map_or(0, char::len_utf8);
        } else {
            if ch == '|' {
                positions.push(pos);
            }
            pos += ch.len_utf8();
        }
    }
    positions
}

fn pipe_cell_ranges(line: &str, delimiters: &[usize]) -> Vec<std::ops::Range<usize>> {
    let mut cells = Vec::new();
    let mut start = usize::from(delimiters.first() == Some(&0));
    for &end in delimiters {
        if end >= start {
            cells.push(start..end);
            start = end + 1;
        }
    }
    if start < line.len() || delimiters.is_empty() {
        cells.push(start..line.len());
    }
    cells
}

fn parse_pipe_table_row(line: &str, config: &ParserOptions) -> Vec<String> {
    let trimmed = line.trim();
    pipe_cell_ranges(trimmed, &pipe_delimiters(trimmed, config))
        .into_iter()
        .map(|range| trimmed[range].trim().to_string())
        .collect()
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

    let line = window.emit_or_dispatch_tail(builder, abs_idx);

    let (line_without_newline, newline_str) = strip_newline(line);
    let trimmed = line_without_newline.trim();

    let pipe_positions = pipe_delimiters(trimmed, config);
    let cells = pipe_cell_ranges(trimmed, &pipe_positions);

    let leading_ws_len = line_without_newline.len() - line_without_newline.trim_start().len();
    if leading_ws_len > 0 {
        builder.token(
            SyntaxKind::WHITESPACE.into(),
            &line_without_newline[..leading_ws_len],
        );
    }

    for (idx, range) in cells.iter().enumerate() {
        if range.start > 0 || idx == 0 && trimmed.starts_with('|') {
            builder.token(SyntaxKind::TEXT.into(), "|");
        }

        let cell_with_ws = &trimmed[range.clone()];
        let cell_content = cell_with_ws.trim();

        let cell_leading_ws = &cell_with_ws[..cell_with_ws.len() - cell_with_ws.trim_start().len()];
        if !cell_leading_ws.is_empty() {
            builder.token(SyntaxKind::WHITESPACE.into(), cell_leading_ws);
        }

        emit_table_cell(builder, cell_content, config);

        let cell_trailing_ws_start = cell_leading_ws.len() + cell_content.len();
        if cell_trailing_ws_start < cell_with_ws.len() {
            builder.token(
                SyntaxKind::WHITESPACE.into(),
                &cell_with_ws[cell_trailing_ws_start..],
            );
        }
    }

    if pipe_positions.last() == trimmed.len().checked_sub(1).as_ref() {
        builder.token(SyntaxKind::TEXT.into(), "|");
    }

    let trailing_ws_start = leading_ws_len + trimmed.len();
    if trailing_ws_start < line_without_newline.len() {
        builder.token(
            SyntaxKind::WHITESPACE.into(),
            &line_without_newline[trailing_ws_start..],
        );
    }

    if !newline_str.is_empty() {
        builder.token(SyntaxKind::NEWLINE.into(), newline_str);
    }

    builder.finish_node();
}

/// Probe for a pipe table opening at the window's first line whose header
/// row and delimiter row agree on their column count, returning the lines
/// it would consume.
///
/// A table of two lines or more is one whose delimiter row is the line after
/// the dispatch line, which is what makes the caller's claim safe: the
/// delimiter row owns the column count, so a header with surplus cells
/// re-parses to the same shape after formatting (the formatter leaves such a
/// table byte-for-byte as written rather than writing a column count of its
/// own).
pub(crate) fn opens_multiline_pipe_table(
    window: &StrippedLines<'_, '_>,
    config: &ParserOptions,
) -> Option<usize> {
    let mut probe = GreenNodeBuilder::new();
    let consumed = try_parse_pipe_table(window, &mut probe, config)?;
    (consumed >= 2).then_some(consumed)
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

    if !window.strip_at(start_pos).contains('|') && !is_caption_followed_by_table(window, start_pos)
    {
        return None;
    }

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

    if pipe_delimiters(window.line(actual_start).trim(), config).is_empty() {
        return None;
    }

    let frame = UniformStripView(window);
    if !within_nonindent_spaces(frame.line(actual_start)) {
        return None;
    }

    if config.dialect != Dialect::CommonMark
        && !within_nonindent_spaces(frame.line(actual_start + 1))
    {
        return None;
    }

    let alignments = try_parse_pipe_separator(window.line(actual_start + 1))?;

    let header_cells = parse_pipe_table_row(window.line(actual_start), config);

    if config.dialect == Dialect::CommonMark && header_cells.len() != alignments.len() {
        return None;
    }

    let mut end_pos = actual_start + 2;
    while end_pos < window.line_count() {
        let line = window.line(end_pos);
        if line.trim().is_empty() || window.ends_container_lines(end_pos) {
            break;
        }
        if !line.contains('|') {
            break;
        }
        end_pos += 1;
    }

    let caption_before = caption_before
        .or_else(|| find_caption_before_table(window, actual_start, window.dispatch_pos()));

    let caption_after = if caption_before.is_some() {
        None
    } else {
        find_caption_after_table(window, end_pos)
    };

    builder.start_node(SyntaxKind::PIPE_TABLE.into());

    if let Some((cap_start, cap_end)) = caption_before {
        emit_table_caption(builder, window, cap_start, cap_end, config);
        emit_caption_blank_lines(builder, window, cap_end, actual_start);
    }

    emit_pipe_table_row(
        builder,
        window,
        actual_start,
        SyntaxKind::TABLE_HEADER,
        config,
    );

    builder.start_node(SyntaxKind::TABLE_SEPARATOR.into());
    let separator_tail = window.emit_or_dispatch_tail(builder, actual_start + 1);
    emit_separator_tokens(builder, separator_tail);
    builder.finish_node();

    for idx in (actual_start + 2)..end_pos {
        emit_pipe_table_row(builder, window, idx, SyntaxKind::TABLE_ROW, config);
    }

    if let Some((cap_start, cap_end)) = caption_after {
        emit_caption_blank_lines(builder, window, end_pos, cap_start);
        emit_table_caption(builder, window, cap_start, cap_end, config);
    }

    builder.finish_node(); // PipeTable

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
        let line = "地號xy";
        assert_eq!(column_offset_to_byte_index(line, 4), 6);
        assert_eq!(column_offset_to_byte_index("abcd", 2), 2);
    }

    #[test]
    fn simple_table_cjk_header_keeps_footnote_ref_intact() {
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
            "-------     ------ ----------   -------",
            "",
        ];

        let mut builder = GreenNodeBuilder::new();
        let prefix = ContainerPrefix::default();
        let window = StrippedLines::new(&input, 0, &prefix);
        let result = try_parse_simple_table(&window, &mut builder, &ParserOptions::default());

        assert!(result.is_some());
        assert_eq!(result.unwrap(), 4); // sep + 2 rows + closer
    }

    /// A headerless single-column table dispatched on a list-item
    /// continuation line, sitting at the item's content column plus the
    /// simple-table writer's 2-space self-indent. The dispatch line keeps
    /// its list indent under `strip_line_0_for_emission` (the marker was
    /// not consumed on this line), so detection must read the uniform
    /// strip or the 4 leading columns fail the `<= 3` indent gate and
    /// the table collapses to paragraph text on reparse.
    #[test]
    fn headerless_table_with_self_indent_in_list_item() {
        use super::super::container_prefix::StripOp;

        let input = vec!["    ---\n", "    x\n", "    ---\n", "\n"];

        let mut builder = GreenNodeBuilder::new();
        let prefix = ContainerPrefix::from_ops(&[StripOp::ListAdvance(2)], false);
        let window = StrippedLines::new(&input, 0, &prefix);
        let result = try_parse_simple_table(&window, &mut builder, &ParserOptions::default());

        assert_eq!(result, Some(3)); // sep + row + closer
    }

    fn assert_closer_is_separator(input: &str, expected_rows: usize) {
        let tree = crate::parser::parse(input, None);
        let table = tree
            .descendants()
            .find(|n| n.kind() == SyntaxKind::SIMPLE_TABLE)
            .expect("input should parse as a simple table");
        let separators = table
            .children()
            .filter(|n| n.kind() == SyntaxKind::TABLE_SEPARATOR)
            .count();
        assert_eq!(separators, 2, "opener and closer should both be separators");
        let rows: Vec<_> = table
            .children()
            .filter(|n| n.kind() == SyntaxKind::TABLE_ROW)
            .map(|n| n.text().to_string())
            .collect();
        assert_eq!(rows.len(), expected_rows, "rows: {rows:?}");
        for row in &rows {
            assert!(
                !row.trim().chars().all(|c| c == '-' || c.is_whitespace()),
                "all-dash closer emitted as TABLE_ROW: {row:?}"
            );
        }
    }

    #[test]
    fn headerless_multi_column_closer_is_separator() {
        assert_closer_is_separator("--- ---\nfoo bar\n--- ---\n", 1);
    }

    #[test]
    fn header_with_closer_is_separator() {
        assert_closer_is_separator("foo bar\n--- ---\n1   2\n--- ---\n", 1);
    }

    #[test]
    fn headerless_single_column_closer_is_separator() {
        assert_closer_is_separator("----------\nfirst row\nsecond row\n----------\n", 2);
    }

    #[test]
    fn headerless_multi_column_requires_closer() {
        let input = vec!["--- ---", "foo bar", "", "after"];

        let mut builder = GreenNodeBuilder::new();
        let prefix = ContainerPrefix::default();
        let window = StrippedLines::new(&input, 0, &prefix);
        let result = try_parse_simple_table(&window, &mut builder, &ParserOptions::default());

        assert!(result.is_none());
    }

    #[test]
    fn headerless_multi_column_requires_closer_at_eof() {
        let input = vec!["--- ---", "foo bar"];

        let mut builder = GreenNodeBuilder::new();
        let prefix = ContainerPrefix::default();
        let window = StrippedLines::new(&input, 0, &prefix);
        let result = try_parse_simple_table(&window, &mut builder, &ParserOptions::default());

        assert!(result.is_none());
    }

    #[test]
    fn header_with_closer_requires_rows() {
        let input = vec!["foo bar", "--- ---", "--- ---", ""];

        let mut builder = GreenNodeBuilder::new();
        let prefix = ContainerPrefix::default();
        let window = StrippedLines::new(&input, 0, &prefix);
        let result = try_parse_simple_table(&window, &mut builder, &ParserOptions::default());

        assert!(result.is_none());
    }

    #[test]
    fn headerless_multi_column_requires_rows() {
        let input = vec!["--- ---", "--- ---", ""];

        let mut builder = GreenNodeBuilder::new();
        let prefix = ContainerPrefix::default();
        let window = StrippedLines::new(&input, 0, &prefix);
        let result = try_parse_simple_table(&window, &mut builder, &ParserOptions::default());

        assert!(result.is_none());
    }

    #[test]
    fn headerless_multi_column_with_closer_at_eof() {
        let input = vec!["--- ---", "foo bar", "--- ---"];

        let mut builder = GreenNodeBuilder::new();
        let prefix = ContainerPrefix::default();
        let window = StrippedLines::new(&input, 0, &prefix);
        let result = try_parse_simple_table(&window, &mut builder, &ParserOptions::default());

        assert_eq!(result, Some(3)); // sep + row + closer
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
        let input = vec!["---", "- a", "- b", "---", ""];

        let mut builder = GreenNodeBuilder::new();
        let prefix = ContainerPrefix::default();
        let window = StrippedLines::new(&input, 0, &prefix);
        let result = try_parse_simple_table(&window, &mut builder, &ParserOptions::default());

        assert_eq!(result, Some(4)); // opener + 2 rows + closer
    }

    #[test]
    fn headerless_single_column_requires_closer() {
        let input = vec!["---", "just prose", "", "after"];

        let mut builder = GreenNodeBuilder::new();
        let prefix = ContainerPrefix::default();
        let window = StrippedLines::new(&input, 0, &prefix);
        let result = try_parse_simple_table(&window, &mut builder, &ParserOptions::default());

        assert!(result.is_none());
    }

    #[test]
    fn headerless_single_column_requires_rows() {
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
        assert!(table_grid_starts_at(&["+---+---+"][..], 0)); // grid
        assert!(table_grid_starts_at(&["----------- -------"][..], 0)); // multiline
        assert!(table_grid_starts_at(&["--- --- ---"][..], 0)); // simple, headerless
        assert!(table_grid_starts_at(&["A | B", "| --- | --- |"][..], 0)); // pipe, header + sep
        assert!(table_grid_starts_at(&["A    B", "--- ---"][..], 0)); // simple, header + sep
        assert!(table_grid_starts_at(&["-------"][..], 0));

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
        let dump = format!("{tree:#?}");
        assert!(
            dump.contains("1."),
            "marker text preserved as a header cell"
        );
        assert!(dump.contains('b'), "overflow cell `b` preserved (lossless)");
    }

    #[test]
    fn lone_ordered_marker_pipe_line_is_a_list() {
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
        let window = StrippedLines::with_dispatch(&input, 2, 0, &prefix);
        let result = try_parse_simple_table(&window, &mut builder, &ParserOptions::default());

        assert!(result.is_some());
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
        let cells = parse_pipe_table_row("| Right | Left | Center |", &ParserOptions::default());
        assert_eq!(cells.len(), 3);
        assert_eq!(cells[0], "Right");
        assert_eq!(cells[1], "Left");
        assert_eq!(cells[2], "Center");

        let cells2 = parse_pipe_table_row("Right | Left | Center", &ParserOptions::default());
        assert_eq!(cells2.len(), 3);
    }

    #[test]
    fn pipe_boundaries_honor_code_attribute_extensions() {
        let mut config = ParserOptions::default();
        let row = "| `x`{title=\"a|b\"} | z |";
        assert_eq!(parse_pipe_table_row(row, &config).len(), 2);
        config.extensions.inline_code_attributes = false;
        assert_eq!(
            parse_pipe_table_row(row, &config),
            ["`x`{title=\"a", "b\"}", "z"]
        );
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
        assert_eq!(result.unwrap(), 6);
    }
}

/// Check if a line is a grid table row separator (starts with +, contains -, ends with +).
/// Returns Some(vec of column info) if valid, None otherwise.
pub(crate) fn try_parse_grid_separator(line: &str) -> Option<Vec<GridColumn>> {
    let trimmed = line.trim_start();
    let leading_spaces = line.len() - trimmed.len();

    if leading_spaces > 0 {
        return None;
    }

    if !trimmed.starts_with('+') || !trimmed.trim_end().ends_with('+') {
        return None;
    }

    let trimmed = trimmed.trim_end();
    let segments: Vec<&str> = trimmed.split('+').collect();

    if segments.len() < 3 {
        return None;
    }

    let mut columns = Vec::new();

    for segment in segments.iter().skip(1).take(segments.len() - 2) {
        if segment.is_empty() {
            continue;
        }

        let seg_trimmed = *segment;

        let inner = seg_trimmed.trim_start_matches(':').trim_end_matches(':');

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

/// A *partial* grid separator — a row boundary under a rowspan, where at
/// least one column's cell continues into the next row and shows spaces
/// between its boundary markers instead of dashes (`+   +---+`,
/// `+---+   |`). Boundary markers may be `+` or `|` (pandoc's grid
/// reader treats both as cell-edge characters), but the line must open
/// with `+`: a `|`-leading hybrid is a content row
/// (`is_grid_content_row`). Requires at least one dashed segment (an
/// all-blank frame is not table structure) and at least one blank
/// segment (a fully dashed line is a full separator,
/// `try_parse_grid_separator`).
pub(crate) fn try_parse_grid_partial_separator(line: &str) -> Option<Vec<GridColumn>> {
    let trimmed = line.trim_start();

    if line.len() != trimmed.len() {
        return None;
    }

    let trimmed = trimmed.trim_end();
    if !trimmed.starts_with('+') || !trimmed.ends_with(['+', '|']) {
        return None;
    }

    let mut columns = Vec::new();
    let mut has_dashes = false;
    let mut has_blank = false;
    for segment in trimmed.split(['+', '|']) {
        if segment.is_empty() {
            continue;
        }
        if segment.chars().all(|c| c == ' ') {
            has_blank = true;
            columns.push(GridColumn {
                is_header_separator: false,
                width: segment.chars().count(),
            });
            continue;
        }
        let inner = segment.trim_start_matches(':').trim_end_matches(':');
        let first_char = inner.chars().next()?;
        if (first_char != '-' && first_char != '=') || !inner.chars().all(|c| c == first_char) {
            return None;
        }
        has_dashes = true;
        columns.push(GridColumn {
            is_header_separator: first_char == '=',
            width: segment.chars().count(),
        });
    }
    (has_dashes && has_blank).then_some(columns)
}

/// Column information for grid tables.
#[derive(Debug, Clone)]
pub(crate) struct GridColumn {
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
pub(crate) fn is_grid_content_row(line: &str) -> bool {
    let trimmed = line.trim_start();
    let leading_spaces = line.len() - trimmed.len();

    if leading_spaces > 3 {
        return false;
    }

    let trimmed = trimmed.trim_end();
    trimmed.starts_with('|') && (trimmed.ends_with('|') || trimmed.ends_with('+'))
}

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

    let first_line = window.emit_or_dispatch_tail(builder, indices[0]);
    let cell_contents = extract_grid_cells_from_line(first_line, columns);
    let (line_without_newline, newline_str) = strip_newline(first_line);
    let trimmed = line_without_newline.trim();
    let expected_pipe_count = columns.len().saturating_add(1);
    let actual_pipe_count = trimmed.chars().filter(|&c| c == '|').count();

    if actual_pipe_count != expected_pipe_count {
        emit_line_tokens(builder, first_line);
        for &idx in &indices[1..] {
            let tail = window.emit_or_dispatch_tail(builder, idx);
            emit_line_tokens(builder, tail);
        }
        builder.finish_node();
        return;
    }

    let leading_ws_len = line_without_newline.len() - line_without_newline.trim_start().len();
    if leading_ws_len > 0 {
        builder.token(
            SyntaxKind::WHITESPACE.into(),
            &line_without_newline[..leading_ws_len],
        );
    }

    if trimmed.starts_with('|') {
        builder.token(SyntaxKind::TEXT.into(), "|");
    }

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

        let cell_trimmed = part.trim();
        let ws_start_len = part.len() - part.trim_start().len();
        if ws_start_len > 0 {
            builder.token(SyntaxKind::WHITESPACE.into(), &part[..ws_start_len]);
        }

        emit_table_cell(builder, cell_content, config);

        let ws_end_start = ws_start_len + cell_trimmed.len();
        if ws_end_start < part.len() {
            builder.token(SyntaxKind::WHITESPACE.into(), &part[ws_end_start..]);
        }

        if idx < cell_contents.len() - 1 || trimmed.ends_with('|') {
            builder.token(SyntaxKind::TEXT.into(), "|");
        }
    }

    let trailing_ws_start = leading_ws_len + trimmed.len();
    if trailing_ws_start < line_without_newline.len() {
        builder.token(
            SyntaxKind::WHITESPACE.into(),
            &line_without_newline[trailing_ws_start..],
        );
    }

    if !newline_str.is_empty() {
        builder.token(SyntaxKind::NEWLINE.into(), newline_str);
    }

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

    let view = UniformStripView(window);

    if try_parse_grid_separator(view.line(start_pos)).is_none()
        && !is_caption_followed_by_table(&view, start_pos)
    {
        return None;
    }

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

    let first_line = view.line(actual_start);
    let _columns = try_parse_grid_separator(first_line)?;

    let mut end_pos = actual_start + 1;
    let mut found_header_sep = false;
    let mut in_footer = false;

    while end_pos < lines.len() {
        let line = view.line(end_pos);

        if line.trim().is_empty() {
            break;
        }

        if view.ends_container_lines(end_pos) {
            break;
        }

        if try_parse_grid_partial_separator(line).is_some() {
            end_pos += 1;
            continue;
        }

        if let Some(sep_cols) = try_parse_grid_separator(line) {
            if sep_cols.iter().any(|c| c.is_header_separator) {
                if !found_header_sep {
                    found_header_sep = true;
                } else if !in_footer {
                    in_footer = true;
                }
            }
            end_pos += 1;
            continue;
        }

        if is_grid_content_row(line) {
            end_pos += 1;
            continue;
        }

        break;
    }

    if end_pos <= actual_start + 1 {
        return None;
    }

    let caption_before = caption_before
        .or_else(|| find_caption_before_table(&view, actual_start, window.dispatch_pos()));

    let caption_after = if caption_before.is_some() {
        None
    } else {
        find_caption_after_table(&view, end_pos)
    };

    builder.start_node(SyntaxKind::GRID_TABLE.into());

    if let Some((cap_start, cap_end)) = caption_before {
        emit_table_caption(builder, window, cap_start, cap_end, config);
        emit_caption_blank_lines(builder, window, cap_end, actual_start);
    }

    let mut past_header_sep = false;
    let mut in_footer_section = false;
    let mut current_row_indices: Vec<usize> = Vec::new();
    let mut current_row_kind = SyntaxKind::TABLE_HEADER;

    for idx in actual_start..end_pos {
        let line = view.line(idx);
        if let Some(sep_cols) = try_parse_grid_separator(line) {
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

            if is_header_sep {
                if !past_header_sep {
                    builder.start_node(SyntaxKind::TABLE_SEPARATOR.into());
                    let tail = window.emit_or_dispatch_tail(builder, idx);
                    emit_separator_tokens(builder, tail);
                    builder.finish_node();
                    past_header_sep = true;
                } else {
                    if !in_footer_section {
                        in_footer_section = true;
                    }
                    builder.start_node(SyntaxKind::TABLE_SEPARATOR.into());
                    let tail = window.emit_or_dispatch_tail(builder, idx);
                    emit_separator_tokens(builder, tail);
                    builder.finish_node();
                }
            } else {
                builder.start_node(SyntaxKind::TABLE_SEPARATOR.into());
                let tail = window.emit_or_dispatch_tail(builder, idx);
                emit_separator_tokens(builder, tail);
                builder.finish_node();
            }
        } else if let Some(sep_cols) = try_parse_grid_partial_separator(line) {
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
            builder.start_node(SyntaxKind::TABLE_SEPARATOR.into());
            let tail = window.emit_or_dispatch_tail(builder, idx);
            emit_separator_tokens(builder, tail);
            builder.finish_node();
        } else if is_grid_content_row(line) {
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

    if !current_row_indices.is_empty()
        && let Some(sep_cols) = try_parse_grid_separator(view.line(actual_start))
    {
        emit_grid_table_row(
            builder,
            window,
            &current_row_indices,
            &sep_cols,
            current_row_kind,
            config,
        );
    }

    if let Some((cap_start, cap_end)) = caption_after {
        emit_caption_blank_lines(builder, window, end_pos, cap_start);
        emit_table_caption(builder, window, cap_start, cap_end, config);
    }

    builder.finish_node(); // GRID_TABLE

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
        let window = StrippedLines::with_dispatch(&input, 2, 0, &prefix);
        let result = try_parse_grid_table(&window, &mut builder, &ParserOptions::default());

        assert!(result.is_some());
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
        assert_eq!(result.unwrap(), 7);
    }
}

/// Check if a line is a multiline table separator (continuous dashes).
/// Multiline table separators span the full width and are all dashes.
/// Returns Some(columns) if valid, None otherwise.
fn try_parse_multiline_separator(line: &str) -> Option<Vec<Column>> {
    let trimmed = line.trim_start();
    let leading_spaces = line.len() - trimmed.len();

    if leading_spaces > 3 {
        return None;
    }

    let trimmed = trimmed.trim_end();

    if trimmed.is_empty() || !trimmed.chars().all(|c| c == '-') {
        return None;
    }

    if trimmed.len() < 2 {
        return None;
    }

    Some(vec![Column {
        start: leading_spaces,
        end: leading_spaces + trimmed.len(),
        alignment: Alignment::Default,
    }])
}

fn is_column_separator(line: &str) -> bool {
    try_parse_table_separator(line).is_some() && !line.contains('*') && !line.contains('_')
}

/// Check if a line can serve as a multiline table's top or bottom border.
///
/// Pandoc's border is `many1 (dashedLine '-')` --- one *or more* dash runs --- so
/// a spaced line (`----- -----`) borders a table exactly like a continuous one
/// (`-----------`). Both shapes therefore open a headed table and close either
/// shape; the narrower `try_parse_multiline_separator` alone would reject the
/// spaced form and leave it to the horizontal-rule parser.
fn is_multiline_border(line: &str) -> bool {
    try_parse_multiline_separator(line).is_some() || is_column_separator(line)
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

struct MultilineShape {
    /// One past the table's last line (the closing border).
    end_pos: usize,
    /// Line whose dash runs define the columns.
    column_sep_pos: usize,
    /// Whether the lines above `column_sep_pos` are a header.
    has_header: bool,
}

/// Scan forward from the window's start for a complete multiline-table shape.
///
/// `is_full_width_start` picks the reading of the opening line: `true` treats it
/// as a top border whose columns come from a later separator line (pandoc's
/// `multilineTable False`), `false` treats it as the column separator itself
/// (pandoc's headerless `multilineTable True`). The caller has already checked
/// that the opening line can serve in the requested role.
fn scan_multiline_table(
    window: &StrippedLines<'_, '_>,
    is_full_width_start: bool,
) -> Option<MultilineShape> {
    let lines = window.raw();
    let start_pos = window.pos();
    let is_column_sep_start = !is_full_width_start;

    if start_pos + 1 >= lines.len() || window.line(start_pos + 1).trim().is_empty() {
        return None;
    }

    let opener_is_continuous = try_parse_multiline_separator(window.line(start_pos)).is_some();

    let headerless_columns = if is_column_sep_start {
        try_parse_table_separator(window.line(start_pos))
    } else {
        None
    };

    let opening_has_wide_column = headerless_columns
        .as_deref()
        .is_some_and(|cols| cols.iter().any(|col| col.end - col.start >= 2));

    let mut pos = start_pos + 1;
    let mut found_column_sep = is_column_sep_start; // Already found if headerless
    let mut column_sep_pos = if is_column_sep_start { start_pos } else { 0 };
    let mut has_header = false;
    let mut found_blank_line = false;
    let mut found_closing_sep = false;
    let mut content_line_count = 0usize;

    let bq_prefixed = window.prefix().bq_depth() > 0;

    while pos < lines.len() {
        let line = window.line(pos);

        if window.ends_container_lines(pos) || (bq_prefixed && lines[pos].trim().is_empty()) {
            break;
        }

        if is_full_width_start && is_column_separator(line) && !found_column_sep {
            found_column_sep = true;
            column_sep_pos = pos;
            has_header = pos > start_pos + 1; // Has header if there's content before column sep
            pos += 1;
            // Pandoc requires the first body row immediately after the header
            // separator. Otherwise a simple table's footer can become a header
            // separator and pull later paragraphs into the table.
            if pos >= lines.len() || window.line(pos).trim().is_empty() {
                return None;
            }
            continue;
        }

        if line.trim().is_empty() {
            found_blank_line = true;
            pos += 1;
            if pos < lines.len()
                && !window.line(pos).trim().is_empty()
                && matches!(
                    window.frame_verdict(pos),
                    FrameVerdict::Dedented { .. } | FrameVerdict::FakedIndent { .. }
                )
            {
                break;
            }
            if pos < lines.len() && !window.ends_container_lines(pos) {
                let next = window.line(pos);
                let is_valid_closer = if is_full_width_start {
                    is_multiline_border(next)
                } else {
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

        if is_full_width_start && is_multiline_border(line) {
            found_closing_sep = true;
            pos += 1;
            break;
        }

        if is_column_sep_start && is_column_separator(line) && content_line_count > 0 {
            found_closing_sep = true;
            pos += 1;
            break;
        }

        content_line_count += 1;
        pos += 1;
    }

    let headerless_single_column = !found_column_sep
        && is_full_width_start
        && opener_is_continuous
        && found_blank_line
        && found_closing_sep;
    if headerless_single_column {
        found_column_sep = true;
        column_sep_pos = start_pos;
    }

    if !found_column_sep {
        return None;
    }

    if !found_blank_line && is_column_sep_start {
        let columns = headerless_columns.as_deref()?;
        if !is_headerless_single_row_without_blank(window, start_pos + 1, pos - 1, columns) {
            return None;
        }
    }

    if !found_closing_sep {
        return None;
    }

    if pos <= start_pos + 2 {
        return None;
    }

    Some(MultilineShape {
        end_pos: pos,
        column_sep_pos,
        has_header,
    })
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

    let first_line = window.strip_at(start_pos);

    let is_continuous_start = try_parse_multiline_separator(first_line).is_some();
    let is_spaced_start = !is_continuous_start && is_column_separator(first_line);
    if !is_continuous_start && !is_spaced_start {
        return None;
    }

    let shape = match scan_multiline_table(window, true) {
        Some(shape) => shape,
        None if is_spaced_start => scan_multiline_table(window, false)?,
        None => return None,
    };
    let MultilineShape {
        end_pos,
        column_sep_pos,
        has_header,
    } = shape;

    let columns = try_parse_table_separator(window.line(column_sep_pos))
        .or_else(|| parse_single_dash_run(window.line(column_sep_pos)))?;

    let caption_before = find_caption_before_table(window, start_pos, window.dispatch_pos());

    let caption_after = if caption_before.is_some() {
        None
    } else {
        find_caption_after_table(window, end_pos)
    };

    builder.start_node(SyntaxKind::MULTILINE_TABLE.into());

    if let Some((cap_start, cap_end)) = caption_before {
        emit_table_caption(builder, window, cap_start, cap_end, config);
        emit_caption_blank_lines(builder, window, cap_end, start_pos);
    }

    builder.start_node(SyntaxKind::TABLE_SEPARATOR.into());
    let tail = window.emit_or_dispatch_tail(builder, start_pos);
    emit_separator_tokens(builder, tail);
    builder.finish_node();

    let mut in_header = has_header;
    let mut current_row_indices: Vec<usize> = Vec::new();

    for i in (start_pos + 1)..end_pos {
        let line = window.line(i);
        if i == column_sep_pos {
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

        if is_multiline_border(line) {
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

        if line.trim().is_empty() {
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

            builder.start_node(SyntaxKind::BLANK_LINE.into());
            let tail = window.emit_or_dispatch_tail(builder, i);
            builder.token(SyntaxKind::BLANK_LINE.into(), tail);
            builder.finish_node();
            continue;
        }

        current_row_indices.push(i);
    }

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

    if let Some((cap_start, cap_end)) = caption_after {
        emit_caption_blank_lines(builder, window, end_pos, cap_start);
        emit_table_caption(builder, window, cap_start, cap_end, config);
    }

    builder.finish_node(); // MultilineTable

    let table_start = caption_before.map(|(start, _)| start).unwrap_or(start_pos);
    let table_end = if let Some((_, cap_end)) = caption_after {
        cap_end
    } else {
        end_pos
    };

    Some(table_end - table_start)
}

fn extract_first_line_cell_contents(line: &str, columns: &[Column]) -> Vec<String> {
    let (line_content, _) = strip_newline(line);
    let mut cells = Vec::new();

    for (i, column) in columns.iter().enumerate() {
        let column_start = column_offset_to_byte_index(line_content, column.start);
        let column_end = if i + 1 == columns.len() {
            line_content.len().max(column_start)
        } else {
            column_offset_to_byte_index(line_content, column.end)
        };

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

/// Whether the window's container prefix contributes no bytes to continuation
/// lines (top-level, i.e. not nested in a blockquote or list). Used to gate the
/// single-column multiline-cell inline join, which reads raw continuation lines
/// and would otherwise fold `>`/list-marker bytes into cell content.
fn prefix_is_empty(prefix: &ContainerPrefix) -> bool {
    prefix.bq_depth() == 0 && prefix.list_content_col() == 0 && prefix.content_indent() == 0
}

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

    if columns.len() == 1 && indices.len() > 1 && prefix_is_empty(window.prefix()) {
        let first = window.strip_at(indices[0]);
        let (first_trimmed, _) = strip_newline(first);
        let cell_start = column_offset_to_byte_index(first_trimmed, columns[0].start);
        if cell_start > 0 {
            builder.token(SyntaxKind::WHITESPACE.into(), &first_trimmed[..cell_start]);
        }

        let mut joined = String::from(&first[cell_start..]);
        for &idx in &indices[1..] {
            joined.push_str(window.strip_at(idx));
        }

        let (cell_text, trailing_nl) = strip_newline(&joined);
        emit_table_cell(builder, cell_text, config);
        if !trailing_nl.is_empty() {
            builder.token(SyntaxKind::NEWLINE.into(), trailing_nl);
        }

        builder.finish_node();
        return;
    }

    let first_line = window.emit_or_dispatch_tail(builder, indices[0]);
    let cell_contents = extract_first_line_cell_contents(first_line, columns);
    let (trimmed, newline_str) = strip_newline(first_line);
    let mut current_pos = 0;

    for (col_idx, column) in columns.iter().enumerate() {
        let cell_text = &cell_contents[col_idx];
        let cell_start = column_offset_to_byte_index(trimmed, column.start);
        let cell_end = if col_idx + 1 == columns.len() {
            trimmed.len().max(cell_start)
        } else {
            column_offset_to_byte_index(trimmed, column.end)
        };

        if current_pos < cell_start {
            builder.token(
                SyntaxKind::WHITESPACE.into(),
                &trimmed[current_pos..cell_start],
            );
        }

        emit_table_cell(builder, cell_text, config);

        current_pos = cell_end;
    }

    if current_pos < trimmed.len() {
        builder.token(SyntaxKind::WHITESPACE.into(), &trimmed[current_pos..]);
    }

    if !newline_str.is_empty() {
        builder.token(SyntaxKind::NEWLINE.into(), newline_str);
    }

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
        assert!(try_parse_multiline_separator("--").is_some()); // pandoc accepts 2 dashes
        assert!(try_parse_multiline_separator("-").is_none()); // list marker/setext territory
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

        assert!(result.is_none());
    }

    fn parse_multiline(input: &[&str]) -> Option<(usize, SyntaxNode)> {
        let mut builder = GreenNodeBuilder::new();
        let prefix = ContainerPrefix::default();
        let window = StrippedLines::new(input, 0, &prefix);
        let consumed = try_parse_multiline_table(&window, &mut builder, &ParserOptions::default())?;
        Some((consumed, SyntaxNode::new_root(builder.finish())))
    }

    fn header_cells(node: &SyntaxNode) -> Vec<String> {
        node.descendants()
            .filter(|n| n.kind() == SyntaxKind::TABLE_HEADER)
            .flat_map(|row| {
                row.descendants()
                    .filter(|n| n.kind() == SyntaxKind::TABLE_CELL)
            })
            .map(|cell| cell.text().to_string().trim().to_string())
            .collect()
    }

    #[test]
    fn test_multiline_border_detection() {
        assert!(is_multiline_border("-------"));
        assert!(is_multiline_border("----- -----"));
        assert!(is_multiline_border("- - -"));
        assert!(!is_multiline_border("+---+"));
        assert!(!is_multiline_border("_____"));
        assert!(!is_multiline_border("A     B"));
    }

    #[test]
    fn test_spaced_top_border_opens_headed_table() {
        let input = vec![
            "----- -----",
            "A     B",
            "----- -----",
            "x     y",
            "",
            "----- -----",
        ];

        let (consumed, node) = parse_multiline(&input).expect("spaced border opens a table");
        assert_eq!(consumed, 6);
        assert_eq!(header_cells(&node), vec!["A", "B"]);
    }

    #[test]
    fn test_spaced_closing_border_closes_table() {
        let input = vec!["-------", "A   B", "--- ---", "x   y", "", "--- ---"];

        let (consumed, node) = parse_multiline(&input).expect("spaced border closes a table");
        assert_eq!(consumed, 6);
        assert_eq!(header_cells(&node), vec!["A", "B"]);
    }

    #[test]
    fn test_spaced_opener_without_column_separator_stays_headerless() {
        let input = vec!["----- -----", "A   B", "", "x   y", "", "----- -----"];

        let (consumed, node) = parse_multiline(&input).expect("headerless reading still applies");
        assert_eq!(consumed, 6);
        assert!(header_cells(&node).is_empty());
    }

    #[test]
    fn test_blank_line_after_opener_disqualifies_every_reading() {
        assert!(
            parse_multiline(&[
                "-----------",
                "",
                "A   B",
                "--- ---",
                "x   y",
                "",
                "-----------"
            ])
            .is_none()
        );

        assert!(
            parse_multiline(&["----- -----", "", "A   B", "", "x   y", "", "----- -----"])
                .is_none()
        );

        assert!(parse_multiline(&["-------", "", "A   B", "", "x   y", "", "-------"]).is_none());

        assert!(parse_multiline(&["-------", "A   B", "", "x   y", "", "-------"]).is_some());
    }

    #[test]
    fn test_emit_table_cell_plain_text() {
        let mut builder = GreenNodeBuilder::new();
        emit_table_cell(&mut builder, "Cell", &ParserOptions::default());
        let green = builder.finish();
        let node = SyntaxNode::new_root(green);

        assert_eq!(node.kind(), SyntaxKind::TABLE_CELL);
        assert_eq!(node.text(), "Cell");

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

        let children: Vec<_> = node.children_with_tokens().collect();
        assert!(children.len() >= 4);

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
        assert_eq!(node.text(), r"A \| B");
    }
}
