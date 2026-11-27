//! Blockquote parsing utilities.
//!
//! Re-exports marker parsing functions from marker_utils for backward compatibility.

use crate::parser::utils::helpers::space_run;
use crate::syntax::SyntaxKind;
use rowan::GreenNodeBuilder;

use crate::parser::utils::container_stack::{Container, ContainerStack};

pub(crate) use crate::parser::utils::marker_utils::{
    count_blockquote_markers, try_parse_blockquote_marker,
};

pub(in crate::parser) const GITHUB_ALERT_MARKERS: [&str; 5] = [
    "[!TIP]",
    "[!WARNING]",
    "[!IMPORTANT]",
    "[!CAUTION]",
    "[!NOTE]",
];

pub(in crate::parser) fn github_alert_marker_len(content: &str) -> Option<usize> {
    GITHUB_ALERT_MARKERS
        .into_iter()
        .find(|marker| *marker == content.trim_end())
        .map(str::len)
}

pub(in crate::parser) fn emit_alert_header(
    builder: &mut GreenNodeBuilder<'static>,
    line: &str,
    newline: &str,
    indent: &str,
    marker_len: usize,
) {
    builder.start_node(SyntaxKind::ALERT.into());
    let header = line.trim_start();
    let leading = &line[..line.len() - header.len()];
    for whitespace in [indent, leading] {
        if !whitespace.is_empty() {
            builder.token(SyntaxKind::WHITESPACE.into(), whitespace);
        }
    }
    let (marker, remainder) = header.split_at(marker_len);
    builder.token(SyntaxKind::ALERT_MARKER.into(), marker);
    let title_and_padding = remainder.trim_start();
    let gap = &remainder[..remainder.len() - title_and_padding.len()];
    let title = title_and_padding.trim_end();
    if !gap.is_empty() {
        builder.token(SyntaxKind::WHITESPACE.into(), gap);
    }
    if !title.is_empty() {
        builder.token(SyntaxKind::ALERT_TITLE.into(), title);
    }
    let trailing = &title_and_padding[title.len()..];
    if !trailing.is_empty() {
        builder.token(SyntaxKind::WHITESPACE.into(), trailing);
    }
    if !newline.is_empty() {
        builder.token(SyntaxKind::NEWLINE.into(), newline);
    }
}

/// Check if we need a blank line before starting a new blockquote.
/// Returns true if a blockquote can start here.
///
/// `fenced_divs_enabled` lets the first child of a fenced div start a
/// blockquote without a preceding blank line: Pandoc treats the line right
/// after a `::: {...}` opener like the start of the document, so a flush
/// blockquote there is still a blockquote (see issue #310).
pub(in crate::parser) fn can_start_blockquote(
    pos: usize,
    lines: &[&str],
    fenced_divs_enabled: bool,
) -> bool {
    if pos == 0 {
        return true;
    }
    if crate::parser::utils::helpers::is_blank_line(lines[pos - 1]) {
        return true;
    }
    if opens_fenced_div_at_depth(lines[pos - 1], 0, fenced_divs_enabled) {
        return true;
    }
    false
}

/// Whether `line` opens a fenced div once `depth` enclosing blockquote
/// markers are stripped from it.
///
/// The line after a `::: {...}` opener is quote-startable because Pandoc
/// treats it like the start of the document. That holds at any nesting
/// depth, so a div inside a quote (`> ::: note` / `> > quoted`) gets the
/// same hatch.
pub(in crate::parser) fn opens_fenced_div_at_depth(
    line: &str,
    depth: usize,
    fenced_divs_enabled: bool,
) -> bool {
    if !fenced_divs_enabled {
        return false;
    }
    let (inner, consumed) = strip_blockquote_markers_counted(line, depth);
    consumed == depth
        && crate::parser::blocks::fenced_divs::try_parse_div_fence_open(inner).is_some()
}

/// Get the current blockquote depth from the container stack.
pub(in crate::parser) fn current_blockquote_depth(containers: &ContainerStack) -> usize {
    containers
        .stack
        .iter()
        .filter(|c| matches!(c, Container::BlockQuote { .. }))
        .count()
}

/// Strip exactly n blockquote markers from a line, returning the rest.
pub(in crate::parser) fn strip_n_blockquote_markers(line: &str, n: usize) -> &str {
    strip_blockquote_markers_counted(line, n).0
}

/// Strip up to `n` blockquote markers, returning the rest and how many were
/// actually consumed.
///
/// A count below `n` means the line is *lazy* at that depth — it carries
/// fewer `>` markers than the open quote. Callers that implement pandoc's
/// gobble need that distinction, which the plain
/// [`strip_n_blockquote_markers`] hides by returning the line unchanged.
pub(in crate::parser) fn strip_blockquote_markers_counted(line: &str, n: usize) -> (&str, usize) {
    let mut remaining = line;
    let mut consumed = 0usize;
    for _ in 0..n {
        if let Some((_, content_start)) = try_parse_blockquote_marker(remaining) {
            remaining = &remaining[content_start..];
            consumed += 1;
        } else {
            break;
        }
    }
    (remaining, consumed)
}

/// Emit one blockquote marker with its whitespace, as container
/// structure (`BLOCK_QUOTE_MARKER`/`WHITESPACE`). For markers landing
/// *inside* a content node use [`emit_one_line_prefix_marker`].
pub(in crate::parser) fn emit_one_blockquote_marker(
    builder: &mut GreenNodeBuilder<'static>,
    leading_spaces: usize,
    has_trailing_space: bool,
) {
    if leading_spaces > 0 {
        builder.token(SyntaxKind::WHITESPACE.into(), &space_run(leading_spaces));
    }
    builder.token(SyntaxKind::BLOCK_QUOTE_MARKER.into(), ">");
    if has_trailing_space {
        builder.token(SyntaxKind::WHITESPACE.into(), " ");
    }
}

/// [`emit_one_blockquote_marker`]'s counterpart for a marker that lands
/// inside a content node: same token boundaries, every token
/// `LINE_PREFIX`.
pub(in crate::parser) fn emit_one_line_prefix_marker(
    builder: &mut GreenNodeBuilder<'static>,
    leading_spaces: usize,
    has_trailing_space: bool,
) {
    if leading_spaces > 0 {
        builder.token(SyntaxKind::LINE_PREFIX.into(), &space_run(leading_spaces));
    }
    builder.token(SyntaxKind::LINE_PREFIX.into(), ">");
    if has_trailing_space {
        builder.token(SyntaxKind::LINE_PREFIX.into(), " ");
    }
}
