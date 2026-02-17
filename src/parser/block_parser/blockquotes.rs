//! Blockquote parsing utilities.
//!
//! Re-exports marker parsing functions from marker_utils for backward compatibility.

use crate::syntax::SyntaxKind;
use rowan::GreenNodeBuilder;

use super::container_stack::{Container, ContainerStack};

pub(crate) use super::marker_utils::{count_blockquote_markers, try_parse_blockquote_marker};

/// Check if we need a blank line before starting a new blockquote.
/// Returns true if a blockquote can start here.
pub(super) fn can_start_blockquote(pos: usize, lines: &[&str]) -> bool {
    // At start of document, no blank line needed
    if pos == 0 {
        return true;
    }
    // After a blank line, can start blockquote
    if pos > 0 && lines[pos - 1].trim().is_empty() {
        return true;
    }
    // If we're already in a blockquote, nested blockquotes need blank line too
    // (blank_before_blockquote extension)
    false
}

/// Get the current blockquote depth from the container stack.
pub(super) fn current_blockquote_depth(containers: &ContainerStack) -> usize {
    containers
        .stack
        .iter()
        .filter(|c| matches!(c, Container::BlockQuote { .. }))
        .count()
}

/// Strip exactly n blockquote markers from a line, returning the rest.
pub(super) fn strip_n_blockquote_markers(line: &str, n: usize) -> &str {
    let mut remaining = line;
    for _ in 0..n {
        if let Some((_, content_start)) = try_parse_blockquote_marker(remaining) {
            remaining = &remaining[content_start..];
        } else {
            break;
        }
    }
    remaining
}

/// Emit one blockquote marker with its whitespace.
pub(super) fn emit_one_blockquote_marker(
    builder: &mut GreenNodeBuilder<'static>,
    leading_spaces: usize,
    has_trailing_space: bool,
) {
    if leading_spaces > 0 {
        builder.token(SyntaxKind::WHITESPACE.into(), &" ".repeat(leading_spaces));
    }
    builder.token(SyntaxKind::BlockQuoteMarker.into(), ">");
    if has_trailing_space {
        builder.token(SyntaxKind::WHITESPACE.into(), " ");
    }
}

/// Close blockquotes down to a target depth.
pub(super) fn close_blockquotes_to_depth(
    containers: &mut ContainerStack,
    builder: &mut GreenNodeBuilder<'static>,
    target_depth: usize,
) {
    let mut current = current_blockquote_depth(containers);
    while current > target_depth {
        // Close everything until we hit a blockquote, then close it
        while !matches!(containers.last(), Some(Container::BlockQuote { .. })) {
            if containers.depth() == 0 {
                break;
            }
            containers.close_to(containers.depth() - 1, builder);
        }
        if matches!(containers.last(), Some(Container::BlockQuote { .. })) {
            containers.close_to(containers.depth() - 1, builder);
            current -= 1;
        } else {
            break;
        }
    }
}
